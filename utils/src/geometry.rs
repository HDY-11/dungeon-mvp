//! 无业务依赖的几何/栅格算法。

pub fn chebyshev(a: (usize, usize), b: (usize, usize)) -> usize {
    a.0.abs_diff(b.0).max(a.1.abs_diff(b.1))
}

pub fn manhattan(a: (usize, usize), b: (usize, usize)) -> usize {
    a.0.abs_diff(b.0) + a.1.abs_diff(b.1)
}

/// Bresenham 直线：返回不含起点的中间格（含终点）。
/// 起点与终点相同时返回空。
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
