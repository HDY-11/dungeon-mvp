//! 怪物模板：把旧架构分散的 match 收敛为数据表。
//!
//! 本轮不迁移掉落表（物品未迁移）。

use crate::components::*;
use crate::entity_cls::CreatureKind;
use crate::map::MapKind;
use bevy_ecs::prelude::*;
use rand::Rng;
use serde::{Deserialize, Serialize};

/// 怪物数据键。派生 `Ord` 只为排序/快照比较，不表示强度序。
#[derive(
    Component, Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
pub enum MonsterKindId {
    Rat,
    Scorpion,
    Goblin,
    Sporeling,
    MushroomGolem,
    CaveFish,
    CaveCrab,
    DeepEel,
}

/// 怪物静态定义。数值为 f64 的成长公式参数。
#[derive(Debug, Clone, Copy)]
pub struct MonsterTemplate {
    pub kind: MonsterKindId,
    pub glyph: char,
    pub color: (u8, u8, u8),
    pub name: &'static str,
    pub attack_name: &'static str,
    pub creature_kind: CreatureKind,
    pub hp_base: f64,
    pub hp_per_floor: f64,
    pub attack_base: f64,
    pub attack_per_floor: f64,
    pub attack_max: f64,
    pub defense: f64,
    pub agility: f64,
    pub magic_mastery: f64,
    pub crit_rate: f64,
    pub crit_damage: f64,
    pub exp_base: f64,
    pub exp_per_floor: f64,
}

/// 由模板生成的实体数值。
#[derive(Debug, Clone, Copy)]
pub struct MonsterStats {
    pub level: u64,
    pub health: Health,
    pub magic: Magic,
    pub experience_reward: ExperienceReward,
    pub attack: Attack,
    pub defense: Defense,
    pub magic_mastery: MagicMastery,
    pub agility: Agility,
    pub crit_rate: CritRate,
    pub crit_damage: CritDamage,
}

impl MonsterTemplate {
    pub fn stats(self, floor: u32) -> MonsterStats {
        let lvl = floor.saturating_sub(1);
        let s = lvl as f64;
        let level = (1 + lvl).min(20) as u64;
        let hp = self.hp_base + s * self.hp_per_floor;
        let attack = (self.attack_base + s * self.attack_per_floor).min(self.attack_max);
        let exp = (self.exp_base + s * self.exp_per_floor).round().max(0.0);

        MonsterStats {
            level,
            health: Health::new(hp),
            magic: Magic::new(0.0),
            experience_reward: ExperienceReward(exp),
            attack: Attack(attack),
            defense: Defense(self.defense),
            magic_mastery: MagicMastery(self.magic_mastery),
            agility: Agility(self.agility),
            crit_rate: CritRate(self.crit_rate),
            crit_damage: CritDamage(self.crit_damage),
        }
    }
}

const RAT: MonsterTemplate = MonsterTemplate {
    kind: MonsterKindId::Rat,
    glyph: 'r',
    color: (255, 0, 0),
    name: "老鼠",
    attack_name: "撕咬",
    creature_kind: CreatureKind::Beast,
    hp_base: 10.0,
    hp_per_floor: 4.0,
    attack_base: 4.0,
    attack_per_floor: 1.0,
    attack_max: 18.0,
    defense: 2.0,
    agility: 5.0,
    magic_mastery: 1.0,
    crit_rate: 0.05,
    crit_damage: 0.50,
    exp_base: 6.0,
    exp_per_floor: 3.0,
};

const SCORPION: MonsterTemplate = MonsterTemplate {
    kind: MonsterKindId::Scorpion,
    glyph: 's',
    color: (180, 180, 0),
    name: "变异蝎子",
    attack_name: "螫刺",
    creature_kind: CreatureKind::Beast,
    hp_base: 14.0,
    hp_per_floor: 5.0,
    attack_base: 5.0,
    attack_per_floor: 1.5,
    attack_max: 20.0,
    defense: 3.0,
    agility: 4.0,
    magic_mastery: 1.0,
    crit_rate: 0.05,
    crit_damage: 0.50,
    exp_base: 10.0,
    exp_per_floor: 5.0,
};

const GOBLIN: MonsterTemplate = MonsterTemplate {
    kind: MonsterKindId::Goblin,
    glyph: 'g',
    color: (0, 255, 0),
    name: "哥布林",
    attack_name: "重击",
    creature_kind: CreatureKind::Humanoid,
    hp_base: 18.0,
    hp_per_floor: 6.0,
    attack_base: 6.0,
    attack_per_floor: 2.0,
    attack_max: 25.0,
    defense: 4.0,
    agility: 3.0,
    magic_mastery: 3.0,
    crit_rate: 0.05,
    crit_damage: 0.50,
    exp_base: 15.0,
    exp_per_floor: 7.5,
};

const SPORELING: MonsterTemplate = MonsterTemplate {
    kind: MonsterKindId::Sporeling,
    glyph: 'm',
    color: (140, 255, 140),
    name: "孢子怪",
    attack_name: "孢子喷吐",
    creature_kind: CreatureKind::Plant,
    hp_base: 12.0,
    hp_per_floor: 2.0,
    attack_base: 4.0,
    attack_per_floor: 1.0,
    attack_max: 16.0,
    defense: 0.0,
    agility: 8.0,
    magic_mastery: 2.0,
    crit_rate: 0.05,
    crit_damage: 0.50,
    exp_base: 6.0,
    exp_per_floor: 3.0,
};

const MUSHROOM_GOLEM: MonsterTemplate = MonsterTemplate {
    kind: MonsterKindId::MushroomGolem,
    glyph: 'M',
    color: (200, 120, 220),
    name: "蘑菇傀儡",
    attack_name: "重拳",
    creature_kind: CreatureKind::Plant,
    hp_base: 22.0,
    hp_per_floor: 3.0,
    attack_base: 7.0,
    attack_per_floor: 1.5,
    attack_max: 24.0,
    defense: 2.0,
    agility: 4.0,
    magic_mastery: 4.0,
    crit_rate: 0.05,
    crit_damage: 0.50,
    exp_base: 11.0,
    exp_per_floor: 5.5,
};

const CAVE_FISH: MonsterTemplate = MonsterTemplate {
    kind: MonsterKindId::CaveFish,
    glyph: 'f',
    color: (120, 200, 255),
    name: "洞穴鱼",
    attack_name: "啃咬",
    creature_kind: CreatureKind::Aquatic,
    hp_base: 10.0,
    hp_per_floor: 2.0,
    attack_base: 3.0,
    attack_per_floor: 1.0,
    attack_max: 15.0,
    defense: 0.0,
    agility: 14.0,
    magic_mastery: 1.0,
    crit_rate: 0.05,
    crit_damage: 0.50,
    exp_base: 6.0,
    exp_per_floor: 3.0,
};

const CAVE_CRAB: MonsterTemplate = MonsterTemplate {
    kind: MonsterKindId::CaveCrab,
    glyph: 'c',
    color: (255, 140, 80),
    name: "洞穴蟹",
    attack_name: "钳击",
    creature_kind: CreatureKind::Aquatic,
    hp_base: 18.0,
    hp_per_floor: 3.0,
    attack_base: 3.0,
    attack_per_floor: 1.2,
    attack_max: 20.0,
    defense: 4.0,
    agility: 3.0,
    magic_mastery: 1.0,
    crit_rate: 0.05,
    crit_damage: 0.50,
    exp_base: 11.0,
    exp_per_floor: 5.5,
};

const DEEP_EEL: MonsterTemplate = MonsterTemplate {
    kind: MonsterKindId::DeepEel,
    glyph: 'e',
    color: (80, 160, 220),
    name: "深鳗",
    attack_name: "缠绕",
    creature_kind: CreatureKind::Aquatic,
    hp_base: 15.0,
    hp_per_floor: 3.0,
    attack_base: 6.0,
    attack_per_floor: 1.5,
    attack_max: 22.0,
    defense: 1.0,
    agility: 10.0,
    magic_mastery: 2.0,
    crit_rate: 0.05,
    crit_damage: 0.50,
    exp_base: 12.0,
    exp_per_floor: 6.0,
};

pub fn monster_template(kind: MonsterKindId) -> &'static MonsterTemplate {
    match kind {
        MonsterKindId::Rat => &RAT,
        MonsterKindId::Scorpion => &SCORPION,
        MonsterKindId::Goblin => &GOBLIN,
        MonsterKindId::Sporeling => &SPORELING,
        MonsterKindId::MushroomGolem => &MUSHROOM_GOLEM,
        MonsterKindId::CaveFish => &CAVE_FISH,
        MonsterKindId::CaveCrab => &CAVE_CRAB,
        MonsterKindId::DeepEel => &DEEP_EEL,
    }
}

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
