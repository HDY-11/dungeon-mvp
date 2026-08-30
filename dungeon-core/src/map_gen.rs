//! 地图环境修饰管线
//!
//! 与 `Map` 的基本查询职责分离——Map 负责容纳 tile 数据 + 简单查询，
//! 此模块负责完整的生成管线：水体、钟乳石、连通性、出生点可达性。

use crate::{MAP_HEIGHT, MAP_WIDTH, Map, Room, RoomShape, Tile};
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
pub fn generate_water(map: &mut Map, _rng: &mut impl Rng, seed: u64, params: &crate::MapEnvParams) {
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
    let deep_count = count_tile(map, Tile::DeepWater) as f32;
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
    params: &crate::MapEnvParams,
    kind: crate::MapKind,
) {
    use rand::{RngExt, SeedableRng};
    let tile = match kind {
        crate::MapKind::Cavern => Tile::Stalactite,
        crate::MapKind::LushCavern => Tile::HangingVine,
        crate::MapKind::Undersea => Tile::CoralReef,
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
    params: &crate::MapEnvParams,
    kind: crate::MapKind,
) {
    use rand::{RngExt, SeedableRng};
    let mut rng2 = rand::rngs::SmallRng::seed_from_u64(seed);
    match kind {
        crate::MapKind::Cavern => {}
        crate::MapKind::LushCavern => {
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
        crate::MapKind::Undersea => {
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
        let line = crate::ops::line_bresenham(cx as usize, cy as usize, tx as usize, ty as usize);
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
    let line = crate::ops::line_bresenham(cx as usize, cy as usize, tx as usize, ty as usize);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MAP_HEIGHT, MAP_WIDTH, Map};
    use rand::SeedableRng;

    /// G22: generate_stalactites 不得覆盖可行走的房间中心（治本修复）
    #[test]
    fn test_stalactites_skips_room_centers() {
        let mut map = Map::new();
        map.tiles = [[Tile::Wall; MAP_WIDTH]; MAP_HEIGHT];
        // 两个矩形房间，中心全 Floor
        for y in 10..20 {
            for x in 10..20 {
                map.tiles[y][x] = Tile::Floor;
            }
        }
        for y in 40..50 {
            for x in 40..50 {
                map.tiles[y][x] = Tile::Floor;
            }
        }
        map.rooms = vec![
            crate::Room {
                x: 10,
                y: 10,
                w: 10,
                h: 10,
                shape: crate::RoomShape::Rect,
            },
            crate::Room {
                x: 40,
                y: 40,
                w: 10,
                h: 10,
                shape: crate::RoomShape::Rect,
            },
        ];
        let mut rng = rand::rngs::SmallRng::seed_from_u64(1);
        for pass in 0..20 {
            generate_obstacles(
                &mut map,
                &mut rng,
                100 + pass,
                &crate::MapKind::Cavern.env_params(),
                crate::MapKind::Cavern,
            );
        }
        // 20 轮钟乳石生成后中心仍必须是 Floor
        assert_eq!(map.tiles[15][15], Tile::Floor, "钟乳石不得覆盖房间 A 中心");
        assert_eq!(map.tiles[45][45], Tile::Floor, "钟乳石不得覆盖房间 B 中心");
        // 但房间内的其他格允许出现钟乳石（装饰保留）
        assert!(
            map.tiles.iter().flatten().any(|&t| t == Tile::Stalactite),
            "钟乳石装饰仍应存在"
        );
    }

    /// G22: nearest_walkable 兜底：中心被墙覆盖时返回最近可行走格
    #[test]
    fn test_nearest_walkable_fallback() {
        let mut map = Map::new();
        map.tiles = [[Tile::Wall; MAP_WIDTH]; MAP_HEIGHT];
        // 在 (20, 20) 周围挖一块地板
        for y in 18..=22 {
            for x in 18..=22 {
                map.tiles[y][x] = Tile::Floor;
            }
        }
        let near = map.nearest_walkable(20, 20);
        assert_eq!(near, (20, 20), "中心本身可行走时应原样返回");
        map.tiles[20][20] = Tile::Stalactite;
        let near = map.nearest_walkable(20, 20);
        assert!(map.tiles[near.1][near.0].walkable(), "兜底格必须可行走");
        assert!((near.0 as isize - 20).abs() + (near.1 as isize - 20).abs() >= 1);
        // 全墙地图：兜底返回地图中心，不 panic
        let mut empty = Map::new();
        empty.tiles = [[Tile::Wall; MAP_WIDTH]; MAP_HEIGHT];
        let _ = empty.nearest_walkable(3, 3);
    }
}

/// Dsn24: map_kind_for 确定性——同种子同楼层恒等，F1 固定 Cavern，三类型均可达
#[test]
fn test_map_kind_for_deterministic() {
    use crate::map_kind_for;
    // F1 恒为 Cavern
    for seed in 0..100u64 {
        assert_eq!(
            map_kind_for(seed, 1),
            crate::MapKind::Cavern,
            "seed {}: F1 必须为 Cavern",
            seed
        );
    }
    // 同 (seed, floor) 恒等
    for seed in [0u64, 1, 42, 9999] {
        for floor in [2u32, 5, 10, 25] {
            assert_eq!(map_kind_for(seed, floor), map_kind_for(seed, floor));
        }
    }
    // 三种类型在 floor 2..=60 上均出现（60 个楼层分布到 3 类）
    let mut seen = std::collections::HashSet::new();
    for floor in 2..=60u32 {
        seen.insert(map_kind_for(42, floor));
    }
    assert!(seen.len() == 3, "深层应出现全部三种类型: {:?}", seen);
    // 不同楼层产生变化（2..=8 楼层类型不全部相同）
    let floors: std::collections::HashSet<_> = (2..=8u32).map(|f| map_kind_for(42, f)).collect();
    assert!(floors.len() >= 2, "类型不应每层恒定: {:?}", floors);
}

/// Dsn24: 各类型环境参数存在且有序（繁茂水少、地海水多）
#[test]
fn test_env_params_ordering() {
    use crate::MapKind;
    let cavern = MapKind::Cavern.env_params();
    let lush = MapKind::LushCavern.env_params();
    let sea = MapKind::Undersea.env_params();
    assert!(
        lush.water_seed_per_mille < cavern.water_seed_per_mille,
        "繁茂水种子应低于洞穴"
    );
    assert!(
        sea.water_seed_per_mille > cavern.water_seed_per_mille,
        "地海水种子应高于洞穴"
    );
    assert!(sea.shallow_expand_chance > lush.shallow_expand_chance);
}

/// Dsn24: Tile serde 回环 + 语义（全部 11 变体）
#[test]
fn test_tile_serde_roundtrip_and_semantics() {
    use crate::Tile;
    let all = [
        Tile::Wall,
        Tile::Floor,
        Tile::ShallowWater,
        Tile::DeepWater,
        Tile::Stalactite,
        Tile::Mycelium,
        Tile::FungalPatch,
        Tile::HangingVine,
        Tile::Sand,
        Tile::Seagrass,
        Tile::CoralReef,
    ];
    for &t in &all {
        // serde_json 数字往返验证（与 bincode u8 tag 同构）
        let json = serde_json::to_string(&t).unwrap();
        let back: Tile = serde_json::from_str(&json).unwrap();
        assert_eq!(t, back, "{:?} serde 回环失败", t);
    }
    // 语义：繁茂方块可走（除垂藤），地海方块可走（除珊瑚礁）
    assert!(Tile::Mycelium.walkable() && Tile::FungalPatch.walkable());
    assert!(!Tile::HangingVine.walkable() && Tile::HangingVine.blocks_vision());
    assert!(Tile::Sand.walkable() && Tile::Seagrass.walkable());
    assert!(!Tile::CoralReef.walkable() && Tile::CoralReef.blocks_vision());
    // 旧存档兼容：已知 tag 4 = Stalactite（serde_json 数字输出验证 u8 tag 不变）
    assert_eq!(serde_json::to_string(&Tile::Stalactite).unwrap(), "4");
    assert_eq!(serde_json::to_string(&Tile::CoralReef).unwrap(), "10");
}

/// Dsn24: 各类型地图生成后包含预期特征方块（多种子）
#[test]
fn test_map_kind_features_present() {
    use crate::{MapKind, Tile, map_kind_for};
    use rand::SeedableRng;
    for seed in 0..40u64 {
        for floor in [2u32, 3, 4, 5, 6, 7, 8] {
            let kind = map_kind_for(seed, floor);
            let mut rng = rand::rngs::SmallRng::seed_from_u64(seed.wrapping_add(floor as u64 * 7));
            let mut map = Map::new();
            map.generate(kind, &mut rng);
            match kind {
                MapKind::Cavern => {
                    assert!(
                        map.count_tile(Tile::Stalactite) > 0,
                        "seed {} f{}: 洞穴应有钟乳石",
                        seed,
                        floor
                    );
                }
                MapKind::LushCavern => {
                    assert!(
                        map.count_tile(Tile::Mycelium) + map.count_tile(Tile::FungalPatch) > 0,
                        "seed {} f{}: 繁茂应有菌丝/蘑菇",
                        seed,
                        floor
                    );
                    assert!(
                        map.count_tile(Tile::DeepWater) == 0,
                        "seed {} f{}: 繁茂应无深水",
                        seed,
                        floor
                    );
                }
                MapKind::Undersea => {
                    assert!(
                        map.count_tile(Tile::DeepWater) > 0,
                        "seed {} f{}: 地海应有深水",
                        seed,
                        floor
                    );
                    assert!(
                        map.count_tile(Tile::CoralReef) > 0,
                        "seed {} f{}: 地海应有珊瑚礁",
                        seed,
                        floor
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod g25_tests {
    use super::*;
    use crate::Map;
    use rand::SeedableRng;

    /// G25 回归：ensure_connectivity 收尾后区域间必须 4 方向可达（2x2 收尾）。
    /// 构造两个隔离的 Floor 区域（不相连），调用 ensure_connectivity 后断言连通。
    #[test]
    fn test_ensure_connectivity_connects_isolated_regions() {
        for seed in 0..20u64 {
            let mut map = Map::new();
            // 全墙地图 + 两个 5x5 区域
            for y in 10..15 {
                for x in 10..15 {
                    map.tiles[y][x] = Tile::Floor;
                }
            }
            for y in 30..35 {
                for x in 50..55 {
                    map.tiles[y][x] = Tile::Floor;
                }
            }
            let regions = collect_walkable_regions(&map);
            assert_eq!(regions.len(), 2, "seed {}: 测试前提——两个隔离区域", seed);
            let from = regions[0][regions[0].len() / 2];
            let to = regions[1][0];

            let mut rng = rand::rngs::SmallRng::seed_from_u64(seed);
            ensure_connectivity(&mut map, &mut rng, seed.wrapping_add(1));

            assert!(
                has_path_between(&map, from, to),
                "seed {}: 区域 ({:?}) → ({:?}) 应连通（G25 2x2 收尾）",
                seed,
                from,
                to
            );
        }
    }

    /// carve_2x2：一次调用挖 4 格（当前格 + 右/下/右下）
    #[test]
    fn test_carve_2x2_digs_block() {
        let mut map = Map::new();
        carve_2x2(&mut map, 10, 10);
        assert!(map.tiles[10][10].walkable());
        assert!(map.tiles[10][11].walkable());
        assert!(map.tiles[11][10].walkable());
        assert!(map.tiles[11][11].walkable());
    }
}
