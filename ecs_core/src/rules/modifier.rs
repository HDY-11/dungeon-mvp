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
//! ④ 折叠       ：先基础值修正（加/减），再乘区修正（独立因子相乘）
//! ⑤ 纯函数     ：规则只吃第 ④ 步的结果（[`apply_modifiers`] 就是这一步）
//! ```
//!
//! 本模块只实现 ③④⑤（**位置无关、可单测**）。①②依赖世界查询，属 H9/H11。
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
//! | 属形状（本模块定） | 属内容（GAME.md，待定） |
//! |---|---|
//! | 有没有"来源"这个维度 | 各来源的系数与取值 |
//! | 折叠顺序（先基础值、后乘区） | 叠加口径（加算还是各自成区）的最终定案 |
//! | `ignored` 是丢桶而不是特例分支 | 哪些具体效果属于哪个桶 |
//!
//! **当前阶段的不变量（Phase H8 验收）：** 修正器列表为**空**时，
//! [`apply_modifiers`] 必须**逐值等于** `base`——这条保证"接上求值侧"
//! 本身不改变任何现有行为，由 `rules_tests.rs` 的 parity 用例钉住。

/// 修正的**来源类别**：折叠时按桶过滤（"无视某类减速" = 丢掉这个桶）。
///
/// 这是**形状**：桶的存在与命名是设计；"哪些效果属于哪个桶"是内容。
///
/// 新增一个桶会改变"无视类效果能表达什么"，因此要按 `LECS22` 先问一句：
/// **加第 N 个桶要改几处？** 当前答案是 1 处（本枚举 + 使用方的 `ignored` 列表），
/// 且**不动** [`apply_modifiers`] 的折叠逻辑——这正是把它做成"数据"的收益。
///
/// `#[non_exhaustive]`：桶集合预期会随内容设计增补；这样加桶不会破坏下游的穷尽匹配。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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

/// 一条修正：来源 + 两类位（DESIGN DsnE10 第 2 条）。
///
/// 两个位**同时存在**而不是"二选一"，因为同一条效果两种都可能需要
/// （例："移动速度 -1 且额外 ×0.8"）。取值属内容，本结构只保证它们有位置可放。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Modifier {
    /// 这条修正由哪一类来源产生（过滤按它做）。
    pub source: EffectSource,
    /// **基础值修正位**：加在 base 上（可为负）。
    pub base_delta: f64,
    /// **乘区位**：作为独立因子相乘（`1.0` = 不改变）。
    pub multiplier: f64,
}

impl Modifier {
    /// 一条只做基础值增减的修正（乘区为中性 `1.0`）。
    pub const fn delta(source: EffectSource, base_delta: f64) -> Self {
        Self {
            source,
            base_delta,
            multiplier: 1.0,
        }
    }

    /// 一条只做乘区缩放的修正（基础值增量为 `0.0`）。
    pub const fn scaled(source: EffectSource, multiplier: f64) -> Self {
        Self {
            source,
            base_delta: 0.0,
            multiplier,
        }
    }

    /// 这条修正是否属于被无视的来源之一。
    pub fn is_ignored_by(&self, ignored: &[EffectSource]) -> bool {
        ignored.contains(&self.source)
    }
}

/// 求值五步的第 ③④⑤ 步：**过滤 → 折叠 → 纯结果**。
///
/// 纯函数：不碰 `World`、不发事件、不改状态；`mods` 为空时逐值返回 `base`
/// （因此"把现有数值接上求值侧"本身是零行为变化）。
///
/// 折叠顺序（**[暂定]**，最终口径属 GAME.md）：`(base + Σ base_delta) × Π multiplier`。
/// 选"先加后乘"只为让两类位的作用可分离、便于单测；若内容阶段定成"各区独立相乘"，
/// 改这里一处即可（调用方只依赖"返回一个 `f64`"）。
pub fn apply_modifiers(base: f64, mods: &[Modifier], ignored: &[EffectSource]) -> f64 {
    let mut value = base;
    // 先折叠基础值修正，再折叠乘区——两个循环是"顺序即语义"的显式表达，
    // 合写成一个循环会让顺序隐式依赖于列表次序。
    for m in mods.iter().filter(|m| !m.is_ignored_by(ignored)) {
        value += m.base_delta;
    }
    for m in mods.iter().filter(|m| !m.is_ignored_by(ignored)) {
        value *= m.multiplier;
    }
    value
}

/// 求值入口的**读数结果**：最终速度 + 计算它时用到的修正（供日志/调试解释"为什么是这个数"）。
///
/// 与 DsnE9 的"返回因子分解"同源：数值链路的每一步都应能回答"它从哪来"。
#[derive(Debug, Clone, PartialEq)]
pub struct ModifierOutcome {
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
    base: f64,
    mods: &[Modifier],
    ignored: &[EffectSource],
) -> ModifierOutcome {
    ModifierOutcome {
        value: apply_modifiers(base, mods, ignored),
        applied: mods.iter().filter(|m| !m.is_ignored_by(ignored)).count(),
        ignored: mods.iter().filter(|m| m.is_ignored_by(ignored)).count(),
    }
}

#[cfg(test)]
#[path = "rules_tests.rs"]
mod tests;
