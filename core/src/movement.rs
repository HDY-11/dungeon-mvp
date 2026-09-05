//! 移动规则与执行。

use crate::components::Position;
use crate::map::{Map, MAP_HEIGHT, MAP_WIDTH};
use crate::resources::OccupancyMap;
use bevy_ecs::prelude::*;

/// 判断 `(x, y)` 能否向 `(dx, dy)` 移动。
/// 对角移动要求两个正交邻格均可通行且未被占用，禁止 corner-cutting。
pub fn can_move_to(
    map: &Map,
    occupancy: &OccupancyMap,
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
    if occupancy.is_occupied(nx, ny) {
        return false;
    }

    if dx != 0 && dy != 0 {
        let sx = x.wrapping_add_signed(dx);
        let wy = y.wrapping_add_signed(dy);
        if sx >= MAP_WIDTH || wy >= MAP_HEIGHT {
            return false;
        }
        if !map.tiles[y][sx].walkable() || occupancy.is_occupied(sx, y) {
            return false;
        }
        if !map.tiles[wy][x].walkable() || occupancy.is_occupied(x, wy) {
            return false;
        }
    }
    true
}

/// 执行移动。成功返回 `true`。
pub fn execute_move(world: &mut World, entity: Entity, dx: isize, dy: isize) -> bool {
    let Some(pos) = world.get::<Position>(entity) else {
        return false;
    };
    let valid = {
        let map = world.resource::<Map>();
        let occupancy = world.resource::<OccupancyMap>();
        can_move_to(map, occupancy, pos.x, pos.y, dx, dy)
    };
    if !valid {
        return false;
    }

    let (nx, ny) = pos.offset(dx, dy);
    if let Some(mut pos) = world.get_mut::<Position>(entity) {
        pos.x = nx;
        pos.y = ny;
    }
    true
}
