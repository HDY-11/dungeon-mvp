//! 掉落表的测试。
//!
//! 掉落是**随机**的，所以这里不去断言"具体掉了什么"（那会把测试绑死在一颗种子上），
//! 而是断言四件不会漂的事：
//!
//! 1. 同一颗种子 → 同一串结果（确定性，回放与存档靠它）；
//! 2. `chance == 1.0` 必掉、`chance == 0.0` 必不掉（边界）；
//! 3. 数量恒在 `min..=max` 之间（**上界闭合**——写 `..max` 会少一个，且没人发现）；
//! 4. 每个物种的掉落表引用**存在的**物品，且概率在 `0..=1`。

use super::*;
use crate::monster::{MonsterKindId, monster_template};
use crate::resources::GameRng;

/// 同一颗种子取两次随机流，结果必须逐项相同。
#[test]
fn loot_is_deterministic() {
    let table = monster_template(MonsterKindId::Goblin).loot;
    let roll_once = |seed: u64| {
        let mut rng = GameRng::new(seed);
        let mut all = Vec::new();
        // 连掷 50 次，把"序列"也纳入比较（只看一次太容易偶然相同）。
        for _ in 0..50 {
            all.push(roll_loot(table, &mut rng));
        }
        all
    };
    assert_eq!(roll_once(7), roll_once(7), "同种子必须给出同一串掉落");
    assert_ne!(roll_once(7), roll_once(8), "不同种子应当给出不同结果");
}

/// 概率 1.0 必掉，0.0 必不掉，数量落在闭区间内。
#[test]
fn chance_and_count_bounds_are_respected() {
    let table = [
        LootEntry::always(ItemId(10), 2, 4),
        LootEntry::maybe(ItemId(11), 0.0, 1, 5),
    ];
    let mut rng = GameRng::new(1234);
    for _ in 0..200 {
        let drops = roll_loot(&table, &mut rng);
        assert_eq!(drops.len(), 1, "只有「必掉」那一条会掉");
        let (item, count) = drops[0];
        assert_eq!(item, ItemId(10));
        assert!(
            (2..=4).contains(&count),
            "数量 {count} 越界（上界必须闭合）"
        );
    }
}

/// 固定数量（`min == max`）不消耗数量掷骰，但概率判定仍消耗一次。
///
/// 这条钉住的是**随机流消耗的次数**：它决定了"改一条掉率"会不会连带改变
/// 后面所有条目的结果。口径是"每条目固定消耗 1 次（概率）+ 可选 1 次（数量）"。
#[test]
fn a_fixed_count_entry_still_rolls_its_chance() {
    let mut rng = GameRng::new(99);
    let before = rng.steps;
    let table = [LootEntry::always(ItemId(10), 1, 1)];
    let drops = roll_loot(&table, &mut rng);
    assert_eq!(drops, vec![(ItemId(10), 1)]);
    assert_eq!(rng.steps - before, 1, "固定数量：只掷概率那一次");
}

/// 空表什么都不掉，也不消耗随机数。
#[test]
fn an_empty_table_drops_nothing() {
    let mut rng = GameRng::new(5);
    let before = rng.steps;
    assert!(roll_loot(&[], &mut rng).is_empty());
    assert_eq!(rng.steps, before);
}

/// 每个物种的掉落表都只引用**注册表里存在**的物品，且概率/数量合法。
///
/// 这条是整张怪物表 × 物品表的交叉校验：任何一条写错编号的掉落
/// （`ItemId(15)` 之类）都会在这里暴露，而不是在玩家打怪时 panic。
#[test]
fn every_loot_table_references_registered_items() {
    let kinds = [
        MonsterKindId::Rat,
        MonsterKindId::Scorpion,
        MonsterKindId::Goblin,
        MonsterKindId::Sporeling,
        MonsterKindId::MushroomGolem,
        MonsterKindId::CaveFish,
        MonsterKindId::CaveCrab,
        MonsterKindId::DeepEel,
    ];

    for kind in kinds {
        let template = monster_template(kind);
        assert!(
            !template.loot.is_empty(),
            "{} 没有任何掉落——玩家会发现打它「什么都不给」",
            template.name
        );
        for entry in template.loot {
            assert!(
                entry.item.exists(),
                "{} 的掉落引用了不存在的物品编号 {}",
                template.name,
                entry.item.0
            );
            assert!(
                (0.0..=1.0).contains(&entry.chance),
                "{} 的掉落概率 {} 越界",
                template.name,
                entry.chance
            );
            assert!(
                entry.min >= 1 && entry.min <= entry.max,
                "{} 的掉落数量区间非法（{}..={}）",
                template.name,
                entry.min,
                entry.max
            );
        }
    }
}

/// 掉落表里的物品**都堆得起来**（`max_stack > 1` 或恰好 1 件）：
/// 数量区间上界不得超过该物品的堆叠上限，否则一次掉落就撑破背包一格。
#[test]
fn loot_counts_fit_within_the_item_stack_limit() {
    let kinds = [
        MonsterKindId::Rat,
        MonsterKindId::Scorpion,
        MonsterKindId::Goblin,
        MonsterKindId::Sporeling,
        MonsterKindId::MushroomGolem,
        MonsterKindId::CaveFish,
        MonsterKindId::CaveCrab,
        MonsterKindId::DeepEel,
    ];
    for kind in kinds {
        let template = monster_template(kind);
        for entry in template.loot {
            let def = entry.item.def();
            assert!(
                entry.max <= def.max_stack,
                "{} 掉 {} 的 {}-{} 个，超过堆叠上限 {}",
                template.name,
                def.name,
                entry.min,
                entry.max,
                def.max_stack
            );
        }
    }
}

/// 装备来源：至少有**一个物种**会在战斗中掉出可装备物品。
///
/// 没有这条，装备系统就只剩"地上随机刷的几件"，玩家感受不到"打怪能换装"。
#[test]
fn at_least_one_species_drops_equipment() {
    let kinds = [
        MonsterKindId::Rat,
        MonsterKindId::Scorpion,
        MonsterKindId::Goblin,
        MonsterKindId::Sporeling,
        MonsterKindId::MushroomGolem,
        MonsterKindId::CaveFish,
        MonsterKindId::CaveCrab,
        MonsterKindId::DeepEel,
    ];
    let drops_equipment = kinds.iter().any(|kind| {
        monster_template(*kind)
            .loot
            .iter()
            .any(|entry| entry.item.def().is_equipment())
    });
    assert!(drops_equipment, "没有任何物种掉装备——装备只能在地上捡");
}
