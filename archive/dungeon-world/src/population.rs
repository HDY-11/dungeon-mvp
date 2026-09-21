//! 怪物种群生成算法（噪声密度层 + 元胞扩散）
//!
//! 自 dungeon-core/monster_def.rs 迁移至此。该算法是世界初始化逻辑，
//! 而非怪物数据定义。

use dungeon_core::{MAP_HEIGHT, MAP_WIDTH, MonsterKindId, Tile};
use rand::Rng;

/// 用噪声密度层 + 元胞扩散生成怪物种群（Dsn24: 种类权重按 map_kind 分派）。
pub fn generate_monster_population(
    map_kind: dungeon_core::MapKind,
    tiles: &[[Tile; MAP_WIDTH]; MAP_HEIGHT],
    floor: u32,
    rng: &mut impl Rng,
    exclude: &[(usize, usize)],
) -> Vec<(MonsterKindId, usize, usize)> {
    use rand::RngExt;

    let threshold = (0.38 - floor as f32 * 0.012).max(0.15);
    let expand_chance = 0.35;
    let min_count = (floor as usize).saturating_mul(2).saturating_add(4);
    let max_count = (floor as usize).saturating_mul(4).saturating_add(8);

    // Phase 1: 噪声密度层
    let mut density = [[0.0f32; MAP_WIDTH]; MAP_HEIGHT];
    for y in 0..MAP_HEIGHT {
        for x in 0..MAP_WIDTH {
            if tiles[y][x].walkable() {
                density[y][x] = rng.random_range(0.0..1.0);
            }
        }
    }

    // Phase 2: 初始种子
    let mut is_monster = [[false; MAP_WIDTH]; MAP_HEIGHT];
    for y in 0..MAP_HEIGHT {
        for x in 0..MAP_WIDTH {
            if density[y][x] > threshold {
                is_monster[y][x] = true;
            }
        }
    }

    // Phase 3: 元胞扩散
    for _pass in 0..3 {
        let snapshot = is_monster;
        let mut added = 0usize;
        for y in 0..MAP_HEIGHT {
            for x in 0..MAP_WIDTH {
                if snapshot[y][x] || !tiles[y][x].walkable() {
                    continue;
                }
                let has_neighbor = {
                    let mut n = false;
                    for dy in [-1isize, 0, 1] {
                        for dx in [-1isize, 0, 1] {
                            if dx == 0 && dy == 0 {
                                continue;
                            }
                            let ny = y.wrapping_add_signed(dy);
                            let nx = x.wrapping_add_signed(dx);
                            if nx < MAP_WIDTH && ny < MAP_HEIGHT && snapshot[ny][nx] {
                                n = true;
                            }
                        }
                    }
                    n
                };
                if has_neighbor && rng.random_range(0.0..1.0) < expand_chance {
                    is_monster[y][x] = true;
                    added += 1;
                }
            }
        }
        if added == 0 {
            break;
        }
    }

    // Phase 4: 收集 + 钳制（排除 exclude）
    let mut positions: Vec<(usize, usize)> = Vec::new();
    for (y, row) in is_monster.iter().enumerate() {
        for (x, &has) in row.iter().enumerate() {
            if has && !exclude.contains(&(x, y)) {
                positions.push((x, y));
            }
        }
    }

    // I82: 补足循环加迭代上限（40×期望数）——可行走格不足时不再死循环，
    // 随机命中未占格概率随填充度下降，上限同时约束最坏耗时
    let max_attempts = min_count.saturating_mul(40);
    let mut attempts = 0;
    while positions.len() < min_count && attempts < max_attempts {
        attempts += 1;
        let x = rng.random_range(3..MAP_WIDTH - 3);
        let y = rng.random_range(3..MAP_HEIGHT - 3);
        if tiles[y][x].walkable() && !positions.contains(&(x, y)) && !exclude.contains(&(x, y)) {
            positions.push((x, y));
        }
    }
    if positions.len() < min_count {
        log::warn!(
            "怪物补足不足：期望 {} 只，实际 {} 只（可行走格不足或排除集过大）",
            min_count,
            positions.len()
        );
    }

    while positions.len() > max_count {
        let idx = rng.random_range(0..positions.len());
        positions.swap_remove(idx);
    }

    // Phase 5: 分配种类
    positions
        .into_iter()
        .map(|(x, y)| {
            (
                dungeon_core::monster_def::roll_one_kind(map_kind, floor, rng),
                x,
                y,
            )
        })
        .collect()
}
