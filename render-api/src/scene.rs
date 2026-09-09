//! 一帧场景快照。
//!
//! [`SceneFrame`] 是后端渲染所需的全部世界视图数据，由 `presentation`
//! 每帧从 ECS 世界提取。它是只读视图模型，不是第二套游戏状态：
//! 不包含规则、不包含可变引用、不持久化。

use crate::ui::{UiTextLine, UiView};
use crate::visual::{VisualKey, VisualLayer};
use bevy_ecs::prelude::*;

/// 实体在渲染快照中的稳定标识。
///
/// 通常由 `Entity::to_bits()` 转换而来，只在同一局 / 同一次运行内有意义；
/// 不要用于存档或跨进程同步。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct EntityId(pub u64);

impl EntityId {
    /// 从 Bevy `Entity` 的 bits 构造。
    pub const fn from_bits(bits: u64) -> Self {
        Self(bits)
    }

    /// 返回原始 bits。
    pub const fn to_bits(self) -> u64 {
        self.0
    }
}

/// 当前值 / 最大值。
///
/// 用于 HP、MP、EXP 等条状数值。数值统一使用 `f32`，因为契约的消费者是渲染后端；
/// `presentation` 负责把 `core` 的 `f64` 转换过来。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Meter {
    pub current: f32,
    pub max: f32,
}

impl Meter {
    pub const fn new(current: f32, max: f32) -> Self {
        Self { current, max }
    }

    /// 当前占比，范围 `[0, 1]`。`max <= 0` 时返回 `0`。
    pub fn ratio(&self) -> f32 {
        if self.max <= 0.0 {
            0.0
        } else {
            (self.current / self.max).clamp(0.0, 1.0)
        }
    }

    pub fn is_empty(&self) -> bool {
        self.current <= 0.0
    }

    pub fn is_full(&self) -> bool {
        self.max > 0.0 && self.current >= self.max
    }
}

/// 2D 相机。
///
/// `center` 是世界坐标（tile 为单位），`viewport` 是后端的表面尺寸
/// （TUI 为终端格子数，GPU 为像素数），`cells_per_unit` 是每个世界单位
/// 占多少表面单位（TUI 通常为 1.0，GPU 通常为 tile 像素尺寸）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera2D {
    pub center: (f32, f32),
    pub viewport: (u16, u16),
    pub cells_per_unit: f32,
}

impl Default for Camera2D {
    fn default() -> Self {
        Self {
            center: (0.0, 0.0),
            viewport: (0, 0),
            cells_per_unit: 1.0,
        }
    }
}

impl Camera2D {
    pub const fn new(center: (f32, f32), viewport: (u16, u16), cells_per_unit: f32) -> Self {
        Self {
            center,
            viewport,
            cells_per_unit,
        }
    }

    fn safe_scale(&self) -> f32 {
        if self.cells_per_unit.is_finite() && self.cells_per_unit > f32::EPSILON {
            self.cells_per_unit
        } else {
            1.0
        }
    }

    /// 当前可见的世界矩形。
    pub fn visible_rect(&self) -> WorldRect {
        let scale = self.safe_scale();
        let half_w = (self.viewport.0 as f32 / scale) * 0.5;
        let half_h = (self.viewport.1 as f32 / scale) * 0.5;
        WorldRect {
            min_x: self.center.0 - half_w,
            min_y: self.center.1 - half_h,
            max_x: self.center.0 + half_w,
            max_y: self.center.1 + half_h,
        }
    }

    /// 世界坐标 → 视口坐标（表面单位）。
    pub fn world_to_view(&self, x: f32, y: f32) -> (f32, f32) {
        let rect = self.visible_rect();
        let scale = self.safe_scale();
        ((x - rect.min_x) * scale, (y - rect.min_y) * scale)
    }

    /// 视口坐标（表面单位）→ 世界坐标。
    pub fn view_to_world(&self, vx: f32, vy: f32) -> (f32, f32) {
        let rect = self.visible_rect();
        let scale = self.safe_scale();
        (rect.min_x + vx / scale, rect.min_y + vy / scale)
    }

    /// 相机参数是否足以计算可见区域。
    pub fn is_valid(&self) -> bool {
        self.cells_per_unit.is_finite()
            && self.cells_per_unit > f32::EPSILON
            && self.viewport.0 > 0
            && self.viewport.1 > 0
    }
}

/// 世界坐标矩形。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WorldRect {
    pub min_x: f32,
    pub min_y: f32,
    pub max_x: f32,
    pub max_y: f32,
}

impl WorldRect {
    pub fn width(&self) -> f32 {
        self.max_x - self.min_x
    }

    pub fn height(&self) -> f32 {
        self.max_y - self.min_y
    }

    pub fn is_empty(&self) -> bool {
        self.width() <= 0.0 || self.height() <= 0.0
    }

    /// 是否包含世界坐标点。
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.min_x && x <= self.max_x && y >= self.min_y && y <= self.max_y
    }
}

/// 地图视图：行主序的 tile + 可见性 / 探索位图。
///
/// 80×60 的地图每帧约 4800 个 [`VisualKey`]，在 30fps 下完全可接受；
/// 未来若 GPU 后端需要，可以在此之上增加 chunk / dirty 区域，
/// 但不要提前优化。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MapView {
    pub width: usize,
    pub height: usize,
    pub tiles: Vec<VisualKey>,
    pub visible: Vec<bool>,
    pub explored: Vec<bool>,
}

impl MapView {
    pub fn new(width: usize, height: usize) -> Self {
        let len = width.saturating_mul(height);
        Self {
            width,
            height,
            tiles: vec![VisualKey::Unknown(0); len],
            visible: vec![false; len],
            explored: vec![false; len],
        }
    }

    pub fn is_empty(&self) -> bool {
        self.tiles.is_empty()
    }

    pub fn len(&self) -> usize {
        self.tiles.len()
    }

    pub fn in_bounds(&self, x: usize, y: usize) -> bool {
        x < self.width && y < self.height
    }

    /// 行主序索引。
    pub fn idx(&self, x: usize, y: usize) -> Option<usize> {
        self.in_bounds(x, y).then(|| y * self.width + x)
    }

    pub fn tile(&self, x: usize, y: usize) -> Option<VisualKey> {
        self.idx(x, y).and_then(|idx| self.tiles.get(idx)).copied()
    }

    pub fn is_visible(&self, x: usize, y: usize) -> bool {
        self.idx(x, y)
            .and_then(|idx| self.visible.get(idx))
            .copied()
            .unwrap_or(false)
    }

    pub fn is_explored(&self, x: usize, y: usize) -> bool {
        self.idx(x, y)
            .and_then(|idx| self.explored.get(idx))
            .copied()
            .unwrap_or(false)
    }

    pub fn set_tile(&mut self, x: usize, y: usize, key: VisualKey) -> bool {
        if let Some(idx) = self.idx(x, y)
            && let Some(slot) = self.tiles.get_mut(idx)
        {
            *slot = key;
            return true;
        }
        false
    }

    pub fn set_visible(&mut self, x: usize, y: usize, value: bool) -> bool {
        if let Some(idx) = self.idx(x, y)
            && let Some(slot) = self.visible.get_mut(idx)
        {
            *slot = value;
            return true;
        }
        false
    }

    pub fn set_explored(&mut self, x: usize, y: usize, value: bool) -> bool {
        if let Some(idx) = self.idx(x, y)
            && let Some(slot) = self.explored.get_mut(idx)
        {
            *slot = value;
            return true;
        }
        false
    }

    pub fn visible_count(&self) -> usize {
        self.visible.iter().filter(|value| **value).count()
    }

    pub fn explored_count(&self) -> usize {
        self.explored.iter().filter(|value| **value).count()
    }
}

/// 一个实体的渲染视图。
#[derive(Clone, Debug, PartialEq)]
pub struct EntityView {
    pub id: EntityId,
    pub position: (i32, i32),
    pub visual: VisualKey,
    pub layer: VisualLayer,
    pub name: String,
    pub hp: Option<Meter>,
    pub visible: bool,
    pub remembered: bool,
}

impl EntityView {
    pub fn new(id: EntityId, position: (i32, i32), visual: VisualKey) -> Self {
        Self {
            id,
            position,
            visual,
            layer: VisualLayer::Actor,
            name: String::new(),
            hp: None,
            visible: true,
            remembered: false,
        }
    }

    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    pub fn with_hp(mut self, hp: Meter) -> Self {
        self.hp = Some(hp);
        self
    }

    pub fn with_layer(mut self, layer: VisualLayer) -> Self {
        self.layer = layer;
        self
    }

    pub fn with_visibility(mut self, visible: bool, remembered: bool) -> Self {
        self.visible = visible;
        self.remembered = remembered;
        self
    }

    /// 是否存活。没有 HP 数据时视为存活（例如楼梯、装饰物）。
    pub fn is_alive(&self) -> bool {
        self.hp.is_none_or(|hp| !hp.is_empty())
    }
}

/// 地形详情，供 Look 页面使用。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TileInfo {
    pub visual: VisualKey,
    pub name: String,
    pub walkable: bool,
    pub blocks_sight: bool,
    pub visible: bool,
    pub explored: bool,
}

impl TileInfo {
    pub fn new(visual: VisualKey, name: impl Into<String>) -> Self {
        Self {
            visual,
            name: name.into(),
            walkable: false,
            blocks_sight: false,
            visible: false,
            explored: false,
        }
    }

    pub fn with_traversal(mut self, walkable: bool, blocks_sight: bool) -> Self {
        self.walkable = walkable;
        self.blocks_sight = blocks_sight;
        self
    }

    pub fn with_visibility(mut self, visible: bool, explored: bool) -> Self {
        self.visible = visible;
        self.explored = explored;
        self
    }
}

/// HUD 数据。
///
/// 只保存结构化数值；标签文案（“HP”“MP”“楼层”等）由各后端决定，
/// 契约层不承担本地化职责。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HudView {
    pub floor: u32,
    pub map_kind: String,
    pub player_name: String,
    pub level: u32,
    pub hp: Meter,
    pub mp: Meter,
    pub exp: Meter,
    pub attack: f32,
    pub defense: f32,
    /// 装备、技能、Buff 等扩展信息；物品 / 技能迁移后填充。
    pub extra_lines: Vec<UiTextLine>,
}

impl HudView {
    pub fn is_ready(&self) -> bool {
        !self.player_name.is_empty()
    }
}

/// 玩家可见日志的等级。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum LogLevel {
    Combat,
    Item,
    Skill,
    #[default]
    System,
    Danger,
    Debug,
}

/// 一条玩家可见日志。
#[derive(Clone, Debug, PartialEq)]
pub struct LogLine {
    pub level: LogLevel,
    pub text: String,
}

impl LogLine {
    pub fn new(level: LogLevel, text: impl Into<String>) -> Self {
        Self {
            level,
            text: text.into(),
        }
    }
}

/// 一帧场景快照。
///
/// `presentation` 每帧重建并写入 ECS；TUI / GPU 后端只读此资源。
/// `revision` 每次提取递增，后端可以用它跳过未变化的帧。
#[derive(Resource, Clone, Debug, Default)]
pub struct SceneFrame {
    pub revision: u64,
    pub camera: Camera2D,
    pub map: MapView,
    pub entities: Vec<EntityView>,
    pub player: Option<EntityView>,
    pub hud: HudView,
    pub log: Vec<LogLine>,
    pub ui: UiView,
    pub game_over: bool,
    pub quit_requested: bool,
}

impl SceneFrame {
    /// 空快照；后端在首帧或初始化完成前可以安全渲染它。
    pub fn empty() -> Self {
        Self::default()
    }

    /// 场景是否已经具备可渲染的世界数据。
    pub fn is_ready(&self) -> bool {
        !self.map.is_empty() || self.player.is_some()
    }

    /// 标记快照已更新。用 wrapping_add 避免极端情况下 panic。
    pub fn bump_revision(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    /// 所有可见实体（不含被视野外记忆实体；记忆状态由 `remembered` 区分）。
    pub fn visible_entities(&self) -> impl Iterator<Item = &EntityView> + '_ {
        self.entities.iter().filter(|entity| entity.visible)
    }

    /// 指定渲染层级的实体。
    pub fn entities_in_layer(&self, layer: VisualLayer) -> impl Iterator<Item = &EntityView> + '_ {
        self.entities
            .iter()
            .filter(move |entity| entity.layer == layer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_view_indexing_and_mutation() {
        let mut map = MapView::new(4, 3);
        assert_eq!(map.len(), 12);
        assert!(!map.is_empty());
        assert!(map.in_bounds(3, 2));
        assert!(!map.in_bounds(4, 2));
        assert_eq!(map.idx(2, 1), Some(6));
        assert_eq!(map.idx(4, 1), None);

        assert_eq!(map.tile(2, 1), Some(VisualKey::Unknown(0)));
        assert!(map.set_tile(2, 1, VisualKey::Tile(5)));
        assert_eq!(map.tile(2, 1), Some(VisualKey::Tile(5)));
        assert!(!map.set_tile(4, 1, VisualKey::Tile(5)));

        assert!(!map.is_visible(2, 1));
        assert!(map.set_visible(2, 1, true));
        assert!(map.is_visible(2, 1));
        assert_eq!(map.visible_count(), 1);

        assert!(!map.is_explored(2, 1));
        assert!(map.set_explored(2, 1, true));
        assert!(map.is_explored(2, 1));
        assert_eq!(map.explored_count(), 1);
    }

    #[test]
    fn empty_map_is_empty() {
        let map = MapView::default();
        assert!(map.is_empty());
        assert_eq!(map.len(), 0);
        assert_eq!(map.tile(0, 0), None);
    }

    #[test]
    fn meter_ratio_is_clamped() {
        assert_eq!(Meter::new(5.0, 10.0).ratio(), 0.5);
        assert_eq!(Meter::new(15.0, 10.0).ratio(), 1.0);
        assert_eq!(Meter::new(-1.0, 10.0).ratio(), 0.0);
        assert_eq!(Meter::new(1.0, 0.0).ratio(), 0.0);
        assert!(Meter::new(0.0, 10.0).is_empty());
        assert!(Meter::new(10.0, 10.0).is_full());
    }

    #[test]
    fn camera_visible_rect_and_round_trip() {
        let camera = Camera2D::new((10.0, 5.0), (80, 60), 1.0);
        let rect = camera.visible_rect();
        assert_eq!(rect.min_x, -30.0);
        assert_eq!(rect.max_x, 50.0);
        assert_eq!(rect.min_y, -25.0);
        assert_eq!(rect.max_y, 35.0);
        assert!(rect.contains(10.0, 5.0));
        assert!(!rect.contains(100.0, 5.0));

        let (vx, vy) = camera.world_to_view(10.0, 5.0);
        assert_eq!((vx, vy), (40.0, 30.0));
        let (wx, wy) = camera.view_to_world(vx, vy);
        assert!((wx - 10.0).abs() < f32::EPSILON);
        assert!((wy - 5.0).abs() < f32::EPSILON);
        assert!(camera.is_valid());
    }

    #[test]
    fn camera_with_zero_scale_does_not_panic() {
        let camera = Camera2D::new((0.0, 0.0), (10, 10), 0.0);
        assert!(!camera.is_valid());
        let rect = camera.visible_rect();
        assert!(rect.width() > 0.0);
    }

    #[test]
    fn entity_view_builder() {
        let entity = EntityView::new(EntityId::from_bits(7), (1, 2), VisualKey::Monster(3))
            .with_name("老鼠")
            .with_hp(Meter::new(3.0, 5.0))
            .with_layer(VisualLayer::Actor)
            .with_visibility(true, false);
        assert_eq!(entity.id.to_bits(), 7);
        assert_eq!(entity.name, "老鼠");
        assert!(entity.is_alive());
        assert!(entity.visible);
        assert!(!entity.remembered);
    }

    #[test]
    fn scene_frame_readiness_and_revision() {
        let mut frame = SceneFrame::empty();
        assert!(!frame.is_ready());
        assert_eq!(frame.revision, 0);
        frame.bump_revision();
        assert_eq!(frame.revision, 1);

        frame.map = MapView::new(2, 2);
        assert!(frame.is_ready());
    }

    #[test]
    fn scene_frame_filters_entities() {
        let visible = EntityView::new(EntityId(1), (0, 0), VisualKey::Player);
        let remembered = EntityView::new(EntityId(2), (1, 1), VisualKey::Monster(1))
            .with_visibility(false, true);
        let frame = SceneFrame {
            entities: vec![visible, remembered],
            ..SceneFrame::default()
        };
        assert_eq!(frame.visible_entities().count(), 1);
        assert_eq!(frame.entities_in_layer(VisualLayer::Actor).count(), 2);
    }

    #[test]
    fn tile_info_builder() {
        let info = TileInfo::new(VisualKey::Tile(1), "墙")
            .with_traversal(false, true)
            .with_visibility(true, true);
        assert_eq!(info.name, "墙");
        assert!(!info.walkable);
        assert!(info.blocks_sight);
    }

    #[test]
    fn hud_view_readiness() {
        let mut hud = HudView::default();
        assert!(!hud.is_ready());
        hud.player_name = "冒险者".to_string();
        assert!(hud.is_ready());
    }
}
