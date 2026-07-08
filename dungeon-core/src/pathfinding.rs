//! A* 寻路（8 方向，支持可选碰撞规避）

use crate::{Tile, MAP_WIDTH, MAP_HEIGHT};
use std::collections::BinaryHeap;
use std::cmp::Ordering;

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

/// A* 寻路。从起点到终点找一条最短路径，返回路径点列表（不含起点，含终点）。
/// `map_tiles` 用于 walkable 检测。`occupied` 可选，传 OccupancyMap 避免走入已占格。
/// 支持 8 方向移动。
pub fn astar(
    start: (usize, usize),
    goal: (usize, usize),
    map_tiles: &[[Tile; MAP_WIDTH]; MAP_HEIGHT],
    occupied: Option<&crate::resources::OccupancyMap>,
) -> Option<Vec<(usize, usize)>> {
    if !map_tiles[goal.1][goal.0].walkable() { return None; }

    let dirs: [(isize, isize); 8] = [
        (0, -1), (0, 1), (-1, 0), (1, 0),
        (-1, -1), (1, -1), (-1, 1), (1, 1),
    ];
    let h = |x: usize, y: usize| -> u32 {
        x.abs_diff(goal.0).max(y.abs_diff(goal.1)) as u32
    };

    let size = MAP_WIDTH * MAP_HEIGHT;
    let mut heap = BinaryHeap::new();
    let mut costs = vec![u32::MAX; size];
    let mut came_from = vec![None as Option<(usize, usize)>; size];

    let idx = |x: usize, y: usize| y * MAP_WIDTH + x;

    heap.push(AStarNode { cost: 0, heuristic: h(start.0, start.1), x: start.0, y: start.1 });
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
            if nx >= MAP_WIDTH || ny >= MAP_HEIGHT { continue; }
            if !map_tiles[ny][nx].walkable() { continue; }
            if let Some(occ) = occupied {
                if (nx, ny) != goal && occ.is_occupied(nx, ny) { continue; }
            }
            let ni = idx(nx, ny);
            if next_cost < costs[ni] {
                costs[ni] = next_cost;
                came_from[ni] = Some((node.x, node.y));
                heap.push(AStarNode { cost: next_cost, heuristic: h(nx, ny), x: nx, y: ny });
            }
        }
    }
    None
}
