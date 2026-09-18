//! 世界 → [`SceneFrame`] 提取。
//!
//! 这是整条渲染链路上**唯一**读 `ecs_core` 组件的地方。后端只拿到
//! [`SceneFrame`]，因此换后端不改这里的提取逻辑，改提取逻辑不影响任何后端。
//!
//! # 四条约束
//!
//! 1. **只读**：提取函数收 `&mut World` 只为拿查询迭代器（Bevy 的
//!    `World::query` 需要 `&mut`），全程不写任何组件/资源。
//! 2. **每帧全量重建**：不增量、不缓存差分。80×60 的地图每帧约 4800 个 key，
//!    在 30fps 下完全可接受；真需要优化时再加 chunk/dirty，不要提前做
//!    （Dsn28「提取成本」）。
//! 3. **不持久化**：快照是派生产物；存档仍然是 `ecs_core` 的职责。
//! 4. **不带外观**：这里只产出 [`VisualKey`]（“这是什么”），glyph/颜色由后端的
//!    catalog 决定。
//!
//! # 可见性与记忆
//!
//! - `MapView.visible` 取玩家 `Viewshed`（视野内的地形正常绘制）；
//! - `MapView.explored` 取 `MapMemory`（探索过但当前不可见的地形由后端画暗）；
//! - **地形本身始终填进 `MapView.tiles`**，可见/探索是**并列的布尔位图**。
//!   让后端自己按「可见 > 探索 > 未知」决定画法，比在这里产出三种 tile 变体
//!   更省事，也让后端能自由选择暗化比例。
//!
//! [`VisualKey`]: render_api::VisualKey

use bevy_ecs::prelude::*;
use ecs_core::{
    Attack, Defense, EventLog, Experience, FloorNumber, Health, Level, Magic, Map, MapMemory,
    MapSeed, MonsterKindId, Player, Position, Stairs, TurnManager, Viewshed, map_kind_for,
    monster_template,
};
use render_api::{
    EntityId, EntityView, HudView, LogLevel, LogLine, MapView, Meter, SceneFrame,
    VisualKey, VisualLayer,
};

use crate::camera::{CameraFollow, camera_for};
use crate::catalog::{map_monster_kind, map_tile};
use crate::ui::PageStack;

/// 提取参数：世界尺寸、视口尺寸、日志保留条数。
///
/// 世界尺寸从 `ecs_core` 传入而不是在这里 import 常量，好处是测试可以用小世界
/// 构造场景（不必为了测 3×3 的行为造一张 80×60 的地图）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExtractConfig {
    pub world_width: usize,
    pub world_height: usize,
    /// 视口尺寸（TUI 为终端格子数）。`(0, 0)` 表示"未知"，相机退回世界中心。
    pub viewport: (u16, u16),
    /// 日志保留条数（取最近 N 条）。
    pub log_lines: usize,
    pub follow: CameraFollow,
}

impl Default for ExtractConfig {
    fn default() -> Self {
        Self {
            world_width: ecs_core::MAP_WIDTH,
            world_height: ecs_core::MAP_HEIGHT,
            viewport: (0, 0),
            log_lines: 12,
            follow: CameraFollow::Player,
        }
    }
}

impl ExtractConfig {
    /// 指定世界尺寸的配置（其余取默认）。
    pub fn new(world_width: usize, world_height: usize) -> Self {
        Self {
            world_width,
            world_height,
            ..Self::default()
        }
    }

    /// 设置视口尺寸。
    pub fn with_viewport(mut self, width: u16, height: u16) -> Self {
        self.viewport = (width, height);
        self
    }

    /// 设置日志保留条数。
    pub fn with_log_lines(mut self, lines: usize) -> Self {
        self.log_lines = lines;
        self
    }

    /// 设置相机跟随目标。
    pub fn with_follow(mut self, follow: CameraFollow) -> Self {
        self.follow = follow;
        self
    }

    fn world_size(&self) -> (usize, usize) {
        (self.world_width, self.world_height)
    }
}

/// 提取出的场景 + 下一帧的相机跟随状态。
///
/// `follow` 会被写回，因为 Look / 投掷瞄准这类页面需要临时把相机钉在某个格子上，
/// 关掉页面再跟回玩家。相机的**意图**属于 UI 状态机，不属于后端。
#[derive(Debug, Clone, PartialEq)]
pub struct ExtractedScene {
    pub frame: SceneFrame,
    pub follow: CameraFollow,
}

/// 从世界提取一帧。
///
/// `revision` 由调用方给出（通常来自 [`SceneFrameSource`]），这样"帧号"这件事
/// 由装配层决定，提取函数保持纯粹。
pub fn extract_scene_frame(
    world: &mut World,
    config: &ExtractConfig,
    pages: &PageStack,
    revision: u64,
) -> SceneFrame {
    let player_position = player_position(world);
    let camera = camera_for(config.follow, player_position, config.viewport, config);

    let mut frame = SceneFrame {
        revision,
        camera,
        map: build_map_view(world, config),
        entities: build_entities(world, config),
        player: build_player_view(world),
        hud: build_hud(world),
        log: build_log(world, config),
        // 先留空，等其余字段就位后再算：Look 页的地形详情**从本帧快照读**，
        // 而不是回头再查一次世界——那样会出现「页栈显示的地形与地图不一致」。
        ui: render_api::UiView::Game,
        game_over: world
            .get_resource::<TurnManager>()
            .is_some_and(|turns| turns.game_over),
        quit_requested: world
            .get_resource::<TurnManager>()
            .is_some_and(|turns| turns.wants_quit),
    };
    frame.ui = pages.view(config, &frame);
    frame
}

/// 帧号自增器：装配层持有它，保证 `revision` 单调递增。
///
/// 后端可以用 `revision` 跳过未变化的帧（Dsn28「后端不得反向修改」的配套：
/// 后端只读 revision，不写）。
#[derive(Debug, Default, Clone, Copy)]
pub struct SceneFrameSource {
    revision: u64,
}

impl SceneFrameSource {
    pub const fn new() -> Self {
        Self { revision: 0 }
    }

    /// 提取下一帧（帧号 +1）。
    pub fn next_frame(
        &mut self,
        world: &mut World,
        config: &ExtractConfig,
        pages: &PageStack,
    ) -> SceneFrame {
        self.revision = self.revision.wrapping_add(1);
        extract_scene_frame(world, config, pages, self.revision)
    }

    pub const fn revision(&self) -> u64 {
        self.revision
    }
}

/// 玩家当前世界坐标。`?` 打开 Look 页时用它做初始光标；找不到玩家返回 `None`。
///
/// 需要 `&mut World` 只为拿查询迭代器（Bevy 的 `World::query_filtered` 要求），
/// 全程只读。
pub fn player_position(world: &mut World) -> Option<(usize, usize)> {
    let mut query = world.query_filtered::<&Position, With<Player>>();
    query.iter(world).next().map(Position::to_tuple)
}

fn build_map_view(world: &mut World, config: &ExtractConfig) -> MapView {
    let (width, height) = config.world_size();
    let mut view = MapView::new(width, height);

    // 地形：始终全量填充，可见/探索由并列位图表达（见模块文档）。
    if let Some(map) = world.get_resource::<Map>() {
        for y in 0..height.min(map.tiles.len()) {
            for x in 0..width.min(map.tiles[y].len()) {
                view.set_tile(x, y, map_tile(map.tiles[y][x]));
            }
        }
    }

    // 探索位图：`MapMemory` 是定长 [[bool; W]; H]，越界索引要按实际尺寸夹。
    if let Some(memory) = world.get_resource::<MapMemory>() {
        for y in 0..height.min(memory.explored.len()) {
            for x in 0..width.min(memory.explored[y].len()) {
                if memory.explored[y][x] {
                    view.set_explored(x, y, true);
                }
            }
        }
    }

    // 可见位图：玩家视野。
    let visible: Vec<(usize, usize)> = {
        let mut query = world.query_filtered::<&Viewshed, With<Player>>();
        query
            .iter(world)
            .next()
            .map(|viewshed| viewshed.visible_tiles.clone())
            .unwrap_or_default()
    };
    for (x, y) in visible {
        view.set_visible(x, y, true);
    }

    view
}

/// 构造实体视图。
///
/// 玩家单独走 `SceneFrame.player`（后端通常要给它特殊样式），所以这里跳过玩家。
/// 视野外的实体只在"玩家当前看得见"时出现；记忆里的实体（`VisibleMemory`）
/// 本帧没有视图数据可用，一律不产出——宁可少画，不要画出过期位置。
fn build_entities(world: &mut World, config: &ExtractConfig) -> Vec<EntityView> {
    let (width, height) = config.world_size();
    let explored = collect_explored(world);

    let mut query = world.query::<(
        Entity,
        &Position,
        Option<&Player>,
        Option<&Stairs>,
        Option<&MonsterKindId>,
        Option<&Health>,
    )>();

    let mut entities: Vec<EntityView> = Vec::new();
    for (entity, position, is_player, is_stairs, kind, health) in query.iter(world) {
        if is_player.is_some() {
            continue;
        }
        let (x, y) = position.to_tuple();
        if x >= width || y >= height {
            // 世界尺寸之外的实体不画（测试用小世界时会出现）。
            continue;
        }

        let visual = if is_stairs.is_some() {
            VisualKey::Stairs
        } else if let Some(kind) = kind {
            map_monster_kind(*kind)
        } else {
            VisualKey::Unknown(u32::from(crate::catalog::tile_id(ecs_core::Tile::Floor)))
        };

        let layer = if is_stairs.is_some() {
            VisualLayer::Terrain
        } else {
            VisualLayer::Actor
        };

        let name = if is_stairs.is_some() {
            "楼梯".to_string()
        } else if let Some(kind) = kind {
            monster_template(*kind).name.to_string()
        } else {
            "未知".to_string()
        };

        // 楼梯一旦被发现就应当画出来（它是导航目标，不该只在视野内可见）。
        // 怪物则严格按视野：看不见就不画，避免"隔着墙看到怪物"。
        let is_stairs = is_stairs.is_some();
        let visible = is_stairs && explored.contains(&(x, y));

        let mut view = EntityView::new(
            EntityId::from_bits(entity.to_bits()),
            (x as i32, y as i32),
            visual,
        )
        .with_name(name)
        .with_layer(layer)
        .with_visibility(visible, false);

        if let Some(health) = health {
            view = view.with_hp(Meter::new(health.current as f32, health.max as f32));
        }
        entities.push(view);
    }

    // 稳定排序：同一帧内实体的顺序不该随 ECS 内部迭代顺序变化，
    // 否则 golden 测试会随机失败，后端也可能出现"同格两个实体交替闪烁"。
    // `EntityId` 不是 `Ord`（它只是装配层用的稳定标识），所以按 bits 排序。
    entities.sort_by_key(|entity| {
        (
            entity.layer,
            entity.position.1,
            entity.position.0,
            entity.id.to_bits(),
        )
    });
    entities
}

fn collect_explored(world: &mut World) -> Vec<(usize, usize)> {
    let Some(memory) = world.get_resource::<MapMemory>() else {
        return Vec::new();
    };
    let mut explored = Vec::new();
    for (y, row) in memory.explored.iter().enumerate() {
        for (x, seen) in row.iter().enumerate() {
            if *seen {
                explored.push((x, y));
            }
        }
    }
    explored
}

fn build_player_view(world: &mut World) -> Option<EntityView> {
    let mut query = world.query_filtered::<
        (
            Entity,
            &Position,
            &Health,
            Option<&Level>,
            Option<&ecs_core::EntityName>,
        ),
        With<Player>,
    >();

    let (entity, position, health, level, name) = query.iter(world).next()?;
    let (x, y) = position.to_tuple();
    let default_name = "冒险者".to_string();
    let name = name.map(|n| n.0.clone()).unwrap_or(default_name);
    let level = level.map(|l| l.0).unwrap_or(1);

    Some(
        EntityView::new(
            EntityId::from_bits(entity.to_bits()),
            (x as i32, y as i32),
            VisualKey::Player,
        )
        .with_name(format!("{name} Lv.{level}"))
        .with_hp(Meter::new(health.current as f32, health.max as f32))
        .with_layer(VisualLayer::Actor)
        .with_visibility(true, false),
    )
}

fn build_hud(world: &mut World) -> HudView {
    let floor = world
        .get_resource::<FloorNumber>()
        .map(|floor| floor.0)
        .unwrap_or(1);
    let map_kind = world
        .get_resource::<MapSeed>()
        .map(|seed| format!("{:?}", map_kind_for(seed.0, floor)))
        .unwrap_or_default();

    let mut query = world.query_filtered::<
        (
            &Health,
            Option<&Magic>,
            Option<&Experience>,
            Option<&Level>,
            Option<&Attack>,
            Option<&Defense>,
            Option<&ecs_core::EntityName>,
        ),
        With<Player>,
    >();

    let Some((health, magic, exp, level, attack, defense, name)) = query.iter(world).next() else {
        return HudView {
            floor,
            map_kind,
            ..HudView::default()
        };
    };

    HudView {
        floor,
        map_kind,
        player_name: name.map(|n| n.0.clone()).unwrap_or_default(),
        level: level.map(|l| l.0 as u32).unwrap_or(1),
        hp: Meter::new(health.current as f32, health.max as f32),
        mp: magic
            .map(|m| Meter::new(m.current as f32, m.max as f32))
            .unwrap_or_default(),
        exp: exp
            .map(|e| Meter::new(e.exp as f32, e.exp_to_next as f32))
            .unwrap_or_default(),
        attack: attack.map(|a| a.0 as f32).unwrap_or_default(),
        defense: defense.map(|d| d.0 as f32).unwrap_or_default(),
        extra_lines: Vec::new(),
    }
}

fn build_log(world: &mut World, config: &ExtractConfig) -> Vec<LogLine> {
    let Some(log) = world.get_resource::<EventLog>() else {
        return Vec::new();
    };
    log.messages
        .iter()
        .rev()
        .take(config.log_lines)
        .rev()
        .map(|message| LogLine::new(map_log_level(message.level), message.text.clone()))
        .collect()
}

/// `ecs_core::EventLevel` → 契约的 [`LogLevel`]。
///
/// 两个枚举故意分层（REFACTOR §10.8：`EventLevel` vs `render-api::LogLevel` 是
/// **有意**的重复），映射集中在这一处，不散落到后端。
fn map_log_level(level: ecs_core::EventLevel) -> LogLevel {
    match level {
        ecs_core::EventLevel::Combat => LogLevel::Combat,
        ecs_core::EventLevel::Item => LogLevel::Item,
        ecs_core::EventLevel::Skill => LogLevel::Skill,
        ecs_core::EventLevel::System => LogLevel::System,
        ecs_core::EventLevel::Danger => LogLevel::Danger,
    }
}

#[cfg(test)]
mod tests;
