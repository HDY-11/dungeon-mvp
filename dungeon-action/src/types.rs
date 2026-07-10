//! 行动系统类型定义（纯数据，无执行逻辑）
//!
//! 自 dungeon-core/action_types.rs 迁移至此。行动系统的领域类型应当
//! 定义在行动 crate 中，而非核心 crate。

use bevy_ecs::prelude::*;

// ══════════════════════════════════════════════════════
// 实体属性
// ══════════════════════════════════════════════════════

/// 反应时：从决策锁定到行动执行的延迟。
/// 由敏捷派生，敏捷越高反应越快（反应时越短）。
#[derive(Component, Clone, Debug)]
pub struct Reaction {
    pub time: f32,
}

/// 从敏捷推算反应时
pub fn agility_to_reaction(agility: u32) -> f32 {
    (100.0 - agility as f32 * 3.0).max(20.0)
}

/// 敏捷对耗时的修正系数：每点敏捷降低 2% 耗时，最高降低 50%。
/// 最终 AV = reaction_time + duration × speed_factor
pub fn agility_speed_factor(agility: u32) -> f32 {
    (1.0 - agility as f32 * 0.02).max(0.5)
}

// ══════════════════════════════════════════════════════
// Action 组件
// ══════════════════════════════════════════════════════

/// 移动行动
#[derive(Component, Clone, Debug)]
pub struct CanMove {
    pub duration: f32,
    pub priority: u32,
}

impl CanMove {
    pub fn new(priority: u32) -> Self {
        Self { duration: 300.0, priority }
    }
}

/// 追击行动（怪物专用）
#[derive(Component, Clone, Debug)]
pub struct CanChase {
    pub duration: f32,
    pub priority: u32,
}

impl CanChase {
    pub fn new(priority: u32) -> Self {
        Self { duration: 250.0, priority }
    }

    pub fn condition(can_see_player: bool) -> bool {
        can_see_player
    }
}

/// 逃跑行动（怪物专用）
#[derive(Component, Clone, Debug)]
pub struct CanFlee {
    pub duration: f32,
    pub priority: u32,
}

impl CanFlee {
    pub fn new(priority: u32) -> Self {
        Self { duration: 250.0, priority }
    }

    pub fn condition(hp_ratio: f32) -> bool {
        hp_ratio < 0.25
    }
}

/// 游荡行动（怪物专用）
#[derive(Component, Clone, Debug)]
pub struct CanWander {
    pub duration: f32,
    pub priority: u32,
}

impl CanWander {
    pub fn new(priority: u32) -> Self {
        Self { duration: 500.0, priority }
    }

    pub fn condition() -> bool {
        true
    }
}

/// 等待行动（玩家/怪物通用）
#[derive(Component, Clone, Debug)]
pub struct CanWait {
    pub duration: f32,
    pub priority: u32,
}

impl CanWait {
    pub fn new(priority: u32) -> Self {
        Self { duration: 800.0, priority }
    }

    pub fn condition() -> bool {
        true
    }
}

// ══════════════════════════════════════════════════════
// 行动队列
// ══════════════════════════════════════════════════════

#[derive(Clone, Debug)]
pub enum ActionKindV3 {
    Move { dx: isize, dy: isize },
    Chase,
    Flee,
    Wander,
    Wait,
    Attack { target: Entity },
    Skill(usize),
    Throw { tx: usize, ty: usize },
}

/// 行动队列条目
#[derive(Clone, Debug)]
pub struct ActionEntry {
    pub entity: Entity,
    pub kind: ActionKindV3,
    pub av_remaining: f32,
}

/// 全局行动队列（FIFO）
#[derive(Resource, Default)]
pub struct ActionQueue {
    pub entries: Vec<ActionEntry>,
}

impl ActionQueue {
    pub fn enqueue(&mut self, entity: Entity, kind: ActionKindV3, av: f32) {
        self.entries.push(ActionEntry { entity, kind, av_remaining: av });
    }

    pub fn advance(&mut self, amount: f32) {
        for entry in &mut self.entries {
            if entry.av_remaining > 0.0 {
                entry.av_remaining = (entry.av_remaining - amount).max(0.0);
            }
        }
    }

    pub fn next_event_distance(&self) -> Option<f32> {
        self.entries.iter()
            .filter(|e| e.av_remaining > 0.0)
            .map(|e| e.av_remaining)
            .min_by(|a, b| a.partial_cmp(b).expect("AV values should never be NaN"))
    }

    pub fn pop_ready(&mut self) -> Vec<ActionEntry> {
        let mut ready = Vec::new();
        self.entries.retain(|e| {
            if e.av_remaining <= 0.0 {
                ready.push(e.clone());
                false
            } else { true }
        });
        ready
    }

    pub fn has_entity(&self, entity: Entity) -> bool {
        self.entries.iter().any(|e| e.entity == entity)
    }

    pub fn enqueue_if_absent(&mut self, entity: Entity, kind: ActionKindV3, av: f32) {
        if !self.entries.iter().any(|e| e.entity == entity) {
            self.entries.push(ActionEntry { entity, kind, av_remaining: av });
        }
    }
}

// ══════════════════════════════════════════════════════
// 玩家行动枚举 + 键位绑定
// ══════════════════════════════════════════════════════

/// 玩家按键触发的所有行动。不包含怪物行为（Chase/Flee/Wander）。
/// 每个变体对应 process_key 中的一个处理路径。
#[derive(Clone, Debug, PartialEq)]
pub enum PlayerAction {
    // ── 直接行动（tap-tap 预览→入 AV 队列） ──
    Move(isize, isize),
    Wait,
    Skill(usize),
    Throw,
    // ── 模态行动（阻塞式 UI） ──
    OpenInventory,
    OpenLook,
    PickupGround,
    DescendStairs,
    SaveGame,
    LoadGame,
    Quit,
}

// ══════════════════════════════════════════════════════
// 输入管线（旧，逐步迁移至 PlayerAction）
// ══════════════════════════════════════════════════════

#[derive(Clone, Debug)]
pub enum RecognizedInput {
    Direction(isize, isize),
    Skill(usize),
    Wait,
    OpenBag,
    Quit,
    Confirm,
}

#[derive(Resource, Default)]
pub struct InputBuffer {
    pub buffer: Vec<RecognizedInput>,
}

impl InputBuffer {
    pub fn push(&mut self, input: RecognizedInput) {
        if self.buffer.len() >= 2 { self.buffer.remove(0); }
        if let Some(last) = self.buffer.last() {
            match (last, &input) {
                (RecognizedInput::Direction(ax, ay), RecognizedInput::Direction(bx, by))
                    if ax == bx && ay == by => return,
                _ => {}
            }
        }
        self.buffer.push(input);
    }

    pub fn pop(&mut self) -> Option<RecognizedInput> {
        if self.buffer.is_empty() { None } else { Some(self.buffer.remove(0)) }
    }
}

#[derive(Resource, Default)]
pub struct PlayerPreview {
    pub kind: Option<ActionKindV3>,
}

// ══════════════════════════════════════════════════════
// 怪物意图收集
// ══════════════════════════════════════════════════════

#[derive(Resource, Default)]
pub struct ChaseIntents(pub Vec<(Entity, u32, f32, ActionKindV3)>);

#[derive(Resource, Default)]
pub struct FleeIntents(pub Vec<(Entity, u32, f32, ActionKindV3)>);

#[derive(Resource, Default)]
pub struct WanderIntents(pub Vec<(Entity, u32, f32, ActionKindV3)>);
