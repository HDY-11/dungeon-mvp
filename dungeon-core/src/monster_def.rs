//! 怪物定义：种类标识、属性公式、掉落、生成权重与概率选择

use crate::{LootEntry, LootTable, MonsterKindId, Stats};
use rand::Rng;

// ── 外观 ────────────────────────────────────────────

pub fn monster_glyph(kind: MonsterKindId) -> char {
    match kind {
        MonsterKindId::Rat => 'r',
        MonsterKindId::Scorpion => 's',
        MonsterKindId::Goblin => 'g',
        MonsterKindId::Sporeling => 'm',
        MonsterKindId::MushroomGolem => 'M',
        MonsterKindId::CaveFish => 'f',
        MonsterKindId::CaveCrab => 'c',
        MonsterKindId::DeepEel => 'e',
    }
}

pub fn monster_color(kind: MonsterKindId) -> (u8, u8, u8) {
    match kind {
        MonsterKindId::Rat => (255, 0, 0),
        MonsterKindId::Scorpion => (180, 180, 0), // 土黄色
        MonsterKindId::Goblin => (0, 255, 0),
        MonsterKindId::Sporeling => (140, 255, 140), // 淡绿
        MonsterKindId::MushroomGolem => (200, 120, 220), // 紫
        MonsterKindId::CaveFish => (120, 200, 255),  // 水蓝
        MonsterKindId::CaveCrab => (255, 140, 80),   // 橙
        MonsterKindId::DeepEel => (80, 160, 220),    // 深蓝
    }
}

pub fn monster_name(kind: MonsterKindId) -> &'static str {
    match kind {
        MonsterKindId::Rat => "老鼠",
        MonsterKindId::Scorpion => "变异蝎子",
        MonsterKindId::Goblin => "哥布林",
        MonsterKindId::Sporeling => "孢子怪",
        MonsterKindId::MushroomGolem => "蘑菇傀儡",
        MonsterKindId::CaveFish => "洞穴鱼",
        MonsterKindId::CaveCrab => "洞穴蟹",
        MonsterKindId::DeepEel => "深鳗",
    }
}

pub fn monster_attack_name(kind: MonsterKindId) -> &'static str {
    match kind {
        MonsterKindId::Rat => "撕咬",
        MonsterKindId::Scorpion => "螫刺",
        MonsterKindId::Goblin => "重击",
        MonsterKindId::Sporeling => "孢子喷吐",
        MonsterKindId::MushroomGolem => "重拳",
        MonsterKindId::CaveFish => "啃咬",
        MonsterKindId::CaveCrab => "钳击",
        MonsterKindId::DeepEel => "缠绕",
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
            mp: 0,
            max_mp: 0,
            exp: (6.0 + s * 6.0 * 0.5).round() as u64,
            exp_to_next: 0,
            attack: (4 + lvl).min(18),
            defense: 2,
            agility: 5,
            magic_mastery: 1,
            crit_rate: 0.05,
            crit_damage: 0.50,
        },
        MonsterKindId::Scorpion => Stats {
            level: (1 + lvl).min(20),
            hp: 14 + (s * 5.0) as i32,
            max_hp: 14 + (s * 5.0) as i32,
            mp: 0,
            max_mp: 0,
            exp: (10.0 + s * 10.0 * 0.5).round() as u64,
            exp_to_next: 0,
            attack: (5 + (s * 1.5) as u32).min(20),
            defense: 3,
            agility: 4,
            magic_mastery: 1,
            crit_rate: 0.05,
            crit_damage: 0.50,
        },
        MonsterKindId::Goblin => Stats {
            level: (1 + lvl).min(20),
            hp: 18 + (s * 6.0) as i32,
            max_hp: 18 + (s * 6.0) as i32,
            mp: 0,
            max_mp: 0,
            exp: (15.0 + s * 15.0 * 0.5).round() as u64,
            exp_to_next: 0,
            attack: (6 + lvl * 2).min(25),
            defense: 4,
            agility: 3,
            magic_mastery: 3,
            crit_rate: 0.05,
            crit_damage: 0.50,
        },
        // Dsn24: 新怪对齐现有定位——弱怪≈老鼠、中怪≈蝎子 [⃞试调]
        MonsterKindId::Sporeling => Stats {
            level: (1 + lvl).min(20),
            hp: 12 + (s * 2.0) as i32,
            max_hp: 12 + (s * 2.0) as i32,
            mp: 0,
            max_mp: 0,
            exp: (6.0 + s * 6.0 * 0.5).round() as u64,
            exp_to_next: 0,
            attack: (4 + lvl).min(16),
            defense: 0,
            agility: 8,
            magic_mastery: 2,
            crit_rate: 0.05,
            crit_damage: 0.50,
        },
        MonsterKindId::MushroomGolem => Stats {
            level: (1 + lvl).min(20),
            hp: 22 + (s * 3.0) as i32,
            max_hp: 22 + (s * 3.0) as i32,
            mp: 0,
            max_mp: 0,
            exp: (11.0 + s * 11.0 * 0.5).round() as u64,
            exp_to_next: 0,
            attack: (7 + (s * 1.5) as u32).min(24),
            defense: 2,
            agility: 4,
            magic_mastery: 4,
            crit_rate: 0.05,
            crit_damage: 0.50,
        },
        MonsterKindId::CaveFish => Stats {
            level: (1 + lvl).min(20),
            hp: 10 + (s * 2.0) as i32,
            max_hp: 10 + (s * 2.0) as i32,
            mp: 0,
            max_mp: 0,
            exp: (6.0 + s * 6.0 * 0.5).round() as u64,
            exp_to_next: 0,
            attack: (3 + lvl).min(15),
            defense: 0,
            agility: 14,
            magic_mastery: 1,
            crit_rate: 0.05,
            crit_damage: 0.50,
        },
        MonsterKindId::CaveCrab => Stats {
            level: (1 + lvl).min(20),
            hp: 18 + (s * 3.0) as i32,
            max_hp: 18 + (s * 3.0) as i32,
            mp: 0,
            max_mp: 0,
            exp: (11.0 + s * 11.0 * 0.5).round() as u64,
            exp_to_next: 0,
            attack: (3 + (s * 1.2) as u32).min(20),
            defense: 4,
            agility: 3,
            magic_mastery: 1,
            crit_rate: 0.05,
            crit_damage: 0.50,
        },
        MonsterKindId::DeepEel => Stats {
            level: (1 + lvl).min(20),
            hp: 15 + (s * 3.0) as i32,
            max_hp: 15 + (s * 3.0) as i32,
            mp: 0,
            max_mp: 0,
            exp: (12.0 + s * 12.0 * 0.5).round() as u64,
            exp_to_next: 0,
            attack: (6 + (s * 1.5) as u32).min(22),
            defense: 1,
            agility: 10,
            magic_mastery: 2,
            crit_rate: 0.05,
            crit_damage: 0.50,
        },
    }
}

// ── 掉落表 ──────────────────────────────────────────

pub fn monster_loot(kind: MonsterKindId) -> LootTable {
    use crate::{
        ITEM_BIOMASS, ITEM_CHITIN, ITEM_CLOTH, ITEM_DAGGER, ITEM_EEL_SKIN, ITEM_FANG,
        ITEM_FISH_BONE, ITEM_MOSS, ITEM_MUSHROOM, ITEM_PEARL, ITEM_SEAWEED, ITEM_SHELL,
        ITEM_SPORE_SAC, ITEM_STICK, ITEM_STONE, ITEM_STONE_HAMMER, ITEM_TEMPLATE_ARMOR,
        ITEM_TEMPLATE_BLADE, ITEM_TEMPLATE_RING, ITEM_TEMPLATE_SHIELD,
    };
    match kind {
        MonsterKindId::Rat => LootTable {
            entries: vec![LootEntry {
                item_id: ITEM_BIOMASS,
                chance: 1.0,
                min_count: 1,
                max_count: 2,
            }],
        },
        MonsterKindId::Scorpion => LootTable {
            entries: vec![
                LootEntry {
                    item_id: ITEM_BIOMASS,
                    chance: 1.0,
                    min_count: 1,
                    max_count: 2,
                },
                LootEntry {
                    item_id: ITEM_CHITIN,
                    chance: 1.0,
                    min_count: 1,
                    max_count: 2,
                },
            ],
        },
        MonsterKindId::Goblin => LootTable {
            entries: vec![
                LootEntry {
                    item_id: ITEM_BIOMASS,
                    chance: 1.0,
                    min_count: 1,
                    max_count: 3,
                },
                LootEntry {
                    item_id: ITEM_CLOTH,
                    chance: 0.6,
                    min_count: 1,
                    max_count: 1,
                },
                LootEntry {
                    item_id: ITEM_STICK,
                    chance: 0.4,
                    min_count: 1,
                    max_count: 1,
                },
                LootEntry {
                    item_id: ITEM_FANG,
                    chance: 0.3,
                    min_count: 1,
                    max_count: 1,
                },
                LootEntry {
                    item_id: ITEM_STONE,
                    chance: 0.6,
                    min_count: 1,
                    max_count: 2,
                },
                // I67: 哥布林携武概率低（石锤/匕首）
                LootEntry {
                    item_id: ITEM_STONE_HAMMER,
                    chance: 0.12,
                    min_count: 1,
                    max_count: 1,
                },
                LootEntry {
                    item_id: ITEM_DAGGER,
                    chance: 0.12,
                    min_count: 1,
                    max_count: 1,
                },
                // I69/Dsn19 Phase 1: 模板碎片掉落（4 种均分）
                LootEntry {
                    item_id: ITEM_TEMPLATE_BLADE,
                    chance: 0.05,
                    min_count: 1,
                    max_count: 1,
                },
                LootEntry {
                    item_id: ITEM_TEMPLATE_SHIELD,
                    chance: 0.05,
                    min_count: 1,
                    max_count: 1,
                },
                LootEntry {
                    item_id: ITEM_TEMPLATE_ARMOR,
                    chance: 0.05,
                    min_count: 1,
                    max_count: 1,
                },
                LootEntry {
                    item_id: ITEM_TEMPLATE_RING,
                    chance: 0.05,
                    min_count: 1,
                    max_count: 1,
                },
            ],
        },
        // Dsn24: 繁茂洞穴生态（真菌掉落） [⃞试调]
        MonsterKindId::Sporeling => LootTable {
            entries: vec![
                LootEntry {
                    item_id: ITEM_MUSHROOM,
                    chance: 0.6,
                    min_count: 1,
                    max_count: 2,
                },
                LootEntry {
                    item_id: ITEM_MOSS,
                    chance: 0.4,
                    min_count: 1,
                    max_count: 1,
                },
            ],
        },
        MonsterKindId::MushroomGolem => LootTable {
            entries: vec![
                LootEntry {
                    item_id: ITEM_MOSS,
                    chance: 0.8,
                    min_count: 1,
                    max_count: 1,
                },
                LootEntry {
                    item_id: ITEM_SPORE_SAC,
                    chance: 0.3,
                    min_count: 1,
                    max_count: 1,
                },
                LootEntry {
                    item_id: ITEM_MUSHROOM,
                    chance: 0.3,
                    min_count: 1,
                    max_count: 2,
                },
            ],
        },
        // Dsn24: 地海水生生态 [⃞试调]
        MonsterKindId::CaveFish => LootTable {
            entries: vec![
                LootEntry {
                    item_id: ITEM_FISH_BONE,
                    chance: 0.6,
                    min_count: 1,
                    max_count: 1,
                },
                LootEntry {
                    item_id: ITEM_SEAWEED,
                    chance: 0.3,
                    min_count: 1,
                    max_count: 1,
                },
            ],
        },
        MonsterKindId::CaveCrab => LootTable {
            entries: vec![
                LootEntry {
                    item_id: ITEM_SHELL,
                    chance: 0.8,
                    min_count: 1,
                    max_count: 1,
                },
                LootEntry {
                    item_id: ITEM_PEARL,
                    chance: 0.1,
                    min_count: 1,
                    max_count: 1,
                },
            ],
        },
        MonsterKindId::DeepEel => LootTable {
            entries: vec![
                LootEntry {
                    item_id: ITEM_EEL_SKIN,
                    chance: 0.6,
                    min_count: 1,
                    max_count: 1,
                },
                LootEntry {
                    item_id: ITEM_SEAWEED,
                    chance: 0.4,
                    min_count: 1,
                    max_count: 1,
                },
            ],
        },
    }
}

// ── 生成权重与选择（Dsn24: 按地图类型分派生态） ──────

/// 每种怪物在各地图类型/楼层的生成权重（数值越高出现概率越大） [⃞试调]
pub fn monster_spawn_weight(map_kind: crate::MapKind, kind: MonsterKindId, floor: u32) -> f32 {
    let f = floor as f32;
    match map_kind {
        crate::MapKind::Cavern => match kind {
            MonsterKindId::Rat => (30.0 - f * 2.0).max(0.5),
            MonsterKindId::Scorpion => (20.0 + f * 1.0).min(40.0),
            MonsterKindId::Goblin => (5.0 + f * 2.5).min(45.0),
            _ => 0.0,
        },
        // 繁茂：真菌生态为主 + 少量洞穴原生物
        crate::MapKind::LushCavern => match kind {
            MonsterKindId::Sporeling => (30.0 - f * 2.0).max(0.5),
            MonsterKindId::MushroomGolem => (5.0 + f * 2.5).min(40.0),
            MonsterKindId::Rat => (12.0 - f * 1.0).max(0.5),
            MonsterKindId::Goblin => (5.0 + f * 1.5).min(30.0),
            _ => 0.0,
        },
        // 地海：水生生态为主 + 少量蝎子
        crate::MapKind::Undersea => match kind {
            MonsterKindId::CaveFish => (30.0 - f * 2.0).max(0.5),
            MonsterKindId::CaveCrab => (12.0 + f * 1.5).min(35.0),
            MonsterKindId::DeepEel => (8.0 + f * 2.0).min(35.0),
            MonsterKindId::Scorpion => (8.0 + f * 0.5).min(20.0),
            _ => 0.0,
        },
    }
}

/// 按地图类型 + 楼层缩放加权选一种怪物种类
pub fn roll_one_kind(map_kind: crate::MapKind, floor: u32, rng: &mut impl Rng) -> MonsterKindId {
    use rand::RngExt;
    let all: &[MonsterKindId] = match map_kind {
        crate::MapKind::Cavern => &[
            MonsterKindId::Rat,
            MonsterKindId::Scorpion,
            MonsterKindId::Goblin,
        ],
        crate::MapKind::LushCavern => &[
            MonsterKindId::Sporeling,
            MonsterKindId::MushroomGolem,
            MonsterKindId::Rat,
            MonsterKindId::Goblin,
        ],
        crate::MapKind::Undersea => &[
            MonsterKindId::CaveFish,
            MonsterKindId::CaveCrab,
            MonsterKindId::DeepEel,
            MonsterKindId::Scorpion,
        ],
    };
    let weights: Vec<f32> = all
        .iter()
        .map(|k| monster_spawn_weight(map_kind, *k, floor))
        .collect();
    let total: f32 = weights.iter().sum();
    let roll = rng.random_range(0.0..total);
    let mut acc = 0.0;
    for (i, &w) in weights.iter().enumerate() {
        acc += w;
        if roll < acc {
            return all[i];
        }
    }
    all[0]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MapKind;

    /// Dsn24: 全部 8 种怪物的定义完整（外观/属性/掉落不 panic 且非空）
    #[test]
    fn test_all_monster_defs_complete() {
        let all = [
            MonsterKindId::Rat,
            MonsterKindId::Scorpion,
            MonsterKindId::Goblin,
            MonsterKindId::Sporeling,
            MonsterKindId::MushroomGolem,
            MonsterKindId::CaveFish,
            MonsterKindId::CaveCrab,
            MonsterKindId::DeepEel,
        ];
        for &k in &all {
            assert!(!monster_name(k).is_empty());
            assert!(!monster_attack_name(k).is_empty());
            assert_ne!(monster_glyph(k), '\0');
            let st = monster_stats(k, 1);
            assert!(st.hp > 0 && st.attack > 0, "{:?} F1 属性异常", k);
            let loot = monster_loot(k);
            assert!(!loot.entries.is_empty(), "{:?} 掉落表为空", k);
            // 繁茂/地海怪物只在其生态类型中有生成权重
            let in_lush = monster_spawn_weight(MapKind::LushCavern, k, 3) > 0.0;
            let in_sea = monster_spawn_weight(MapKind::Undersea, k, 3) > 0.0;
            match k {
                MonsterKindId::Sporeling | MonsterKindId::MushroomGolem => {
                    assert!(in_lush && !in_sea, "{:?} 应只属于繁茂生态", k);
                }
                MonsterKindId::CaveFish | MonsterKindId::CaveCrab | MonsterKindId::DeepEel => {
                    assert!(!in_lush && in_sea, "{:?} 应只属于地海生态", k);
                }
                _ => {}
            }
        }
    }

    /// Dsn24: 各类型加权选择只返回该生态的怪物
    #[test]
    fn test_roll_one_kind_by_map_kind() {
        use rand::SeedableRng;
        let mut rng = rand::rngs::SmallRng::seed_from_u64(7);
        for _ in 0..200 {
            let k = roll_one_kind(MapKind::LushCavern, 3, &mut rng);
            assert!(matches!(
                k,
                MonsterKindId::Sporeling
                    | MonsterKindId::MushroomGolem
                    | MonsterKindId::Rat
                    | MonsterKindId::Goblin
            ));
        }
        let mut rng = rand::rngs::SmallRng::seed_from_u64(7);
        for _ in 0..200 {
            let k = roll_one_kind(MapKind::Undersea, 4, &mut rng);
            assert!(matches!(
                k,
                MonsterKindId::CaveFish
                    | MonsterKindId::CaveCrab
                    | MonsterKindId::DeepEel
                    | MonsterKindId::Scorpion
            ));
        }
    }
}
