//! 怪物生成：**种类池**与**加权挑选**。
//!
//! 物种的数值、能力与出现权重都在 `template.rs`（Phase H6/H7，ISSUES ECS32）。
//! 这里只剩跨物种的两件事：
//!
//! 1. **种类池**：某张地图里有哪些物种。池的**顺序是 RNG 敏感**的——`roll_one_kind`
//!    按池序累加权重，所以池序变了，同一颗种子会生成不同的怪。池因此是显式数组，
//!    不是"按枚举顺序自动派生"（那样 LushCavern 的池会从
//!    `[孢子怪, 蘑菇傀儡, 鼠, 哥布林]` 悄悄变成 `[鼠, 哥布林, 孢子怪, 蘑菇傀儡]`）；
//!    `kind_pools_keep_their_declared_order` 钉死顺序。
//! 2. **加权挑选**：把 `[0, total)` 的一次抽样映射到池里的下标。
//!
//! 为什么权重按地图类型分开、仍留在模板里：调平衡时"这个怪多强"与"这个怪多常见"
//! 是两个独立旋钮（混在一处就得同时改两件事），而"在哪张地图出现"是物种的内容属性
//! ——两者都天然属于"这一行物种数据"，因此归位后同住 `template.rs`。

use crate::components::*;
use crate::map::MapKind;
use crate::monster::{Ability, MonsterKindId, monster_template};
use bevy_ecs::prelude::EntityCommands;
use rand::Rng;

/// 按地图类型 + 楼层计算生成权重（读模板；模板里没有该地图的条目 = `0.0`）。
pub fn monster_spawn_weight(map_kind: MapKind, kind: MonsterKindId, floor: u32) -> f64 {
    monster_template(kind).spawn_weight(map_kind, floor)
}

const CAVERN_KINDS: [MonsterKindId; 3] = [
    MonsterKindId::Rat,
    MonsterKindId::Scorpion,
    MonsterKindId::Goblin,
];
const LUSH_KINDS: [MonsterKindId; 4] = [
    MonsterKindId::Sporeling,
    MonsterKindId::MushroomGolem,
    MonsterKindId::Rat,
    MonsterKindId::Goblin,
];
const UNDERSEA_KINDS: [MonsterKindId; 4] = [
    MonsterKindId::CaveFish,
    MonsterKindId::CaveCrab,
    MonsterKindId::DeepEel,
    MonsterKindId::Scorpion,
];

/// 某张地图的种类池。**返回值的顺序是行为契约**（见模块文档）。
pub fn kinds_for(map_kind: MapKind) -> &'static [MonsterKindId] {
    match map_kind {
        MapKind::Cavern => &CAVERN_KINDS,
        MapKind::LushCavern => &LUSH_KINDS,
        MapKind::Undersea => &UNDERSEA_KINDS,
    }
}

/// 按权重累积挑选下标；`roll` 必须落在 `[0, total)`。
///
/// 抽出来是为了可测：池序 + 累加 + 兜底是**行为**（不是随机数生成器的性质），
/// 用精确的 `roll` 就能断言，不必依赖某版 `rand` 的内部流。
/// 越界（浮点累加误差导致 `roll >= total`）时回落第 0 个，与归位前的实现一致。
pub fn pick_by_weight(weights: &[f64], roll: f64) -> usize {
    let mut acc = 0.0;
    for (index, &weight) in weights.iter().enumerate() {
        acc += weight;
        if roll < acc {
            return index;
        }
    }
    0
}

/// 按地图类型 + 楼层加权随机选择一种怪物。
pub fn roll_one_kind(map_kind: MapKind, floor: u32, rng: &mut impl Rng) -> MonsterKindId {
    use rand::RngExt;

    let kinds = kinds_for(map_kind);
    let weights: Vec<f64> = kinds
        .iter()
        .map(|kind| monster_spawn_weight(map_kind, *kind, floor))
        .collect();
    let total: f64 = weights.iter().sum();
    // 全零池：取第 0 个且**不消耗随机数**（归位前如此，破坏它会平移整条 RNG 流）。
    if total <= 0.0 {
        return kinds[0];
    }

    let roll = rng.random_range(0.0..total);
    kinds[pick_by_weight(&weights, roll)]
}

/// 把模板的能力列表挂成 `Can*` 组件——`Ability` → 组件的**唯一**映射点。
///
/// 加一种能力 = 在 `Ability` 加一个变体（编译器会要求这里补一支），
/// 而不是回 spawn 代码里逐条 `insert`。
pub fn attach_abilities(cmd: &mut EntityCommands, abilities: &[Ability]) {
    for ability in abilities {
        match *ability {
            Ability::Chase => cmd.insert(CanChase),
            Ability::Flee => cmd.insert(CanFlee),
            Ability::Wander => cmd.insert(CanWander),
            Ability::Wait => cmd.insert(CanWait),
        };
    }
}
