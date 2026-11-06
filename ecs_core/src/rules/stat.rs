//! 属性词表：`StatId` + 静态 `StatDef` 表（DESIGN DsnE22 阶段 ②a）。
//!
//! # 为什么不是枚举
//!
//! 阶段 ① 之前，可修正的数值是一个 `#[non_exhaustive] enum Stat`：加一项属性要改
//! 枚举、改 `label()` 的匹配、改所有 `match` 的地方，数据文件（`assets/items.json`）
//! 也只能写编译期就知道的名字。DsnE22 的判断是"属性是长尾的"——内容作者会不断
//! 加"火抗""施法专注""暴击倍率"这类只服务少数物品的属性，枚举会让每次新增都变成
//! 一次代码改动。
//!
//! 所以形状换成：**数值身份是数据**（`StatId`），**词表是可查询的表**（[`STATS`]）。
//! 折叠逻辑（[`crate::rules::apply_modifiers`]）只认 `StatId`，不再逐项 `match`；
//! 加一项属性 = 在 [`STATS`] 里加一行（本阶段仍在代码里，阶段 ② 的后续可迁进数据）。
//!
//! # 序列化口径
//!
//! 数据文件里写**名字**而不是编号（`"stat": "attack"`）：
//!
//! - 名字是人读的，也是加载期校验的锚点——未知名字直接报错，不会静默变成 `StatId(99)`；
//! - 编号可以在内部随意排序，不构成对外契约；
//! - 与 `ItemId` 不同，属性名不需要"编号上盘"：存档里目前不存属性 id（派生数据）。
//!
//! # 边界
//!
//! [`StatDef::min`] / [`StatDef::max`] 是"这项数值的合法区间"（DsnE22 已拍板：
//! 属性值必须是 `[min, max]` 内的有限数）。阶段 ②b 起，夹取在
//! [`crate::components::Stats::set`] 这一条缝上做——写属性的地方不再各自 clamp。
//! 基准值（[`StatDef::default`]）也在这里，`Stats::new()` 按词表逐项落默认。

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// 一项可修正数值的身份。
///
/// 用 `u16` 而不是枚举：属性集合是内容（长尾），代码只需要一个稳定的键。
/// 未知编号不能凭空产生数据——`Deserialize` 走名字查表，构造只能从常量或表里来。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StatId(pub u16);

impl StatId {
    /// 近战攻击力（绝对值）。
    pub const ATTACK: Self = Self(0);
    /// 防御力（绝对值，减伤）。
    pub const DEFENSE: Self = Self(1);
    /// 移动速度（倍率）。
    pub const MOVE_SPEED: Self = Self(2);
    /// 攻击速度（倍率）。
    pub const ATTACK_SPEED: Self = Self(3);
    /// 暴击率（概率，`[0, 1]`）。
    pub const CRIT_RATE: Self = Self(4);
    /// 暴击伤害加成（暴击时伤害 × `1 + crit_damage`）。
    pub const CRIT_DAMAGE: Self = Self(5);

    /// 当前词表里的全部属性。
    pub const ALL: [Self; 6] = [
        Self::ATTACK,
        Self::DEFENSE,
        Self::MOVE_SPEED,
        Self::ATTACK_SPEED,
        Self::CRIT_RATE,
        Self::CRIT_DAMAGE,
    ];

    /// 查这一项的定义；未知编号是程序错误（数据在加载期已校验）。
    ///
    /// 与 `ItemId::def()` 同口径：静默返回默认值会把"忘记加表行"变成数值悄悄错。
    pub fn def(self) -> &'static StatDef {
        stat_def(self).unwrap_or_else(|| panic!("未知属性编号 {}（先加进 STATS 表）", self.0))
    }

    /// 这个编号在词表里是否存在（加载期校验与测试用）。
    pub fn exists(self) -> bool {
        stat_def(self).is_some()
    }

    /// 数据文件里用的名字（`"attack"`）。
    pub fn name(self) -> &'static str {
        self.def().name
    }

    /// 界面/日志用的中文名。
    pub fn label(self) -> &'static str {
        self.def().label
    }

    /// 这一项是不是"倍率型"数值（只用于文档/调试输出，不参与折叠）。
    pub fn is_ratio(self) -> bool {
        self.def().is_ratio
    }

    /// 合法区间下界。
    pub fn min(self) -> f64 {
        self.def().min
    }

    /// 合法区间上界。
    pub fn max(self) -> f64 {
        self.def().max
    }

    /// 未装配时的基准值（`Stats::new()` 用它初始化）。
    pub fn default_value(self) -> f64 {
        self.def().default
    }
}

/// 一项属性的定义（词表的一行）。
///
/// 词表目前写在代码里：它是"引擎认识哪些数值"的形状，跟着折叠逻辑一起演进；
/// 等阶段 ② 的属性真正动态化后，可以把这张表搬进数据文件而不改 [`StatId`] 与折叠。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StatDef {
    /// 稳定身份。
    pub id: StatId,
    /// 数据文件里的名字（snake_case，唯一）。
    pub name: &'static str,
    /// 界面/日志用的中文名。
    pub label: &'static str,
    /// 倍率型（速度）还是绝对值型（攻防）——只影响显示说明。
    pub is_ratio: bool,
    /// 合法区间下界（阶段 ②b 起在 `Stats::set` 上夹取）。
    pub min: f64,
    /// 合法区间上界。
    pub max: f64,
    /// 未装配时的基准值（`Stats::new()` 初始化值）。
    pub default: f64,
}

/// 属性词表。
///
/// 顺序就是编号顺序：`STATS[i].id == StatId(i)` 由测试钉住。
/// 速度的区间下限取 `0.05` 而不是 `0`：`0` 会让行动计时变成除零；这也与
/// `GAME.md` 的"速度倍率"口径一致（最小仍可行动）。
pub static STATS: &[StatDef] = &[
    StatDef {
        id: StatId::ATTACK,
        name: "attack",
        label: "攻击",
        is_ratio: false,
        min: 0.0,
        max: 1000.0,
        default: 0.0,
    },
    StatDef {
        id: StatId::DEFENSE,
        name: "defense",
        label: "防御",
        is_ratio: false,
        min: 0.0,
        max: 1000.0,
        default: 0.0,
    },
    StatDef {
        id: StatId::MOVE_SPEED,
        name: "move_speed",
        label: "移动速度",
        is_ratio: true,
        min: 0.05,
        max: 20.0,
        default: 1.0,
    },
    StatDef {
        id: StatId::ATTACK_SPEED,
        name: "attack_speed",
        label: "攻击速度",
        is_ratio: true,
        min: 0.05,
        max: 20.0,
        default: 1.0,
    },
    StatDef {
        id: StatId::CRIT_RATE,
        name: "crit_rate",
        label: "暴击率",
        is_ratio: false,
        min: 0.0,
        max: 1.0,
        default: 0.0,
    },
    StatDef {
        id: StatId::CRIT_DAMAGE,
        name: "crit_damage",
        label: "暴击伤害",
        is_ratio: false,
        min: 0.0,
        max: 10.0,
        default: 0.0,
    },
];

fn stat_def(id: StatId) -> Option<&'static StatDef> {
    STATS.iter().find(|def| def.id == id)
}

/// 按数据文件里的名字查属性。
pub fn stat_by_name(name: &str) -> Option<StatId> {
    STATS.iter().find(|def| def.name == name).map(|def| def.id)
}

impl Serialize for StatId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.name())
    }
}

impl<'de> Deserialize<'de> for StatId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let name = String::deserialize(deserializer)?;
        stat_by_name(&name).ok_or_else(|| {
            let known = STATS.iter().map(|def| def.name).collect::<Vec<_>>().join(" / ");
            serde::de::Error::custom(format!("未知属性 `{name}`（可用：{known}）"))
        })
    }
}

#[cfg(test)]
#[path = "stat_tests.rs"]
mod tests;
