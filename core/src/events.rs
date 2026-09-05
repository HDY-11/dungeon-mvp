//! 领域事件。
//!
//! 事件是系统之间的意图/结果传递机制；不要在事件中直接执行世界变更。

use bevy_ecs::prelude::*;

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
/// 死亡系统应消费该事件执行清理；清理逻辑不放在伤害系统中。
#[derive(Event, Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeathEvent {
    pub entity: Entity,
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

/// 玩家升级。
#[derive(Event, Debug, Clone, Copy, PartialEq, Eq)]
pub struct LevelUpEvent {
    pub entity: Entity,
    pub new_level: u64,
}

/// 仇恨事件。本轮只预留接口，暂不接入完整仇恨算法。
#[derive(Event, Debug, Clone, Copy, PartialEq)]
pub struct ThreatEvent {
    pub source: Entity,
    pub target: Entity,
    pub amount: f64,
    pub reason: ThreatReason,
}

/// 仇恨来源类型。后续仇恨系统按此加权。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreatReason {
    Damage,
    Sight,
    Noise,
    Heal,
    Proximity,
}
