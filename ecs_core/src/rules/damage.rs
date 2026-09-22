//! 近战伤害：**结构化输入 + 因子分解返回**（DESIGN DsnE9 / Phase H2）。
//!
//! # 为什么改形状
//!
//! 旧签名是 5 个裸标量：
//!
//! ```text
//! compute_melee_damage(attack, defense, crit_rate, crit_damage, crit_roll) -> MeleeResult { damage, is_crit }
//! ```
//!
//! 它**恰好表达"减一次防、乘一次暴击"**：每新增一个伤害修正（装备、Buff、技能、
//! 生物范畴的固有性质）都要改签名 + 改所有调用点；返回类型也只有最终值，
//! 没有各因子的分解，因此日志无法解释"为什么是这个数"，也没有"按因子触发"的落点。
//!
//! 改后的形状（**公式与数值均未改动**）：
//!
//! - **输入按来源分组**（[`MeleeInput`]）：攻击方数值 / 受击方数值 / 随机输入；
//! - **返回因子分解**（[`MeleeBreakdown`]）：各因子 + 最终值。
//!
//! # 形状 vs 内容（红线）
//!
//! `基础值 = max(攻击 − 防御, 1)` 与 `暴击 → × (1 + 暴击伤害)` 是**当前实现的口径**，
//! 本阶段只把它**原样搬进新结构**。分区（有哪些因子）、系数（防御系数、
//! 增伤分区的叠加方式）**全部待定，属 GAME.md**：
//!
//! - 要改成 `attack * 攻击系数 − defense * 防御系数` 之类的形式 → [`MeleeInput`]
//!   已经是"按来源分组的输入"，加字段即可，**不必再动签名与调用点**；
//! - 要按因子触发效果 → [`MeleeBreakdown`] 已经带上 `crit_multiplier`，
//!   可直接判"这一击是否吃到了暴击分区"。
//!
//! # 随机数仍在系统层取
//!
//! `crit_roll` 由调用方从 `GameRng` 取（沿用既有做法），因此本函数保持**纯函数**：
//! 同样输入必得同样输出，可在无 ECS 的单测里逐条钉住。

/// 近战伤害的输入，**按来源分组**（DsnE9 第 1 条）。
///
/// 分三组而不是一串裸标量：调用点因此可读（谁是谁一目了然），
/// 且新增一类输入时**改的是结构体字段，不是所有调用点的参数表**。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeleeInput {
    /// 攻击方数值。
    pub attack: f64,
    /// 受击方数值。
    pub defense: f64,
    /// 受击方的暴击参数（暴击率与暴击伤害加成）。
    pub target_crit: CritProfile,
    /// 随机输入：由系统层从 `GameRng` 取的 `[0, 1)` 掷数。
    pub crit_roll: f64,
}

/// 暴击参数：暴击率与暴击伤害加成。
///
/// 单独成结构体而不是两个字段：它总是一起被读取、一起被替换
/// （换武器/换 Buff 时两者同时变），拆开会让"忘记同步另一个"变成可能。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CritProfile {
    /// 暴击率（`crit_rate > crit_roll` 即暴击）。
    pub rate: f64,
    /// 暴击伤害加成（暴击时伤害 × `1 + damage`）。
    pub damage: f64,
}

/// 伤害的**因子分解**（DsnE9 第 2 条）：数值链路要能回答"它从哪来"。
///
/// 三个真实消费者：战斗日志（解释这个数）、调试面板、未来"按因子触发"的效果。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeleeBreakdown {
    /// 暴击前的伤害：`max(attack − defense, 1)`。
    pub base: f64,
    /// 本次的暴击倍率（未暴击为 `1.0`）。
    pub crit_multiplier: f64,
    /// 最终伤害 = `base * crit_multiplier`。
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
        self.base * self.crit_multiplier
    }
}

/// 计算一次近战的结果（**公式与数值与本阶段之前完全一致**）。
///
/// 纯函数：不碰 `World`、不取随机数（`crit_roll` 由调用方给）、不发事件。
pub fn melee(input: MeleeInput) -> MeleeBreakdown {
    let base = (input.attack - input.defense).max(1.0);
    let is_crit = input.target_crit.rate > input.crit_roll;
    let crit_multiplier = if is_crit {
        1.0 + input.target_crit.damage.max(0.0)
    } else {
        1.0
    };
    MeleeBreakdown {
        base,
        crit_multiplier,
        damage: base * crit_multiplier,
        is_crit,
    }
}

#[cfg(test)]
#[path = "damage_tests.rs"]
mod tests;
