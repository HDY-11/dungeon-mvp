//! 与具体行动组件一一对应的执行系统。
//!
//! 每个系统只消费自己负责的行动组件；执行成功回 `Idle`，保活失败回 `Failure`。

pub mod movement;

use crate::action::{finish_action_failure, finish_action_success};
use crate::action::execution::movement::can_move_to;
use crate::action::generation::ai::{chase_condition, flee_condition, player_visible_to};
use crate::combat::{adjacent_8, can_attack};
use crate::components::*;
use crate::events::AttackIntentEvent;
use crate::map::Map;
use crate::spatial::pathfinding::astar;
use crate::world::query::{player_entity, player_pos};
use crate::resources::{GameRng, OccupancyMap};
use bevy_ecs::prelude::*;
use bevy_ecs::event::Events;

/// 推进所有行动计时器：所有剩余 AV 同时减去当前最小正剩余值。
///
/// `remaining_av <= 0` 的实体插入 [`Ready`]，执行系统只处理 `With<Ready>`。
pub fn tick_action_timers_system(world: &mut World) {
    let min = {
        let mut query = world.query_filtered::<&ActionTimer, With<Active>>();
        query
            .iter(world)
            .map(|timer| timer.remaining_av)
            .filter(|remaining| *remaining > 0.0)
            .min_by(|a, b| a.partial_cmp(b).expect("ActionTimer must not be NaN"))
            .unwrap_or(0.0)
    };

    let mut ready_entities = Vec::new();
    {
        let mut query = world.query_filtered::<(Entity, &mut ActionTimer), With<Active>>();
        for (entity, mut timer) in query.iter_mut(world) {
            if timer.remaining_av > 0.0 {
                timer.remaining_av = (timer.remaining_av - min).max(0.0);
            }
            if timer.remaining_av <= 0.0 {
                ready_entities.push(entity);
            }
        }
    }

    for entity in ready_entities {
        world.entity_mut(entity).insert(Ready);
    }
}

pub fn execute_wait_system(world: &mut World) {
    let entities: Vec<Entity> = {
        let mut query = world.query_filtered::<Entity, (With<Active>, With<Wait>, With<Ready>)>();
        query.iter(world).collect()
    };
    for entity in entities {
        log::debug!("执行等待: {entity:?}");
        world.entity_mut(entity).remove::<Wait>();
        finish_action_success(world, entity);
    }
}

pub fn execute_move_system(world: &mut World) {
    let actions: Vec<(Entity, isize, isize)> = {
        let mut query = world.query_filtered::<(Entity, &Move), (With<Active>, With<Ready>)>();
        query
            .iter(world)
            .map(|(e, m)| (e, m.dx, m.dy))
            .collect()
    };

    for (entity, dx, dy) in actions {
        let ok = crate::action::execution::movement::execute_move(world, entity, dx, dy);
        log::debug!("执行移动: {entity:?} ({dx},{dy}) ok={ok}");
        world.entity_mut(entity).remove::<Move>();
        if ok {
            finish_action_success(world, entity);
        } else {
            finish_action_failure(world, entity);
        }
    }
}

pub fn execute_basic_attack_system(world: &mut World) {
    let actions: Vec<(Entity, Entity)> = {
        let mut query = world.query_filtered::<(Entity, &BasicAttack), (With<Active>, With<Ready>)>();
        query
            .iter(world)
            .map(|(e, a)| (e, a.target))
            .collect()
    };

    for (entity, target) in actions {
        let ok = can_attack(world, entity, target);
        log::debug!("攻击保活检查: attacker={entity:?}, target={target:?}, ok={ok}");
        world.entity_mut(entity).remove::<BasicAttack>();
        if !ok {
            finish_action_failure(world, entity);
            continue;
        }

        if let Some(mut events) = world.get_resource_mut::<Events<AttackIntentEvent>>() {
            events.send(AttackIntentEvent {
                attacker: entity,
                target,
            });
        }
        finish_action_success(world, entity);
    }
}

pub fn execute_chase_system(world: &mut World) {
    let actors: Vec<Entity> = {
        let mut query = world.query_filtered::<Entity, (With<Active>, With<Chase>, With<Ready>)>();
        query.iter(world).collect()
    };

    for entity in actors {
        if !chase_condition(world, entity) {
            world.entity_mut(entity).remove::<Chase>();
            finish_action_failure(world, entity);
            continue;
        }

        let Some(player) = player_entity(world) else {
            world.entity_mut(entity).remove::<Chase>();
            finish_action_failure(world, entity);
            continue;
        };
        let Some(player_pos) = world.get::<Position>(player).map(Position::to_tuple) else {
            world.entity_mut(entity).remove::<Chase>();
            finish_action_failure(world, entity);
            continue;
        };
        let Some(self_pos) = world.get::<Position>(entity).map(Position::to_tuple) else {
            world.entity_mut(entity).remove::<Chase>();
            finish_action_failure(world, entity);
            continue;
        };

        let can_see = player_visible_to(world, entity);
        if can_see {
            if let Some(mut lkp) = world.get_mut::<LastKnownPlayerPos>(entity) {
                lkp.0 = Some(player_pos);
            }
        }
        let target = if can_see {
            Some(player_pos)
        } else {
            world.get::<LastKnownPlayerPos>(entity).and_then(|l| l.0)
        };

        if let Some((px, py)) = target {
            let self_position = Position::new(self_pos.0, self_pos.1);
            let target_position = Position::new(px, py);
            if can_see && self_position.is_near(target_position) {
                if let Some(mut events) = world.get_resource_mut::<Events<AttackIntentEvent>>() {
                    events.send(AttackIntentEvent {
                        attacker: entity,
                        target: player,
                    });
                }
            } else {
                let next_step = {
                    let map = world.resource::<Map>();
                    let occupancy = world.resource::<OccupancyMap>();
                    astar(self_pos, (px, py), &map.tiles, Some(occupancy))
                        .and_then(|path| path.first().copied())
                };
                if let Some((nx, ny)) = next_step
                    && let Some(mut pos) = world.get_mut::<Position>(entity)
                {
                    pos.x = nx;
                    pos.y = ny;
                }
            }

            if !can_see
                && let Some(mut lkp) = world.get_mut::<LastKnownPlayerPos>(entity)
                && let Some((lkx, lky)) = lkp.0
                && self_pos.0.abs_diff(lkx) <= 2
                && self_pos.1.abs_diff(lky) <= 2
            {
                lkp.0 = None;
            }
        }

        log::debug!("执行追击: {entity:?}");
        world.entity_mut(entity).remove::<Chase>();
        finish_action_success(world, entity);
    }
}

pub fn execute_flee_system(world: &mut World) {
    let actors: Vec<Entity> = {
        let mut query = world.query_filtered::<Entity, (With<Active>, With<Flee>, With<Ready>)>();
        query.iter(world).collect()
    };

    for entity in actors {
        if !flee_condition(world, entity) {
            world.entity_mut(entity).remove::<Flee>();
            finish_action_failure(world, entity);
            continue;
        }

        let Some(player_pos) = player_pos(world) else {
            world.entity_mut(entity).remove::<Flee>();
            finish_action_failure(world, entity);
            continue;
        };
        let Some(pos) = world.get::<Position>(entity).map(Position::to_tuple) else {
            world.entity_mut(entity).remove::<Flee>();
            finish_action_failure(world, entity);
            continue;
        };

        let dirs: [(isize, isize); 8] = [
            (0, -1),
            (0, 1),
            (-1, 0),
            (1, 0),
            (-1, -1),
            (1, -1),
            (-1, 1),
            (1, 1),
        ];
        let best = {
            let map = world.resource::<Map>();
            let occupancy = world.resource::<OccupancyMap>();
            let mut best: Option<(usize, usize)> = None;
            let mut best_dist = 0usize;
            for &(dx, dy) in &dirs {
                if !can_move_to(map, occupancy, pos.0, pos.1, dx, dy) {
                    continue;
                }
                let (nx, ny) = Position::new(pos.0, pos.1).offset(dx, dy);
                let d = Position::new(nx, ny)
                    .manhattan(Position::new(player_pos.0, player_pos.1));
                if d > best_dist {
                    best_dist = d;
                    best = Some((nx, ny));
                }
            }
            best
        };

        if let Some((nx, ny)) = best
            && let Some(mut p) = world.get_mut::<Position>(entity)
        {
            p.x = nx;
            p.y = ny;
        } else if let Some(player) = player_entity(world)
            && adjacent_8(world, entity, player)
            && player_visible_to(world, entity)
            && let Some(mut events) = world.get_resource_mut::<Events<AttackIntentEvent>>()
        {
            events.send(AttackIntentEvent {
                attacker: entity,
                target: player,
            });
        }

        log::debug!("执行逃跑: {entity:?}");
        world.entity_mut(entity).remove::<Flee>();
        finish_action_success(world, entity);
    }
}

pub fn execute_wander_system(world: &mut World) {
    let actors: Vec<Entity> = {
        let mut query = world.query_filtered::<Entity, (With<Active>, With<Wander>, With<Ready>)>();
        query.iter(world).collect()
    };

    for entity in actors {
        let dirs: [(isize, isize); 8] = [
            (0, -1),
            (0, 1),
            (-1, 0),
            (1, 0),
            (-1, -1),
            (1, -1),
            (-1, 1),
            (1, 1),
        ];
        let idx = world.resource_mut::<GameRng>().random_range(0, 8) as usize;
        let (dx, dy) = dirs[idx.min(dirs.len() - 1)];
        let Some(pos) = world.get::<Position>(entity) else {
            world.entity_mut(entity).remove::<Wander>();
            finish_action_failure(world, entity);
            continue;
        };
        let valid = {
            let map = world.resource::<Map>();
            let occupancy = world.resource::<OccupancyMap>();
            can_move_to(map, occupancy, pos.x, pos.y, dx, dy)
        };
        if valid
            && let (nx, ny) = pos.offset(dx, dy)
            && let Some(mut p) = world.get_mut::<Position>(entity)
        {
            p.x = nx;
            p.y = ny;
        }

        log::debug!("执行游荡: {entity:?}");
        world.entity_mut(entity).remove::<Wander>();
        finish_action_success(world, entity);
    }
}

/// 运行一轮行动系统：推进计时器，执行所有到期行动。
pub fn run_action_cycle(world: &mut World) {
    tick_action_timers_system(world);
    execute_wait_system(world);
    execute_move_system(world);
    execute_basic_attack_system(world);
    execute_chase_system(world);
    execute_flee_system(world);
    execute_wander_system(world);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn av_gate_only_executes_ready_actions() {
        let mut world = World::new();
        let ready = world
            .spawn((Active, Wait, ActionTimer { remaining_av: 100.0 }))
            .id();
        let waiting = world
            .spawn((Active, Wait, ActionTimer { remaining_av: 200.0 }))
            .id();

        tick_action_timers_system(&mut world);
        assert!(world.get::<Ready>(ready).is_some());
        assert!(world.get::<Ready>(waiting).is_none());

        execute_wait_system(&mut world);
        assert!(world.get::<Idle>(ready).is_some());
        assert!(world.get::<Wait>(waiting).is_some());
        assert!(world.get::<Active>(waiting).is_some());
        assert_eq!(
            world.get::<ActionTimer>(waiting).unwrap().remaining_av,
            100.0
        );
    }

    #[test]
    fn zero_timer_is_marked_ready_without_positive_peers() {
        let mut world = World::new();
        let entity = world
            .spawn((Active, Wait, ActionTimer { remaining_av: 0.0 }))
            .id();

        tick_action_timers_system(&mut world);
        assert!(world.get::<Ready>(entity).is_some());

        execute_wait_system(&mut world);
        assert!(world.get::<Idle>(entity).is_some());
        assert!(world.get::<Ready>(entity).is_none());
    }
}
