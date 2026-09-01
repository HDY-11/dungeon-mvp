//! Movement execution helpers.

use bevy_ecs::prelude::*;
use dungeon_core::{MAP_HEIGHT, MAP_WIDTH, Map, OccupancyMap, Position};

/// Checks whether (x,y) can move to (x+dx, y+dy).
/// Diagonal movement verifies both orthogonal neighbors (G31).
pub(crate) fn can_move_to(
    map: &Map,
    occ: &OccupancyMap,
    x: usize,
    y: usize,
    dx: isize,
    dy: isize,
) -> bool {
    let nx = x.wrapping_add_signed(dx);
    let ny = y.wrapping_add_signed(dy);
    if nx >= MAP_WIDTH || ny >= MAP_HEIGHT {
        return false;
    }
    if !map.tiles[ny][nx].walkable() {
        return false;
    }
    if occ.is_occupied(nx, ny) {
        return false;
    }
    if dx != 0 && dy != 0 {
        let sx = x.wrapping_add_signed(dx);
        let wy = y.wrapping_add_signed(dy);
        if sx >= MAP_WIDTH || wy >= MAP_HEIGHT {
            return false;
        }
        if !map.tiles[y][sx].walkable() || occ.is_occupied(sx, y) {
            return false;
        }
        if !map.tiles[wy][x].walkable() || occ.is_occupied(x, wy) {
            return false;
        }
    }
    true
}

pub(crate) fn execute_player_move(world: &mut World, entity: Entity, dx: isize, dy: isize) {
    let (nx, ny) = {
        let ppos = match world.get::<Position>(entity) {
            Some(p) => (p.x, p.y),
            None => return,
        };
        let map = world.resource::<Map>();
        let occ = world.resource::<OccupancyMap>();
        if !can_move_to(map, occ, ppos.0, ppos.1, dx, dy) {
            return;
        }
        (
            ppos.0.wrapping_add_signed(dx),
            ppos.1.wrapping_add_signed(dy),
        )
    };
    if let Some(mut p) = world.get_mut::<Position>(entity) {
        p.x = nx;
        p.y = ny;
    }
}
