//! 掉落表：**每个物种一行 `&'static [LootEntry]`**（GAME.md Gm7「掉落表」）。
//!
//! # 为什么放在物品模块而不是怪物模块
//!
//! `MonsterTemplate` 已经有 `loot` 字段（H6/H7 定下的"一个物种一行"），但
//! **掉落表的数据**留在这里：条目要引用 `ItemId`，而 `ItemId` 的权威在物品注册表。
//! 怪物模块只持有 `&'static [LootEntry]` 这个切片——于是"加一件可掉落物品"只需要
//! 改物品相关两个文件，不必进怪物模块。
//!
//! # 掷骰口径
//!
//! 每条目**独立**掷一次 `chance`（与旧实现一致，GAME.md Gm7 的设计参考写明了这一点）。
//! 因此"掉率 12% 的两件武器"可以同时掉，而"必掉 + 概率"是两条独立的事。
//!
//! 数量 `min..=max` 也在同一次判定里掷——顺序固定为"先判概率、再掷数量"，
//! 保证同一颗种子在同一套表上永远给出同一串结果（`loot_is_deterministic` 钉住）。

use crate::item::registry::ItemId;
use crate::resources::GameRng;

/// 一条掉落：掉什么、多大概率、几个。
#[derive(Debug, Clone, Copy)]
pub struct LootEntry {
    pub item: ItemId,
    /// 独立判定概率（`0.0..=1.0`）。
    pub chance: f64,
    /// 数量下界（含）。
    pub min: u32,
    /// 数量上界（含）；`min == max` 时固定数量。
    pub max: u32,
}

impl LootEntry {
    /// 必掉一条。
    pub const fn always(item: ItemId, min: u32, max: u32) -> Self {
        Self {
            item,
            chance: 1.0,
            min,
            max,
        }
    }

    /// 概率掉落一条。
    pub const fn maybe(item: ItemId, chance: f64, min: u32, max: u32) -> Self {
        Self {
            item,
            chance,
            min,
            max,
        }
    }
}

/// 掷一次掉落表：返回实际掉出来的 `(物品, 数量)` 列表。
///
/// **纯 `GameRng` 消费者**：不碰世界、不 spawn。调用方（死亡结算）决定掉出来的东西
/// 放在哪。顺序严格是"逐条：概率 → 数量"，所以结果只取决于种子与表的长度。
pub fn roll_loot(table: &[LootEntry], rng: &mut GameRng) -> Vec<(ItemId, u32)> {
    let mut drops = Vec::new();
    for entry in table {
        // 概率判定固定消耗一次随机数（即使 `chance == 1.0` 也判）——
        // 让"改一条概率"不会连带改变后面所有条目的随机流。
        let roll = rng.random_f64();
        if roll >= entry.chance {
            continue;
        }
        let count = if entry.max <= entry.min {
            entry.min
        } else {
            // `random_range` 是 `[lo, hi)`，所以上界要 +1 才能取到 `max`。
            entry.min + rng.random_range(0, u64::from(entry.max - entry.min) + 1) as u32
        };
        if count > 0 {
            drops.push((entry.item, count));
        }
    }
    drops
}

#[cfg(test)]
#[path = "loot_tests.rs"]
mod tests;
