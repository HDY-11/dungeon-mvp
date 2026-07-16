use crate::{MAP_HEIGHT, MAP_WIDTH};
use bevy_ecs::prelude::*;
use rand::SeedableRng;
// use serde::{Deserialize, Serialize};

// ── 资源定义 ───────────────────────────────────────

#[derive(Resource, Clone, Copy)]
pub struct MapMemory {
    pub explored: [[bool; MAP_WIDTH]; MAP_HEIGHT],
}
impl MapMemory {
    pub fn new() -> Self { Self { explored: [[false; MAP_WIDTH]; MAP_HEIGHT] } }
}
impl Default for MapMemory { fn default() -> Self { Self::new() } }

#[derive(Resource)]
pub struct GameRng { pub rng: rand::rngs::SmallRng }

impl GameRng {
    pub fn new(seed: u64) -> Self {
        Self { rng: rand::rngs::SmallRng::seed_from_u64(seed) }
    }

    /// 生成 [0, 1) 随机浮点
    pub fn random_f32(&mut self) -> f32 {
        use rand::RngExt;
        self.rng.random_range(0.0..1.0)
    }

    /// 生成 [lo, hi) 随机整数
    pub fn random_range(&mut self, lo: u8, hi: u8) -> u8 {
        use rand::RngExt;
        self.rng.random_range(lo..hi)
    }
}

#[derive(Resource, Default)]
pub struct PendingExp { pub amount: u64 }

// PendingSkill 已移除（技能通过 ActionQueue execute_skill 执行）
// PendingPickup 已移除（拾取由 main.rs pickup_ground 直接处理）

#[derive(Resource)]
pub struct EventLog {
    pub messages: Vec<String>,
    max: usize,
}
impl EventLog {
    pub fn new() -> Self { Self { messages: Vec::new(), max: 50 } }
    pub fn push(&mut self, msg: impl Into<String>) {
        self.messages.push(msg.into());
        if self.messages.len() > self.max { self.messages.remove(0); }
    }
}

#[derive(Resource)]
pub struct TurnManager {
    pub game_over: bool,
    pub wants_quit: bool,
}
impl Default for EventLog { fn default() -> Self { Self::new() } }
impl TurnManager {
    pub fn new() -> Self { Self { game_over: false, wants_quit: false } }
}
impl Default for TurnManager { fn default() -> Self { Self::new() } }

#[derive(Resource, Clone, Copy)]
pub struct FloorNumber(pub u32);

/// 地图种子（随机初始化，用于各楼层地图生成，使每次游戏地图不同）
#[derive(Resource, Clone, Copy)]
pub struct MapSeed(pub u64);


/// 最后看到的实体信息（用于视野外灰色显示）。
/// 实体离开视野后永久保留记忆，直到再次被看到或实体被销毁。
#[derive(Resource, Default)]
pub struct VisibleMemory {
    pub entries: std::collections::HashMap<Entity, (usize, usize, char, (u8, u8, u8))>,
}

/// 光标查看模式（按 x 激活，方向键移动，x/Esc 退出）
#[derive(Resource, Default)]
pub struct ThrowPreview {
    pub active: bool,
    pub cursor: (usize, usize),
    /// Bresenham 路径格（不含玩家，含目标），渲染用
    pub path: Vec<(usize, usize)>,
    /// 目标是否在射程且视线畅通
    pub valid_target: bool,
}

#[derive(Resource)]
pub struct LookCursor {
    pub active: bool,
    pub x: usize,
    pub y: usize,
}

/// 模态种类（仅标识，无数据——具体交互数据在各自的 World resource 里）
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModalKind {
    Look,
    Confirm { title: &'static str, on_yes: ConfirmAction },
    Inventory,
    ThrowSelect,
    ThrowAim,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfirmAction {
    Quit,
    Descend,
}

/// 模态请求信号（handler 设置 → 主循环消费 → subscribe）
#[derive(Resource, Default)]
pub struct ModalRequest {
    pub kind: Option<ModalKind>,
}

/// 模态活跃状态（渲染层读取，主循环维护）
#[derive(Resource, Default)]
pub struct ModalState {
    pub active_kind: Option<ModalKind>,
}

#[derive(Resource)]
pub struct OccupancyMap {
    pub cells: [[Option<Entity>; MAP_WIDTH]; MAP_HEIGHT],
}
impl OccupancyMap {
    pub fn new() -> Self { Self { cells: [[None; MAP_WIDTH]; MAP_HEIGHT] } }
    pub fn is_occupied(&self, x: usize, y: usize) -> bool {
        if x >= MAP_WIDTH || y >= MAP_HEIGHT { return true; }
        self.cells[y][x].is_some()
    }
    pub(crate) fn set(&mut self, x: usize, y: usize, entity: Entity) {
        if x < MAP_WIDTH && y < MAP_HEIGHT { self.cells[y][x] = Some(entity); }
    }
    pub(crate) fn clear(&mut self) { self.cells = [[None; MAP_WIDTH]; MAP_HEIGHT]; }
}
impl Default for OccupancyMap { fn default() -> Self { Self::new() } }
