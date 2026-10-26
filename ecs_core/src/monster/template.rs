//! 怪物模板：把旧架构分散的 match 收敛为数据表。
//!
//! 一个物种 = 一条 `MonsterTemplate` 常量，**三件事都在这一行里**：
//!
//! | 这一行里的字段 | 回答的问题 | 归位前住在哪 |
//! |---|---|---|
//! | 数值（`hp_base`…`exp_per_floor`） | 这只怪多强 | 本文件（Phase H 之前的收敛） |
//! | [`MonsterTemplate::abilities`] | 这只怪能做什么 | `world/init.rs` 的 spawn 代码逐条 `insert` |
//! | [`MonsterTemplate::spawn`] | 这只怪多常见 | `monster/spawn.rs` 的三层 `match` |
//!
//! 于是"加一只怪"= 加一条常量 + 在种类池里排一次位（`monster/spawn.rs`），
//! 而不是改三个文件（Phase H6/H7，ISSUES ECS32）。
//!
//! **掉落表不迁**：物品未迁移，字段等到物品系统落地再加。

use crate::components::*;
use crate::map::MapKind;
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

/// 物种的**能力**：模板里写语义，`spawn.rs::attach_abilities` 里映射成 `Can*` 组件。
///
/// 为什么不直接把 `Can*` ZST 放进模板的列表：模板是**数据**（`Copy` 字面量表），
/// 而 `Can*` 是 bevy 组件，两者混在一张表里会把"数据表"变成"组件清单"，
/// 加数据行就得引用 `components`。中间这一层枚举是 `Ability → Can*` 的**唯一**映射点。
///
/// 只列**模板授予**的能力。玩家不是模板行（是单例），能力仍写在 `spawn_player_system`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ability {
    Chase,
    Flee,
    Wander,
    Wait,
}

/// 一个物种在某张地图类型下的**出现权重曲线**：`w(floor) = (base + per_floor × floor).clamp(min, max)`。
///
/// 归位（H7）前这是 `spawn.rs` 里三层 `match` 的裸表达式，共 10 条：
/// 其中 4 条是"随楼层下降、有底"（`.max(底)`），6 条是"随楼层上升、有顶"（`.min(顶)`）。
/// 两种形态统一成 `clamp`，因此[构造器](Self::rising) / [`falling`](Self::falling)
/// 只是把"没用到的那一侧"填成无穷 —— 这样 `.max(x)` 与 `.clamp(0.5, INF)` 在
/// 所有有限输入上**逐位相等**，归位不产生数值差异（`spawn_weights_match_the_legacy_match_arms` 钉住）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpawnWeight {
    base: f64,
    per_floor: f64,
    min: f64,
    max: f64,
}

impl SpawnWeight {
    /// 随楼层**上升**、上限 `cap`（旧口径 `(base + per_floor × f).min(cap)`）。
    pub const fn rising(base: f64, per_floor: f64, cap: f64) -> Self {
        Self {
            base,
            per_floor,
            min: f64::NEG_INFINITY,
            max: cap,
        }
    }

    /// 随楼层**下降**、下限 `floor_min`（旧口径 `(base - |per_floor| × f).max(floor_min)`）。
    ///
    /// `per_floor` 写负数（如 `-2.0`）：`a + f × (-2.0)` 与 `a - f × 2.0` 在 IEEE-754 下恒等，
    /// 所以这里不需要第二个构造器。
    pub const fn falling(base: f64, per_floor: f64, floor_min: f64) -> Self {
        Self {
            base,
            per_floor,
            min: floor_min,
            max: f64::INFINITY,
        }
    }

    pub fn at(self, floor: u32) -> f64 {
        let f = floor as f64;
        (self.base + f * self.per_floor).clamp(self.min, self.max)
    }
}

/// 物种在**某张地图类型**下的权重条目。缺席于 [`MonsterTemplate::spawn`] = 该地图不生成此物种。
///
/// 权重必须按地图类型分开，而不是一个全局数：同一种怪在不同生态里的常见程度不同
/// （老鼠在洞穴里多、在繁茂洞穴里少、在地海里没有），这是**内容差异**不是**强度差异**。
#[derive(Debug, Clone, Copy)]
pub struct SpawnWeightFor {
    pub map_kind: MapKind,
    pub weight: SpawnWeight,
}

/// 当前八种怪物共有的能力集。
///
/// 归位（H6）**只换存放位置**：这四件与原先 `spawn_monsters_system` 里逐一 `insert`
/// 的完全相同，集合相等即可（组件的插入顺序无语义）。要做出"不会逃跑""只会游荡"
/// 的怪，就把某个物种的 `abilities` 改成它自己的列表——这是归位之后才**可能**的事。
const MONSTER_ABILITIES: &[Ability] = &[
    Ability::Chase,
    Ability::Flee,
    Ability::Wander,
    Ability::Wait,
];

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
    /// 该物种能做什么（挂成 `Can*` 标记）。空列表 = 不动的木桩。
    pub abilities: &'static [Ability],
    /// 该物种在各**地图类型**下的出现权重；缺席 = 该地图不生成它。
    pub spawn: &'static [SpawnWeightFor],
}

/// 由模板生成的实体数值。
///
/// **只装 100% 需要的字段**（原 ECS33 并入 ECS32 的那半条）：这里曾有一个
/// `magic: Magic` 恒为 `0.0` 的占位——没有任何系统读怪物的 `Magic`，
/// 它唯一的效果是让"怪物也有蓝条"这个不存在的设计看起来存在。
/// 需要蓝条的物种（法系怪，H10 之后）应在 spawn 时按需 `insert`，不是每只怪都带一个 0。
#[derive(Debug, Clone, Copy)]
pub struct MonsterStats {
    pub level: u64,
    pub health: Health,
    pub experience_reward: ExperienceReward,
    pub attack: Attack,
    pub defense: Defense,
    /// 保留：法术精通是**每物种**的平衡数值（1–4），H10 的技能层会读它。
    /// 与已删除的 `Magic` 占位不同——它是真数据，只是暂时没有读者。
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

    /// 该物种在 `(map_kind, floor)` 的出现权重；没有对应条目即 `0.0`（不生成）。
    ///
    /// 归位前这是 `monster_spawn_weight` 里三层 `match` 的 `_ => 0.0` 分支，
    /// 现在是"表里没有这一行"——**加地图类型时不需要改这个函数**，
    /// 也不必再给每个物种补一条 `_ => 0.0`。
    pub fn spawn_weight(&self, map_kind: MapKind, floor: u32) -> f64 {
        self.spawn
            .iter()
            .find(|entry| entry.map_kind == map_kind)
            .map_or(0.0, |entry| entry.weight.at(floor))
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
    abilities: MONSTER_ABILITIES,
    // 洞穴的"基础杂兵"：随楼层让位给蝎子与哥布林，但有底（不会彻底消失）。
    spawn: &[
        SpawnWeightFor {
            map_kind: MapKind::Cavern,
            weight: SpawnWeight::falling(30.0, -2.0, 0.5),
        },
        SpawnWeightFor {
            map_kind: MapKind::LushCavern,
            weight: SpawnWeight::falling(12.0, -1.0, 0.5),
        },
    ],
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
    abilities: MONSTER_ABILITIES,
    // 跨两种生态：洞穴里随楼层变多，地海里作为"少量混入"。
    spawn: &[
        SpawnWeightFor {
            map_kind: MapKind::Cavern,
            weight: SpawnWeight::rising(20.0, 1.0, 40.0),
        },
        SpawnWeightFor {
            map_kind: MapKind::Undersea,
            weight: SpawnWeight::rising(8.0, 0.5, 20.0),
        },
    ],
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
    abilities: MONSTER_ABILITIES,
    // 两种生态里都随楼层快速上升（威胁替换）：洞穴的顶更高、繁茂的顶更低。
    spawn: &[
        SpawnWeightFor {
            map_kind: MapKind::Cavern,
            weight: SpawnWeight::rising(5.0, 2.5, 45.0),
        },
        SpawnWeightFor {
            map_kind: MapKind::LushCavern,
            weight: SpawnWeight::rising(5.0, 1.5, 30.0),
        },
    ],
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
    abilities: MONSTER_ABILITIES,
    // 繁茂洞穴的基础杂兵（对应洞穴里的老鼠）。
    spawn: &[SpawnWeightFor {
        map_kind: MapKind::LushCavern,
        weight: SpawnWeight::falling(30.0, -2.0, 0.5),
    }],
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
    abilities: MONSTER_ABILITIES,
    // 繁茂洞穴的"精英"：迟来但持续上升。
    spawn: &[SpawnWeightFor {
        map_kind: MapKind::LushCavern,
        weight: SpawnWeight::rising(5.0, 2.5, 40.0),
    }],
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
    abilities: MONSTER_ABILITIES,
    // 地海的基础杂兵。
    spawn: &[SpawnWeightFor {
        map_kind: MapKind::Undersea,
        weight: SpawnWeight::falling(30.0, -2.0, 0.5),
    }],
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
    abilities: MONSTER_ABILITIES,
    // 地海的"重甲"：稳中有升。
    spawn: &[SpawnWeightFor {
        map_kind: MapKind::Undersea,
        weight: SpawnWeight::rising(12.0, 1.5, 35.0),
    }],
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
    abilities: MONSTER_ABILITIES,
    // 地海的深层威胁。
    spawn: &[SpawnWeightFor {
        map_kind: MapKind::Undersea,
        weight: SpawnWeight::rising(8.0, 2.0, 35.0),
    }],
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
