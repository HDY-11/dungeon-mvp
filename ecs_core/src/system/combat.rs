//! 伤害结算与扣血。
//!
//! 攻击的**落地**在 `action::entity::settle_action_system`（校验阶段已定下
//! `AttackTarget`）：它只声明「谁打谁」（`AttackIntentEvent`），不算伤害、不扣血。
//! 这里负责把意图变成结果，于是「伤害只结算一次」这件事只有一个地方需要保证（I90）。

use crate::balance::UNARMED_STRENGTH_CAP;
use crate::components::*;
use crate::effect::EffectContext;
use crate::events::{AttackEvent, AttackIntentEvent};
use crate::item::{Container, Item, ItemTag, WeaponProgress};
use crate::resources::{EventLog, EventMessage, GameRng};
use crate::rules::{CritProfile, MeleeInput, StatId, apply_modifiers, melee};
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
/// 读的是**有效**攻击/防御：`Stats` 里的基础值，装备带来的修正由
/// [`EffectContext::collect_actor`] 收成一份 `&[Modifier]`，再按 [`StatId::ATTACK`] /
/// [`StatId::DEFENSE`] 折叠（求值五步的第 ②–⑤ 步）。所以"一把 +3 的剑"与"一条 +3 的
/// buff"在规则层完全同构，这里不需要知道加成从哪来。
///
/// 与格子无关：攻击时玩家可能站在减速格上，但那影响的是**节奏**（AV），不是伤害，
/// 所以这里用 `collect_actor` 而不是带格子的 `collect`。
#[allow(clippy::too_many_arguments)]
pub fn resolve_attack_system(
    mut attack_intents: EventReader<AttackIntentEvent>,
    stats: Query<&Stats>,
    names: Query<&EntityName>,
    effects: EffectContext,
    mut rng: ResMut<GameRng>,
    mut attack_events: EventWriter<AttackEvent>,
    mut event_log: ResMut<EventLog>,
) {
    for intent in attack_intents.read() {
        let attacker_mods = effects.collect_actor(intent.attacker);
        let defender_mods = effects.collect_actor(intent.target);
        let attacker_stats = stats.get(intent.attacker).ok();
        let defender_stats = stats.get(intent.target).ok();
        let base_attack = attacker_stats
            .map(|s| s.get(StatId::ATTACK))
            .unwrap_or(0.0);
        let base_defense = defender_stats
            .map(|s| s.get(StatId::DEFENSE))
            .unwrap_or(0.0);
        let base_strength = attacker_stats
            .map(|s| s.get(StatId::STRENGTH))
            .unwrap_or(0.0);
        let base_damage_bonus = attacker_stats
            .map(|s| s.get(StatId::DAMAGE_BONUS))
            .unwrap_or(0.0);
        let base_physical_resist = defender_stats
            .map(|s| s.get(StatId::PHYSICAL_RESIST))
            .unwrap_or(0.0);
        let attack = apply_modifiers(StatId::ATTACK, base_attack, &attacker_mods, &[]);
        let defense = apply_modifiers(StatId::DEFENSE, base_defense, &defender_mods, &[]);
        let strength = apply_modifiers(StatId::STRENGTH, base_strength, &attacker_mods, &[]);
        let damage_bonus = apply_modifiers(
            StatId::DAMAGE_BONUS,
            base_damage_bonus,
            &attacker_mods,
            &[],
        );
        let physical_resist = apply_modifiers(
            StatId::PHYSICAL_RESIST,
            base_physical_resist,
            &defender_mods,
            &[],
        );
        // 力量上限由主手武器决定（DsnE24）：蓝图上每把武器一个旋钮；
        // 空手用 `UNARMED_STRENGTH_CAP`，避免"没武器反而无限吃力量"。
        let strength_cap = effects
            .main_hand_strength_cap(intent.attacker)
            .unwrap_or(UNARMED_STRENGTH_CAP);
        let (crit_rate, crit_damage) = attacker_stats
            .map(|s| {
                (
                    s.get(StatId::CRIT_RATE),
                    s.get(StatId::CRIT_DAMAGE),
                )
            })
            .unwrap_or((0.0, 0.0));
        let crit_roll = rng.random_f64();
        let result = melee(MeleeInput {
            attack,
            strength,
            strength_cap,
            damage_bonus,
            defense,
            physical_resist,
            crit: CritProfile {
                rate: crit_rate,
                damage: crit_damage,
            },
            crit_roll,
        });

        log::debug!(
            "伤害结算: attacker={:?}, target={:?}, attack={attack:.2}(基础 {base_attack:.2}), strength={strength:.2}/{strength_cap:.2}, defense={defense:.2}(基础 {base_defense:.2}), base={:.2}, 增伤={:.2}, 暴击={:.2}, 护甲={:.2}, 抗性={:.2}, damage={:.2}, crit={}",
            intent.attacker,
            intent.target,
            result.base,
            result.damage_multiplier,
            result.crit_multiplier,
            result.armor_multiplier,
            result.resistance_multiplier,
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

/// 消费 `AttackEvent`，把伤害扣到目标身上；同时给攻击者的主手武器记养成经验。
///
/// 经验量 = 造成的伤害 × 物品品质的 EXP 倍率（DsnE24 阶段 ②）：品质越高升级越慢，
/// 于是"打同样的伤害，粗劣武器更快追上高等级"。只有**造成伤害**才给经验，
/// 空挥/未命中（未来有闪避时）不给。
pub fn apply_damage_system(
    mut attack_events: EventReader<AttackEvent>,
    mut healths: Query<&mut Health>,
    bodies: Query<&Container>,
    items: Query<&Item>,
    mut weapons: Query<&mut WeaponProgress>,
    mut event_log: ResMut<EventLog>,
) {
    for event in attack_events.read() {
        if let Ok(mut health) = healths.get_mut(event.target) {
            *health = health.damage(event.damage);
        }
        award_weapon_exp(
            &bodies,
            &items,
            &mut weapons,
            &mut event_log,
            event.attacker,
            event.damage,
        );
    }
}

/// 找攻击者主手槽里的武器（与 `EffectContext::main_hand_weapon_entity` 同口径）。
///
/// 这里不直接用 `EffectContext`：它内部持着 `&WeaponProgress` 的只读查询，
/// 与本系统的 `&mut WeaponProgress` 会在运行时查询冲突（B0001），所以只取需要的
/// 两个只读查询（身体容器 + 物品实体），mut 查询保持独立。
fn main_hand_weapon(
    bodies: &Query<&Container>,
    items: &Query<&Item>,
    actor: Entity,
) -> Option<(Entity, &'static crate::item::ItemDef)> {
    let body = bodies.get(actor).ok()?;
    body.iter().find_map(|(_, occupant)| {
        let item = items.get(occupant).ok()?;
        let def = item.def();
        def.tags
            .contains(&ItemTag::Weapon)
            .then_some((occupant, def))
    })
}

/// 把一次伤害记到主手武器上，并在升级时记一条事件。
fn award_weapon_exp(
    bodies: &Query<&Container>,
    items: &Query<&Item>,
    weapons: &mut Query<&mut WeaponProgress>,
    event_log: &mut EventLog,
    attacker: Entity,
    damage: f64,
) {
    let Some((weapon, def)) = main_hand_weapon(bodies, items, attacker) else {
        return;
    };
    let Some(growth) = def.weapon_growth else {
        return;
    };
    let Ok(mut progress) = weapons.get_mut(weapon) else {
        return;
    };
    let amount = damage.max(0.0) * def.quality.exp_factor();
    let gained = progress.add_exp(amount, &growth);
    if gained > 0 {
        event_log.push(EventMessage::item(format!(
            "{} 升到了 {} 级（+{gained}）",
            def.name, progress.level
        )));
    }
}