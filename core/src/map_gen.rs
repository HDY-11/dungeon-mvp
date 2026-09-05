//! 地图环境修饰管线。
//!
//! 与 Map 的查询职责分离；本模块负责地形生成、水体、障碍、装饰与连通性。

//! 地图环境修饰管线
//!
//! 与 `Map` 的基本查询职责分离——Map 负责容纳 tile 数据 + 简单查询，
//! 此模块负责完整的生成管线：水体、钟乳石、连通性、出生点可达性。

use crate::map::{Map, MapEnvParams, MapKind, Room, RoomShape, Tile, MAP_HEIGHT, MAP_WIDTH};
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
            if visited[sy][sx] || !map.tiles[sy][sx].walkable() {
                continue;
            }
            let mut stack = vec![(sx, sy)];
            let mut region = Vec::new();
            while let Some((x, y)) = stack.pop() {
                if visited[y][x] {
                    continue;
                }
                visited[y][x] = true;
                region.push((x, y));
                for (ny, nx) in [
                    (y.wrapping_sub(1), x),
                    (y + 1, x),
                    (y, x.wrapping_sub(1)),
                    (y, x + 1),
                ] {
                    if nx < MAP_WIDTH
                        && ny < MAP_HEIGHT
                        && !visited[ny][nx]
                        && map.tiles[ny][nx].walkable()
                    {
                        stack.push((nx, ny));
                    }
                }
            }
            if region.len() >= 6 {
                regions.push(region);
            }
        }
    }
    regions.sort_by_key(|b| std::cmp::Reverse(b.len()));
    regions
}

/// 从洞穴 walkable 区域中检测连通区域，返回按大小降序排列的房间列表。
/// `max_rooms` 限制最大房间数。返回的房间用 Room 近似（bounding box）。
pub fn detect_cave_regions(map: &Map, max_rooms: usize) -> Vec<Room> {
    let mut visited = [[false; MAP_WIDTH]; MAP_HEIGHT];
    let mut regions: Vec<Vec<(usize, usize)>> = Vec::new();

    for sy in 0..MAP_HEIGHT {
        for sx in 0..MAP_WIDTH {
            if visited[sy][sx] || !map.tiles[sy][sx].walkable() {
                continue;
            }

            let mut stack = vec![(sx, sy)];
            let mut region = Vec::new();
            while let Some((x, y)) = stack.pop() {
                if visited[y][x] {
                    continue;
                }
                visited[y][x] = true;
                region.push((x, y));
                for (ny, nx) in [
                    (y.wrapping_sub(1), x),
                    (y + 1, x),
                    (y, x.wrapping_sub(1)),
                    (y, x + 1),
                ] {
                    if nx < MAP_WIDTH
                        && ny < MAP_HEIGHT
                        && !visited[ny][nx]
                        && map.tiles[ny][nx].walkable()
                    {
                        stack.push((nx, ny));
                    }
                }
            }

            if region.len() >= 6 {
                regions.push(region);
            }
        }
    }

    regions.sort_by_key(|b| std::cmp::Reverse(b.len()));
    regions
        .into_iter()
        .take(max_rooms)
        .map(|r| {
            let min_x = r.iter().map(|&(x, _)| x).min().unwrap_or(0);
            let max_x = r.iter().map(|&(x, _)| x).max().unwrap_or(0);
            let min_y = r.iter().map(|&(_, y)| y).min().unwrap_or(0);
            let max_y = r.iter().map(|&(_, y)| y).max().unwrap_or(0);
            Room {
                x: min_x,
                y: min_y,
                w: max_x - min_x + 1,
                h: max_y - min_y + 1,
                shape: RoomShape::Rect,
            }
        })
        .collect()
}

// ══════════════════════════════════════════════════════
// 水体生成
// ══════════════════════════════════════════════════════

/// 用噪声在水域放置深水种子 → 元胞扩散（75% 浅水 / 25% 深水）
/// 水域生成（Dsn24: 参数按 MapKind 分派——种子率/扩散/浅水概率）
pub fn generate_water(map: &mut Map, _rng: &mut impl Rng, seed: u64, params: &MapEnvParams) {
    use rand::{RngExt, SeedableRng};
    let mut rng2 = rand::rngs::SmallRng::seed_from_u64(seed);

    // Phase 1: 噪声深水种子（概率 = water_seed_per_mille ‰，距房间 ≥ water_seed_min_dist）
    for y in 0..MAP_HEIGHT {
        for x in 0..MAP_WIDTH {
            if map.tiles[y][x] == Tile::Floor
                && rng2.random_range(0..1000) < params.water_seed_per_mille
                && is_away_from_rooms(map, x, y, params.water_seed_min_dist as usize)
            {
                map.tiles[y][x] = Tile::DeepWater;
            }
        }
    }

    // Phase 2: 深水→扩散（8 方向独立判定；expend_bonus 调整水域规模）
    let deep_count = count_tile(map, Tile::DeepWater) as f64;
    let expand_chance = (0.25 - deep_count * 0.002 + params.water_expand_bonus).max(0.02);
    {
        let mut next = map.tiles;
        for y in 0..MAP_HEIGHT {
            for x in 0..MAP_WIDTH {
                if map.tiles[y][x] != Tile::DeepWater {
                    continue;
                }
                for (dx, dy) in &[
                    (-1, -1),
                    (0, -1),
                    (1, -1),
                    (-1, 0),
                    (1, 0),
                    (-1, 1),
                    (0, 1),
                    (1, 1),
                ] {
                    let nx = x.wrapping_add_signed(*dx);
                    let ny = y.wrapping_add_signed(*dy);
                    if nx >= MAP_WIDTH || ny >= MAP_HEIGHT || map.tiles[ny][nx] != Tile::Floor {
                        continue;
                    }
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

    // Phase 3: 浅水→扩散（8 方向 shallow_expand_chance% 概率）
    {
        let mut next = map.tiles;
        for y in 0..MAP_HEIGHT {
            for x in 0..MAP_WIDTH {
                if map.tiles[y][x] != Tile::ShallowWater {
                    continue;
                }
                for (dx, dy) in &[
                    (-1, -1),
                    (0, -1),
                    (1, -1),
                    (-1, 0),
                    (1, 0),
                    (-1, 1),
                    (0, 1),
                    (1, 1),
                ] {
                    let nx = x.wrapping_add_signed(*dx);
                    let ny = y.wrapping_add_signed(*dy);
                    if nx >= MAP_WIDTH || ny >= MAP_HEIGHT {
                        continue;
                    }
                    if next[ny][nx] != Tile::Floor {
                        continue;
                    }
                    if rng2.random_range(0..100) < params.shallow_expand_chance {
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

/// 元胞扩张：对每格墙，若邻接可行走格则 25% 概率挖成 Floor（拓宽通道）。
/// Dsn24: 只挖 Wall——DeepWater/Stalactite 等装饰性不可走格不得被覆盖（原逻辑挖所有不可走格，
/// 地海贴近房间的水会被大量挖成 Floor）。
pub fn carve_expand(map: &mut Map, _rng: &mut impl Rng, seed: u64) {
    use rand::{RngExt, SeedableRng};
    let mut rng2 = rand::rngs::SmallRng::seed_from_u64(seed);
    let mut next = map.tiles;
    for (y, row) in map.tiles.iter().enumerate() {
        for (x, _) in row.iter().enumerate() {
            if map.tiles[y][x] != Tile::Wall {
                continue;
            }
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
/// 障碍生成（Dsn24）：Cavern → 钟乳石、LushCavern → 垂藤、Undersea → 珊瑚礁。
/// 概率按 MapKind 分派，房间中心保留（G22）。
pub fn generate_obstacles(
    map: &mut Map,
    _rng: &mut impl Rng,
    seed: u64,
    params: &MapEnvParams,
    kind: MapKind,
) {
    use rand::{RngExt, SeedableRng};
    let tile = match kind {
        MapKind::Cavern => Tile::Stalactite,
        MapKind::LushCavern => Tile::HangingVine,
        MapKind::Undersea => Tile::CoralReef,
    };
    let mut rng2 = rand::rngs::SmallRng::seed_from_u64(seed);
    for room in &map.rooms.clone() {
        let (ccx, ccy) = room.center();
        for y in room.y..room.y + room.h {
            for x in room.x..room.x + room.w {
                // G22: 房间中心保留为 Floor（楼梯/物品/出生点都以房间中心为落点候选）
                if x == ccx && y == ccy {
                    continue;
                }
                if map.tiles[y][x] == Tile::Floor
                    && rng2.random_range(0..100) < params.obstacle_chance
                {
                    map.tiles[y][x] = tile;
                }
            }
        }
    }
}

/// 地形装饰（Dsn24）：
/// - LushCavern：房间内 Floor → 菌丝（Mycelium，decor_chance%），菌丝上 15% → 蘑菇丛（FungalPatch）
/// - Undersea：水域 8 邻域的 Floor → 沙岸（Sand，decor_chance%），浅水 10% → 海草（Seagrass）
pub fn generate_terrain_decor(
    map: &mut Map,
    _rng: &mut impl Rng,
    seed: u64,
    params: &MapEnvParams,
    kind: MapKind,
) {
    use rand::{RngExt, SeedableRng};
    let mut rng2 = rand::rngs::SmallRng::seed_from_u64(seed);
    match kind {
        MapKind::Cavern => {}
        MapKind::LushCavern => {
            for room in &map.rooms.clone() {
                let (ccx, ccy) = room.center();
                for y in room.y..room.y + room.h {
                    for x in room.x..room.x + room.w {
                        // G22: 房间中心保留（楼梯/物品落点）
                        if x == ccx && y == ccy {
                            continue;
                        }
                        if map.tiles[y][x] == Tile::Floor
                            && rng2.random_range(0..100) < params.decor_chance
                        {
                            map.tiles[y][x] = Tile::Mycelium;
                        }
                    }
                }
            }
            // 蘑菇丛：菌丝上二次生成
            for y in 0..MAP_HEIGHT {
                for x in 0..MAP_WIDTH {
                    if map.tiles[y][x] == Tile::Mycelium && rng2.random_range(0..100) < 15 {
                        map.tiles[y][x] = Tile::FungalPatch;
                    }
                }
            }
        }
        MapKind::Undersea => {
            // 沙岸 + 海草：单一副本分阶段判定，最后一次性写回（clippy almost_swapped；
            // 沙岸只改 Floor、海草只改 ShallowWater，互不影响，合并后语义等价）
            let mut next = map.tiles;
            // 沙岸：紧邻水域的 Floor
            for (y, row) in map.tiles.iter().enumerate() {
                for (x, _) in row.iter().enumerate() {
                    if map.tiles[y][x] != Tile::Floor {
                        continue;
                    }
                    let near_water = [
                        (-1isize, 0isize),
                        (1, 0),
                        (0, -1),
                        (0, 1),
                        (-1, -1),
                        (1, 1),
                        (-1, 1),
                        (1, -1),
                    ]
                    .iter()
                    .any(|&(dx, dy)| {
                        let nx = x.wrapping_add_signed(dx);
                        let ny = y.wrapping_add_signed(dy);
                        nx < MAP_WIDTH
                            && ny < MAP_HEIGHT
                            && matches!(map.tiles[ny][nx], Tile::ShallowWater | Tile::DeepWater)
                    });
                    if near_water && rng2.random_range(0..100) < params.decor_chance {
                        next[y][x] = Tile::Sand;
                    }
                }
            }
            // 海草：浅水 10% 点缀（判定基于原快照，与合并前一致）
            for (y, row) in map.tiles.iter().enumerate() {
                for (x, _) in row.iter().enumerate() {
                    if map.tiles[y][x] == Tile::ShallowWater && rng2.random_range(0..100) < 10 {
                        next[y][x] = Tile::Seagrass;
                    }
                }
            }
            map.tiles = next;
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
    if regions.len() <= 1 {
        return;
    }

    let passages = rng2.random_range(2..=3);
    for p in 0..passages {
        let from_idx = p % regions.len();
        let to_idx = (p + 1) % regions.len();
        if from_idx >= regions.len() || to_idx >= regions.len() {
            break;
        }

        let from = regions[from_idx][regions[from_idx].len() / 2];
        let to = regions[to_idx][0];

        let (mut cx, mut cy) = (from.0 as isize, from.1 as isize);
        let (tx, ty) = (to.0 as isize, to.1 as isize);
        for _ in 0..500 {
            if (cx - tx).abs() + (cy - ty).abs() < 3 {
                break;
            }
            let dx = if rng2.random_range(0..100) < 50 {
                (tx - cx).signum()
            } else {
                rng2.random_range(-1i32..2) as isize
            };
            let dy = if rng2.random_range(0..100) < 50 {
                (ty - cy).signum()
            } else {
                rng2.random_range(-1i32..2) as isize
            };
            cx = (cx + dx).clamp(0, MAP_WIDTH as isize - 1);
            cy = (cy + dy).clamp(0, MAP_HEIGHT as isize - 1);
            carve_2x2(map, cx, cy);
        }
        // G22/G25（同类）：游走可能未达目标区域，直线收尾保证通道连通（同样挖 2x2——G25 修复前为单格，对角转折处 4 方向断裂）
        let line = crate::spatial::line_bresenham(cx as usize, cy as usize, tx as usize, ty as usize);
        for (px, py) in line {
            carve_2x2(map, px as isize, py as isize);
        }
    }
}

/// 通道挖掘（Dsn24）：墙/障碍挖成 Floor；DeepWater 变为 ShallowWater（涉水通道，保留水域）。
/// 所有通道函数（ensure_connectivity/ensure_connection_between/ensure_spawn_accessible）共用。
pub(crate) fn carve_channel(map: &mut Map, x: usize, y: usize) {
    if x >= MAP_WIDTH || y >= MAP_HEIGHT {
        return;
    }
    match map.tiles[y][x] {
        Tile::DeepWater => map.tiles[y][x] = Tile::ShallowWater,
        t if !t.walkable() => map.tiles[y][x] = Tile::Floor,
        _ => {}
    }
}

/// 2x2 块挖掘（G22/G25）：当前格 + 右/下/右下。
/// 单格宽通道在 8 方向路径的对角转折处会 4 方向断裂（玩家移动是 4 方向），
/// 2x2 块保证相邻块重叠、通道 4 方向连通。
pub(crate) fn carve_2x2(map: &mut Map, cx: isize, cy: isize) {
    for (ox, oy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
        let (ux, uy) = ((cx + ox) as usize, (cy + oy) as usize);
        if ux < MAP_WIDTH && uy < MAP_HEIGHT {
            carve_channel(map, ux, uy);
        }
    }
}

/// 确保 from 到 to 之间有 walkable 路径。
/// G22: 挖 2x2 块而非单格——游走/直线是 8 方向路径，单格路径会对角断裂，
/// 4 方向移动的玩家无法通过。2x2 块保证相邻块重叠，通道 4 方向连通。
pub fn ensure_connection_between(
    map: &mut Map,
    rng: &mut impl Rng,
    from: (usize, usize),
    to: (usize, usize),
) {
    use rand::RngExt;
    if has_path_between(map, from, to) {
        return;
    }

    let (mut cx, mut cy) = (from.0 as isize, from.1 as isize);
    let (tx, ty) = (to.0 as isize, to.1 as isize);
    for _ in 0..500 {
        if (cx - tx).abs() + (ty - cy).abs() < 3 {
            break;
        }
        let dx = if rng.random_range(0..100) < 70 {
            (tx - cx).signum()
        } else {
            rng.random_range(-1i32..2) as isize
        };
        let dy = if rng.random_range(0..100) < 70 {
            (ty - cy).signum()
        } else {
            rng.random_range(-1i32..2) as isize
        };
        if dx == 0 && dy == 0 {
            continue;
        }
        cx = (cx + dx).clamp(0, MAP_WIDTH as isize - 1);
        cy = (cy + dy).clamp(0, MAP_HEIGHT as isize - 1);
        carve_2x2(map, cx, cy);
    }
    // G22: 醉汉游走可能因提前停止（距离<3）或 500 步耗尽而挖不到终点，
    // 用 Bresenham 直线从当前位置强制打通到终点（同样挖 2x2 块）。
    let line = crate::spatial::line_bresenham(cx as usize, cy as usize, tx as usize, ty as usize);
    for (px, py) in line {
        carve_2x2(map, px as isize, py as isize);
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
        if (x, y) == to {
            return true;
        }
        if visited[y][x] {
            continue;
        }
        visited[y][x] = true;
        for (ny, nx) in [
            (y.wrapping_sub(1), x),
            (y + 1, x),
            (y, x.wrapping_sub(1)),
            (y, x + 1),
        ] {
            if nx < MAP_WIDTH && ny < MAP_HEIGHT && !visited[ny][nx] && map.tiles[ny][nx].walkable()
            {
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
    if map.rooms.is_empty() {
        return;
    }
    let (sx, sy) = map.rooms[0].center();
    for dy in -1isize..=1 {
        for dx in -1isize..=1 {
            if dx == 0 && dy == 0 {
                continue;
            }
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
        if dx == 0 && dy == 0 {
            continue;
        }
        cx = (cx + dx).clamp(0, MAP_WIDTH as isize - 1);
        cy = (cy + dy).clamp(0, MAP_HEIGHT as isize - 1);
        let (ux, uy) = (cx as usize, cy as usize);
        carve_channel(map, ux, uy);
        if ux.abs_diff(sx) + uy.abs_diff(sy) > 3 {
            let mut free = false;
            for dy in -1isize..=1 {
                for dx in -1isize..=1 {
                    let nx = ux.wrapping_add_signed(dx);
                    let ny = uy.wrapping_add_signed(dy);
                    if nx < MAP_WIDTH
                        && ny < MAP_HEIGHT
                        && map.tiles[ny][nx].walkable()
                        && (nx != sx || ny != sy)
                    {
                        free = true;
                    }
                }
            }
            if free {
                break;
            }
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
            if dx == 0 && dy == 0 {
                continue;
            }
            let nx = x.wrapping_add_signed(dx);
            let ny = y.wrapping_add_signed(dy);
            if nx < MAP_WIDTH && ny < MAP_HEIGHT && map.tiles[ny][nx].walkable() {
                n += 1;
            }
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


// ══════════════════════════════════════════════════════
// 地图生成入口
// ══════════════════════════════════════════════════════

/// 使用 terrain-forge 生成洞穴地图并执行环境修饰管线。
pub fn generate_map(map: &mut Map, kind: MapKind, rng: &mut impl Rng) {
    use rand::RngExt;

    let seed: u64 = rng.random();
    map.tiles = [[Tile::Wall; MAP_WIDTH]; MAP_HEIGHT];
    map.rooms.clear();

    let mut grid = terrain_forge::Grid::new(MAP_WIDTH, MAP_HEIGHT);
    let mut params = terrain_forge::ops::Params::new();
    params.insert(
        "templates".into(),
        serde_json::json!([
            {"Rectangle": {"min": 4, "max": 9}},
            {"Circle": {"min_radius": 2, "max_radius": 4}},
            {"Blob": {"size": 6, "smoothing": 2}},
        ]),
    );
    params.insert("max_rooms".into(), serde_json::json!(14));
    params.insert("loop_chance".into(), serde_json::json!(0.05));
    if terrain_forge::ops::generate("room_accretion", &mut grid, Some(seed), Some(&params)).is_err()
    {
        let _ = terrain_forge::ops::generate(
            "cellular",
            &mut grid,
            Some(seed.wrapping_add(1)),
            None,
        );
    }

    for y in 0..MAP_HEIGHT {
        for x in 0..MAP_WIDTH {
            map.tiles[y][x] = if grid[(x, y)].is_floor() {
                Tile::Floor
            } else {
                Tile::Wall
            };
        }
    }

    map.rooms = detect_cave_regions(map, 12);
    if map.rooms.is_empty() {
        map.rooms.push(Room {
            x: MAP_WIDTH / 2 - 5,
            y: MAP_HEIGHT / 2 - 5,
            w: 10,
            h: 10,
            shape: RoomShape::Rect,
        });
        let room = map.rooms[0].clone();
        for y in room.y..room.y + room.h {
            for x in room.x..room.x + room.w {
                if x < MAP_WIDTH && y < MAP_HEIGHT {
                    map.tiles[y][x] = Tile::Floor;
                }
            }
        }
    }

    let env = kind.env_params();
    generate_water(map, rng, seed.wrapping_add(100), &env);
    carve_expand(map, rng, seed.wrapping_add(150));
    generate_obstacles(map, rng, seed.wrapping_add(200), &env, kind);
    generate_terrain_decor(map, rng, seed.wrapping_add(250), &env, kind);
    ensure_connectivity(map, rng, seed.wrapping_add(300));
    ensure_spawn_accessible(map, rng, seed.wrapping_add(350));
}