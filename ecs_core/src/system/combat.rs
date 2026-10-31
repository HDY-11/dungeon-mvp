//! 伤害结算与扣血。
//!
//! 攻击的**落地**在 `action::entity::settle_action_system`（校验阶段已定下
//! `AttackTarget`）：它只声明「谁打谁」（`AttackIntentEvent`），不算伤害、不扣血。
//! 这里负责把意图变成结果，于是「伤害只结算一次」这件事只有一个地方需要保证（I90）。

use crate::combat::compute_melee_damage;
use crate::components::*;
use crate::effect::EffectContext;
use crate::events::{AttackEvent, AttackIntentEvent};
use crate::resources::{EventLog, EventMessage, GameRng};
use crate::rules::{Stat, apply_modifiers};
use bevy_ecs::prelude::*;

/// 消费 `AttackIntentEvent`，计算最终伤害并写 `AttackEvent`。
///
/// 参数偏多（8 个）是 ECS 系统的固有形状——`EventReader` / `Query` / `ResMut`
/// 每类都是一个独立入参，折成 DTO 会丢掉 Bevy 的参数校验（「可变访问是否冲突」
/// 这类错误会从编译期推迟到运行期）。这里保留显式签名。
///
/// 顺带记录一条实测结论：把这四个查询抽成 `type CombatQueries = (Query<..>, ..)`
/// **不行**——`Query<'w, 's, D, F>` 的 `'w`/`'s` 必须由系统参数推导，而 `type`
/// 别名里没有 `'w`/`'s` 可写（`Query<&'static Attack>` 会把 `'w`/`'s` 固定死，
/// 报 `not a valid SystemParam`）。要抽就只能抽成 `#[derive(SystemParam)]` 结构体。
///
/// # 装备加成在哪一步生效
///
/// 读的是**有效**攻击/防御：`Attack` 组件是基础值，装备带来的修正由
/// [`EffectContext::collect_actor`] 收成一份 `&[Modifier]`，再按 [`Stat::Attack`] /
/// [`Stat::Defense`] 折叠（求值五步的第 ②–⑤ 步）。所以"一把 +3 的剑"与"一条 +3 的
/// buff"在规则层完全同构，这里不需要知道加成从哪来。
///
/// 与格子无关：攻击时玩家可能站在减速格上，但那影响的是**节奏**（AV），不是伤害，
/// 所以这里用 `collect_actor` 而不是带格子的 `collect`。
#[allow(clippy::too_many_arguments)]
pub fn resolve_attack_system(
    mut attack_intents: EventReader<AttackIntentEvent>,
    attacks: Query<&Attack>,
    defenses: Query<&Defense>,
    crits: Query<(&CritRate, &CritDamage)>,
    names: Query<&EntityName>,
    effects: EffectContext,
    mut rng: ResMut<GameRng>,
    mut attack_events: EventWriter<AttackEvent>,
    mut event_log: ResMut<EventLog>,
) {
    for intent in attack_intents.read() {
        let attacker_mods = effects.collect_actor(intent.attacker);
        let defender_mods = effects.collect_actor(intent.target);
        let base_attack = attacks.get(intent.attacker).map(|a| a.0).unwrap_or(0.0);
        let base_defense = defenses.get(intent.target).map(|d| d.0).unwrap_or(0.0);
        let attack = apply_modifiers(Stat::Attack, base_attack, &attacker_mods, &[]);
        let defense = apply_modifiers(Stat::Defense, base_defense, &defender_mods, &[]);
        let (crit_rate, crit_damage) = crits
            .get(intent.attacker)
            .map(|(r, d)| (r.0, d.0))
            .unwrap_or((0.0, 0.0));
        let crit_roll = rng.random_f64();
        let result = compute_melee_damage(attack, defense, crit_rate, crit_damage, crit_roll);

        log::debug!(
            "伤害结算: attacker={:?}, target={:?}, attack={attack:.2}(基础 {base_attack:.2}), defense={defense:.2}(基础 {base_defense:.2}), damage={:.2}, crit={}",
            intent.attacker,
            intent.target,
            result.damage,
            result.is_crit
        );

        let target_name = names
            .get(intent.target)
            .map(|n| n.0.as_str())
            .unwrap_or("目标");
        event_log.push(EventMessage::combat(format!(
            "对{target_name}造成 {:.0} 点伤害{}",
            result.damage,
            if result.is_crit { "（暴击）" } else { "" }
        )));

        attack_events.write(AttackEvent {
            attacker: intent.attacker,
            target: intent.target,
            damage: result.damage,
            is_crit: result.is_crit,
        });
    }
}

/// 消费 `AttackEvent`，把伤害扣到目标身上。
pub fn apply_damage_system(
    mut attack_events: EventReader<AttackEvent>,
    mut healths: Query<&mut Health>,
) {
    for event in attack_events.read() {
        if let Ok(mut health) = healths.get_mut(event.target) {
            *health = health.damage(event.damage);
        }
    }
}