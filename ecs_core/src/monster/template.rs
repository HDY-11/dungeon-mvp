//! 怪物模板：把旧架构分散的 match 收敛为数据表。
//!
//! 一个物种 = 一条 `MonsterTemplate` 常量；数值随楼层由 `stats()` 缩放。
//! 生成侧（权重、种类池、随机挑选）在 `spawn.rs`。
//!
//! 本轮不迁移掉落表（物品未迁移）。

use crate::components::*;
use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};

/// 模板里**直接写速度数值**，不再保留可推导出速度的旧"敏捷"字段：
/// 旧敏捷同时承担反应时与耗时修正两件事，留着它就会有人继续拿它算 AV，
/// 迁移映射（见 [`MonsterSpeeds::MIGRATION_NOTE`]）只作为数值来源记录在文档里。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MonsterSpeeds {
    pub move_speed: f64,
    pub attack_speed: f64,
}

impl MonsterSpeeds {
    /// 移动与攻击同速。当前八种怪物都用这个（迁移映射的产物），
    /// 后续要做出「偏移动」或「偏攻击」的怪，就分开写两个数。
    pub const fn uniform(speed: f64) -> Self {
        Self {
            move_speed: speed,
            attack_speed: speed,
        }
    }

    /// 迁移来源备忘：`速度 = 1 / max(1 - 旧敏捷 × 0.02, 0.5)`，即旧耗时系数的倒数。
    ///
    /// | 怪物 | 旧敏捷 | 速度 |
    /// |---|---|---|
    /// | 老鼠 | 5 | 1.1111 |
    /// | 蝎子 / 蘑菇傀儡 | 4 | 1.0870 |
    /// | 哥布林 / 洞穴蟹 | 3 | 1.0638 |
    /// | 孢子怪 | 8 | 1.1905 |
    /// | 深鳗 | 10 | 1.2500 |
    /// | 洞穴鱼 | 14 | 1.3889 |
    ///
    /// GAME.md `[试调]` 重新校准时改这里，不要再回头引用旧敏捷。
    pub const MIGRATION_NOTE: &'static str = "速度 = 1 / 旧耗时系数（Phase D 迁移映射）";
}

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
    pub hp_base: f64,
    pub hp_per_floor: f64,
    pub attack_base: f64,
    pub attack_per_floor: f64,
    pub attack_max: f64,
    pub defense: f64,
    pub speeds: MonsterSpeeds,
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
    pub move_speed: MoveSpeed,
    pub attack_speed: AttackSpeed,
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
            move_speed: MoveSpeed(self.speeds.move_speed),
            attack_speed: AttackSpeed(self.speeds.attack_speed),
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
    hp_base: 10.0,
    hp_per_floor: 4.0,
    attack_base: 4.0,
    attack_per_floor: 1.0,
    attack_max: 18.0,
    defense: 2.0,
    speeds: MonsterSpeeds::uniform(1.1111111111111112),
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
    hp_base: 14.0,
    hp_per_floor: 5.0,
    attack_base: 5.0,
    attack_per_floor: 1.5,
    attack_max: 20.0,
    defense: 3.0,
    speeds: MonsterSpeeds::uniform(1.0869565217391304),
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
    hp_base: 18.0,
    hp_per_floor: 6.0,
    attack_base: 6.0,
    attack_per_floor: 2.0,
    attack_max: 25.0,
    defense: 4.0,
    speeds: MonsterSpeeds::uniform(1.0638297872340425),
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
    hp_base: 12.0,
    hp_per_floor: 2.0,
    attack_base: 4.0,
    attack_per_floor: 1.0,
    attack_max: 16.0,
    defense: 0.0,
    speeds: MonsterSpeeds::uniform(1.1904761904761905),
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
    hp_base: 22.0,
    hp_per_floor: 3.0,
    attack_base: 7.0,
    attack_per_floor: 1.5,
    attack_max: 24.0,
    defense: 2.0,
    speeds: MonsterSpeeds::uniform(1.0869565217391304),
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
    hp_base: 10.0,
    hp_per_floor: 2.0,
    attack_base: 3.0,
    attack_per_floor: 1.0,
    attack_max: 15.0,
    defense: 0.0,
    speeds: MonsterSpeeds::uniform(1.3888888888888888),
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
    hp_base: 18.0,
    hp_per_floor: 3.0,
    attack_base: 3.0,
    attack_per_floor: 1.2,
    attack_max: 20.0,
    defense: 4.0,
    speeds: MonsterSpeeds::uniform(1.0638297872340425),
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
    hp_base: 15.0,
    hp_per_floor: 3.0,
    attack_base: 6.0,
    attack_per_floor: 1.5,
    attack_max: 22.0,
    defense: 1.0,
    speeds: MonsterSpeeds::uniform(1.25),
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
