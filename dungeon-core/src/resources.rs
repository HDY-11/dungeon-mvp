use crate::{MAP_HEIGHT, MAP_WIDTH};
use bevy_ecs::prelude::*;
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

#[derive(Resource, Default)]
pub struct PendingExp { pub amount: u64 }

#[derive(Resource, Default)]
pub struct PendingPickup { pub entries: Vec<(Entity, crate::ItemInstance)> }

#[derive(Resource, Default)]
pub struct PendingSkill { pub idx: Option<usize> }

#[derive(Resource)]
pub struct EventLog {
    pub messages: Vec<String>,
    max: usize,
}
impl EventLog {
    pub fn new() -> Self { Self { messages: Vec::new(), max: 10 } }
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
impl TurnManager {
    pub fn new() -> Self { Self { game_over: false, wants_quit: false } }
}

#[derive(Resource, Clone, Copy)]
pub struct FloorNumber(pub u32);

#[derive(Resource, Default)]
pub struct PendingLevelUp { pub points: u32 }

/// 玩家行动轴预览 — 战斗档中，玩家当前选中的待确认行动
#[derive(Resource, Clone)]
pub struct PendingPlayerAction {
    /// 显示的描述
    pub action_name: String,
    /// 是否为技能的待确认状态
    pub is_pending_skill: bool,
    pub skill_idx: Option<usize>,
    /// 基础 AV 成本
    pub action_cost: f32,
}
impl Default for PendingPlayerAction {
    fn default() -> Self { Self {
        action_name: "移动".into(),
        is_pending_skill: false,
        skill_idx: None,
        action_cost: crate::action_cost::MOVE,
    } }
}
impl PendingPlayerAction {
    pub fn new_skill(idx: usize, name: &str) -> Self { Self {
        action_name: format!("技能:{}", name),
        is_pending_skill: true,
        skill_idx: Some(idx),
        action_cost: crate::action_cost::SKILL_CAST,
    } }
}

// ── 游戏节奏 ───────────────────────────────────────

/// 手动/自动覆盖状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ManualOverride {
    /// 跟随战斗状态（探索=自动，战斗=手动）
    None,
    /// 强制手动模式（所有场景都需要 Enter 确认）
    ForceManual,
    /// 强制自动模式（方向键立即提交）
    ForceAuto,
}

#[derive(Resource)]
pub struct GamePacing {
    pub mode: PacingMode,
    pub combat_active: bool,
    /// 手动/自动覆盖。None 时跟随 combat_active 自动切换。
    pub manual_override: ManualOverride,
    pub blink_phase: bool,
}
impl Default for GamePacing {
    fn default() -> Self { Self {
        mode: PacingMode::Exploration,
        combat_active: false,
        manual_override: ManualOverride::None,
        blink_phase: false,
    } }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PacingMode {
    /// 漫游：输入立即响应，行动轴后台跑
    Exploration,
    /// 战斗暂停：等待玩家读轴 + 选动作 + Enter 确认
    CombatPaused,
    /// 战斗推进中：调度器连续运行直到遇到战斗事件
    CombatRunning,
}

// ── 单槽暂存输入 ───────────────────────────────────

#[derive(Resource, Default)]
pub struct PendingInput {
    /// 最近一次未被处理的方向键
    pub direction: Option<(isize, isize)>,
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
