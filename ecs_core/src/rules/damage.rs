//! 近战伤害：**结构化输入 + 因子分解返回**（DESIGN DsnE9 形状 / DsnE24 内容）。
//!
//! # 口径（DsnE24 阶段 ①，GAME.md Gm2）
//!
//! ```text
//! 基础值   = 攻击力 + min(力量, 武器力量上限) × φ
//! 增伤域   = 1 + 伤害加成
//! 暴击域   = 暴击 ? 1 + 暴击伤害 : 1
//! 护甲域   = δ / (目标有效防御 + δ)                 // δ = ARMOR_DELTA
//! 抗性域   = 1 - 物理抗性                          // 独立乘区，上限 75%
//! 最终伤害 = max(1, 基础值 × 增伤域 × 暴击域 × 护甲域 × 抗性域)
//! ```
//!
//! 与旧口径 `max(攻击 − 防御, 1)` 的差别：防御从**减法**变成**护甲曲线**（防御 0 不减伤、
//! 防御 = δ 时减伤一半、每点防御让有效生命线性增长），并新增力量、增伤、抗性三个域。
//! 公式里的常数/系数都在 [`crate::balance`]，这里只表达"因子怎么乘"。
//!
//! # 输入按来源分组
//!
//! - **攻击方**：`attack / strength / strength_cap / damage_bonus / crit`；
//! - **受击方**：`defense / physical_resist`；
//! - **随机输入**：`crit_roll`（由系统层从 `GameRng` 取）。
//!
//! 新增一类伤害修正时，改的是结构体字段，不是所有调用点的参数表。
//!
//! # 下限在最终值上
//!
//! `max(1, …)` 放在五个域乘完之后：高护甲/高抗性只把伤害压到 1，不会出现 0 或负伤害；
//! 基础值本身不单独设下限（否则"防御越高反而打得更痛"这类反直觉会出现）。
//!
//! # 非有限值的口径
//!
//! 各输入先做 `max(0)` / `min` 清理：NaN 会落到"最保守的 0"，`inf` 会被上限或护甲曲线
//! 吃掉；最终仍保证 `damage >= 1` 且有限（除攻击力为 `inf` 这种构造性输入外）。

use crate::balance::{ARMOR_DELTA, STRENGTH_TO_DAMAGE};

/// 暴击参数：暴击率与暴击伤害加成。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CritProfile {
    /// 暴击率（`rate > crit_roll` 即暴击）。
    pub rate: f64,
    /// 暴击伤害加成（暴击时伤害 × `1 + damage`）。
    pub damage: f64,
}

/// 近战伤害的输入，**按来源分组**。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeleeInput {
    /// 攻击方有效攻击力（基础 + 装备 + Buff 修正）。
    pub attack: f64,
    /// 攻击方有效力量。
    pub strength: f64,
    /// 攻击方主手武器的力量上限 S_m（空手用 `UNARMED_STRENGTH_CAP`）。
    pub strength_cap: f64,
    /// 攻击方伤害加成 p_d（乘区 `1 + p_d`）。
    pub damage_bonus: f64,
    /// 受击方有效防御力。
    pub defense: f64,
    /// 受击方物理抗性（`[0, 0.75]`，乘区 `1 - r`）。
    pub physical_resist: f64,
    /// 攻击方的暴击参数。
    pub crit: CritProfile,
    /// 随机输入：由系统层从 `GameRng` 取的 `[0, 1)` 掷数。
    pub crit_roll: f64,
}

/// 伤害的**因子分解**：数值链路要能回答"它从哪来"。
///
/// 消费者：战斗日志（解释这个数）、调试面板、未来"按因子触发"的效果。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeleeBreakdown {
    /// 力量转成的那部分基础伤害（已按 `strength_cap` 封顶）。
    pub strength_bonus: f64,
    /// 暴击/护甲/抗性之前的伤害：`攻击力 + strength_bonus`。
    pub base: f64,
    /// 增伤域 `1 + damage_bonus`。
    pub damage_multiplier: f64,
    /// 本次的暴击倍率（未暴击为 `1.0`）。
    pub crit_multiplier: f64,
    /// 护甲域 `δ / (防御 + δ)`。
    pub armor_multiplier: f64,
    /// 抗性域 `1 - 物理抗性`。
    pub resistance_multiplier: f64,
    /// 最终伤害（五个域乘完后的下限 1）。
    pub damage: f64,
    /// 本次是否暴击。
    pub is_crit: bool,
}

impl MeleeBreakdown {
    /// 用分解出的因子复算最终值——**日志/调试面板与"按因子触发"的公共口径**。
    ///
    /// 存在意义：让"返回的分解能不能复算出最终值"成为一条**可断言**的性质
    /// （见 `damage_tests.rs` 的 `factors_reconstruct_the_final_damage`），
    /// 而不是靠实现者自觉。
    pub fn recompute(&self) -> f64 {
        (self.base
            * self.damage_multiplier
            * self.crit_multiplier
            * self.armor_multiplier
            * self.resistance_multiplier)
            .max(1.0)
    }
}

/// 护甲域：`δ / (防御 + δ)`。
///
/// 防御 0 → 1.0（不减伤）；防御 = δ → 0.5；防御越大越接近 0 但永不为负。
/// 负防御按 0 处理；非有限值按 0 处理（NaN 防御不放大伤害）。
pub fn armor_multiplier(defense: f64) -> f64 {
    let defense = finite_at_least_zero(defense);
    ARMOR_DELTA / (defense + ARMOR_DELTA)
}

/// 把抗性收进 `[0, 0.75]`；NaN 按 0（不额外减伤），`+inf` 夹到上限 75%。
pub fn clamp_resistance(value: f64) -> f64 {
    if value.is_nan() {
        0.0
    } else {
        value.clamp(0.0, 0.75)
    }
}

/// 把参与乘法的输入清理到"合理的方向"：
///
/// - `NaN` 落到 0（最保守的取值，不放大也不缩小伤害）；
/// - 负值抬到 0；
/// - `+inf` **保留**：无限防御经护甲曲线得到 0 减伤、无限攻击得到无限伤害，
///   这类构造性输入应保持其原有语义，而不是被静默改成 0。
fn finite_at_least_zero(value: f64) -> f64 {
    if value.is_nan() {
        0.0
    } else {
        value.max(0.0)
    }
}

/// 计算一次近战的结果。
///
/// 纯函数：不碰 `World`、不取随机数（`crit_roll` 由调用方给）、不发事件。
pub fn melee(input: MeleeInput) -> MeleeBreakdown {
    let attack = finite_at_least_zero(input.attack);
    let strength = finite_at_least_zero(input.strength);
    let strength_cap = finite_at_least_zero(input.strength_cap);
    let strength_bonus = strength.min(strength_cap) * STRENGTH_TO_DAMAGE;
    let base = attack + strength_bonus;

    let damage_multiplier = 1.0 + finite_at_least_zero(input.damage_bonus);
    let is_crit = input.crit.rate > input.crit_roll;
    let crit_multiplier = if is_crit {
        1.0 + finite_at_least_zero(input.crit.damage)
    } else {
        1.0
    };
    let armor_multiplier = armor_multiplier(input.defense);
    let resistance_multiplier = 1.0 - clamp_resistance(input.physical_resist);

    let damage = (base * damage_multiplier * crit_multiplier * armor_multiplier
        * resistance_multiplier)
        .max(1.0);

    MeleeBreakdown {
        strength_bonus,
        base,
        damage_multiplier,
        crit_multiplier,
        armor_multiplier,
        resistance_multiplier,
        damage,
        is_crit,
    }
}

#[cfg(test)]
#[path = "damage_tests.rs"]
mod tests;
