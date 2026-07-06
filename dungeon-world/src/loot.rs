//! 怪物掉落表定义

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
