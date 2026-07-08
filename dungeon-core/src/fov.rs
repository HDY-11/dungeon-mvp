//! 对称阴影投射视野计算

use crate::{Map, MAP_WIDTH, MAP_HEIGHT};

/// 对称阴影投射视野计算
pub fn calculate_visible_tiles(x: usize, y: usize, range: usize, map: &Map) -> Vec<(usize, usize)> {
    use symmetric_shadowcasting::compute_fov;
    let r2 = (range * range) as isize;
    let mut visible = Vec::new();
    let origin = (x as isize, y as isize);

    let mut is_blocking = |pos: (isize, isize)| {
        if pos.0 < 0 || pos.0 >= MAP_WIDTH as isize || pos.1 < 0 || pos.1 >= MAP_HEIGHT as isize {
            return true;
        }
        map.tiles[pos.1 as usize][pos.0 as usize].blocks_vision()
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
