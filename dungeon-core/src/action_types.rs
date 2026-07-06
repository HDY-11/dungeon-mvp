//! 行动系统类型定义（纯数据，无执行逻辑）
//!
//! 按关注点划分归入 dungeon-core，因为它们是"游戏中有什么行动"的数据描述。
//! 执行逻辑在 dungeon-action crate 中。

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

// ══════════════════════════════════════════════════════
// Action 组件
// ══════════════════════════════════════════════════════
//
// 每个 Action 组件包含：
//   - duration: 该行动的耗时
//   - priority: 仲裁优先级
//
// AV = reaction_time + duration，作为单一值入队倒计时。
// 反应时（reaction_time）不在此处——它是实体的属性（见 Reaction 组件）。

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

    pub fn condition(target_is_walkable: bool, target_is_occupied_by_enemy: bool) -> bool {
        target_is_walkable || target_is_occupied_by_enemy
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
}

/// 行动队列条目
#[derive(Clone, Debug)]
pub struct ActionEntry {
    pub entity: Entity,
    pub kind: ActionKindV3,
    /// AV 剩余 = reaction_time + duration，单一倒计时
    pub av_remaining: f32,
}

/// 全局行动队列（FIFO）
#[derive(Resource)]
pub struct ActionQueue {
    pub entries: Vec<ActionEntry>,
}

impl Default for ActionQueue {
    fn default() -> Self { Self { entries: Vec::new() } }
}

impl ActionQueue {
    /// 入队：av = reaction_time + duration
    pub fn enqueue(&mut self, entity: Entity, kind: ActionKindV3, av: f32) {
        self.entries.push(ActionEntry {
            entity,
            kind,
            av_remaining: av,
        });
    }

    pub fn advance(&mut self, amount: f32) {
        for entry in &mut self.entries {
            if entry.av_remaining > 0.0 {
                entry.av_remaining = (entry.av_remaining - amount).max(0.0);
            }
        }
    }

    /// 找最小正 av_remaining（= 下一次事件的距离）
    pub fn next_event_distance(&self) -> Option<f32> {
        self.entries
            .iter()
            .filter(|e| e.av_remaining > 0.0)
            .map(|e| e.av_remaining)
            .min_by(|a, b| a.partial_cmp(b).unwrap())
    }

    /// 取出所有 av_remaining ≤ 0 的条目
    pub fn pop_ready(&mut self) -> Vec<ActionEntry> {
        let mut ready = Vec::new();
        self.entries.retain(|e| {
            if e.av_remaining <= 0.0 {
                ready.push(e.clone());
                false
            } else {
                true
            }
        });
        ready
    }

    /// 检查实体是否已在队列中
    pub fn has_entity(&self, entity: Entity) -> bool {
        self.entries.iter().any(|e| e.entity == entity)
    }

    /// 入队或跳过：如果实体已在队列中，忽略（保留已有行动的 av）
    pub fn enqueue_if_absent(&mut self, entity: Entity, kind: ActionKindV3, av: f32) {
        if !self.entries.iter().any(|e| e.entity == entity) {
            self.entries.push(ActionEntry { entity, kind, av_remaining: av });
        }
    }
}

// ══════════════════════════════════════════════════════
// 输入管线
// ══════════════════════════════════════════════════════

/// 已识别但未消费的玩家输入
#[derive(Clone, Debug)]
pub enum RecognizedInput {
    Direction(isize, isize),
    Skill(usize),
    Wait,
    OpenBag,
    Quit,
    Confirm,
}

/// 缓冲区
#[derive(Resource, Default)]
pub struct InputBuffer {
    /// 已识别待消费的输入
    pub buffer: Vec<RecognizedInput>,
}

impl InputBuffer {
    pub fn push(&mut self, input: RecognizedInput) {
        if self.buffer.len() >= 2 {
            self.buffer.remove(0);
        }
        // 去重：连续相同方向只保留一个
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
        if self.buffer.is_empty() {
            None
        } else {
            Some(self.buffer.remove(0))
        }
    }
}

/// 玩家预览态
#[derive(Resource)]
pub struct PlayerPreview {
    pub kind: Option<ActionKindV3>,
}

impl Default for PlayerPreview {
    fn default() -> Self { Self { kind: None } }
}

// ══════════════════════════════════════════════════════
// 怪物意图收集（用于并行决策）
// ══════════════════════════════════════════════════════

/// 追击意图缓冲区（chase_decision_system 写入，arbitration_system 读取）
#[derive(Resource, Default)]
pub struct ChaseIntents(pub Vec<(Entity, u32, f32, ActionKindV3)>);

/// 逃跑意图缓冲区（flee_decision_system 写入，arbitration_system 读取）
#[derive(Resource, Default)]
pub struct FleeIntents(pub Vec<(Entity, u32, f32, ActionKindV3)>);

/// 游荡意图缓冲区（wander_decision_system 写入，arbitration_system 读取）
#[derive(Resource, Default)]
pub struct WanderIntents(pub Vec<(Entity, u32, f32, ActionKindV3)>);
