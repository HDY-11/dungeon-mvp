//! 怪物掉落表定义（保留供 persist.rs 旧版存档兼容引用）
//! 新代码请使用 dungeon_core::monster_def::monster_loot()

use dungeon_core::{LootTable, LootEntry};

pub fn rat_loot() -> LootTable {
    LootTable {
        entries: vec![
            LootEntry { item_id: 10, chance: 1.0, min_count: 1, max_count: 2 },
        ],
    }
}

pub fn goblin_loot() -> LootTable {
    LootTable {
        entries: vec![
            LootEntry { item_id: 10, chance: 1.0, min_count: 1, max_count: 3 },
            LootEntry { item_id: 11, chance: 0.6, min_count: 1, max_count: 1 },
            LootEntry { item_id: 12, chance: 0.4, min_count: 1, max_count: 1 },
            LootEntry { item_id: 13, chance: 0.3, min_count: 1, max_count: 1 },
        ],
    }
}

pub fn scorpion_loot() -> LootTable {
    LootTable {
        entries: vec![
            LootEntry { item_id: 10, chance: 1.0, min_count: 1, max_count: 2 },
            LootEntry { item_id: 14, chance: 1.0, min_count: 1, max_count: 2 },
        ],
    }
}
