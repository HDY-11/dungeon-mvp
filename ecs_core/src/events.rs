//! 领域事件。
//!
//! 事件是系统之间的意图/结果传递机制；不要在事件中直接执行世界变更。

use bevy_ecs::prelude::*;

/// 一次已确认的普通攻击意图。伤害尚未计算。
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttackIntentEvent {
    pub attacker: Entity,
    pub target: Entity,
}

/// 一次已结算的普通攻击结果。
///
/// `damage` 是已经过攻击/防御/暴击计算后的最终伤害。
#[derive(Message, Debug, Clone, Copy, PartialEq)]
pub struct AttackEvent {
    pub attacker: Entity,
    pub target: Entity,
    pub damage: f64,
    pub is_crit: bool,
}

/// 一次伤害已经落到目标身上（DsnE29）。
///
/// 与 [`AttackEvent`]（"近战这一次打出的结果"）不同，这是**所有伤害来源的统一
/// 出口**：近战、法术、投掷都发；将来的环境伤害/持续伤害也应走这里。仇恨系统只
/// 订阅这一个事件，因此不会随伤害来源增加而改。
///
/// `source` 可以为空（陷阱、毒、坠落等无施害者的伤害），此时不产生仇恨。
#[derive(Message, Debug, Clone, Copy, PartialEq)]
pub struct DamageTakenEvent {
    pub source: Option<Entity>,
    pub target: Entity,
    pub amount: f64,
}

/// 实体死亡。
///
/// **携带经验奖励**是刻意的：奖励数值只有死亡那一刻能拿到——`check_death_system`
/// 发完事件就 `despawn` 实体。若不带，经验系统就只能反过来在一张"旁路表"里
/// 找奖励（旧实现的 `PendingExp` 资源），于是同一件事同时存在事件与旁路两条路
/// （REFACTOR.md §10.8：`PendingExp` 是绕过 `DeathEvent` 的旁路，已删除）。
///
/// 消费者：`system::experience::apply_exp_system`（只把奖励给玩家）。
#[derive(Message, Debug, Clone, Copy, PartialEq)]
pub struct DeathEvent {
    pub entity: Entity,
    /// 该实体死亡时应给玩家的经验；无奖励的实体（例如玩家自己）为 0。
    pub reward: f64,
}

/// 行动执行成功。
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionSucceededEvent {
    pub entity: Entity,
}

/// 一次已确认的施法意图（DsnE26）。法力扣除与效果结算都在结算链路里，不在落地器里。
///
/// 与 [`AttackIntentEvent`] 同构：落地阶段只发"我要对谁放什么"，具体伤害/治疗
/// 由 `spell::resolve_spell_system` 按规则计算——战斗公式（DsnE9）与法术公式
/// 都不认识"落地"这件事。
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpellIntentEvent {
    pub caster: Entity,
    pub spell: crate::spell::SpellId,
    pub target: Entity,
}

/// 一次已确认的投掷意图（Gm9）。消耗投掷物、伤害结算与落地都在结算链路里。
///
/// 与 [`AttackIntentEvent`] / [`SpellIntentEvent`] 同构：落地器只发"谁往哪格扔"，
/// 命中判定（落点格上有没有活物）与伤害公式都不在落地阶段。
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThrowIntentEvent {
    pub thrower: Entity,
    /// 落点格（世界坐标）；落点必中，即使没有活物也照常落地。
    pub target: (usize, usize),
}

/// 行动执行失败（保活条件不再成立）。
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionFailedEvent {
    pub entity: Entity,
}

/// 玩家升级（可能一轮多次）。
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct LevelUpEvent {
    pub entity: Entity,
    pub new_level: u64,
}
