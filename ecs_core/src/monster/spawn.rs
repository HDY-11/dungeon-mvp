//! 怪物生成：按地图类型与楼层的权重、种类池与随机挑选。
//!
//! 数值分两层：物种**属性**在 `template.rs`，物种**出现概率**在这里。
//! 两者分开的理由：调平衡时「这个怪多强」与「这个怪多常见」是两个独立旋钮，
//! 混在一个常量里就得同时改两件事。

use crate::map::MapKind;
use crate::monster::MonsterKindId;
use rand::Rng;

/// 按地图类型 + 楼层计算生成权重。
pub fn monster_spawn_weight(map_kind: MapKind, kind: MonsterKindId, floor: u32) -> f64 {
    let f = floor as f64;
    match map_kind {
        MapKind::Cavern => match kind {
            MonsterKindId::Rat => (30.0 - f * 2.0).max(0.5),
            MonsterKindId::Scorpion => (20.0 + f * 1.0).min(40.0),
            MonsterKindId::Goblin => (5.0 + f * 2.5).min(45.0),
            _ => 0.0,
        },
        MapKind::LushCavern => match kind {
            MonsterKindId::Sporeling => (30.0 - f * 2.0).max(0.5),
            MonsterKindId::MushroomGolem => (5.0 + f * 2.5).min(40.0),
            MonsterKindId::Rat => (12.0 - f * 1.0).max(0.5),
            MonsterKindId::Goblin => (5.0 + f * 1.5).min(30.0),
            _ => 0.0,
        },
        MapKind::Undersea => match kind {
            MonsterKindId::CaveFish => (30.0 - f * 2.0).max(0.5),
            MonsterKindId::CaveCrab => (12.0 + f * 1.5).min(35.0),
            MonsterKindId::DeepEel => (8.0 + f * 2.0).min(35.0),
            MonsterKindId::Scorpion => (8.0 + f * 0.5).min(20.0),
            _ => 0.0,
        },
    }
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

pub fn kinds_for(map_kind: MapKind) -> &'static [MonsterKindId] {
    match map_kind {
        MapKind::Cavern => &CAVERN_KINDS,
        MapKind::LushCavern => &LUSH_KINDS,
        MapKind::Undersea => &UNDERSEA_KINDS,
    }
}

/// 按地图类型 + 楼层加权随机选择一种怪物。
pub fn roll_one_kind(map_kind: MapKind, floor: u32, rng: &mut impl Rng) -> MonsterKindId {
    use rand::RngExt;

    let kinds = kinds_for(map_kind);
    let weights: Vec<f64> = kinds
        .iter()
        .map(|k| monster_spawn_weight(map_kind, *k, floor))
        .collect();
    let total: f64 = weights.iter().sum();
    if total <= 0.0 {
        return kinds[0];
    }

    let roll = rng.random_range(0.0..total);
    let mut acc = 0.0;
    for (i, &w) in weights.iter().enumerate() {
        acc += w;
        if roll < acc {
            return kinds[i];
        }
    }
    kinds[0]
}

