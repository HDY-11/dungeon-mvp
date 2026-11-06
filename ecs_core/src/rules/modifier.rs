//! 规则修正器：把"效果"折算成纯函数的输入（DESIGN DsnE10 的求值侧 / Phase H8）。
//!
//! # 两侧分工
//!
//! | 侧 | 回答什么 | 在哪 |
//! |---|---|---|
//! | **载体侧** | 效果**存在**于哪里、由谁终结 | DsnE12：效果实体 + 归属关系 / 格索引 |
//! | **求值侧** | 效果**折算成**纯函数的什么输入 | 本模块 |
//!
//! 求值五步（顺序即语义，见 DsnE10）：
//!
//! ```text
//! ① base       ：actor 上的权威数值（如 MoveSpeed）
//! ② 收桶       ：actor 名下的效果 + 当前格索引里的效果 → 一份 &[Modifier]
//! ③ 过滤       ：丢掉 `ignored` 里列出的来源类别     ← "无视某类减速"只作用在这一步
//! ④ 折叠       ：按 `Stat` 挑出作用于这一项的修正，再先加后乘
//! ⑤ 纯函数     ：规则只吃第 ④ 步的结果（[`apply_modifiers`] 就是这一步）
//! ```
//!
//! # 为什么第 ④ 步需要 `Stat`（2026-09 物品轮补的维度）
//!
//! 装备落地之前，"一条修正作用于什么"是隐含的：唯一的调用点折算的是速度，
//! 所以列表里每条都当速度用。装备一进来立刻破功——一把剑的"攻击 +3"如果被折进
//! 速度，玩家的移动速度会凭白涨 3；同样的列表既当攻击又当速度，只是没人踩到而已。
//!
//! 两条路：给每一项数值各配一个组件（`AttackModifier` / `DefenseModifier` / …），
//! 或者给修正自己加一个"作用于哪一项"的字段。前者让"加一项可修正的数值"要改组件表，
//! 后者只多一个枚举值——按 `LECS22`（扩展点用"加第 N 个要改几处"度量）选后者。
//!
//! 与 [`EffectSource`] 的关系：两者是**正交**的两维——`source` 回答"谁带来的"
//! （过滤用），`stat` 回答"加到哪一项上"（选择用）。"无视地形减速"只丢 `source`，
//! 不该顺手把"地形给的攻击加成"也丢掉，所以两维必须分开。
//!
//! # 为什么"无视某类"必须是丢桶，而不是规则里的分支
//!
//! 把"某一类东西"写进规则分支，是本项目已经踩过两次的失败模式
//! （`EntityClass::Item` 恒假判断、"若是火焰且是某效果则删除"）。
//! 因此这里的形状是：规则里**永远不出现**"若带了某装备则……"，
//! 只出现"这一桶丢了"——`ignored` 是数据。
//!
//! # 形状 vs 内容（红线，DESIGN DsnX16）
//!
//! | 属形状（本模块定） | 属内容（GAME.md） |
//! |---|---|
//! | 有没有"来源"与"作用于哪一项"这两个维度 | 各来源的系数与取值 |
//! | 折叠顺序（先基础值、后乘区） | 叠加口径（加算还是各自成区）的最终定案 |
//! | `ignored` 是丢桶而不是特例分支 | 哪些具体效果属于哪个桶 |
//!
//! **当前阶段的不变量：** 修正器列表为**空**时，[`apply_modifiers`] 必须**逐值等于**
//! `base`——这条保证"接上求值侧"本身不改变任何现有行为，由 `rules_tests.rs` 的
//! parity 用例钉住。

use serde::{Deserialize, Serialize};

/// 修正**作用于哪一项**数值（求值第 ④ 步的选择键）。
///
/// 这一维只回答"加到谁身上"，不回答"谁加的"（那是 [`EffectSource`]）。
/// `#[non_exhaustive]`：可修正的数值会随内容增补（法术强度、暴击率…）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Stat {
    /// 近战攻击力。
    Attack,
    /// 防御力（减伤）。
    Defense,
    /// 移动速度倍率。
    MoveSpeed,
    /// 攻击速度倍率。
    AttackSpeed,
}

impl Stat {
    /// 这一项是不是"倍率型"数值（速度是倍率，攻防是绝对值）。
    ///
    /// **只用于文档与调试输出**，不参与折叠：折叠对两类数值是同一条公式
    /// （`(base + Σdelta) × Πmult`）。把它写成数据是为了让日志能说清
    /// "1.25 是倍率、+3 是加成"，而不是让规则分叉。
    pub fn is_ratio(self) -> bool {
        matches!(self, Stat::MoveSpeed | Stat::AttackSpeed)
    }

    /// 界面/日志用的中文名。
    ///
    /// 权威措辞在 `GAME.md`（Gm4 装备表 / Gm8 怪物属性）；这里只是为了详情面板与
    /// 日志不必各自再写一遍 `match stat`——显示名放一处，改文案不会漏改。
    pub const fn label(self) -> &'static str {
        match self {
            Stat::Attack => "攻击",
            Stat::Defense => "防御",
            Stat::MoveSpeed => "移动速度",
            Stat::AttackSpeed => "攻击速度",
        }
    }
}

/// 修正的**来源类别**：折叠时按桶过滤（"无视某类减速" = 丢掉这个桶）。
///
/// 这是**形状**：桶的存在与命名是设计；"哪些效果属于哪个桶"是内容。
///
/// 新增一个桶会改变"无视类效果能表达什么"，因此要按 `LECS22` 先问一句：
/// **加第 N 个桶要改几处？** 当前答案是 1 处（本枚举 + 使用方的 `ignored` 列表），
/// 且**不动** [`apply_modifiers`] 的折叠逻辑——这正是把它做成"数据"的收益。
///
/// `#[non_exhaustive]`：桶集合预期会随内容设计增补；这样加桶不会破坏下游的穷尽匹配。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum EffectSource {
    /// actor 自身的固有值（基准属性，不是"效果"）。
    Intrinsic,
    /// 装备带来的修正。
    Equipment,
    /// 状态效果（buff/debuff）带来的修正。
    Status,
    /// 地形带来的修正（减速等）。
    Terrain,
}

impl EffectSource {
    /// 界面/日志用的中文名（"无视某类来源"要能说清是哪一类）。
    ///
    /// 这里**不**用通配兜底：`#[non_exhaustive]` 只约束下游 crate，本 crate 内是穷尽匹配。
    /// 将来新增来源桶时，编译器会在这里报"未覆盖"，正好逼着把中文名补上——
    /// 显示层漏一个桶，比编译失败更难发现。下游 crate 加桶时仍可用自己的兜底。
    pub const fn label(self) -> &'static str {
        match self {
            EffectSource::Intrinsic => "先天",
            EffectSource::Equipment => "装备",
            EffectSource::Status => "状态",
            EffectSource::Terrain => "地形",
        }
    }
}

/// 一条修正：**作用于哪一项** + **来源** + 两类位（DESIGN DsnE10 第 2 条）。
///
/// 两个位**同时存在**而不是"二选一"，因为同一条效果两种都可能需要
/// （例："移动速度 -1 且额外 ×0.8"）。取值属内容，本结构只保证它们有位置可放。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Modifier {
    /// 作用于哪一项数值。
    pub stat: Stat,
    /// 这条修正由哪一类来源产生（过滤按它做）。
    pub source: EffectSource,
    /// **基础值修正位**：加在 base 上（可为负）。数据里省略按 `0.0`。
    #[serde(default)]
    pub base_delta: f64,
    /// **乘区位**：作为独立因子相乘（`1.0` = 不改变）。数据里省略按中性值。
    #[serde(default = "neutral_multiplier")]
    pub multiplier: f64,
}

/// `multiplier` 在数据里省略时的中性值（`1.0`）。
fn neutral_multiplier() -> f64 {
    1.0
}

impl Modifier {
    /// 一条只做基础值增减的修正（乘区为中性 `1.0`）。
    ///
    /// 参数顺序是"改什么 → 谁改的 → 改多少"：调用点读作
    /// `Modifier::delta(Stat::Attack, EffectSource::Equipment, 3.0)`，
    /// 即"攻击、来自装备、加 3"。
    pub const fn delta(stat: Stat, source: EffectSource, base_delta: f64) -> Self {
        Self {
            stat,
            source,
            base_delta,
            multiplier: 1.0,
        }
    }

    /// 一条只做乘区缩放的修正（基础值增量为 `0.0`）。
    pub const fn scaled(stat: Stat, source: EffectSource, multiplier: f64) -> Self {
        Self {
            stat,
            source,
            base_delta: 0.0,
            multiplier,
        }
    }

    /// 这条修正是否属于被无视的来源之一。
    pub fn is_ignored_by(&self, ignored: &[EffectSource]) -> bool {
        ignored.contains(&self.source)
    }

    /// 这条修正是作用于 `stat` 的吗。
    pub fn affects(&self, stat: Stat) -> bool {
        self.stat == stat
    }

    /// 一条修正的中文描述（详情面板/日志用）：`攻击 +1`、`移动速度 ×0.8`、`攻击 +1 攻速 ×1.2`。
    ///
    /// 显示口径与折叠公式分开：这里只说"这条修正写了什么"，不说"最终值是多少"——
    /// 后者要经过过滤与其它修正，属于 [`apply_modifiers`] 的职责。
    pub fn describe(self) -> String {
        let mut text = format!("{} ", self.stat.label());
        // 两处都中性时也说一声，避免详情里出现只有数值名、没有数值的空行。
        if self.base_delta == 0.0 && self.multiplier == 1.0 {
            text.push_str("+0");
            return text;
        }
        if self.base_delta != 0.0 {
            text.push_str(&format!("{:+}", self.base_delta));
        }
        if self.multiplier != 1.0 {
            if self.base_delta != 0.0 {
                text.push(' ');
            }
            text.push_str(&format!("×{}", self.multiplier));
        }
        text
    }
}

/// 只保留作用于 `stat` 的修正（求值第 ④ 步的选择）。
///
/// 单独一个函数而不是在每个调用点写 `.filter(|m| m.stat == ..)`：
/// "算某一项数值时该忽略别的项"这件事只有一份实现，漏写就是数值串味
/// （攻击加成混进速度，且**不会报错**）。
pub fn for_stat(mods: &[Modifier], stat: Stat) -> impl Iterator<Item = &Modifier> {
    mods.iter().filter(move |m| m.affects(stat))
}

/// 求值五步的第 ③④⑤ 步：**过滤 → 选择 → 折叠 → 纯结果**。
///
/// 纯函数：不碰 `World`、不发事件、不改状态；`mods` 为空时逐值返回 `base`
/// （因此"把现有数值接上求值侧"本身是零行为变化）。
///
/// 折叠顺序（**[暂定]**，最终口径属 GAME.md）：`(base + Σ base_delta) × Π multiplier`。
/// 选"先加后乘"只为让两类位的作用可分离、便于单测；若内容阶段定成"各区独立相乘"，
/// 改这里一处即可（调用方只依赖"返回一个 `f64`"）。
pub fn apply_modifiers(
    stat: Stat,
    base: f64,
    mods: &[Modifier],
    ignored: &[EffectSource],
) -> f64 {
    let mut value = base;
    // 先折叠基础值修正，再折叠乘区——两个循环是"顺序即语义"的显式表达，
    // 合写成一个循环会让顺序隐式依赖于列表次序。
    for m in for_stat(mods, stat).filter(|m| !m.is_ignored_by(ignored)) {
        value += m.base_delta;
    }
    for m in for_stat(mods, stat).filter(|m| !m.is_ignored_by(ignored)) {
        value *= m.multiplier;
    }
    value
}

/// 求值入口的**读数结果**：最终数值 + 计算它时用到的修正（供日志/调试解释"为什么是这个数"）。
///
/// 与 DsnE9 的"返回因子分解"同源：数值链路的每一步都应能回答"它从哪来"。
#[derive(Debug, Clone, PartialEq)]
pub struct ModifierOutcome {
    /// 作用于这一项的修正总数（含被无视的）。
    pub relevant: usize,
    /// 折叠后的最终值。
    pub value: f64,
    /// 实际参与了折叠的修正条数（已扣掉被 `ignored` 过滤掉的）。
    pub applied: usize,
    /// 被 `ignored` 过滤掉的修正条数。
    pub ignored: usize,
}

/// 与 [`apply_modifiers`] 同语义，但**附带分解信息**（谁参与了、谁被无视了）。
///
/// 规则层用 [`apply_modifiers`]；日志、调试面板与未来的"按因子触发"用这个。
pub fn evaluate_modifiers(
    stat: Stat,
    base: f64,
    mods: &[Modifier],
    ignored: &[EffectSource],
) -> ModifierOutcome {
    let relevant: Vec<&Modifier> = for_stat(mods, stat).collect();
    ModifierOutcome {
        value: apply_modifiers(stat, base, mods, ignored),
        applied: relevant.iter().filter(|m| !m.is_ignored_by(ignored)).count(),
        ignored: relevant.iter().filter(|m| m.is_ignored_by(ignored)).count(),
        relevant: relevant.len(),
    }
}

#[cfg(test)]
#[path = "rules_tests.rs"]
mod tests;
