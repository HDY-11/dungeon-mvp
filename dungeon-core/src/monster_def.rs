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

/// 为每一间可用房间独立掷骰决定是否生成怪物。
/// spawn_chance 随楼层递增：1 层 ≈70%（~7 只/10 间房），高层渐近 95%。
pub fn roll_monster_kinds(room_count: usize, floor: u32, rng: &mut impl Rng) -> Vec<MonsterKindId> {
    use rand::RngExt;
    let spawn_chance = (0.7 + floor as f32 * 0.04).min(0.95);
    let all = [MonsterKindId::Rat, MonsterKindId::Scorpion, MonsterKindId::Goblin];
    let mut result = Vec::new();
    for _ in 0..room_count {
        if rng.random_range(0.0..1.0) < spawn_chance {
            // 加权随机选种类
            let weights: Vec<f32> = all.iter().map(|k| monster_spawn_weight(*k, floor)).collect();
            let total: f32 = weights.iter().sum();
            let roll = rng.random_range(0.0..total);
            let mut acc = 0.0;
            for (i, &w) in weights.iter().enumerate() {
                acc += w;
                if roll < acc { result.push(all[i]); break; }
            }
        }
    }
    result
}
