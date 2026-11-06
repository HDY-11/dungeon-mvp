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
//! 第 4 件事是 [MonsterTemplate::loot]（掉落表）：数据住在 item/loot.rs 旁边的物种表里，
//! 这里只持有切片——与本文件其余字段一样，&'static 数据、零运行期查找。

use crate::components::*;
use crate::item::{ItemId, LootEntry};
use crate::map::MapKind;
use crate::rules::StatId;
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

/// **完整行为谱**：会追、重伤会逃、会游荡、会等待。
///
/// 起初八种怪物共用这一份列表（Phase H6 归位时"只换存放位置，不改能力"）。
/// 内容轮（C1）起按物种分化：F1 可见的鼠与蝎子各取不同子集，
/// 本常量现在代表"完整"那一档（哥布林 + 五种尚未出现在 F1 的物种）。
const MONSTER_ABILITIES: &[Ability] = &[
    Ability::Chase,
    Ability::Flee,
    Ability::Wander,
    Ability::Wait,
];

/// **猎物**：不主动追击，但受伤会逃（鼠）。
///
/// 去掉 `Chase` 的直接后果：鼠不会主动进攻，被打了才跑。它是场上最快的物种
/// （速度 1.11），所以"追一只受伤的鼠"与"追蝎子"是两种手感——这正是 C1 想让
/// 玩家先感受到的差异。
const PREY_ABILITIES: &[Ability] = &[Ability::Flee, Ability::Wander, Ability::Wait];

/// **不知恐惧**：会追击，但永不逃跑（蝎子）。
///
/// 去掉 `Flee` 的直接后果：`FLEE_HP_RATIO` 那套滞回机制对它完全不生效，
/// 它会把这一仗打完或者当场死掉；哥布林保留逃跑，于是两者残血时表现相反。
const FEARLESS_ABILITIES: &[Ability] = &[Ability::Chase, Ability::Wander, Ability::Wait];

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
    /// 死亡时的掉落表（GAME.md Gm7）。空列表 = 不掉东西。
    pub loot: &'static [LootEntry],
    /// **身上带的装备**（DsnE18 第②片）：死亡时进尸体容器，不掉在地上。
    ///
    /// 只给"会用工具"的物种配：目前是哥布林（匕首）与蘑菇傀儡（石锤）。
    /// 数值一律引用注册表里的物品编号，不在模板里复制属性。
    pub equipment: &'static [ItemId],
}

/// 由模板生成的实体数值。
///
/// **只装 100% 需要的字段**（原 ECS33 并入 ECS32 的那半条）：这里曾有一个
/// `magic: Magic` 恒为 `0.0` 的占位——没有任何系统读怪物的 `Magic`，
/// 它唯一的效果是让"怪物也有蓝条"这个不存在的设计看起来存在。
/// 需要蓝条的物种（法系怪，H10 之后）应在 spawn 时按需 `insert`，不是每只怪都带一个 0。
#[derive(Debug, Clone)]
pub struct MonsterStats {
    pub level: u64,
    pub health: Health,
    pub experience_reward: ExperienceReward,
    /// 基础属性（DsnE22 阶段 ②b）：攻击/防御/速度/暴击一次装进稠密表。
    pub stats: Stats,
    /// 保留：法术精通是**每物种**的平衡数值（1–4），H10 的技能层会读它。
    /// 与已删除的 `Magic` 占位不同——它是真数据，只是暂时没有读者。
    pub magic_mastery: MagicMastery,
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
            stats: Stats::from_pairs([
                (StatId::ATTACK, attack),
                (StatId::DEFENSE, self.defense),
                (StatId::MOVE_SPEED, self.speeds.move_speed),
                (StatId::ATTACK_SPEED, self.speeds.attack_speed),
                (StatId::CRIT_RATE, self.crit_rate),
                (StatId::CRIT_DAMAGE, self.crit_damage),
            ]),
            magic_mastery: MagicMastery(self.magic_mastery),
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

// ── 掉落表用到的物品编号 ─────────────────────────────
//
// 单独起短名字只为一件事：掉落表要能一眼读出"这只怪掉什么"，
// 而不是满屏 `ItemId(10)` 让人回头查表。编号与 `item/registry.rs` 一一对应
// （`loot_tables_reference_registered_items` 会在测试里核对这一点）。
const ITEM_RAW_FLESH: ItemId = ItemId(10);
const ITEM_RAG: ItemId = ItemId(11);
const ITEM_STICK: ItemId = ItemId(12);
const ITEM_FANG: ItemId = ItemId(13);
const ITEM_CARAPACE: ItemId = ItemId(14);
const ITEM_PEBBLE: ItemId = ItemId(18);
const ITEM_HAMMER: ItemId = ItemId(23);
const ITEM_DAGGER: ItemId = ItemId(24);
const ITEM_MUSHROOM: ItemId = ItemId(25);
const ITEM_MOSS: ItemId = ItemId(26);
const ITEM_SPORE_SAC: ItemId = ItemId(27);
const ITEM_SHELL: ItemId = ItemId(28);
const ITEM_PEARL: ItemId = ItemId(29);
const ITEM_FISH_BONE: ItemId = ItemId(30);
const ITEM_EEL_SKIN: ItemId = ItemId(31);
const ITEM_SEAWEED: ItemId = ItemId(32);

// ── 掉落表（GAME.md Gm7「掉落表」） ───────────────────
//
// 每条独立掷一次概率，所以"必掉 + 概率"可以同时成立。
// 哥布林身上那 12% 的武器与 5% 的模板碎片是"装备来源"——没有它们，
// 装备系统就只能靠地上随机刷的几件，玩家感受不到"打怪能换装"。

/// 老鼠：只有肉。
const RAT_LOOT: &[LootEntry] = &[LootEntry::always(ITEM_RAW_FLESH, 1, 2)];

/// 蝎子：肉 + 甲壳（甲片模板的材料）。
const SCORPION_LOOT: &[LootEntry] = &[
    LootEntry::always(ITEM_RAW_FLESH, 1, 2),
    LootEntry::always(ITEM_CARAPACE, 1, 2),
];

/// 哥布林：最富的杂兵——材料、石子、一把锤子。
///
/// 匕首**不在这里**：它由 `MonsterTemplate.equipment` 挂在哥布林尸体上
/// （DsnE18 第②片）。两个来源都在时会成对掉匕首（AUDIT-data D9）。
const GOBLIN_LOOT: &[LootEntry] = &[
    LootEntry::always(ITEM_RAW_FLESH, 1, 3),
    LootEntry::maybe(ITEM_RAG, 0.60, 1, 1),
    LootEntry::maybe(ITEM_STICK, 0.40, 1, 1),
    LootEntry::maybe(ITEM_FANG, 0.30, 1, 1),
    LootEntry::maybe(ITEM_PEBBLE, 0.60, 1, 2),
    LootEntry::maybe(ITEM_HAMMER, 0.12, 1, 1),
];

/// 孢子怪：蘑菇与苔藓。
const SPORELING_LOOT: &[LootEntry] = &[
    LootEntry::maybe(ITEM_MUSHROOM, 0.60, 1, 2),
    LootEntry::maybe(ITEM_MOSS, 0.40, 1, 1),
];

/// 蘑菇傀儡：苔藓必掉，孢子囊稀有。
const MUSHROOM_GOLEM_LOOT: &[LootEntry] = &[
    LootEntry::maybe(ITEM_MOSS, 0.80, 1, 1),
    LootEntry::maybe(ITEM_SPORE_SAC, 0.30, 1, 1),
    LootEntry::maybe(ITEM_MUSHROOM, 0.30, 1, 2),
];

/// 洞穴鱼：鱼骨与海藻。
const CAVE_FISH_LOOT: &[LootEntry] = &[
    LootEntry::maybe(ITEM_FISH_BONE, 0.60, 1, 1),
    LootEntry::maybe(ITEM_SEAWEED, 0.30, 1, 1),
];

/// 洞穴蟹：贝壳常见，珍珠稀有。
const CAVE_CRAB_LOOT: &[LootEntry] = &[
    LootEntry::maybe(ITEM_SHELL, 0.80, 1, 1),
    LootEntry::maybe(ITEM_PEARL, 0.10, 1, 1),
];

/// 深鳗：鳗皮与海藻。
const DEEP_EEL_LOOT: &[LootEntry] = &[
    LootEntry::maybe(ITEM_EEL_SKIN, 0.60, 1, 1),
    LootEntry::maybe(ITEM_SEAWEED, 0.40, 1, 1),
];

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
    abilities: PREY_ABILITIES,
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
    loot: RAT_LOOT,
    equipment: &[],
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
    abilities: FEARLESS_ABILITIES,
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
    loot: SCORPION_LOOT,
    equipment: &[],
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
    // 完整行为谱（会追、残血会逃）——哥布林是 F1 上最难缠的那一种。
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
    loot: GOBLIN_LOOT,
    equipment: &[ITEM_DAGGER],
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
    loot: SPORELING_LOOT,
    equipment: &[],
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
    loot: MUSHROOM_GOLEM_LOOT,
    equipment: &[ITEM_HAMMER],
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
    loot: CAVE_FISH_LOOT,
    equipment: &[],
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
    loot: CAVE_CRAB_LOOT,
    equipment: &[],
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
    loot: DEEP_EEL_LOOT,
    equipment: &[],
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
