use crate::{resources::OccupancyMap, MAP_HEIGHT, MAP_WIDTH, Tile, Map};
use std::collections::{BinaryHeap, HashMap, HashSet};

/// A* 寻路
pub fn find_path(
    start: (usize, usize), end: (usize, usize),
    map: &Map, occupancy: &OccupancyMap, can_walk_on_end: bool,
) -> Option<Vec<(usize, usize)>> {
    if start == end { return Some(vec![start]); }
    if map.tiles[end.1][end.0] == Tile::Wall { return None; }

    #[derive(Clone, Copy, PartialEq, Eq)]
    struct Node { pos: (usize, usize), f: u32 }
    impl Ord for Node {
        fn cmp(&self, other: &Self) -> std::cmp::Ordering { other.f.cmp(&self.f) }
    }
    impl PartialOrd for Node {
        fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> { Some(self.cmp(other)) }
    }

    let heuristic = |(x, y): (usize, usize)| -> u32 { x.abs_diff(end.0) as u32 + y.abs_diff(end.1) as u32 };
    let mut open = BinaryHeap::new();
    let mut g_scores = HashMap::new();
    let mut came_from = HashMap::new();
    let mut closed = HashSet::new();

    open.push(Node { pos: start, f: heuristic(start) });
    g_scores.insert(start, 0u32);
    const DIRS: [(isize, isize); 4] = [(0, -1), (0, 1), (-1, 0), (1, 0)];

    while let Some(current) = open.pop() {
        if current.pos == end {
            let mut path = vec![end];
            let mut p = end;
            while let Some(&prev) = came_from.get(&p) { path.push(prev); p = prev; }
            path.reverse();
            return Some(path);
        }
        if !closed.insert(current.pos) { continue; }
        let current_g = g_scores[&current.pos];
        for &(dx, dy) in &DIRS {
            let nx = current.pos.0.wrapping_add_signed(dx);
            let ny = current.pos.1.wrapping_add_signed(dy);
            if nx >= MAP_WIDTH || ny >= MAP_HEIGHT { continue; }
            if map.tiles[ny][nx] == Tile::Wall { continue; }
            let neighbor = (nx, ny);
            if neighbor != end && occupancy.is_occupied(nx, ny) { continue; }
            if neighbor == end && !can_walk_on_end && occupancy.is_occupied(nx, ny) { continue; }
            let tentative_g = current_g + 1;
            if tentative_g < *g_scores.get(&neighbor).unwrap_or(&u32::MAX) {
                g_scores.insert(neighbor, tentative_g);
                open.push(Node { pos: neighbor, f: tentative_g + heuristic(neighbor) });
                came_from.insert(neighbor, current.pos);
            }
        }
    }
    None
}
