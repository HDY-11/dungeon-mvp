//! 地图环境修饰管线
//!
//! 与 `Map` 的基本查询职责分离——Map 负责容纳 tile 数据 + 简单查询，
//! 此模块负责完整的生成管线：水体、钟乳石、连通性、出生点可达性。

use crate::{Map, Tile, Room, RoomShape, MAP_WIDTH, MAP_HEIGHT};
use rand::Rng;

// ══════════════════════════════════════════════════════
// 房间/区域检测
// ══════════════════════════════════════════════════════

/// BFS 收集所有可行走连通区，按大小降序返回
pub fn collect_walkable_regions(map: &Map) -> Vec<Vec<(usize, usize)>> {
    let mut visited = [[false; MAP_WIDTH]; MAP_HEIGHT];
    let mut regions = Vec::new();
    for sy in 0..MAP_HEIGHT {
        for sx in 0..MAP_WIDTH {
            if visited[sy][sx] || !map.tiles[sy][sx].walkable() { continue; }
            let mut stack = vec![(sx, sy)];
            let mut region = Vec::new();
            while let Some((x, y)) = stack.pop() {
                if visited[y][x] { continue; }
                visited[y][x] = true;
                region.push((x, y));
                for (ny, nx) in [(y.wrapping_sub(1), x), (y + 1, x), (y, x.wrapping_sub(1)), (y, x + 1)] {
                    if nx < MAP_WIDTH && ny < MAP_HEIGHT && !visited[ny][nx] && map.tiles[ny][nx].walkable() {
                        stack.push((nx, ny));
                    }
                }
            }
            if region.len() >= 6 { regions.push(region); }
        }
    }
    regions.sort_by(|a, b| b.len().cmp(&a.len()));
    regions
}

/// 从洞穴 walkable 区域中检测连通区域，返回按大小降序排列的房间列表。
/// `max_rooms` 限制最大房间数。返回的房间用 Room 近似（bounding box）。
pub fn detect_cave_regions(map: &Map, max_rooms: usize) -> Vec<Room> {
    let mut visited = [[false; MAP_WIDTH]; MAP_HEIGHT];
    let mut regions: Vec<Vec<(usize, usize)>> = Vec::new();

    for sy in 0..MAP_HEIGHT {
        for sx in 0..MAP_WIDTH {
            if visited[sy][sx] || !map.tiles[sy][sx].walkable() { continue; }

            let mut stack = vec![(sx, sy)];
            let mut region = Vec::new();
            while let Some((x, y)) = stack.pop() {
                if visited[y][x] { continue; }
                visited[y][x] = true;
                region.push((x, y));
                for (ny, nx) in [(y.wrapping_sub(1), x), (y + 1, x), (y, x.wrapping_sub(1)), (y, x + 1)] {
                    if nx < MAP_WIDTH && ny < MAP_HEIGHT && !visited[ny][nx] && map.tiles[ny][nx].walkable() {
                        stack.push((nx, ny));
                    }
                }
            }

            if region.len() >= 6 {
                regions.push(region);
            }
        }
    }

    regions.sort_by(|a, b| b.len().cmp(&a.len()));
    regions.into_iter().take(max_rooms).map(|r| {
        let min_x = r.iter().map(|&(x, _)| x).min().unwrap_or(0);
        let max_x = r.iter().map(|&(x, _)| x).max().unwrap_or(0);
        let min_y = r.iter().map(|&(_, y)| y).min().unwrap_or(0);
        let max_y = r.iter().map(|&(_, y)| y).max().unwrap_or(0);
        Room {
            x: min_x, y: min_y,
            w: max_x - min_x + 1, h: max_y - min_y + 1,
            shape: RoomShape::Rect,
        }
    }).collect()
}

// ══════════════════════════════════════════════════════
// 水体生成
// ══════════════════════════════════════════════════════

/// 用噪声在水域放置深水种子 → 元胞扩散（75% 浅水 / 25% 深水）
pub fn generate_water(map: &mut Map, _rng: &mut impl Rng, seed: u64) {
    use rand::{RngExt, SeedableRng};
    let mut rng2 = rand::rngs::SmallRng::seed_from_u64(seed);

    // Phase 1: 噪声深水种子（2% 概率）
    for y in 0..MAP_HEIGHT {
        for x in 0..MAP_WIDTH {
            if map.tiles[y][x] == Tile::Floor && rng2.random_range(0..1000) < 20
                && is_away_from_rooms(map, x, y, 6)
            {
                map.tiles[y][x] = Tile::DeepWater;
            }
        }
    }

    // Phase 2: 深水→扩散（8 方向独立判定）
    let deep_count = count_tile(map, Tile::DeepWater) as f32;
    let expand_chance = (0.25 - deep_count * 0.002).max(0.02);
    {
        let mut next = map.tiles;
        for y in 0..MAP_HEIGHT {
            for x in 0..MAP_WIDTH {
                if map.tiles[y][x] != Tile::DeepWater { continue; }
                for (dx, dy) in &[(-1,-1),(0,-1),(1,-1),(-1,0),(1,0),(-1,1),(0,1),(1,1)] {
                    let nx = x.wrapping_add_signed(*dx);
                    let ny = y.wrapping_add_signed(*dy);
                    if nx >= MAP_WIDTH || ny >= MAP_HEIGHT || map.tiles[ny][nx] != Tile::Floor { continue; }
                    next[ny][nx] = if rng2.random_range(0.0..1.0) < expand_chance {
                        Tile::DeepWater
                    } else {
                        Tile::ShallowWater
                    };
                }
            }
        }
        map.tiles = next;
    }

    // Phase 3: 浅水→扩散（8 方向 10% 概率）
    {
        let mut next = map.tiles;
        for y in 0..MAP_HEIGHT {
            for x in 0..MAP_WIDTH {
                if map.tiles[y][x] != Tile::ShallowWater { continue; }
                for (dx, dy) in &[(-1,-1),(0,-1),(1,-1),(-1,0),(1,0),(-1,1),(0,1),(1,1)] {
                    let nx = x.wrapping_add_signed(*dx);
                    let ny = y.wrapping_add_signed(*dy);
                    if nx >= MAP_WIDTH || ny >= MAP_HEIGHT { continue; }
                    if next[ny][nx] != Tile::Floor { continue; }
                    if rng2.random_range(0..100) < 10 {
                        next[ny][nx] = Tile::ShallowWater;
                    }
                }
            }
        }
        map.tiles = next;
    }
}

// ══════════════════════════════════════════════════════
// 元胞扩张
// ══════════════════════════════════════════════════════

/// 元胞扩张：对每格墙，若邻接可行走格则 25% 概率挖成 Floor（拓宽通道）
pub fn carve_expand(map: &mut Map, _rng: &mut impl Rng, seed: u64) {
    use rand::{RngExt, SeedableRng};
    let mut rng2 = rand::rngs::SmallRng::seed_from_u64(seed);
    let mut next = map.tiles;
    for y in 0..MAP_HEIGHT {
        for x in 0..MAP_WIDTH {
            if map.tiles[y][x].walkable() { continue; }
            let walkable_near = count_walkable_neighbors(map, x, y);
            if walkable_near >= 1 && rng2.random_range(0..100) < 25 {
                next[y][x] = Tile::Floor;
            }
        }
    }
    map.tiles = next;
}

// ══════════════════════════════════════════════════════
// 钟乳石
// ══════════════════════════════════════════════════════

/// 在每个房间中随机放置钟乳石（# 黄色，约 7% 密度）
pub fn generate_stalactites(map: &mut Map, _rng: &mut impl Rng, seed: u64) {
    use rand::{RngExt, SeedableRng};
    let mut rng2 = rand::rngs::SmallRng::seed_from_u64(seed);
    for room in &map.rooms.clone() {
        for y in room.y..room.y + room.h {
            for x in room.x..room.x + room.w {
                if map.tiles[y][x] == Tile::Floor && rng2.random_range(0..100) < 7 {
                    map.tiles[y][x] = Tile::Stalactite;
                }
            }
        }
    }
}

// ══════════════════════════════════════════════════════
// 连通性保障
// ══════════════════════════════════════════════════════

/// 检查最大连通区是否覆盖大部分可行走区域；若不连通，用醉汉游走挖 2-3 条通道
pub fn ensure_connectivity(map: &mut Map, _rng: &mut impl Rng, seed: u64) {
    use rand::{RngExt, SeedableRng};
    let mut rng2 = rand::rngs::SmallRng::seed_from_u64(seed);
    let regions = collect_walkable_regions(map);
    if regions.len() <= 1 { return; }

    let passages = rng2.random_range(2..=3);
    for p in 0..passages {
        let from_idx = p % regions.len();
        let to_idx = (p + 1) % regions.len();
        if from_idx >= regions.len() || to_idx >= regions.len() { break; }

        let from = regions[from_idx][regions[from_idx].len() / 2];
        let to = regions[to_idx][0];

        let (mut cx, mut cy) = (from.0 as isize, from.1 as isize);
        let (tx, ty) = (to.0 as isize, to.1 as isize);
        for _ in 0..500 {
            if (cx - tx).abs() + (cy - ty).abs() < 3 { break; }
            let dx = if rng2.random_range(0..100) < 50 { (tx - cx).signum() } else { rng2.random_range(-1i32..2) as isize };
            let dy = if rng2.random_range(0..100) < 50 { (ty - cy).signum() } else { rng2.random_range(-1i32..2) as isize };
            cx = (cx + dx).clamp(0, MAP_WIDTH as isize - 1);
            cy = (cy + dy).clamp(0, MAP_HEIGHT as isize - 1);
            for (ox, oy) in &[(0, 0), (1, 0), (0, 1), (1, 1)] {
                let (ux, uy) = ((cx + ox) as usize, (cy + oy) as usize);
                if ux < MAP_WIDTH && uy < MAP_HEIGHT && !map.tiles[uy][ux].walkable() {
                    map.tiles[uy][ux] = Tile::Floor;
                }
            }
        }
    }
}

/// 确保 from 到 to 之间有 walkable 路径。
pub fn ensure_connection_between(map: &mut Map, rng: &mut impl Rng, from: (usize, usize), to: (usize, usize)) {
    use rand::RngExt;
    if has_path_between(map, from, to) { return; }

    let (mut cx, mut cy) = (from.0 as isize, from.1 as isize);
    let (tx, ty) = (to.0 as isize, to.1 as isize);
    for _ in 0..500 {
        if (cx - tx).abs() + (ty - cy).abs() < 3 { break; }
        let dx = if rng.random_range(0..100) < 70 { (tx - cx).signum() } else { rng.random_range(-1i32..2) as isize };
        let dy = if rng.random_range(0..100) < 70 { (ty - cy).signum() } else { rng.random_range(-1i32..2) as isize };
        if dx == 0 && dy == 0 { continue; }
        cx = (cx + dx).clamp(0, MAP_WIDTH as isize - 1);
        cy = (cy + dy).clamp(0, MAP_HEIGHT as isize - 1);
        let (ux, uy) = (cx as usize, cy as usize);
        if !map.tiles[uy][ux].walkable() {
            map.tiles[uy][ux] = Tile::Floor;
        }
    }
}

/// BFS 检查 from 到 to 是否有 walkable 路径
pub fn has_path_between(map: &Map, from: (usize, usize), to: (usize, usize)) -> bool {
    if !map.tiles[from.1][from.0].walkable() || !map.tiles[to.1][to.0].walkable() {
        return false;
    }
    let mut visited = [[false; MAP_WIDTH]; MAP_HEIGHT];
    let mut stack = vec![from];
    while let Some((x, y)) = stack.pop() {
        if (x, y) == to { return true; }
        if visited[y][x] { continue; }
        visited[y][x] = true;
        for (ny, nx) in [(y.wrapping_sub(1), x), (y + 1, x), (y, x.wrapping_sub(1)), (y, x + 1)] {
            if nx < MAP_WIDTH && ny < MAP_HEIGHT && !visited[ny][nx] && map.tiles[ny][nx].walkable() {
                stack.push((nx, ny));
            }
        }
    }
    false
}

// ══════════════════════════════════════════════════════
// 出生点可达性
// ══════════════════════════════════════════════════════

/// 确保出生点不会被封闭在墙壁中。
/// 如果出生点 8 方向都没有可行走格，就用醉汉游走凿一条路出去。
pub fn ensure_spawn_accessible(map: &mut Map, _rng: &mut impl Rng, seed: u64) {
    use rand::{RngExt, SeedableRng};
    if map.rooms.is_empty() { return; }
    let (sx, sy) = map.rooms[0].center();
    for dy in -1isize..=1 {
        for dx in -1isize..=1 {
            if dx == 0 && dy == 0 { continue; }
            let nx = sx.wrapping_add_signed(dx);
            let ny = sy.wrapping_add_signed(dy);
            if nx < MAP_WIDTH && ny < MAP_HEIGHT && map.tiles[ny][nx].walkable() {
                return;
            }
        }
    }
    let mut rng2 = rand::rngs::SmallRng::seed_from_u64(seed);
    let (mut cx, mut cy) = (sx as isize, sy as isize);
    for _ in 0..100 {
        let dx = rng2.random_range(-1i32..2) as isize;
        let dy = rng2.random_range(-1i32..2) as isize;
        if dx == 0 && dy == 0 { continue; }
        cx = (cx + dx).clamp(0, MAP_WIDTH as isize - 1);
        cy = (cy + dy).clamp(0, MAP_HEIGHT as isize - 1);
        let (ux, uy) = (cx as usize, cy as usize);
        if !map.tiles[uy][ux].walkable() {
            map.tiles[uy][ux] = Tile::Floor;
        }
        if ux.abs_diff(sx) + uy.abs_diff(sy) > 3 {
            let mut free = false;
            for dy in -1isize..=1 {
                for dx in -1isize..=1 {
                    let nx = ux.wrapping_add_signed(dx);
                    let ny = uy.wrapping_add_signed(dy);
                    if nx < MAP_WIDTH && ny < MAP_HEIGHT && map.tiles[ny][nx].walkable() && (nx != sx || ny != sy) {
                        free = true;
                    }
                }
            }
            if free { break; }
        }
    }
}

// ══════════════════════════════════════════════════════
// 工具函数（本模块内部使用）
// ══════════════════════════════════════════════════════

fn count_tile(map: &Map, tile: Tile) -> usize {
    map.tiles.iter().flatten().filter(|&&t| t == tile).count()
}

fn count_walkable_neighbors(map: &Map, x: usize, y: usize) -> usize {
    let mut n = 0;
    for dy in -1isize..=1 {
        for dx in -1isize..=1 {
            if dx == 0 && dy == 0 { continue; }
            let nx = x.wrapping_add_signed(dx);
            let ny = y.wrapping_add_signed(dy);
            if nx < MAP_WIDTH && ny < MAP_HEIGHT && map.tiles[ny][nx].walkable() { n += 1; }
        }
    }
    n
}

fn is_away_from_rooms(map: &Map, x: usize, y: usize, min_dist: usize) -> bool {
    map.rooms.iter().all(|r| {
        let (cx, cy) = r.center();
        x.abs_diff(cx) + y.abs_diff(cy) >= min_dist
    })
}
