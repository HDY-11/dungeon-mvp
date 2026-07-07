//! 怪物定义：种类标识、属性公式、掉落、生成权重与概率选择

use crate::{MonsterKindId, Stats, LootTable, LootEntry};
use rand::Rng;

// ── 外观 ────────────────────────────────────────────

pub fn monster_glyph(kind: MonsterKindId) -> char {
    match kind {
        MonsterKindId::Rat => 'r',
        MonsterKindId::Scorpion => 's',
        MonsterKindId::Goblin => 'g',
    }
}

pub fn monster_color(kind: MonsterKindId) -> (u8, u8, u8) {
    match kind {
        MonsterKindId::Rat => (255, 0, 0),
        MonsterKindId::Scorpion => (180, 180, 0),    // 土黄色
        MonsterKindId::Goblin => (0, 255, 0),
    }
}

pub fn monster_name(kind: MonsterKindId) -> &'static str {
    match kind {
        MonsterKindId::Rat => "老鼠",
        MonsterKindId::Scorpion => "变异蝎子",
        MonsterKindId::Goblin => "哥布林",
    }
}

pub fn monster_attack_name(kind: MonsterKindId) -> &'static str {
    match kind {
        MonsterKindId::Rat => "撕咬",
        MonsterKindId::Scorpion => "螫刺",
        MonsterKindId::Goblin => "重击",
    }
}

// ── 属性公式 ────────────────────────────────────────

pub fn monster_stats(kind: MonsterKindId, floor: u32) -> Stats {
    let lvl = floor.saturating_sub(1);
    let s = lvl as f64;
    match kind {
        MonsterKindId::Rat => Stats {
            level: (1 + lvl).min(20),
            hp: 10 + (s * 4.0) as i32,
            max_hp: 10 + (s * 4.0) as i32,
            mp: 0, max_mp: 0,
            exp: (6.0 + s * 6.0 * 0.5).round() as u64,
            exp_to_next: 0,
            attack: (4 + lvl).min(18) as u32,
            defense: 2, agility: 5, magic_mastery: 1,
            crit_rate: 0.05, crit_damage: 0.50,
        },
        MonsterKindId::Scorpion => Stats {
            level: (1 + lvl).min(20),
            hp: 14 + (s * 5.0) as i32,
            max_hp: 14 + (s * 5.0) as i32,
            mp: 0, max_mp: 0,
            exp: (10.0 + s * 10.0 * 0.5).round() as u64,
            exp_to_next: 0,
            attack: (5 + (s * 1.5) as u32).min(20),
            defense: 3, agility: 4, magic_mastery: 1,
            crit_rate: 0.05, crit_damage: 0.50,
        },
        MonsterKindId::Goblin => Stats {
            level: (1 + lvl).min(20),
            hp: 18 + (s * 6.0) as i32,
            max_hp: 18 + (s * 6.0) as i32,
            mp: 0, max_mp: 0,
            exp: (15.0 + s * 15.0 * 0.5).round() as u64,
            exp_to_next: 0,
            attack: (6 + lvl * 2).min(25) as u32,
            defense: 4, agility: 3, magic_mastery: 3,
            crit_rate: 0.05, crit_damage: 0.50,
        },
    }
}

// ── 掉落表 ──────────────────────────────────────────

pub fn monster_loot(kind: MonsterKindId) -> LootTable {
    match kind {
        MonsterKindId::Rat => LootTable {
            entries: vec![
                LootEntry { item_id: 10, chance: 1.0, min_count: 1, max_count: 2 },
            ],
        },
        MonsterKindId::Scorpion => LootTable {
            entries: vec![
                LootEntry { item_id: 10, chance: 1.0, min_count: 1, max_count: 2 },
                LootEntry { item_id: 14, chance: 1.0, min_count: 1, max_count: 2 },
            ],
        },
        MonsterKindId::Goblin => LootTable {
            entries: vec![
                LootEntry { item_id: 10, chance: 1.0, min_count: 1, max_count: 3 },
                LootEntry { item_id: 11, chance: 0.6, min_count: 1, max_count: 1 },
                LootEntry { item_id: 12, chance: 0.4, min_count: 1, max_count: 1 },
                LootEntry { item_id: 13, chance: 0.3, min_count: 1, max_count: 1 },
            ],
        },
    }
}

// ── 生成权重与选择 ────────────────────────────────

/// 每种怪物在各楼层的生成权重（数值越高出现概率越大）
pub fn monster_spawn_weight(kind: MonsterKindId, floor: u32) -> f32 {
    let f = floor as f32;
    match kind {
        MonsterKindId::Rat      => (30.0 - f * 2.0).max(0.5),
        MonsterKindId::Scorpion => (20.0 + f * 1.0).min(40.0),
        MonsterKindId::Goblin   => ( 5.0 + f * 2.5).min(45.0),
    }
}

/// 按楼层缩放加权选一种怪物种类
fn roll_one_kind(floor: u32, rng: &mut impl Rng) -> MonsterKindId {
    use rand::RngExt;
    let all = [MonsterKindId::Rat, MonsterKindId::Scorpion, MonsterKindId::Goblin];
    let weights: Vec<f32> = all.iter().map(|k| monster_spawn_weight(*k, floor)).collect();
    let total: f32 = weights.iter().sum();
    let roll = rng.random_range(0.0..total);
    let mut acc = 0.0;
    for (i, &w) in weights.iter().enumerate() {
        acc += w;
        if roll < acc { return all[i]; }
    }
    all[0]
}

/// 用噪声密度层 + 元胞扩散生成怪物种群。
///
/// # 流程
/// 1. 对每格 walkable 地形生成伪随机密度值 → 密度层
/// 2. 密度 > 阈值的格子成为初始种子
/// 3. 邻接种子的 walkable 格以一定概率扩展为怪物格（元胞扩散，聚类）
/// 4. 数量钳制到 [min_count, max_count] 区间
/// 5. 每只独立按楼层加权分配种类
///
/// # 参数
/// - `tiles`: 地图地形，walkable 格才是候选
/// - `floor`: 当前楼层，影响密度阈值和数量区间
/// - `rng`: 随机源
///
/// # 返回
/// `Vec<(MonsterKindId, usize, usize)>` — (种类, x, y)
pub fn generate_monster_population(
    tiles: &[[crate::Tile; crate::MAP_WIDTH]; crate::MAP_HEIGHT],
    floor: u32,
    rng: &mut impl Rng,
    exclude: &[(usize, usize)],
) -> Vec<(MonsterKindId, usize, usize)> {
    use rand::RngExt;

    // ── 参数 ──
    // 密度阈值：楼层越高阈值越低（深层怪物更密集）
    let threshold = (0.38 - floor as f32 * 0.012).max(0.15);
    // 元胞扩散概率
    let expand_chance = 0.35;
    // 数量上下限：随楼层线性增长
    let min_count = (floor as usize).saturating_mul(2).saturating_add(4);
    let max_count = (floor as usize).saturating_mul(4).saturating_add(8);

    // ── Phase 1: 噪声密度层（仅 walkable 格）──
    let mut density = [[0.0f32; crate::MAP_WIDTH]; crate::MAP_HEIGHT];
    for y in 0..crate::MAP_HEIGHT {
        for x in 0..crate::MAP_WIDTH {
            if tiles[y][x].walkable() {
                density[y][x] = rng.random_range(0.0..1.0);
            }
        }
    }

    // ── Phase 2: 初始种子（密度 > 阈值）──
    let mut is_monster = [[false; crate::MAP_WIDTH]; crate::MAP_HEIGHT];
    for y in 0..crate::MAP_HEIGHT {
        for x in 0..crate::MAP_WIDTH {
            if density[y][x] > threshold {
                is_monster[y][x] = true;
            }
        }
    }

    // ── Phase 3: 元胞扩散（聚类）──
    // 最多扩散 3 轮，每轮邻接种子的 walkable 格以 expand_chance 变成怪物格
    for _pass in 0..3 {
        let snapshot = is_monster;
        let mut added = 0usize;
        for y in 0..crate::MAP_HEIGHT {
            for x in 0..crate::MAP_WIDTH {
                if snapshot[y][x] || !tiles[y][x].walkable() { continue; }
                // 检查 8 方向是否有怪物邻居
                let has_neighbor = {
                    let mut n = false;
                    for dy in [-1isize, 0, 1] {
                        for dx in [-1isize, 0, 1] {
                            if dx == 0 && dy == 0 { continue; }
                            let ny = y.wrapping_add_signed(dy);
                            let nx = x.wrapping_add_signed(dx);
                            if nx < crate::MAP_WIDTH && ny < crate::MAP_HEIGHT && snapshot[ny][nx] {
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
        if added == 0 { break; }
    }

    // ── Phase 4: 收集并钳制数量（排除 exclude 中的坐标）──
    let mut positions: Vec<(usize, usize)> = Vec::new();
    for y in 0..crate::MAP_HEIGHT {
        for x in 0..crate::MAP_WIDTH {
            if is_monster[y][x] && !exclude.contains(&(x, y)) {
                positions.push((x, y));
            }
        }
    }

    // 低于下限 → 在可行走格上随机补充（避开 exclude）
    while positions.len() < min_count {
        let x = rng.random_range(3..crate::MAP_WIDTH - 3);
        let y = rng.random_range(3..crate::MAP_HEIGHT - 3);
        if tiles[y][x].walkable() && !positions.contains(&(x, y)) && !exclude.contains(&(x, y)) {
            positions.push((x, y));
        }
    }

    // 高于上限 → 随机裁剪
    while positions.len() > max_count {
        let idx = rng.random_range(0..positions.len());
        positions.swap_remove(idx);
    }

    // ── Phase 5: 分配种类 ──
    positions.into_iter().map(|(x, y)| (roll_one_kind(floor, rng), x, y)).collect()
}
