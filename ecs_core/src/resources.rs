//! 领域资源：与实体无关的全局状态。

use crate::map::{MAP_HEIGHT, MAP_WIDTH};
use bevy_ecs::prelude::*;
use std::collections::HashMap;

// ── 随机数 ───────────────────────────────────────────

/// 可序列化的 xorshift64* 随机源。
///
/// 所有游戏规则随机数都应走这个状态机，便于未来存档回放。
#[derive(Resource, Debug, Clone, PartialEq, Eq)]
pub struct GameRng {
    pub state: u64,
    pub steps: u64,
}

impl GameRng {
    pub fn new(seed: u64) -> Self {
        // splitmix64 混洗，避免简单种子退化。
        let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        Self {
            state: (z ^ (z >> 31)) | 1,
            steps: 0,
        }
    }

    pub const fn from_state(state: u64, steps: u64) -> Self {
        Self { state, steps }
    }

    /// 生成 `[0, 1)` 的 f64（消耗 1 步，53 位精度）。
    pub fn random_f64(&mut self) -> f64 {
        (self.xorshift_next() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// 生成 `[0, 1)` 的 f32（消耗 1 步，24 位精度）。
    pub fn random_f32(&mut self) -> f32 {
        (self.xorshift_next() >> 40) as f32 / (1u64 << 24) as f32
    }

    /// 生成 `[lo, hi)` 的随机整数（消耗 1 步；要求 `lo < hi`）。
    pub fn random_range(&mut self, lo: u64, hi: u64) -> u64 {
        debug_assert!(lo < hi);
        if hi <= lo {
            return lo;
        }
        lo + (self.xorshift_next() % (hi - lo))
    }

    fn xorshift_next(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        self.steps = self.steps.wrapping_add(1);
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
}

impl rand::rand_core::TryRng for GameRng {
    type Error = std::convert::Infallible;

    fn try_next_u32(&mut self) -> Result<u32, Self::Error> {
        Ok(self.xorshift_next() as u32)
    }

    fn try_next_u64(&mut self) -> Result<u64, Self::Error> {
        Ok(self.xorshift_next())
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), Self::Error> {
        let mut chunks = dest.chunks_exact_mut(8);
        for chunk in &mut chunks {
            chunk.copy_from_slice(&self.xorshift_next().to_le_bytes());
        }
        let rem = chunks.into_remainder();
        if !rem.is_empty() {
            let v = self.xorshift_next().to_le_bytes();
            rem.copy_from_slice(&v[..rem.len()]);
        }
        Ok(())
    }
}

// ── 地图/视野记忆 ───────────────────────────────────

#[derive(Resource, Clone, Copy)]
pub struct MapMemory {
    pub explored: [[bool; MAP_WIDTH]; MAP_HEIGHT],
}

impl MapMemory {
    pub fn new() -> Self {
        Self {
            explored: [[false; MAP_WIDTH]; MAP_HEIGHT],
        }
    }
}

impl Default for MapMemory {
    fn default() -> Self {
        Self::new()
    }
}

/// **本回合已被认领的格子**（ISSUES ECS45）。
///
/// `OccupancyMap` 是"每轮结算末尾重建"的派生快照，所以**一个回合内部**它落后于真实位置：
/// 同一轮里两个 actor 先后走向同一格，读到的都是"这格是空的"→ 两个实体叠在一格。
///
/// 这里补一层**批内预留**：移动执行时先把目标格认领掉，`clear()` 在每轮结算末尾
/// （跟着 `rebuild_occupancy_system`）归零。它不改"占用图是派生数据"这条性质
/// （DsnE2），只是让批内的检查能看到"这一格本回合已经被占了"。
#[derive(Resource)]
pub struct RoundClaims {
    cells: [[bool; MAP_WIDTH]; MAP_HEIGHT],
}

impl RoundClaims {
    pub fn new() -> Self {
        Self {
            cells: [[false; MAP_WIDTH]; MAP_HEIGHT],
        }
    }

    pub fn is_claimed(&self, x: usize, y: usize) -> bool {
        if x >= MAP_WIDTH || y >= MAP_HEIGHT {
            return true;
        }
        self.cells[y][x]
    }

    /// 认领一格；返回 `false` 表示**本回合已被别人认领**（调用方应放弃这一步）。
    pub fn claim(&mut self, x: usize, y: usize) -> bool {
        if x >= MAP_WIDTH || y >= MAP_HEIGHT {
            return false;
        }
        if self.cells[y][x] {
            return false;
        }
        self.cells[y][x] = true;
        true
    }

    pub fn clear(&mut self) {
        self.cells = [[false; MAP_WIDTH]; MAP_HEIGHT];
    }
}

impl Default for RoundClaims {
    fn default() -> Self {
        Self::new()
    }
}

/// 世界时钟：**AV 时间轴**。
///
/// 状态效果（中毒等）按它计时，而不是按"结算了多少轮"（ISSUES ECS46）——
/// 玩家的一次行动会跨越若干个世界轮，用轮次计时会让"毒多久"随场上怪物数量漂移。
///
/// 只在 `tick_action_timers_system` 里推进：那一轮推进了多少 AV，就是这一轮的时间流逝。
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct WorldClock {
    /// 累计推进的 AV（单调递增，可用于"效果从何时开始"之类的查询）。
    pub total_av: f64,
    /// 上一轮推进了多少 AV（`0` = 本轮没有行动到点，时间没走）。
    pub last_step_av: f64,
}

/// 最后看到的实体位置。实体离开视野后保留记忆，直到再次看见或实体销毁。
#[derive(Resource, Default)]
pub struct VisibleMemory {
    pub entries: HashMap<Entity, (usize, usize)>,
}

/// 碰撞占用图：每格被哪个实体占据。
#[derive(Resource)]
pub struct OccupancyMap {
    pub cells: [[Option<Entity>; MAP_WIDTH]; MAP_HEIGHT],
}

impl OccupancyMap {
    pub fn new() -> Self {
        Self {
            cells: [[None; MAP_WIDTH]; MAP_HEIGHT],
        }
    }

    pub fn is_occupied(&self, x: usize, y: usize) -> bool {
        if x >= MAP_WIDTH || y >= MAP_HEIGHT {
            return true;
        }
        self.cells[y][x].is_some()
    }

    pub fn entity_at(&self, x: usize, y: usize) -> Option<Entity> {
        if x >= MAP_WIDTH || y >= MAP_HEIGHT {
            return None;
        }
        self.cells[y][x]
    }

    pub fn set(&mut self, x: usize, y: usize, entity: Entity) {
        if x < MAP_WIDTH && y < MAP_HEIGHT {
            self.cells[y][x] = Some(entity);
        }
    }

    pub fn clear(&mut self) {
        self.cells = [[None; MAP_WIDTH]; MAP_HEIGHT];
    }
}

impl Default for OccupancyMap {
    fn default() -> Self {
        Self::new()
    }
}

// ── 日志 ─────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventLevel {
    Combat,
    Item,
    Skill,
    System,
    Danger,
}

#[derive(Debug, Clone)]
pub struct EventMessage {
    pub level: EventLevel,
    pub text: String,
    /// 同一条消息连续出现了几次（>1 时后端显示 `(×N)`）。
    ///
    /// 为什么要合并：中毒每个发作点推一条，一次玩家行动可能推 2–3 条，
    /// 直接把"变异蝎子倒下了""你捡起了踏水靴"这类一次性信息挤出日志区（ISSUES SYN8）。
    pub repeat: u32,
}

impl EventMessage {
    pub fn combat(text: impl Into<String>) -> Self {
        Self { level: EventLevel::Combat, text: text.into(), repeat: 1 }
    }
    pub fn item(text: impl Into<String>) -> Self {
        Self { level: EventLevel::Item, text: text.into(), repeat: 1 }
    }
    pub fn skill(text: impl Into<String>) -> Self {
        Self { level: EventLevel::Skill, text: text.into(), repeat: 1 }
    }
    pub fn system(text: impl Into<String>) -> Self {
        Self { level: EventLevel::System, text: text.into(), repeat: 1 }
    }
    pub fn danger(text: impl Into<String>) -> Self {
        Self { level: EventLevel::Danger, text: text.into(), repeat: 1 }
    }
}

#[derive(Resource)]
pub struct EventLog {
    pub messages: Vec<EventMessage>,
    max: usize,
}

impl EventLog {
    pub const fn new() -> Self {
        Self { messages: Vec::new(), max: 50 }
    }

    pub fn push(&mut self, msg: EventMessage) {
        // 连续相同的消息合并成一条并计数，而不是一条条堆进日志（ISSUES SYN8）。
        if let Some(last) = self.messages.last_mut()
            && last.level == msg.level
            && last.text == msg.text
        {
            last.repeat = last.repeat.saturating_add(1);
            return;
        }
        self.messages.push(msg);
        if self.messages.len() > self.max {
            self.messages.remove(0);
        }
    }
}

impl Default for EventLog {
    fn default() -> Self {
        Self::new()
    }
}

// ── 世界状态 ─────────────────────────────────────────

#[derive(Resource)]
pub struct TurnManager {
    pub game_over: bool,
    pub wants_quit: bool,
}

impl TurnManager {
    pub const fn new() -> Self {
        Self { game_over: false, wants_quit: false }
    }
}

impl Default for TurnManager {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Resource, Clone, Copy)]
pub struct FloorNumber(pub u32);

#[derive(Resource, Clone, Copy)]
pub struct MapSeed(pub u64);
