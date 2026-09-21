//! A* 寻路（8 方向，支持可选碰撞规避）

use crate::{MAP_HEIGHT, MAP_WIDTH, Tile};
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

/// A* 寻路。从起点到终点找一条最短路径，返回路径点列表（不含起点，含终点）。
/// `map_tiles` 用于 walkable 检测。`occupied` 可选，传 OccupancyMap 避免走入已占格。
/// 支持 8 方向移动。
pub fn astar(
    start: (usize, usize),
    goal: (usize, usize),
    map_tiles: &[[Tile; MAP_WIDTH]; MAP_HEIGHT],
    occupied: Option<&crate::resources::OccupancyMap>,
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
            // G31: 对角移动需两侧正交格可通行且未被占用（与 can_move_to 规则一致，防 corner-cutting）
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MAP_HEIGHT, MAP_WIDTH, Tile};

    fn wall_map() -> [[Tile; MAP_WIDTH]; MAP_HEIGHT] {
        let mut tiles = [[Tile::Wall; MAP_WIDTH]; MAP_HEIGHT];
        for row in tiles.iter_mut() {
            for t in row.iter_mut() {
                *t = Tile::Floor;
            }
        }
        tiles
    }

    /// G31: A* 不得斜穿两侧都是墙的墙角（corner-cutting）
    #[test]
    fn test_astar_no_corner_cutting() {
        let mut tiles = wall_map();
        // 起点 (5,5) 与对角 (6,6) 都可行走，但两侧 (6,5)/(5,6) 是墙
        tiles[6][5] = Tile::Wall;
        tiles[5][6] = Tile::Wall;
        // 目标放在对角格；正常路径需绕过墙角
        let path = astar((5, 5), (6, 6), &tiles, None).expect("应存在绕行路径");
        // 路径不得包含对角穿越：不存在从 (5,5) 直接到 (6,6) 的相邻段
        let mut prev = (5, 5);
        for &(x, y) in &path {
            assert!(
                !(prev == (5, 5) && (x, y) == (6, 6)),
                "路径不得直接对角穿越墙角"
            );
            prev = (x, y);
        }
        // 路径必须到达目标
        assert_eq!(path.last(), Some(&(6, 6)));
    }

    /// G31: 两侧都可行走时 A* 允许对角移动（不误伤正常对角路径）
    #[test]
    fn test_astar_allows_diagonal_when_sides_open() {
        let tiles = wall_map();
        let path = astar((5, 5), (6, 6), &tiles, None).expect("开放地形应有路径");
        assert_eq!(path, vec![(6, 6)], "两侧开放时对角是最短路径");
    }
}
