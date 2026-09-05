//! 行动生命周期与时间推进。
//!
//! 状态轮转：`Idle/Failure` --决策--> `Active + 具体行动组件 + ActionTimer`
//! --执行--> 成功回 `Idle` / 保活失败回 `Failure`。

use crate::components::*;
use crate::entity_cls::Player;
use crate::{ai, combat, movement};
use bevy_ecs::prelude::*;

/// 可挂载的具体行动。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionKind {
    Wait,
    Move { dx: isize, dy: isize },
    BasicAttack { target: Entity },
    Chase,
    Flee,
    Wander,
}

/// 挂载行动：清理旧行动，写入 `Active + ActionTimer + 具体行动组件`。
pub fn mount_action(world: &mut World, entity: Entity, action: ActionKind, av: f64) {
    clear_concrete_actions(world, entity);

    let mut entity_mut = world.entity_mut(entity);
    entity_mut.remove::<Idle>();
    entity_mut.remove::<Failure>();
    entity_mut.remove::<Active>();
    entity_mut.remove::<ActionTimer>();
    entity_mut.insert(Active);
    entity_mut.insert(ActionTimer { remaining_av: av.max(0.0) });

    match action {
        ActionKind::Wait => {
            entity_mut.insert(Wait);
        }
        ActionKind::Move { dx, dy } => {
            entity_mut.insert(Move { dx, dy });
        }
        ActionKind::BasicAttack { target } => {
            entity_mut.insert(BasicAttack { target });
        }
        ActionKind::Chase => {
            entity_mut.insert(Chase);
        }
        ActionKind::Flee => {
            entity_mut.insert(Flee);
        }
        ActionKind::Wander => {
            entity_mut.insert(Wander);
        }
    }
}

pub fn finish_action_success(world: &mut World, entity: Entity) {
    clear_action_state(world, entity);
    world.entity_mut(entity).insert(Idle);
}

pub fn finish_action_failure(world: &mut World, entity: Entity) {
    clear_action_state(world, entity);
    world.entity_mut(entity).insert(Failure);
}

pub fn clear_concrete_actions(world: &mut World, entity: Entity) {
    let mut entity_mut = world.entity_mut(entity);
    entity_mut.remove::<Wait>();
    entity_mut.remove::<Move>();
    entity_mut.remove::<BasicAttack>();
    entity_mut.remove::<Chase>();
    entity_mut.remove::<Flee>();
    entity_mut.remove::<Wander>();
}

fn clear_action_state(world: &mut World, entity: Entity) {
    clear_concrete_actions(world, entity);
    let mut entity_mut = world.entity_mut(entity);
    entity_mut.remove::<Active>();
    entity_mut.remove::<ActionTimer>();
}

/// 所有 `Active` 实体中最小正剩余 AV。
pub fn next_action_distance(world: &mut World) -> Option<f64> {
    let mut query = world.query_filtered::<&ActionTimer, With<Active>>();
    query
        .iter(world)
        .map(|timer| timer.remaining_av)
        .filter(|remaining| *remaining > 0.0)
        .min_by(|a, b| a.partial_cmp(b).expect("ActionTimer must not be NaN"))
}

pub fn advance_action_timers(world: &mut World, amount: f64) {
    if amount <= 0.0 {
        return;
    }
    let mut query = world.query::<&mut ActionTimer>();
    for mut timer in query.iter_mut(world) {
        if timer.remaining_av > 0.0 {
            timer.remaining_av = (timer.remaining_av - amount).max(0.0);
        }
    }
}

pub fn ready_entities(world: &mut World) -> Vec<Entity> {
    let mut query = world.query_filtered::<(Entity, &ActionTimer), With<Active>>();
    query
        .iter(world)
        .filter(|(_, timer)| timer.remaining_av <= 0.0)
        .map(|(entity, _)| entity)
        .collect()
}

/// 执行所有 AV 归零的行动。
pub fn execute_ready_actions(world: &mut World) {
    let ready = ready_entities(world);
    for entity in ready {
        let ok = execute_one(world, entity);
        if ok {
            finish_action_success(world, entity);
        } else {
            finish_action_failure(world, entity);
        }
    }
}

fn execute_one(world: &mut World, entity: Entity) -> bool {
    if world.get::<Wait>(entity).is_some() {
        return true;
    }

    if let Some(action) = world.get::<Move>(entity).copied() {
        return movement::execute_move(world, entity, action.dx, action.dy);
    }

    if let Some(action) = world.get::<BasicAttack>(entity).copied() {
        return combat::resolve_melee(world, entity, action.target).is_some();
    }

    if world.get::<Chase>(entity).is_some() {
        if !ai::chase_condition(world, entity) {
            return false;
        }
        ai::execute_chase(world, entity);
        return true;
    }

    if world.get::<Flee>(entity).is_some() {
        if !ai::flee_condition(world, entity) {
            return false;
        }
        ai::execute_flee(world, entity);
        return true;
    }

    if world.get::<Wander>(entity).is_some() {
        ai::execute_wander(world, entity);
        return true;
    }

    false
}

/// 推进世界，直到玩家行动执行完毕或没有可推进行动。
pub fn advance_until_player_acted(world: &mut World) {
    loop {
        let ready = ready_entities(world);
        if ready.is_empty() {
            let Some(dist) = next_action_distance(world) else {
                break;
            };
            if dist <= 0.0 {
                break;
            }
            advance_action_timers(world, dist);
            continue;
        }

        execute_ready_actions(world);

        let player_done = {
            let player = {
                let mut q = world.try_query::<(Entity, &Player)>();
                q.as_mut().and_then(|q| q.iter(world).next()).map(|(e, _)| e)
            };
            player
                .map(|p| world.get::<Active>(p).is_none())
                .unwrap_or(true)
        };

        if player_done {
            break;
        }
    }
}
