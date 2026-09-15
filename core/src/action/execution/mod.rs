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

/// 本轮要推进的最小正 AV：所有行动同时减去它，保证最快的一个（且只有一个）归零。
///
/// 没有 `Active` 行动、或全部已归零时返回 `0.0`（不再推进）。
fn positive_timer_delta(world: &mut World) -> f64 {
    let mut query = world.query_filtered::<&ActionTimer, With<Active>>();
    query
        .iter(world)
        .map(|timer| timer.remaining_av)
        .filter(|remaining| *remaining > 0.0)
        .min_by(|a, b| a.partial_cmp(b).expect("ActionTimer must not be NaN"))
        .unwrap_or(0.0)
}

/// 推进所有行动计时器：所有剩余 AV 同时减去当前最小正剩余值。
///
/// `remaining_av <= 0` 的实体插入 [`Ready`]，执行系统只处理 `With<Ready>`。
pub fn tick_action_timers_system(world: &mut World) {
    // 没有可推进的正剩余 AV 时 `min == 0.0`：不做写回，只补齐已归零行动的 `Ready`。
    let min = positive_timer_delta(world);

    let mut ready_entities = Vec::new();
    {
        let mut query = world.query_filtered::<(Entity, &mut ActionTimer), With<Active>>();
        for (entity, mut timer) in query.iter_mut(world) {
            if min > 0.0 && timer.remaining_av > 0.0 {
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
    use crate::components::Move;
    use crate::map::Tile;
    use crate::test_util::{fill_map, test_world};

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

    /// 测试用挂载：等价于 `action::mount_action` 的状态部分（清 `Idle`/`Failure`，
    /// 写回 `Active + ActionTimer + 具体行动`）。测试里没有仲裁系统，需要自己接上。
    fn mount_test_action(world: &mut World, entity: Entity, action: impl Component, av: f64) {
        let mut cmd = world.entity_mut(entity);
        cmd.remove::<Idle>();
        cmd.remove::<Failure>();
        cmd.insert(Active);
        cmd.insert(ActionTimer { remaining_av: av });
        cmd.insert(action);
    }

    /// A6：AV 更小的行动者在同样的时间预算内执行更多次（速度真正决定行动频率）。
    ///
    /// 计时器语义（与 `dungeon-action/src/state_action/runtime.rs::timer_advances_to_next_event`
    /// 及本模块 `positive_timer_delta` 一致）：每轮把所有行动按「当前最小正剩余 AV」推进，
    /// 于是每轮至少有一个行动归零并执行。因此“固定时间预算内的执行次数”就等于 AV 的倒数。
    #[test]
    fn fast_actor_gets_more_actions() {
        let mut world = test_world();
        // 两格地板给 fast 来回走，另外两格给 slow 来回走，中间隔一堵墙避免互相阻挡。
        fill_map(&mut world, Tile::Wall);
        {
            let mut map = world.resource_mut::<crate::map::Map>();
            map.tiles[0][0] = Tile::Floor;
            map.tiles[0][1] = Tile::Floor;
            map.tiles[0][3] = Tile::Floor;
            map.tiles[0][4] = Tile::Floor;
        }

        let fast_av = 100.0;
        let slow_av = 300.0;
        let fast = world
            .spawn((
                Position::new(0, 0),
                Active,
                Move { dx: 1, dy: 0 },
                ActionTimer {
                    remaining_av: fast_av,
                },
            ))
            .id();
        let slow = world
            .spawn((
                Position::new(3, 0),
                Active,
                Move { dx: 1, dy: 0 },
                ActionTimer {
                    remaining_av: slow_av,
                },
            ))
            .id();

        // 时间预算 = 快行动者 30 次行动的 AV 量。
        let budget = fast_av * 30.0;
        let mut elapsed = 0.0;
        // 每只行动者：当前方向（执行成功就反向，失败保持原方向重试）+ 已执行次数。
        let mut fast_dir: isize = 1;
        let mut slow_dir: isize = 1;
        let mut fast_moves = 0usize;
        let mut slow_moves = 0usize;

        while elapsed < budget {
            // 与 tick 系统同源的距离口径：本轮推进量。
            let step = positive_timer_delta(&mut world);
            if step <= 0.0 {
                break;
            }
            elapsed += step;
            run_action_cycle(&mut world);

            // 重新挂载行动（等价于仲裁系统 mount_action：清掉 Idle/Failure，写回 Active + 计时器）。
            if world.get::<Move>(fast).is_none() {
                if world.get::<Idle>(fast).is_some() {
                    fast_moves += 1;
                    fast_dir = -fast_dir;
                }
                mount_test_action(&mut world, fast, Move { dx: fast_dir, dy: 0 }, fast_av);
            }
            if world.get::<Move>(slow).is_none() {
                if world.get::<Idle>(slow).is_some() {
                    slow_moves += 1;
                    slow_dir = -slow_dir;
                }
                mount_test_action(&mut world, slow, Move { dx: slow_dir, dy: 0 }, slow_av);
            }
        }

        assert!(fast_moves > 0, "快行动者必须至少执行一次");
        assert!(slow_moves > 0, "慢行动者应当也执行过（否则测试没有对照）");
        assert!(
            fast_moves > slow_moves,
            "AV 更小的行动者执行次数必须更多: fast={fast_moves}, slow={slow_moves}"
        );
    }

    /// A6 补充：AV=100 与 AV=300 的时间轴顺序与旧实现一致（`timer_advances_to_next_event`）。
    #[test]
    fn tick_advances_to_the_next_event_and_empties_only_the_fastest() {
        let mut world = test_world();
        let fast = world
            .spawn((Active, Wait, ActionTimer { remaining_av: 100.0 }))
            .id();
        let slow = world
            .spawn((Active, Wait, ActionTimer { remaining_av: 300.0 }))
            .id();

        assert_eq!(positive_timer_delta(&mut world), 100.0);
        tick_action_timers_system(&mut world);

        assert_eq!(world.get::<ActionTimer>(fast).unwrap().remaining_av, 0.0);
        assert_eq!(world.get::<ActionTimer>(slow).unwrap().remaining_av, 200.0);
        assert!(world.get::<Ready>(fast).is_some());
        assert!(world.get::<Ready>(slow).is_none(), "只有归零的行动可以执行");
    }

    /// A6 补充：只有 AV 最小的行动在本轮归零并执行，其余保持 `Active` 且计时器同步扣减。
    ///
    /// 注意“最小正 AV”口径：若某个行动已经停在 0（上一轮就该执行），它不会拖住时钟，
    /// 本轮推进量仍取**正的**最小值。
    #[test]
    fn only_the_action_whose_timer_hit_zero_executes() {
        let mut world = test_world();
        fill_map(&mut world, Tile::Wall);

        let ready = world
            .spawn((
                Active,
                Move { dx: 0, dy: 0 },
                ActionTimer {
                    remaining_av: 100.0,
                },
            ))
            .id();
        let waiting = world
            .spawn((
                Active,
                Move { dx: 0, dy: 0 },
                ActionTimer {
                    remaining_av: 300.0,
                },
            ))
            .id();

        run_action_cycle(&mut world);

        // 归零者：行动组件、计时器与 Ready 都被清掉（执行过），并落到 Failure。
        assert!(world.get::<Move>(ready).is_none(), "归零行动必须被执行");
        assert!(
            world.get::<ActionTimer>(ready).is_none(),
            "执行后必须清掉计时器"
        );
        assert!(world.get::<Ready>(ready).is_none(), "Ready 必须被清理");
        assert!(
            world.get::<Failure>(ready).is_some(),
            "全墙地图上移动必然失败 → Failure"
        );

        // 未归零者：完全没被执行，只按推进量扣掉 100。
        assert!(world.get::<Move>(waiting).is_some(), "未归零行动不得被执行");
        assert!(world.get::<Active>(waiting).is_some());
        assert!(world.get::<Failure>(waiting).is_none());
        assert!(world.get::<Ready>(waiting).is_none());
        assert_eq!(
            world.get::<ActionTimer>(waiting).unwrap().remaining_av,
            200.0
        );
    }
}
