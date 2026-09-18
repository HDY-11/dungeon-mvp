//! A* 寻路：8 方向，支持对角穿墙约束与可选碰撞规避。

use crate::map::{Tile, MAP_HEIGHT, MAP_WIDTH};
use crate::resources::OccupancyMap;
use std::cmp::Ordering;
use std::collections::BinaryHeap;

#[derive(Clone, Copy, Eq, PartialEq)]
struct AStarNode {
    cost: u32,
    heuristic: u32,
    x: usize,
    y: usize,
}

impl Ord for AStarNode {
    fn cmp(&self, other: &Self) -> Ordering {
        (other.cost + other.heuristic).cmp(&(self.cost + self.heuristic))
    }
}

impl PartialOrd for AStarNode {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// 返回路径点列表（不含起点，含终点）；无路返回 `None`。
pub fn astar(
    start: (usize, usize),
    goal: (usize, usize),
    map_tiles: &[[Tile; MAP_WIDTH]; MAP_HEIGHT],
    occupied: Option<&OccupancyMap>,
) -> Option<Vec<(usize, usize)>> {
    if !map_tiles[goal.1][goal.0].walkable() {
        return None;
    }

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
    let h = |x: usize, y: usize| -> u32 { x.abs_diff(goal.0).max(y.abs_diff(goal.1)) as u32 };

    let size = MAP_WIDTH * MAP_HEIGHT;
    let mut heap = BinaryHeap::new();
    let mut costs = vec![u32::MAX; size];
    let mut came_from = vec![None as Option<(usize, usize)>; size];
    let idx = |x: usize, y: usize| y * MAP_WIDTH + x;

    heap.push(AStarNode {
        cost: 0,
        heuristic: h(start.0, start.1),
        x: start.0,
        y: start.1,
    });
    costs[idx(start.0, start.1)] = 0;

    while let Some(node) = heap.pop() {
        if (node.x, node.y) == goal {
            let mut path = Vec::new();
            let mut cur = (node.x, node.y);
            while let Some(prev) = came_from[idx(cur.0, cur.1)] {
                path.push(cur);
                cur = prev;
            }
            path.reverse();
            return Some(path);
        }

        let next_cost = node.cost + 1;
        for &(dx, dy) in &dirs {
            let nx = node.x.wrapping_add_signed(dx);
            let ny = node.y.wrapping_add_signed(dy);
            if nx >= MAP_WIDTH || ny >= MAP_HEIGHT {
                continue;
            }
            if !map_tiles[ny][nx].walkable() {
                continue;
            }
            if let Some(occ) = occupied
                && (nx, ny) != goal
                && occ.is_occupied(nx, ny)
            {
                continue;
            }

            // 对角移动要求两个正交邻格可通行且未被占用。
            if dx != 0 && dy != 0 {
                let sx = node.x.wrapping_add_signed(dx);
                let wy = node.y.wrapping_add_signed(dy);
                if sx >= MAP_WIDTH || wy >= MAP_HEIGHT {
                    continue;
                }
                if !map_tiles[node.y][sx].walkable() || !map_tiles[wy][node.x].walkable() {
                    continue;
                }
                if let Some(occ) = occupied
                    && (occ.is_occupied(sx, node.y) || occ.is_occupied(node.x, wy))
                {
                    continue;
                }
            }

            let ni = idx(nx, ny);
            if next_cost < costs[ni] {
                costs[ni] = next_cost;
                came_from[ni] = Some((node.x, node.y));
                heap.push(AStarNode {
                    cost: next_cost,
                    heuristic: h(nx, ny),
                    x: nx,
                    y: ny,
                });
            }
        }
    }
    None
}
