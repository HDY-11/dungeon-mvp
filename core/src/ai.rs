//! 怪物 AI：决策与追击/逃跑/游荡执行。
//!
//! 本轮只迁移旧有基于视野/生命值的决策；完整仇恨系统只预留接口。

use crate::action::{mount_action, ActionKind};
use crate::balance::{
    action_av, CHASE_DURATION, FLEE_DURATION, FLEE_HP_RATIO, FLEE_HP_RATIO_EXIT, WANDER_DURATION,
    WAIT_DURATION,
};
use crate::combat::{adjacent_8, resolve_melee};
use crate::components::*;
use crate::map::Map;
use crate::movement::can_move_to;
use crate::pathfinding::astar;
use crate::query::player_entity;
use crate::resources::{GameRng, OccupancyMap};
use bevy_ecs::prelude::*;
use bevy_ecs::query::Or;

pub fn player_visible_to(world: &World, entity: Entity) -> bool {
    let Some(pp) = crate::query::player_pos(world) else {
        return false;
    };
    world
        .get::<Viewshed>(entity)
        .map(|v| v.can_see(pp))
        .unwrap_or(false)
}

/// 追击保活条件：仍能看到玩家，或仍有最后已知位置。
pub fn chase_condition(world: &World, entity: Entity) -> bool {
    player_visible_to(world, entity)
        || world
            .get::<LastKnownPlayerPos>(entity)
            .map(|l| l.0.is_some())
            .unwrap_or(false)
}

/// 逃跑保活条件（滞回退出阈值）。
pub fn flee_condition(world: &World, entity: Entity) -> bool {
    world
        .get::<Health>(entity)
        .map(|h| h.ratio() < FLEE_HP_RATIO_EXIT)
        .unwrap_or(false)
}

/// 逃跑决策条件（进入阈值）。
pub fn wants_to_flee(world: &World, entity: Entity) -> bool {
    world
        .get::<Health>(entity)
        .map(|h| h.ratio() < FLEE_HP_RATIO)
        .unwrap_or(false)
}

pub fn execute_chase(world: &mut World, entity: Entity) {
    let Some(player_entity) = player_entity(world) else {
        return;
    };
    let Some(player_pos) = world.get::<Position>(player_entity).map(Position::to_tuple) else {
        return;
    };
    let Some(self_pos) = world.get::<Position>(entity).map(Position::to_tuple) else {
        return;
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
        world
            .get::<LastKnownPlayerPos>(entity)
            .and_then(|l| l.0)
    };
    let Some((px, py)) = target else {
        if let Some(mut lkp) = world.get_mut::<LastKnownPlayerPos>(entity) {
            lkp.0 = None;
        }
        return;
    };

    let self_position = Position::new(self_pos.0, self_pos.1);
    let target_position = Position::new(px, py);

    if can_see && self_position.is_near(target_position) {
        resolve_melee(world, entity, player_entity);
    } else {
        let next_step = {
            let map = world.resource::<Map>();
            let occupancy = world.resource::<OccupancyMap>();
            astar(self_pos, (px, py), &map.tiles, Some(occupancy))
                .and_then(|path| path.first().copied())
        };
        if let Some((nx, ny)) = next_step
            && let Some(mut p) = world.get_mut::<Position>(entity)
        {
            p.x = nx;
            p.y = ny;
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

pub fn execute_flee(world: &mut World, entity: Entity) {
    let Some(player_pos) = crate::query::player_pos(world) else {
        return;
    };
    let Some(pos) = world.get::<Position>(entity).map(Position::to_tuple) else {
        return;
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
            let d = Position::new(nx, ny).manhattan(Position::new(player_pos.0, player_pos.1));
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
    {
        resolve_melee(world, entity, player);
    }
}

pub fn execute_wander(world: &mut World, entity: Entity) {
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
        return;
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
}

/// 为所有空闲/失败且有能力的怪物挂载下一轮行动。
pub fn decide_monster_actions(world: &mut World) {
    let candidates: Vec<Entity> = {
        let mut query = world.query_filtered::<
            Entity,
            (
                Or<(With<Idle>, With<Failure>)>,
                Or<(
                    With<CanChase>,
                    With<CanFlee>,
                    With<CanWander>,
                    With<CanWait>,
                )>,
                Without<Active>,
            ),
        >();
        query.iter(world).collect()
    };

    for entity in candidates {
        if let Some((action, av)) = choose_action(world, entity) {
            mount_action(world, entity, action, av);
        }
    }
}

fn choose_action(world: &World, entity: Entity) -> Option<(ActionKind, f64)> {
    let agility = world.get::<Agility>(entity).map(|a| a.0).unwrap_or(0.0);

    if world.get::<CanFlee>(entity).is_some() && wants_to_flee(world, entity) {
        return Some((ActionKind::Flee, action_av(FLEE_DURATION, agility)));
    }

    if world.get::<CanChase>(entity).is_some() && chase_condition(world, entity) {
        return Some((ActionKind::Chase, action_av(CHASE_DURATION, agility)));
    }

    if world.get::<CanWander>(entity).is_some() {
        return Some((ActionKind::Wander, action_av(WANDER_DURATION, agility)));
    }

    if world.get::<CanWait>(entity).is_some() {
        return Some((ActionKind::Wait, action_av(WAIT_DURATION, agility)));
    }

    None
}
