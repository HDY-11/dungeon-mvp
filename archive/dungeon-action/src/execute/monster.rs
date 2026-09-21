//! Monster action execution: chase, flee, wander.

use bevy_ecs::prelude::*;
use dungeon_core::OptionLogExt;
use dungeon_core::{
    Map, OccupancyMap, Player, Position, Viewshed, components::*,
    resources::*,
};

use super::combat::{adjacent_8, monster_attack_player};
use super::movement::can_move_to;

pub(crate) fn chase_condition(world: &World, entity: Entity) -> bool {
    let player_pos = world
        .try_query::<(&Player, &Position)>()
        .expect_log("Player+Position registered at init")
        .iter(world)
        .next()
        .map(|(_, p)| (p.x, p.y));
    if let Some((px, py)) = player_pos
        && world
            .get::<Viewshed>(entity)
            .map(|v| v.can_see((px, py)))
            .unwrap_or(false)
    {
        return true;
    }
    world
        .get::<LastKnownPlayerPos>(entity)
        .map(|l| l.0.is_some())
        .unwrap_or(false)
}

pub(crate) fn execute_chase(world: &mut World, entity: Entity) {
    let Some(player_entity) = world
        .query::<(Entity, &Player)>()
        .iter(world)
        .next()
        .map(|(e, _)| e)
    else {
        return;
    };
    let Some(player_pos) = world
        .get::<Position>(player_entity)
        .map(Position::to_tuple)
    else {
        return;
    };
    let Some(self_pos) = world.get::<Position>(entity).map(Position::to_tuple) else {
        return;
    };

    let can_see = world
        .get::<Viewshed>(entity)
        .map(|v| v.can_see(player_pos))
        .unwrap_or(false);

    let target = if can_see {
        Some(player_pos)
    } else {
        world.get::<LastKnownPlayerPos>(entity).and_then(|l| l.0)
    };

    let Some((px, py)) = target else {
        if let Some(mut lkp) = world.get_mut::<LastKnownPlayerPos>(entity) {
            lkp.0 = None;
        }
        return;
    };

    let self_position = Position {
        x: self_pos.0,
        y: self_pos.1,
    };
    let target_position = Position { x: px, y: py };

    if can_see && self_position.is_near(&target_position) {
        monster_attack_player(world, entity, player_entity);
    } else {
        let next_step = {
            let map = world.resource::<Map>();
            let occ = world.resource::<OccupancyMap>();
            dungeon_core::pathfinding::astar(self_pos, (px, py), &map.tiles, Some(occ))
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

pub(crate) fn execute_flee(world: &mut World, entity: Entity) {
    let player_pos = world
        .query::<(&Player, &Position)>()
        .iter(world)
        .next()
        .map(|(_, p)| (p.x, p.y));
    let Some((px, py)) = player_pos else { return };
    let pos = match world.get::<Position>(entity) {
        Some(p) => (p.x, p.y),
        None => return,
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
        let occ = world.resource::<OccupancyMap>();
        let mut best: Option<(usize, usize)> = None;
        let mut best_dist = 0usize;
        for &(dx, dy) in &dirs {
            if !can_move_to(map, occ, pos.0, pos.1, dx, dy) {
                continue;
            }
            let nx = pos.0.wrapping_add_signed(dx);
            let ny = pos.1.wrapping_add_signed(dy);
            let d = Position { x: nx, y: ny }.manhattan(&Position { x: px, y: py });
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
    } else {
        if let Some(pe) = dungeon_core::ops::player_entity(world)
            && adjacent_8(world, entity, pe)
            && monster_atk_visible(world, entity, pe)
        {
            monster_attack_player(world, entity, pe);
        }
    }
}

fn monster_atk_visible(world: &World, entity: Entity, target: Entity) -> bool {
    let Some((px, py)) = world.get::<Position>(target).map(|p| (p.x, p.y)) else {
        return false;
    };
    world
        .get::<Viewshed>(entity)
        .map(|v| v.can_see((px, py)))
        .unwrap_or(false)
}

pub(crate) fn execute_wander(world: &mut World, entity: Entity) {
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
    let r = world.resource_mut::<GameRng>().random_range(0, 8) as usize;
    let (dx, dy) = dirs[r];
    let target = if let Some(pos) = world.get::<Position>(entity) {
        let map = world.resource::<Map>();
        let occ = world.resource::<OccupancyMap>();
        can_move_to(map, occ, pos.x, pos.y, dx, dy)
            .then_some((pos.x.wrapping_add_signed(dx), pos.y.wrapping_add_signed(dy)))
    } else {
        None
    };
    if let Some((nx, ny)) = target
        && let Some(mut p) = world.get_mut::<Position>(entity)
    {
        p.x = nx;
        p.y = ny;
    }
}
