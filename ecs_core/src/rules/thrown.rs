//! 投掷伤害（GAME.md Gm9；用户 m10559 Q4：**另设新计算方式，注意预留**）。
//!
//! 与近战分开的原因：
//!
//! - 基值来自**物品**（[`crate::item::Throw::damage`]），不掺攻击力/力量项；
//! - 其余四域（增伤、暴击、护甲、抗性）与近战同构，但分开成独立纯函数，
//!   将来接穿透、弹药、距离衰减等新维度时只动这里，近战公式不受牵连。
//!
//! 楼层加成等新维度目前不接：调用方把基础值算好后传 `base` 即可。纯函数：
//! 不碰 `World`、不取随机数（`crit_roll` 由系统层给）。

use crate::rules::damage::{CritProfile, armor_multiplier, cleaned, clamp_resistance};

/// 投掷伤害的输入，按来源分组。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThrownInput {
    /// 物品给出的固定基础伤害（可含楼层加成，由调用方先算好）。
    pub base: f64,
    /// 投掷者伤害加成 p_d（乘区 `1 + p_d`）。
    pub damage_bonus: f64,
    /// 投掷者暴击参数。
    pub crit: CritProfile,
    /// 系统层从 `GameRng` 取的 `[0, 1)` 掷数。
    pub crit_roll: f64,
    /// 受击方有效防御力（护甲域与近战相同）。
    pub defense: f64,
    /// 受击方物理抗性（`[0, 0.75]`）。
    pub physical_resist: f64,
}

/// 投掷伤害的因子分解（日志/调试/将来按因子触发）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThrownBreakdown {
    /// 物品基础伤害。
    pub base: f64,
    /// 增伤域 `1 + damage_bonus`。
    pub damage_multiplier: f64,
    /// 暴击倍率（未暴击为 1.0）。
    pub crit_multiplier: f64,
    /// 护甲域 `δ / (防御 + δ)`。
    pub armor_multiplier: f64,
    /// 抗性域 `1 - 物理抗性`。
    pub resistance_multiplier: f64,
    /// 最终伤害（下限 1）。
    pub damage: f64,
    /// 本次是否暴击。
    pub is_crit: bool,
}

impl ThrownBreakdown {
    /// 用分解出的因子复算最终值（与 [`Self::damage`] 必须一致）。
    pub fn recompute(&self) -> f64 {
        (self.base
            * self.damage_multiplier
            * self.crit_multiplier
            * self.armor_multiplier
            * self.resistance_multiplier)
            .max(1.0)
    }
}

/// 计算一次投掷伤害：`max(1, base × 增伤 × 暴击 × 护甲 × 抗性)`。
pub fn thrown(input: ThrownInput) -> ThrownBreakdown {
    let base = cleaned(input.base);
    let damage_multiplier = 1.0 + cleaned(input.damage_bonus);
    let is_crit = input.crit.rate > input.crit_roll;
    let crit_multiplier = if is_crit {
        1.0 + cleaned(input.crit.damage)
    } else {
        1.0
    };
    let armor = armor_multiplier(input.defense);
    let resistance = 1.0 - clamp_resistance(input.physical_resist);
    let damage = (base * damage_multiplier * crit_multiplier * armor * resistance).max(1.0);
    ThrownBreakdown {
        base,
        damage_multiplier,
        crit_multiplier,
        armor_multiplier: armor,
        resistance_multiplier: resistance,
        damage,
        is_crit,
    }
}

#[cfg(test)]
#[path = "thrown_tests.rs"]
mod tests;
