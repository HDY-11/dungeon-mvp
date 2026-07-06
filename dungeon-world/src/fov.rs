//! 视野（FOV）计算

use dungeon_core::{Map, Tile, MAP_WIDTH, MAP_HEIGHT};

/// 计算从 (x, y) 出发在 range 范围内的可见格子
pub fn calculate_visible_tiles(x: usize, y: usize, range: usize, map: &Map) -> Vec<(usize, usize)> {
    use symmetric_shadowcasting::compute_fov;
    let r2 = (range * range) as isize;
    let mut visible = Vec::new();
    let origin = (x as isize, y as isize);

    let mut is_blocking = |pos: (isize, isize)| {
        if pos.0 < 0 || pos.0 >= MAP_WIDTH as isize || pos.1 < 0 || pos.1 >= MAP_HEIGHT as isize {
            return true;
        }
        map.tiles[pos.1 as usize][pos.0 as usize] == Tile::Wall
    };

    let mut mark_visible = |pos: (isize, isize)| {
        if pos.0 < 0 || pos.0 >= MAP_WIDTH as isize || pos.1 < 0 || pos.1 >= MAP_HEIGHT as isize {
            return;
        }
        let dx = pos.0 - origin.0;
        let dy = pos.1 - origin.1;
        if dx * dx + dy * dy <= r2 {
            visible.push((pos.0 as usize, pos.1 as usize));
        }
    };

    compute_fov(origin, &mut is_blocking, &mut mark_visible);
    visible
}
