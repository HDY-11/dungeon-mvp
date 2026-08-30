//! ECS 全局资源：地图记忆、可见记忆、事件日志、行动队列、RNG、UI 状态等。

use crate::components::RenderableView;
use crate::{MAP_HEIGHT, MAP_WIDTH};
use bevy_ecs::prelude::*;
// use serde::{Deserialize, Serialize};

// ── 资源定义 ───────────────────────────────────────

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

/// 统一随机源（G32：状态可序列化——存档/读档精确恢复，杜绝 SL 刷掉落）
///
/// 实现 xorshift64*（经 splitmix64 混洗种子），并实现 `rand::RngCore`——
/// 所有随机消耗（暴击/掉落/游荡）都经过同一状态机，每个 `next_u64` 计一步。
/// 相比 SmallRng：内部状态仅 2 个 u64，可直接存入存档。
#[derive(Resource, Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GameRng {
    pub state: u64,
    pub steps: u64,
}

impl GameRng {
    pub fn new(seed: u64) -> Self {
        // splitmix64 混洗：避免全零/线性种子的劣化状态
        let mut z = seed.wrapping_add(0x9E3779B97F4A7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        Self {
            state: (z ^ (z >> 31)) | 1,
            steps: 0,
        }
    }

    /// 从序列化状态精确恢复（G32 读档）
    pub fn from_state(state: u64, steps: u64) -> Self {
        Self { state, steps }
    }

    /// 生成 [0, 1) 随机浮点（固定消耗 1 步，24 位精度）
    pub fn random_f32(&mut self) -> f32 {
        (self.xorshift_next() >> 40) as f32 / (1u64 << 24) as f32
    }

    /// 生成 [lo, hi) 随机整数（固定消耗 1 步）
    pub fn random_range(&mut self, lo: u8, hi: u8) -> u8 {
        (self.xorshift_next() % (hi.wrapping_sub(lo) as u64)) as u8 + lo
    }

    /// xorshift64* 单步：输出经乘法混洗消除低位线性依赖
    fn xorshift_next(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        self.steps = self.steps.wrapping_add(1);
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }
}

impl rand::rand_core::TryRng for GameRng {
    type Error = core::convert::Infallible;

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

#[derive(Resource, Default)]
pub struct PendingExp {
    pub amount: u64,
}

// PendingSkill 已移除（技能通过 ActionQueue execute_skill 执行）
// PendingPickup 已移除（拾取由 main.rs pickup_ground 直接处理）

/// 事件级别 —— 渲染时按级别着色
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EventLevel {
    Combat, // 战斗（红）
    Item,   // 物品（黄）
    Skill,  // 技能（青）
    System, // 系统（灰）
    Danger, // 危险（亮红）
}

/// 结构化的事件消息
#[derive(Clone, Debug)]
pub struct EventMessage {
    pub level: EventLevel,
    pub text: String,
}

impl EventMessage {
    pub fn combat(text: impl Into<String>) -> Self {
        Self {
            level: EventLevel::Combat,
            text: text.into(),
        }
    }
    pub fn item(text: impl Into<String>) -> Self {
        Self {
            level: EventLevel::Item,
            text: text.into(),
        }
    }
    pub fn skill(text: impl Into<String>) -> Self {
        Self {
            level: EventLevel::Skill,
            text: text.into(),
        }
    }
    pub fn system(text: impl Into<String>) -> Self {
        Self {
            level: EventLevel::System,
            text: text.into(),
        }
    }
    pub fn danger(text: impl Into<String>) -> Self {
        Self {
            level: EventLevel::Danger,
            text: text.into(),
        }
    }
}

#[derive(Resource)]
pub struct EventLog {
    pub messages: Vec<EventMessage>,
    max: usize,
}
impl EventLog {
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
            max: 50,
        }
    }
    /// 推送一条结构化消息。
    /// 同时自动转发到 `log` crate（开发者日志），按级别选择 `info!` 或 `warn!`。
    pub fn push(&mut self, msg: EventMessage) {
        match msg.level {
            EventLevel::Danger => log::warn!("[玩家] {}", msg.text),
            _ => log::info!("[玩家] {}", msg.text),
        }
        self.messages.push(msg);
        if self.messages.len() > self.max {
            self.messages.remove(0);
        }
    }
}

#[derive(Resource)]
pub struct TurnManager {
    pub game_over: bool,
    pub wants_quit: bool,
}
impl Default for EventLog {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_message_constructors() {
        let msg = EventMessage::combat("打中");
        assert_eq!(msg.level, EventLevel::Combat);
        assert_eq!(msg.text, "打中");

        let msg = EventMessage::item("拾取");
        assert_eq!(msg.level, EventLevel::Item);

        let msg = EventMessage::skill("施法");
        assert_eq!(msg.level, EventLevel::Skill);

        let msg = EventMessage::system("下楼");
        assert_eq!(msg.level, EventLevel::System);

        let msg = EventMessage::danger("快跑");
        assert_eq!(msg.level, EventLevel::Danger);
    }

    #[test]
    fn test_event_log_push_and_retrieve() {
        let mut log = EventLog::new();
        assert_eq!(log.messages.len(), 0);

        log.push(EventMessage::combat("战斗日志"));
        assert_eq!(log.messages.len(), 1);
        assert_eq!(log.messages[0].level, EventLevel::Combat);
        assert_eq!(log.messages[0].text, "战斗日志");
    }

    #[test]
    fn test_event_log_max_capacity() {
        let mut log = EventLog {
            messages: Vec::new(),
            max: 3,
        };
        log.push(EventMessage::system("1"));
        log.push(EventMessage::system("2"));
        log.push(EventMessage::system("3"));
        assert_eq!(log.messages.len(), 3);
        assert_eq!(log.messages[0].text, "1");

        // 第 4 条应挤出第 1 条
        log.push(EventMessage::system("4"));
        assert_eq!(log.messages.len(), 3);
        assert_eq!(log.messages[0].text, "2");
        assert_eq!(log.messages[2].text, "4");
    }

    #[test]
    fn test_event_log_rev_iter() {
        let mut log = EventLog {
            messages: Vec::new(),
            max: 10,
        };
        log.push(EventMessage::system("A"));
        log.push(EventMessage::system("B"));
        log.push(EventMessage::system("C"));

        let rev: Vec<&str> = log.messages.iter().rev().map(|m| m.text.as_str()).collect();
        assert_eq!(rev, vec!["C", "B", "A"]);
    }

    #[test]
    fn test_event_log_take_12() {
        let mut log = EventLog::new();
        for i in 0..20 {
            log.push(EventMessage::system(format!("msg_{}", i)));
        }
        assert_eq!(log.messages.len(), 20);
        let last_12: Vec<&str> = log
            .messages
            .iter()
            .rev()
            .take(12)
            .map(|m| m.text.as_str())
            .collect();
        assert_eq!(last_12.len(), 12);
        assert_eq!(last_12[0], "msg_19");
    }
}
impl TurnManager {
    pub fn new() -> Self {
        Self {
            game_over: false,
            wants_quit: false,
        }
    }
}
impl Default for TurnManager {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Resource, Clone, Copy)]
pub struct FloorNumber(pub u32);

/// 地图种子（随机初始化，用于各楼层地图生成，使每次游戏地图不同）
#[derive(Resource, Clone, Copy)]
pub struct MapSeed(pub u64);

/// 最后看到的实体信息（用于视野外灰色显示）。
/// 实体离开视野后永久保留记忆，直到再次被看到或实体被销毁。
#[derive(Resource, Default)]
pub struct VisibleMemory {
    pub entries: std::collections::HashMap<Entity, RenderableView>,
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

/// 背包 UI 状态（被页栈 + 管道消费）
#[derive(Resource, Default)]
pub struct InventoryUI {
    pub active: bool,
    pub panel: bool, // false=Left, true=Right
    pub left_sel: usize,
    pub right_sel: usize,
    pub detail: bool,
    pub detail_source: usize, // 0=LeftInv, 1=LeftEquip, 2=Right
    pub detail_idx: usize,
}

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
    pub(crate) fn set(&mut self, x: usize, y: usize, entity: Entity) {
        if x < MAP_WIDTH && y < MAP_HEIGHT {
            self.cells[y][x] = Some(entity);
        }
    }
    pub(crate) fn clear(&mut self) {
        self.cells = [[None; MAP_WIDTH]; MAP_HEIGHT];
    }
}
impl Default for OccupancyMap {
    fn default() -> Self {
        Self::new()
    }
}
