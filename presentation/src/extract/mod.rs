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
//!    （DsnX14「提取成本」）。
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
    MapSeed, MonsterKindId, Observable, Player, Position, Stairs, TurnManager, Viewshed,
    map_kind_for, monster_template,
};
use render_api::{
    EntityId, EntityView, EventLine, HudView, InfoView, LogLevel, MapView, Meter, SceneFrame,
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
        focus: pages.pane_focus(),
        map: build_map_view(world, config),
        entities: build_entities(world, config),
        player: build_player_view(world),
        hud: build_hud(world),
        info: build_info(world, pages.info_cursor()),
        events: build_events(world, config),
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
    // 背包页要读世界（身体/行囊容器），所以在这里构造而不是在 `PageStack::view` 里：
    // 页栈只持有光标，视图每帧从世界重建，规则与界面因此不可能分叉。
    frame.ui = match pages.kind() {
        render_api::PageKind::Inventory => render_api::UiView::Inventory(inventory_view(
            &*world,
            pages.inventory_cursor(),
        )),
        _ => pages.view(config, &frame),
    };
    frame
}

/// 背包页的视图（DsnE18 第①片的只读版）：**左栏身体、右栏行囊、脚下有尸体时多一栏**。
///
/// 数据来源只有三处：身体容器（七个部位）、行囊容器（12 格）、脚下尸体容器（4 格）。
/// 名字一律来自注册表/布局表，于是"界面里写了什么"与"规则里是什么"不可能分叉。
///
/// `cursor` 是当前选中的行号（在"身体 + 行囊 + 脚下"的拼接列表上），渲染层只负责把它画出来。
pub fn inventory_view(world: &World, cursor: usize) -> render_api::InventoryView {
    use render_api::{InventoryPanel, InventoryPanelView};

    let player = player_entity_of(world);
    let mut page = match player {
        Some(player) => build_inventory_page(world, player),
        None => InventoryPage::empty(),
    };
    let cursor = cursor.min(page.targets.len().saturating_sub(1));
    page.focus(cursor);
    let focus = page.focused_panel(cursor);

    // 详情面板：选中行的物品说明 + 这一行能做什么。行 → 目标 → 占用实体三步
    // 与 `inventory_target` 同源（同一份 `build_inventory_page`），所以"选中的是
    // 第几行"与"详情讲的是哪件物品"不可能错位。
    let detail = player
        .zip(page.targets.get(cursor).copied())
        .and_then(|(player, target)| {
            let occupant = occupant_for(world, player, target)?;
            item_detail(world, player, target, occupant)
        });

    let mut panels = vec![
        InventoryPanelView::new(InventoryPanel::Equipment, page.equipment),
        InventoryPanelView::new(InventoryPanel::Backpack, page.backpack),
    ];
    if let Some(corpse) = page.corpse {
        panels.push(InventoryPanelView::new(InventoryPanel::Ground, corpse));
    }
    if let Some(ground) = page.ground {
        panels.push(InventoryPanelView::new(InventoryPanel::Ground, ground));
    }

    render_api::InventoryView {
        title: "背包".to_string(),
        panels,
        focus: render_api::InventoryFocus::Panel(focus),
        detail,
        footer: "↑↓ 选择    Enter 装/卸/拿    r 使用    d 丢弃    e/Esc 关闭".to_string(),
    }
}

/// 背包页的一栏数据：渲染层看到的 `ListView` 与"行 → 世界目标"的 `targets` 同源生成。
///
/// 这两个东西**必须一起建**：以前视图按 `items.len()`（含占位行）分段、目标按真实行
/// 分段，身体空着两栏就会错位一格。现在长度只在这里数一次。
struct InventoryPage {
    equipment: render_api::ListView,
    backpack: render_api::ListView,
    /// 脚下有尸体容器时才存在的「尸体」栏。
    corpse: Option<render_api::ListView>,
    /// 脚下有地面物品时才存在的「地面」栏（DsnE21 阶段 4：拾取全显式）。
    ground: Option<render_api::ListView>,
    /// 行序：身体（部位顺序）→ 行囊（格子顺序）→ 尸体（格子顺序）→ 地面（实体 id 升序）。
    targets: Vec<InventoryTarget>,
    /// 真实行数（`（…）` 占位行不计入光标）。
    equipment_len: usize,
    backpack_len: usize,
}

impl InventoryPage {
    /// 没有玩家时的一页空数据（标题/占位仍然要画出来）。
    fn empty() -> Self {
        let mut equipment = render_api::ListView::new("身体");
        let mut backpack = render_api::ListView::new("行囊");
        equipment
            .items
            .push(render_api::ListItemView::new("（身上没穿东西）"));
        backpack
            .items
            .push(render_api::ListItemView::new("（行囊是空的）"));
        Self {
            equipment,
            backpack,
            corpse: None,
            ground: None,
            targets: Vec::new(),
            equipment_len: 0,
            backpack_len: 0,
        }
    }

    /// 把光标夹到合法行，并给每一栏标出选中项。
    fn focus(&mut self, cursor: usize) {
        if self.targets.is_empty() {
            return;
        }
        if cursor < self.equipment_len {
            self.equipment.selected = cursor;
        } else if cursor < self.equipment_len + self.backpack_len {
            self.backpack.selected = cursor - self.equipment_len;
        } else if let Some(corpse) = self.corpse.as_mut() {
            let len = corpse.items.len();
            let offset = cursor - self.equipment_len - self.backpack_len;
            if offset < len {
                corpse.selected = offset;
                return;
            }
            if let Some(ground) = self.ground.as_mut()
                && offset - len < ground.items.len()
            {
                ground.selected = offset - len;
            }
        } else if let Some(ground) = self.ground.as_mut() {
            let offset = cursor - self.equipment_len - self.backpack_len;
            if offset < ground.items.len() {
                ground.selected = offset;
            }
        }
    }

    /// 光标落在哪一栏（空页按身体栏算，焦点总得有个去处）。
    fn focused_panel(&self, cursor: usize) -> render_api::InventoryPanel {
        use render_api::InventoryPanel;
        if self.targets.is_empty() || cursor < self.equipment_len {
            InventoryPanel::Equipment
        } else if cursor < self.equipment_len + self.backpack_len {
            InventoryPanel::Backpack
        } else {
            InventoryPanel::Ground
        }
    }
}

/// 行囊当前的载重与上限（`None` = 不限重）。
///
/// 与 `ecs_core` 同一口径：整堆重量 = 每单位重量 × 数量；界面只**显示**，不做判定
/// （判定唯一权威在核心层，界面算错了也不该影响能不能装）。
fn pack_load(world: &World, pack: &ecs_core::Container) -> (f32, Option<f32>) {
    let weight = pack.total_weight(|item| {
        world
            .get::<ecs_core::Item>(item)
            .map(|stack| {
                let amount = world
                    .get::<ecs_core::Amount>(item)
                    .map(|amount| amount.0)
                    .unwrap_or(1);
                ecs_core::stack_weight(stack.id, amount)
            })
            .unwrap_or(0.0)
    });
    (weight, pack.max_weight())
}

/// 从世界重建一页：身体 → 行囊 → 脚下尸体。
fn build_inventory_page(world: &World, player: Entity) -> InventoryPage {
    use render_api::{ListItemView, ListView};

    let mut page = InventoryPage {
        equipment: ListView::new("身体"),
        backpack: ListView::new("行囊"),
        corpse: None,
        ground: None,
        targets: Vec::new(),
        equipment_len: 0,
        backpack_len: 0,
    };

    let Some(body) = world.get::<ecs_core::Container>(player) else {
        page.equipment
            .items
            .push(ListItemView::new("（身上没穿东西）"));
        page.backpack.items.push(ListItemView::new("（行囊是空的）"));
        return page;
    };

    for (index, occupant) in body.iter() {
        let Some(item) = world.get::<ecs_core::Item>(occupant) else {
            continue;
        };
        // 行囊本身不算"穿在身上"，它在下面的行囊栏里展开。
        if item.def().container_def().is_some() {
            continue;
        }
        let slot = body.def().slots[index].name;
        let mut entry = ListItemView::new(format!("{slot}：{}", item.def().name));
        if let Some(amount) = world.get::<ecs_core::Amount>(occupant)
            && amount.0 > 1
        {
            entry = entry.with_detail(format!("×{}", amount.0));
        }
        page.equipment.items.push(entry);
        page.targets.push(InventoryTarget::Body { slot: index });
        page.equipment_len += 1;
    }

    if let Some(pack) = body
        .slot(ecs_core::item::pack_slot_index())
        .and_then(|pack| world.get::<ecs_core::Container>(pack))
    {
        let (weight, max_weight) = pack_load(world, pack);
        page.backpack.footer = match max_weight {
            Some(max) => format!(
                "{}/{} 格 · {weight:.1}/{max:.1} 载重",
                pack.used(),
                pack.len()
            ),
            None => format!("{}/{} 格 · {weight:.1} 载重", pack.used(), pack.len()),
        };
        for (index, occupant) in pack.iter() {
            let Some(item) = world.get::<ecs_core::Item>(occupant) else {
                continue;
            };
            let amount = world
                .get::<ecs_core::Amount>(occupant)
                .map(|amount| amount.0)
                .unwrap_or(1);
            let label = if amount > 1 {
                format!("{} ×{}", item.def().name, amount)
            } else {
                item.def().name.to_string()
            };
            page.backpack
                .items
                .push(ListItemView::new(label).with_detail(item.def().desc));
            page.targets.push(InventoryTarget::Pack { slot: index });
            page.backpack_len += 1;
        }
    }

    if let Some(corpse) = corpse_underfoot(world, player)
        && let Some(container) = world.get::<ecs_core::Container>(corpse)
    {
        let name = world
            .get::<ecs_core::monster::Corpse>(corpse)
            .map(|corpse| corpse.display_name())
            .unwrap_or_else(|| "尸体".to_string());
        let mut list = ListView::new(format!("脚下：{name}"));
        for (index, occupant) in container.iter() {
            let Some(item) = world.get::<ecs_core::Item>(occupant) else {
                continue;
            };
            let amount = world
                .get::<ecs_core::Amount>(occupant)
                .map(|amount| amount.0)
                .unwrap_or(1);
            let label = if amount > 1 {
                format!("{} ×{}", item.def().name, amount)
            } else {
                item.def().name.to_string()
            };
            list.items
                .push(ListItemView::new(label).with_detail("Enter 拿取"));
            page.targets.push(InventoryTarget::Corpse { slot: index });
        }
        if list.items.is_empty() {
            list.items.push(ListItemView::new("（尸体上没东西）"));
        }
        list.footer = "Enter 拿取".to_string();
        page.corpse = Some(list);
    }

    // 地面：踩上不再自动拾取，玩家在这里选一件、按 Enter 拿（DsnE21 阶段 4）。
    // 行序 = 实体 id 升序，与 `ecs_core::take_from_ground(world, index)` 的排序同一份。
    let ground_items = ground_items_underfoot(world, player);
    if !ground_items.is_empty() {
        let mut list = ListView::new("地面");
        for (index, entity) in ground_items.iter().copied().enumerate() {
            let Some(item) = world.get::<ecs_core::Item>(entity) else {
                continue;
            };
            let amount = world
                .get::<ecs_core::Amount>(entity)
                .map(|amount| amount.0)
                .unwrap_or(1);
            let label = if amount > 1 {
                format!("{} ×{}", item.def().name, amount)
            } else {
                item.def().name.to_string()
            };
            list.items
                .push(ListItemView::new(label).with_detail("Enter 拿取"));
            page.targets.push(InventoryTarget::Ground { index });
        }
        list.footer = "Enter 拿取".to_string();
        page.ground = Some(list);
    }

    if page.equipment_len == 0 {
        page.equipment
            .items
            .push(ListItemView::new("（身上没穿东西）"));
    }
    if page.backpack_len == 0 {
        page.backpack.items.push(ListItemView::new("（行囊是空的）"));
    }
    page
}

/// 玩家**脚下**那具尸体（同一格、带容器）。没尸体就没有"脚下"栏。
fn corpse_underfoot(world: &World, player: Entity) -> Option<Entity> {
    let here = world.get::<Position>(player)?.to_tuple();
    world.iter_entities().find_map(|entity| {
        entity.get::<ecs_core::monster::Corpse>()?;
        if !entity.contains::<ecs_core::Container>() {
            return None;
        }
        let position = entity.get::<Position>()?;
        (position.to_tuple() == here).then(|| entity.id())
    })
}

/// 玩家脚下同格的地面物品（按实体 id 升序，与 `take_from_ground` 的下标一致）。
fn ground_items_underfoot(world: &World, player: Entity) -> Vec<Entity> {
    let Some(here) = world.get::<Position>(player).map(Position::to_tuple) else {
        return Vec::new();
    };
    let mut items: Vec<Entity> = world
        .iter_entities()
        .filter_map(|entity| {
            let at = entity.get::<ecs_core::GroundItemAt>()?;
            (at.to_tuple() == here && entity.contains::<ecs_core::Item>()).then(|| entity.id())
        })
        .collect();
    items.sort_by_key(|entity| entity.to_bits());
    items
}

/// 背包页选中行对应的**世界目标**（身体槽位 / 行囊槽位 / 脚下尸体槽位 / 地面物品下标）。
///
/// 页栈只存"光标在第几行"，行 → 槽位的换算由这里给出。视图构造与目标解析共用同一份
/// [`build_inventory_page`]，所以"看到第几行"与"动的是哪一格"不可能对不上。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InventoryTarget {
    /// 身体第 `slot` 格。
    Body { slot: usize },
    /// 行囊第 `slot` 格。
    Pack { slot: usize },
    /// 脚下尸体第 `slot` 格。
    Corpse { slot: usize },
    /// 脚下地面物品第 `index` 件（排序键=实体 id，与核心动作一致）。
    Ground { index: usize },
}

/// 背包页当前选中的目标（`None` = 空行/没有玩家）。
pub fn inventory_target(world: &World, cursor: usize) -> Option<InventoryTarget> {
    let player = player_entity_of(world)?;
    build_inventory_page(world, player).targets.get(cursor).copied()
}

/// 目标槽位里现在放着哪件物品（空槽或没有玩家 → `None`）。
fn occupant_for(world: &World, player: Entity, target: InventoryTarget) -> Option<Entity> {
    let body = world.get::<ecs_core::Container>(player)?;
    match target {
        InventoryTarget::Body { slot } => body.slot(slot),
        InventoryTarget::Pack { slot } => {
            let pack = body.slot(ecs_core::item::pack_slot_index())?;
            world.get::<ecs_core::Container>(pack)?.slot(slot)
        }
        InventoryTarget::Corpse { slot } => {
            let corpse = corpse_underfoot(world, player)?;
            world.get::<ecs_core::Container>(corpse)?.slot(slot)
        }
        InventoryTarget::Ground { index } => {
            ground_items_underfoot(world, player).get(index).copied()
        }
    }
}

/// 背包页详情面板的内容：物品说明 + 它带来的修正 + 放在哪里 + 能执行什么动作。
///
/// 文案模板在装配层，**数值与名字全部来自注册表 / 布局表**：详情面板不该出现
/// 任何"界面自己记得的"装备属性，否则改一行注册表就会与界面分叉。
fn item_detail(
    world: &World,
    player: Entity,
    target: InventoryTarget,
    occupant: Entity,
) -> Option<render_api::DetailView> {
    use render_api::{DetailView, UiTextLine};

    let item = world.get::<ecs_core::Item>(occupant)?;
    let def = item.def();
    let amount = world
        .get::<ecs_core::Amount>(occupant)
        .map(|amount| amount.0)
        .unwrap_or(1);
    let title = if amount > 1 {
        format!("{} ×{}", def.name, amount)
    } else {
        def.name.to_string()
    };
    let mut detail = DetailView::new(title);
    detail.push_line(UiTextLine::plain(def.desc));
    // 重量按**整堆**算（DsnE20）：单件重量 × 数量，与行囊载重同一口径。
    detail.push_line(UiTextLine::plain(format!(
        "重量：{:.1}",
        ecs_core::stack_weight(def.id, amount)
    )));

    match def.kind {
        ecs_core::ItemKind::Equipment { modifiers, ignores } => {
            detail.push_line(UiTextLine::plain("类型：装备"));
            if let Some(ms) = def.weapon_speed_ms {
                detail.push_line(UiTextLine::plain(format!("主手攻速：{ms:.0}ms")));
            }
            for modifier in modifiers {
                detail.push_line(UiTextLine::plain(modifier.describe()));
            }
            if !ignores.is_empty() {
                let names: Vec<&str> = ignores.iter().map(|source| source.label()).collect();
                detail.push_line(UiTextLine::plain(format!("无视：{}", names.join("、"))));
            }
        }
        ecs_core::ItemKind::Consumable(effect) => {
            detail.push_line(UiTextLine::plain(format!("使用：{}", effect.describe())));
        }
        ecs_core::ItemKind::Material => {
            detail.push_line(UiTextLine::plain("材料：合成原料"));
        }
        ecs_core::ItemKind::Pack(layout) => {
            detail.push_line(UiTextLine::plain(format!("容器：{} 格", layout.len())));
        }
    }

    let body = world.get::<ecs_core::Container>(player)?;
    match target {
        InventoryTarget::Body { slot } => {
            let name = body
                .def()
                .slots
                .get(slot)
                .map(|slot| slot.name)
                .unwrap_or("?");
            detail.push_line(UiTextLine::plain(format!("部位：{name}")));
            detail.footer = "Enter 卸下    d 丢弃".to_string();
        }
        InventoryTarget::Pack { slot } => {
            detail.push_line(UiTextLine::plain(format!("行囊第 {} 格", slot + 1)));
            detail.footer = if def.is_equipment() {
                "Enter 装上    d 丢弃".to_string()
            } else {
                "r 使用    d 丢弃".to_string()
            };
        }
        InventoryTarget::Corpse { slot } => {
            let corpse_name = corpse_underfoot(world, player)
                .and_then(|corpse| world.get::<ecs_core::monster::Corpse>(corpse))
                .map(|corpse| corpse.display_name())
                .unwrap_or_else(|| "尸体".to_string());
            detail.push_line(UiTextLine::plain(format!("脚下：{corpse_name}")));
            detail.push_line(UiTextLine::plain(format!("尸体第 {} 格", slot + 1)));
            detail.footer = "Enter 拿取".to_string();
        }
        InventoryTarget::Ground { index } => {
            detail.push_line(UiTextLine::plain("位置：脚下地面"));
            detail.push_line(UiTextLine::plain(format!("地面第 {} 件", index + 1)));
            detail.footer = "Enter 拾取".to_string();
        }
    }

    // "能装到哪个部位"由两份数据求交（物品 tags × 槽位过滤），与拾取路由同一条判据。
    if def.is_equipment() {
        let slots: Vec<&str> = ecs_core::BODY
            .slots
            .iter()
            .filter(|slot| slot.accept.accepts(def.tags))
            .map(|slot| slot.name)
            .collect();
        if !slots.is_empty() {
            detail.push_line(UiTextLine::plain(format!("可装：{}", slots.join("/"))));
        }
    }
    Some(detail)
}

/// 帧号自增器：装配层持有它，保证 `revision` 单调递增。
///
/// 后端可以用 `revision` 跳过未变化的帧（DsnX14「后端不得反向修改」的配套：
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

/// 玩家实体（只读；给不持有 `&mut World` 的视图构造用）。
fn player_entity_of(world: &World) -> Option<Entity> {
    let mut query = world.try_query_filtered::<Entity, With<ecs_core::entity_cls::Player>>()?;
    query.iter(world).next()
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

    // 格子效果：覆盖地形外观（"这一格怎么了"比"它下面的地板"更重要）。
    overlay_cell_effects(world, &mut view, width, height);

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

/// 把格子效果画在地形之上（C2 起）。
///
/// **为什么覆盖而不是另起一层**：`MapView` 每格只有一个 `VisualKey`，而玩家必须能看见
/// "这一格会让我变慢"——否则减速就是一次无声的惩罚。地形本身仍在 `ecs_core::Map` 里可查，
/// 视觉只是有优先级的一层（`TileInfo` 这类"看这一格"的接口仍然读地形）。
///
/// 只画**已知**效果：`catalog::cell_effect_key` 认不出的保持地形外观，不画成通用 `*`
/// ——"一个不知道是什么的符号"比不画更糟。
fn overlay_cell_effects(world: &mut World, view: &mut MapView, width: usize, height: usize) {
    let positions: Vec<(usize, usize, Entity)> = {
        let Some(index) = world.get_resource::<ecs_core::CellEffects>() else {
            return;
        };
        let mut out = Vec::new();
        for y in 0..height {
            for x in 0..width {
                if let Some(entity) = index.entity_at(x, y) {
                    out.push((x, y, entity));
                }
            }
        }
        out
    };
    if positions.is_empty() {
        return;
    }

    let mut costs = world.query::<&ecs_core::CellMoveCost>();
    for (x, y, entity) in positions {
        let Ok(cost) = costs.get(world, entity) else {
            continue;
        };
        if let Some(key) = crate::catalog::cell_effect_key(cost.move_cost) {
            view.set_tile(x, y, key);
        }
    }
}

/// 构造实体视图。
///
/// 玩家单独走 `SceneFrame.player`（后端通常要给它特殊样式），所以这里跳过玩家。
/// 视野外的实体只在"玩家当前看得见"时出现；记忆里的实体（`VisibleMemory`）
/// 本帧没有视图数据可用，一律不产出——宁可少画，不要画出过期位置。
fn build_entities(world: &mut World, config: &ExtractConfig) -> Vec<EntityView> {
    let (width, height) = config.world_size();
    let explored = collect_explored(world);
    // 玩家**当前看得见**的格子。怪物必须严格按它可见（SYN5：此前这条判据写错，
    // 导致所有怪物恒不可见，战斗系统对玩家完全不可感知）。
    let seen: std::collections::HashSet<(usize, usize)> = {
        let mut query = world.query_filtered::<&Viewshed, With<Player>>();
        query
            .iter(world)
            .next()
            .map(|viewshed| viewshed.visible_tiles.iter().copied().collect())
            .unwrap_or_default()
    };

    let mut query = world.query::<(
        Entity,
        &Position,
        Option<&Player>,
        Option<&Stairs>,
        Option<&MonsterKindId>,
        Option<&Health>,
        Option<&ecs_core::monster::Corpse>,
    )>();

    let mut entities: Vec<EntityView> = Vec::new();
    for (entity, position, is_player, is_stairs, kind, health, corpse) in query.iter(world) {
        if is_player.is_some() {
            continue;
        }
        let (x, y) = position.to_tuple();
        if x >= width || y >= height {
            // 世界尺寸之外的实体不画（测试用小世界时会出现）。
            continue;
        }

        let visual = if corpse.is_some() {
            // 尸体有**独立外观**：同类怪物死了要一眼看得出，不能还画成活怪。
            VisualKey::Corpse
        } else if is_stairs.is_some() {
            VisualKey::Stairs
        } else if let Some(kind) = kind {
            map_monster_kind(*kind)
        } else {
            VisualKey::Unknown(u32::from(crate::catalog::tile_id(ecs_core::Tile::Floor)))
        };

        let layer = if corpse.is_some() {
            // 尸体是地上的物件（不占格），要画在怪物下面。
            VisualLayer::Item
        } else if is_stairs.is_some() {
            VisualLayer::Terrain
        } else {
            VisualLayer::Actor
        };

        let name = if let Some(corpse) = corpse {
            corpse.display_name()
        } else if is_stairs.is_some() {
            "楼梯".to_string()
        } else if let Some(kind) = kind {
            monster_template(*kind).name.to_string()
        } else {
            "未知".to_string()
        };

        // 可见性**按实体类型分别裁定**：
        // - 楼梯是导航目标：探索过就画（否则玩家连往哪走都不知道，SYN7）；
        // - 尸体与地面物品同类：探索过就画（躺着不动，玩家需要记得它在哪）；
        // - 其它实体（怪物）严格按当前视野：看不见就不画，避免"隔着墙看到怪物"。
        // 旧代码把这两条合成一句 `is_stairs && explored`，等价于"只有楼梯可见"。
        let is_stairs = is_stairs.is_some();
        let visible = if is_stairs || corpse.is_some() {
            explored.contains(&(x, y))
        } else {
            seen.contains(&(x, y))
        };

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

    // 地面物品：物品用 `GroundItemAt` 而不是 `Position`（否则会挡住格子），
    // 所以它们不在上面的 `Position` 查询里，得单独扫一遍。
    //
    // 名字与外观都从**注册表**读（`Item::def()`）：物品编号是权威，
    // 这里不维护第二份"哪件物品长什么样"。
    {
        let mut query =
            world.query::<(Entity, &ecs_core::GroundItemAt, &ecs_core::Item, &ecs_core::Amount)>();
        let items: Vec<(Entity, (usize, usize), ecs_core::ItemId, u32)> = query
            .iter(world)
            .map(|(entity, at, item, amount)| (entity, at.to_tuple(), item.id, amount.0))
            .collect();
        for (entity, (x, y), id, count) in items {
            if x >= width || y >= height {
                continue;
            }
            let discoverable = explored.contains(&(x, y));
            entities.push(
                EntityView::new(
                    EntityId::from_bits(entity.to_bits()),
                    (x as i32, y as i32),
                    crate::catalog::map_item(id),
                )
                .with_name(item_label(id, count))
                .with_layer(VisualLayer::Item)
                .with_visibility(discoverable, false),
            );
        }
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
        extra_lines: {
            let mut lines = goal_line(world);
            lines.extend(player_status_lines(world));
            lines
        },
    }
}

/// "这一层该干什么"（ISSUES SYN7）：楼梯发现与否。
///
/// 在此之前玩家对目标**零信息**——楼梯探索过才画，而视野只有 10 格，
/// 于是开局完全不知道该往哪走。这里只给"发现/未发现"，不剧透坐标式导航。
fn goal_line(world: &mut World) -> Vec<render_api::UiTextLine> {
    let stairs_pos = {
        let mut query = world.query_filtered::<&Position, With<Stairs>>();
        query.iter(world).next().map(Position::to_tuple)
    };
    let Some((sx, sy)) = stairs_pos else {
        return vec![render_api::UiTextLine::plain("目标：本层没有楼梯")];
    };
    let explored = world
        .get_resource::<ecs_core::MapMemory>()
        .is_some_and(|memory| {
            memory
                .explored
                .get(sy)
                .and_then(|row| row.get(sx))
                .copied()
                .unwrap_or(false)
        });
    vec![render_api::UiTextLine::plain(if explored {
        format!("目标：楼梯已发现 ({sx}, {sy})")
    } else {
        "目标：楼梯未发现 —— 向未探索处走".to_string()
    })]
}

/// 玩家的状态行（C3 起：中毒）。挂在 `HudView::extra_lines` 上，由后端渲染。
///
/// **只读 `EffectOf` 那一组效果**：装备（`EquippedBy`）不在这里——它要显示成"装备"，
/// 不是"状态"。这条区分正是 DsnE12 用两个关系类型换来的。
fn player_status_lines(world: &mut World) -> Vec<render_api::UiTextLine> {
    let Some(player) = world
        .query_filtered::<Entity, With<Player>>()
        .iter(world)
        .next()
    else {
        return Vec::new();
    };
    // `OwnedEffects` 只在第一个孩子出现时才存在，所以是 `Option`；没有效果时
    // 其余状态行（装备、行囊载重）照样要画——早年这里直接 return，导致"没有 buff
    // 就看不出自己穿了什么"。
    let mut lines = Vec::new();
    if let Some(owned) = world.get::<ecs_core::OwnedEffects>(player) {
        let effects: Vec<Entity> = owned.iter().collect();
        for effect in effects {
            if let Some(poison) = world.get::<ecs_core::Poison>(effect) {
                // 中毒按 **AV** 计时（ISSUES ECS46），所以这里显示"还剩几次发作 + 下一次的 AV"，
                // 不显示"回合"——回合是玩家概念，而毒的时间轴是世界时间。
                lines.push(render_api::UiTextLine::plain(format!(
                    "中毒 还剩 {} 次（下一次 {:.0} AV）",
                    poison.ticks_left,
                    poison.remaining_av.max(0.0)
                )));
            }
        }
    }
    // 身上带着什么：玩家必须能看出"我现在穿着/背着什么"，否则捡到装备等于没发生
    // （实测：捡到踏水靴后 HUD 毫无变化，玩家无法判断自己是否变快了）。
    //
    // 读**身体容器**的槽位（DsnE18）：槽位名来自布局表，物品名来自注册表——
    // 渲染层不维护"哪个部位穿了什么"的第二份真相。
    if let Some(body) = world.get::<ecs_core::Container>(player) {
        let mut worn: Vec<(usize, &'static str, String)> = Vec::new();
        for (index, occupant) in body.iter() {
            let Some(item) = world.get::<ecs_core::Item>(occupant) else {
                continue;
            };
            // 只报"穿在身上的"，行囊里的东西是背包页的事。
            if !item.def().is_equipment() {
                continue;
            }
            worn.push((index, body.def().slots[index].name, item.def().name.to_string()));
        }
        if !worn.is_empty() {
            let joined = worn
                .iter()
                .map(|(_, slot, name)| format!("{slot}:{name}"))
                .collect::<Vec<_>>()
                .join("、");
            lines.push(render_api::UiTextLine::plain(format!("装备：{joined}")));
        }
    }
    // 行囊载重（DsnE20）：玩家得能看出还剩多少余量，否则"捡不起来"会显得莫名其妙。
    if let Some(body) = world.get::<ecs_core::Container>(player)
        && let Some(pack) = body
            .slot(ecs_core::item::pack_slot_index())
            .and_then(|pack| world.get::<ecs_core::Container>(pack))
    {
        let (weight, max_weight) = pack_load(world, pack);
        let text = match max_weight {
            Some(max) => format!("行囊载重：{weight:.1}/{max:.1}"),
            None => format!("行囊载重：{weight:.1}（不限重）"),
        };
        lines.push(render_api::UiTextLine::plain(text));
    }
    lines
}

/// 信息栏（DsnA4 / DsnE21 阶段 3）：只列**视野内、带 `Observable` 标记**的实体。
///
/// 排序：先按到玩家的距离从近到远；同距离按**方位顺时针**（正上方为 0，
/// 右为 1/4 圈，下为 1/2 圈，左为 3/4 圈）——这样"同一排的怪物"不会因实体
/// id 的生成顺序而跳动。选中行由 `PageStack.info_cursor` 给出，越界落在最后一行。
fn build_info(world: &mut World, cursor: usize) -> InfoView {
    let Some(player) = ecs_core::player_entity(world) else {
        return InfoView::default();
    };
    let Some(player_pos) = world.get::<Position>(player).map(Position::to_tuple) else {
        return InfoView::default();
    };
    let seen: std::collections::HashSet<(usize, usize)> = {
        let mut query = world.query_filtered::<&Viewshed, With<Player>>();
        query
            .iter(world)
            .next()
            .map(|viewshed| viewshed.visible_tiles.iter().copied().collect())
            .unwrap_or_default()
    };

    let mut candidates: Vec<(usize, f64, Entity)> = Vec::new();
    {
        let mut query = world.query_filtered::<(Entity, &Position), With<Observable>>();
        for (entity, position) in query.iter(world) {
            if entity == player {
                continue;
            }
            let at = position.to_tuple();
            if !seen.contains(&at) {
                continue;
            }
            let dx = at.0 as f64 - player_pos.0 as f64;
            let dy = at.1 as f64 - player_pos.1 as f64;
            // 屏幕坐标 y 向下：`-dy` 让"正上方"成为 0 度；`rem_euclid` 归一化到
            // [0, 2π)，于是顺时针方向角度递增。
            let clockwise = dx.atan2(-dy).rem_euclid(std::f64::consts::TAU);
            candidates.push((dx.mul_add(dx, dy * dy) as usize, clockwise, entity));
        }
    }
    candidates.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then(a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
    });

    let entries: Vec<EntityId> = candidates
        .iter()
        .map(|(_, _, entity)| EntityId::from_bits(entity.to_bits()))
        .collect();
    let selected = if entries.is_empty() {
        None
    } else {
        Some(cursor.min(entries.len() - 1))
    };
    InfoView { entries, selected }
}

/// 事件栏的一列（DsnE21）：已落定事实（旧→新）+ 进行中的行动（剩余时间降序）。
///
/// 进行中的行动**不另存状态**：玩家可见（等待输入）时自己的行动必然已完成，
/// 怪物名下的 `ActiveAction` 带着 `ActionTimer` 剩余 AV，本身就是"已起手、还没落"
/// 的唯一权威。行动实体被回收后这一行自然消失，不需要清理、也不进存档。
fn build_events(world: &mut World, config: &ExtractConfig) -> Vec<EventLine> {
    let mut events = build_done_events(world, config);

    // 只列玩家视野内的行动，避免远处怪物在事件栏里刷屏；过滤规则在后续阶段
    // 会扩展到"观察集 ∪ 重大事件"（DsnE21 决策 5 之前的一步）。
    let seen: std::collections::HashSet<(usize, usize)> = {
        let mut query = world.query_filtered::<&Viewshed, With<Player>>();
        query
            .iter(world)
            .next()
            .map(|viewshed| viewshed.visible_tiles.iter().copied().collect())
            .unwrap_or_default()
    };

    let mut pending: Vec<(f64, EventLine)> = Vec::new();
    {
        let mut query = world.query_filtered::<
            (&ecs_core::ActionOf, &ecs_core::ActionName, &ecs_core::ActionTimer),
            With<ecs_core::ActiveAction>,
        >();
        for (action_of, name, timer) in query.iter(world) {
            let actor = action_of.parent();
            let Some(position) = world.get::<Position>(actor) else {
                continue;
            };
            if !seen.contains(&position.to_tuple()) {
                continue;
            }
            let actor_name = actor_name_of(world, actor);
            let remaining = timer.remaining_av.max(0.0);
            let level = if name.0 == "BasicAttack" {
                LogLevel::Combat
            } else {
                LogLevel::System
            };
            let text = format!("{actor_name}{}（{remaining:.0}AV）", name.phrase());
            pending.push((remaining, EventLine::running(level, text)));
        }
    }
    // 剩余时间降序：剩得多的在上、越往下越接近"现在"，栏底就是 0 刻度线。
    pending.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    events.extend(pending.into_iter().map(|(_, line)| line));
    events
}

/// 事件文案里的行动者名字：玩家是"你"，其余用实体名。
fn actor_name_of(world: &World, actor: Entity) -> String {
    if world.get::<Player>(actor).is_some() {
        return "你".to_string();
    }
    world
        .get::<ecs_core::EntityName>(actor)
        .map(|name| name.0.clone())
        .unwrap_or_else(|| "???".to_string())
}

/// 已落定的事实（`EventLog`）。
fn build_done_events(world: &mut World, config: &ExtractConfig) -> Vec<EventLine> {
    let Some(log) = world.get_resource::<EventLog>() else {
        return Vec::new();
    };
    log.messages
        .iter()
        .rev()
        .take(config.log_lines)
        .rev()
        .map(|message| {
            // 合并过的重复消息在文本里带计数（ISSUES SYN8）：中毒这类高频消息
            // 不再一条条挤占事件栏。聚合是"怎么显示"的问题，属呈现层。
            let text = if message.repeat > 1 {
                format!("{} (×{})", message.text, message.repeat)
            } else {
                message.text.clone()
            };
            EventLine::done(map_log_level(message.level), text)
        })
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

/// 地面物品的显示名：`锈铁剑` / `生物血肉 ×3`。
///
/// 从注册表读名字（物品编号是权威），数量来自 `Amount`——**不在渲染层拼业务规则**，
/// 只是把两段数据念出来。
fn item_label(id: ecs_core::ItemId, count: u32) -> String {
    let name = id.def().name;
    if count > 1 {
        format!("{name} ×{count}")
    } else {
        name.to_string()
    }
}