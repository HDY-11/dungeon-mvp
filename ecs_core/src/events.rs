//! 领域事件。
//!
//! 事件是系统之间的意图/结果传递机制；不要在事件中直接执行世界变更。

use bevy_ecs::prelude::*;

/// 一次已确认的普通攻击意图。伤害尚未计算。
#[derive(Event, Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttackIntentEvent {
    pub attacker: Entity,
    pub target: Entity,
}

/// 一次已结算的普通攻击结果。
///
/// `damage` 是已经过攻击/防御/暴击计算后的最终伤害。
#[derive(Event, Debug, Clone, Copy, PartialEq)]
pub struct AttackEvent {
    pub attacker: Entity,
    pub target: Entity,
    pub damage: f64,
    pub is_crit: bool,
}

/// 实体死亡。
///
/// **携带经验奖励**是刻意的：奖励数值只有死亡那一刻能拿到——`check_death_system`
/// 发完事件就 `despawn` 实体。若不带，经验系统就只能反过来在一张"旁路表"里
/// 找奖励（旧实现的 `PendingExp` 资源），于是同一件事同时存在事件与旁路两条路
/// （REFACTOR.md §10.8：`PendingExp` 是绕过 `DeathEvent` 的旁路，已删除）。
///
/// 消费者：`system::experience::apply_exp_system`（只把奖励给玩家）。
#[derive(Event, Debug, Clone, Copy, PartialEq)]
pub struct DeathEvent {
    pub entity: Entity,
    /// 该实体死亡时应给玩家的经验；无奖励的实体（例如玩家自己）为 0。
    pub reward: f64,
}

/// 行动执行成功。
#[derive(Event, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionSucceededEvent {
    pub entity: Entity,
}

/// 行动执行失败（保活条件不再成立）。
#[derive(Event, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionFailedEvent {
    pub entity: Entity,
}

/// 玩家升级（可能一轮多次）。
#[derive(Event, Debug, Clone, Copy, PartialEq, Eq)]
pub struct LevelUpEvent {
    pub entity: Entity,
    pub new_level: u64,
}
