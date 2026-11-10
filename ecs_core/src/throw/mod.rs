//! 投掷（GAME.md Gm9）：可达性判定与弹道。
//!
//! 投掷的目标是**一格地面**，不是实体：落点必中，落点上站着谁就打谁，
//! 没人就只是把东西丢在那里（落地可拾）。因此这里的规则只回答
//! "这一格够不够得着"，不回答"打谁"——后者在结算时按格上的占用者查。
//!
//! 可达条件与 Gm9 一致：
//! - 切比雪夫距离不超过 [`crate::balance::THROW_RANGE`]；
//! - 视线不被地形挡住（`spatial::los_clear`，Bresenham 线，目标格本身豁免）；
//! - 目标格在地图内。
//!
//! 规则放在这里而不是 `item`：投掷物只是"能投的东西"，射程/视线是**投掷这个
//! 动作**的规则；将来法杖、敌人投掷、抛物陷阱都复用同一套判定。

use crate::balance::THROW_RANGE;
use crate::components::{EntityName, Health, Position, Stats};
use crate::effect::EffectContext;
use crate::events::{DamageTakenEvent, ThrowIntentEvent};
use crate::item::{Amount, Container, Item, Throw, off_hand_slot_index, spawn_ground_item};
use crate::map::{MAP_HEIGHT, MAP_WIDTH, Map};
use crate::resources::{EventLog, EventMessage, GameRng};
use crate::rules::{CritProfile, StatId, ThrownInput, apply_modifiers, thrown};
use crate::spatial::{chebyshev, line_bresenham, los_clear};
use bevy_ecs::prelude::*;
use bevy_ecs::system::SystemParam;

/// 投掷行动的载荷：目标**格**（校验阶段定案后写进 action 实体）。
///
/// 与 `Move`/`BasicAttack`/`CastSpell` 一样，载荷只在生成期写一次；
/// 校验器若认为目标不再可达，会让行动失败而不是改写载荷。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThrowAction {
    pub target: (usize, usize),
}

/// 从 `from` 能否投到 `to`：界内 + 射程内 + 视线通。
///
/// 目标格本身是阻挡格时视为"砸在墙面上"：`los_clear` 只检查终点之前的路径，
/// 因此这里不额外拒绝——但落地物会落在该格，玩家可能捡不回来（见 DsnE28 的开放项）。
pub fn throw_reachable(map: &Map, from: (usize, usize), to: (usize, usize)) -> bool {
    if to.0 >= MAP_WIDTH || to.1 >= MAP_HEIGHT {
        return false;
    }
    chebyshev(from, to) <= THROW_RANGE && los_clear(map, from, to)
}

/// 弹道格（不含起点、含目标），供瞄准界面画轨迹。
pub fn throw_path(from: (usize, usize), to: (usize, usize)) -> Vec<(usize, usize)> {
    line_bresenham(from.0, from.1, to.0, to.1)
}

/// 一次投掷结算要读写的世界状态（打包成 [`SystemParam`]）。
///
/// 投掷结算要回答"谁投的、投的什么、落点有谁、伤害多少、掉落在哪"，
/// 参数自然会到 10 个；字段分组对应这些问题，系统签名保持一个参数。
#[derive(SystemParam)]
pub struct ThrowResolution<'w, 's> {
    intents: EventReader<'w, 's, ThrowIntentEvent>,
    effects: EffectContext<'w, 's>,
    positions: Query<'w, 's, &'static Position>,
    throwers: Query<
        'w,
        's,
        (
            &'static Container,
            Option<&'static Stats>,
            Option<&'static EntityName>,
        ),
    >,
    items: Query<'w, 's, (&'static Item, &'static Throw)>,
    bodies: Query<
        'w,
        's,
        (
            Entity,
            &'static Position,
            Option<&'static Stats>,
            Option<&'static EntityName>,
        ),
    >,
    healths: Query<'w, 's, &'static mut Health>,
    rng: ResMut<'w, GameRng>,
    commands: Commands<'w, 's>,
    event_log: ResMut<'w, EventLog>,
    damage_taken: EventWriter<'w, DamageTakenEvent>,
}

/// 结算一次投掷（`ThrowIntentEvent`）：落点必中——格上站着谁就打谁，没人就只是落地。
///
/// 与 [`consume_thrown_item_system`] 分开：本系统用 [`EffectContext`]（内部持有
/// 只读的 `Query<&Container>`），消耗系统要 `&mut Container`；放在同一系统里会触发
/// bevy 的 `B0001` 查询冲突（与 `resolve_attack_system` 不同，这里的分工是
/// "伤害"与"资源变更"两件事，分开也更好读）。
///
/// 伤害只走 [`thrown`]：基值来自物品的 `Throw.damage`，四个域分别是
/// 投掷者增伤/暴击与受击者护甲/物理抗性——**不计投掷者攻击力与力量**（用户 m10559 Q4）。
pub fn resolve_throw_system(world: ThrowResolution) {
    let ThrowResolution {
        mut intents,
        effects,
        positions,
        throwers,
        items,
        bodies,
        mut healths,
        mut rng,
        mut commands,
        mut event_log,
        mut damage_taken,
    } = world;
    for intent in intents.read() {
        let Ok((body, thrower_stats, thrower_name)) = throwers.get(intent.thrower) else {
            continue;
        };
        let Some(weapon) = body.slot(off_hand_slot_index()) else {
            continue;
        };
        let Ok((item, throw_spec)) = items.get(weapon) else {
            continue;
        };
        let Ok(thrower_position) = positions.get(intent.thrower) else {
            continue;
        };

        let collected = effects.collect_actor(intent.thrower);
        let (crit_rate, crit_damage) = thrower_stats.map_or((0.0, 0.0), |stats| {
            (
                stats.get(StatId::CRIT_RATE),
                stats.get(StatId::CRIT_DAMAGE),
            )
        });
        let damage_bonus = apply_modifiers(StatId::DAMAGE_BONUS, 0.0, &collected, &[]);
        let crit_roll = rng.random_f64();

        let victim = bodies
            .iter()
            .find(|(_, position, _, _)| position.to_tuple() == intent.target);
        let (defense, physical_resist) = victim
            .and_then(|(_, _, stats, _)| stats)
            .map_or((0.0, 0.0), |stats| {
                (
                    stats.get(StatId::DEFENSE),
                    stats.get(StatId::PHYSICAL_RESIST),
                )
            });
        let breakdown = thrown(ThrownInput {
            base: throw_spec.damage,
            damage_bonus,
            crit: CritProfile {
                rate: crit_rate,
                damage: crit_damage,
            },
            crit_roll,
            defense,
            physical_resist,
        });

        let item_name = item.id.def().name;
        if let Some((victim_entity, _, _, victim_name)) = victim {
            let victim_label = victim_name.map_or("它", |name| name.0.as_str());
            let thrower_label = thrower_name.map_or("你", |name| name.0.as_str());
            if let Ok(mut health) = healths.get_mut(victim_entity) {
                *health = health.damage(breakdown.damage);
                // 统一伤害出口（DsnE29）：投掷也参与仇恨。
                damage_taken.write(DamageTakenEvent {
                    source: Some(intent.thrower),
                    target: victim_entity,
                    amount: breakdown.damage,
                });
            }
            let crit = if breakdown.is_crit { "（暴击）" } else { "" };
            event_log.push(EventMessage::combat(format!(
                "{thrower_label}投出{item_name}，对{victim_label}造成 {:.0} 点伤害{crit}",
                breakdown.damage
            )));
        }
        log::debug!(
            "投掷: thrower={:?} item={item_name} 从 ({},{}) 到 ({},{}) 伤害={} 暴击={}",
            intent.thrower,
            thrower_position.x,
            thrower_position.y,
            intent.target.0,
            intent.target.1,
            breakdown.damage,
            breakdown.is_crit
        );
        spawn_ground_item(&mut commands, item.id, 1, intent.target);
    }
}

/// 消耗投出去的这一颗：副手堆减 1，归零则从身体容器移除并 despawn。
///
/// 与 [`resolve_throw_system`] 读同一个事件：处理顺序由调度决定，互不依赖全文；
/// 这样即使将来投掷伤害被别的系统接走，消耗仍只发生一次（事件读到就扣）。
pub fn consume_thrown_item_system(
    mut intents: EventReader<ThrowIntentEvent>,
    mut bodies: Query<&mut Container>,
    mut amounts: Query<&mut Amount>,
    mut commands: Commands,
) {
    for intent in intents.read() {
        let Ok(mut body) = bodies.get_mut(intent.thrower) else {
            continue;
        };
        let slot = off_hand_slot_index();
        let Some(item) = body.slot(slot) else {
            continue;
        };
        let Ok(mut amount) = amounts.get_mut(item) else {
            continue;
        };
        amount.0 = amount.0.saturating_sub(1);
        if amount.0 > 0 {
            continue;
        }
        body.take_slot(slot);
        commands.entity(item).despawn();
    }
}

#[cfg(test)]
#[path = "throw_tests.rs"]
mod tests;
