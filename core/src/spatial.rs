//! 通用空间查询：距离、视线、Bresenham 直线。

use crate::map::Map;

/// 切比雪夫距离。
pub fn chebyshev(a: (usize, usize), b: (usize, usize)) -> usize {
    a.0.abs_diff(b.0).max(a.1.abs_diff(b.1))
}

/// 曼哈顿距离。
pub fn manhattan(a: (usize, usize), b: (usize, usize)) -> usize {
    a.0.abs_diff(b.0) + a.1.abs_diff(b.1)
}

/// Bresenham 直线：返回不含起点的中间格（含目标）。
pub fn line_bresenham(
    x0: usize,
    y0: usize,
    x1: usize,
    y1: usize,
) -> Vec<(usize, usize)> {
    if x0 == x1 && y0 == y1 {
        return Vec::new();
    }

    let dx = (x1 as isize - x0 as isize).abs();
    let dy = -(y1 as isize - y0 as isize).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;
    let mut x = x0 as isize;
    let mut y = y0 as isize;

    let mut points = Vec::new();
    loop {
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
        if x == x1 as isize && y == y1 as isize {
            points.push((x as usize, y as usize));
            break;
        }
        points.push((x as usize, y as usize));
    }
    points
}

/// `from -> to` 视线是否畅通：除目标格外，路径上任何阻挡视线格都会阻断。
pub fn los_clear(map: &Map, from: (usize, usize), to: (usize, usize)) -> bool {
    line_bresenham(from.0, from.1, to.0, to.1)
        .iter()
        .all(|&(px, py)| (px == to.0 && py == to.1) || !map.tiles[py][px].blocks_vision())
}
