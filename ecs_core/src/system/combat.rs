//! 伤害结算与扣血。
//!
//! 攻击**执行**系统在 `action::entity::execute_basic_attack_system`：它只声明
//! 「谁打谁」（`AttackIntentEvent`），不算伤害、不扣血。这里负责把意图变成结果，
//! 于是「伤害只结算一次」这件事只有一个地方需要保证（I90）。

use crate::combat::compute_melee_damage;
use crate::components::*;
use crate::events::{AttackEvent, AttackIntentEvent};
use crate::resources::{EventLog, EventMessage, GameRng};
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
#[allow(clippy::too_many_arguments)]
pub fn resolve_attack_system(
    mut attack_intents: EventReader<AttackIntentEvent>,
    attacks: Query<&Attack>,
    defenses: Query<&Defense>,
    crits: Query<(&CritRate, &CritDamage)>,
    names: Query<&EntityName>,
    mut rng: ResMut<GameRng>,
    mut attack_events: EventWriter<AttackEvent>,
    mut event_log: ResMut<EventLog>,
) {
    for intent in attack_intents.read() {
        let attack = attacks.get(intent.attacker).map(|a| a.0).unwrap_or(0.0);
        let defense = defenses.get(intent.target).map(|d| d.0).unwrap_or(0.0);
        let (crit_rate, crit_damage) = crits
            .get(intent.attacker)
            .map(|(r, d)| (r.0, d.0))
            .unwrap_or((0.0, 0.0));
        let crit_roll = rng.random_f64();
        let result = compute_melee_damage(attack, defense, crit_rate, crit_damage, crit_roll);

        log::debug!(
            "伤害结算: attacker={:?}, target={:?}, attack={attack:.2}, defense={defense:.2}, damage={:.2}, crit={}",
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