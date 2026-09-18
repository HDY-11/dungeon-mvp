//! 近战战斗：距离校验、伤害/暴击计算、直接执行辅助。
//!
//! 目标架构使用 `AttackEvent -> apply_damage_system`；这里同时提供直接执行函数，
//! 便于尚未接入完整 Schedule 的迁移期使用。

use crate::components::*;
use crate::events::AttackEvent;
use crate::resources::GameRng;
use bevy_ecs::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeleeResult {
    pub damage: f64,
    pub is_crit: bool,
}

/// 两个实体是否在 8 方向相邻。
pub fn adjacent_8(world: &World, a: Entity, b: Entity) -> bool {
    world
        .get::<Position>(a)
        .zip(world.get::<Position>(b))
        .map(|(pa, pb)| pa.x.abs_diff(pb.x) <= 1 && pa.y.abs_diff(pb.y) <= 1)
        .unwrap_or(false)
}

/// 目标当前是否可被近战攻击。
pub fn can_attack(world: &World, attacker: Entity, target: Entity) -> bool {
    adjacent_8(world, attacker, target) && world.get::<Health>(target).is_some_and(|h| h.is_alive())
}

/// 目标当前是否可被近战攻击（纯组件版）。
///
/// 与 [`can_attack`] 是同一条规则，只是不经过 `World`：参数化执行器
/// （`action::entity::execute_basic_attack_system`）不能对任意实体做 `World::get`，
/// 所以规则本身必须能只靠组件引用表达。两处共用 [`Position::is_near`] 与
/// [`Health::is_alive`]，不存在规则漂移。
pub fn can_attack_positions(attacker: Position, target: Position, target_health: &Health) -> bool {
    attacker.is_near(target) && target_health.is_alive()
}

/// 根据攻击/防御与暴击参数计算最终伤害。
pub fn compute_melee_damage(
    attack: f64,
    defense: f64,
    crit_rate: f64,
    crit_damage: f64,
    crit_roll: f64,
) -> MeleeResult {
    let base = (attack - defense).max(1.0);
    let is_crit = crit_rate > crit_roll;
    let damage = if is_crit {
        base * (1.0 + crit_damage.max(0.0))
    } else {
        base
    };
    MeleeResult { damage, is_crit }
}

/// 准备一个已结算的普通攻击事件，不修改世界状态。
pub fn prepare_attack_event(
    world: &mut World,
    attacker: Entity,
    target: Entity,
) -> Option<AttackEvent> {
    if !can_attack(world, attacker, target) {
        return None;
    }

    let (attack, defense, crit_rate, crit_damage) = (
        world.get::<Attack>(attacker).map(|a| a.0).unwrap_or(0.0),
        world.get::<Defense>(target).map(|d| d.0).unwrap_or(0.0),
        world.get::<CritRate>(attacker).map(|c| c.0).unwrap_or(0.0),
        world
            .get::<CritDamage>(attacker)
            .map(|c| c.0)
            .unwrap_or(0.0),
    );
    let crit_roll = world.resource_mut::<GameRng>().random_f64();
    let result = compute_melee_damage(attack, defense, crit_rate, crit_damage, crit_roll);

    Some(AttackEvent {
        attacker,
        target,
        damage: result.damage,
        is_crit: result.is_crit,
    })
}

/// 直接执行一次近战攻击并立刻结算生命值。
///
/// 返回 `Some(MeleeResult)` 表示命中；`None` 表示保活条件不成立。
pub fn resolve_melee(world: &mut World, attacker: Entity, target: Entity) -> Option<MeleeResult> {
    let event = prepare_attack_event(world, attacker, target)?;
    if let Some(mut health) = world.get_mut::<Health>(target) {
        *health = health.damage(event.damage);
    }
    Some(MeleeResult {
        damage: event.damage,
        is_crit: event.is_crit,
    })
}

/// 对目标直接施加伤害。供伤害系统/投射物等未来系统复用。
pub fn damage_entity(world: &mut World, target: Entity, amount: f64) -> bool {
    let Some(mut health) = world.get_mut::<Health>(target) else {
        return false;
    };
    *health = health.damage(amount);
    true
}
