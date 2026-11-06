//! `rules::damage` 的测试：公式各域的口径 + 因子分解可复算（DsnE24 阶段 ①）。
//!
//! 验收线：
//!
//! 1. **每个域只做自己那件事**：力量封顶、增伤乘区、暴击（`>` 与 `max(0)`）、
//!    护甲曲线、抗性上限；
//! 2. **下限在最终值上**：护甲/抗性再高也只压到 1；
//! 3. **因子分解可复算**：`recompute()` 必须精确等于 `damage`。

use super::*;
use crate::balance::{ARMOR_DELTA, STRENGTH_TO_DAMAGE, UNARMED_STRENGTH_CAP};

fn input(attack: f64, defense: f64) -> MeleeInput {
    MeleeInput {
        attack,
        strength: 0.0,
        strength_cap: UNARMED_STRENGTH_CAP,
        damage_bonus: 0.0,
        defense,
        physical_resist: 0.0,
        crit: CritProfile {
            rate: 0.0,
            damage: 0.0,
        },
        crit_roll: 1.0,
    }
}

/// 一组覆盖边界与各域输入的用例。
fn cases() -> Vec<MeleeInput> {
    vec![
        input(10.0, 4.0),
        input(10.0, 0.0),
        input(3.0, 10.0),
        input(0.0, 0.0),
        MeleeInput {
            strength: 10.0,
            strength_cap: 3.0,
            ..input(8.0, 4.0)
        },
        MeleeInput {
            damage_bonus: 0.5,
            ..input(10.0, 4.0)
        },
        MeleeInput {
            physical_resist: 0.5,
            ..input(10.0, 4.0)
        },
        MeleeInput {
            crit: CritProfile {
                rate: 1.0,
                damage: 0.5,
            },
            crit_roll: 0.0,
            ..input(10.0, 4.0)
        },
        MeleeInput {
            crit: CritProfile {
                rate: 1.0,
                damage: -3.0,
            },
            crit_roll: 0.0,
            ..input(10.0, 4.0)
        },
        MeleeInput {
            attack: f64::NAN,
            defense: f64::NAN,
            ..input(0.0, 0.0)
        },
        MeleeInput {
            defense: f64::INFINITY,
            ..input(10.0, 0.0)
        },
    ]
}

/// 分解出的因子必须能复算出最终值。
#[test]
fn factors_reconstruct_the_final_damage() {
    for case in cases() {
        let breakdown = melee(case);
        assert_eq!(
            breakdown.recompute().to_bits(),
            breakdown.damage.to_bits(),
            "五域相乘必须精确等于 damage：case={case:?}"
        );
    }
}

/// 护甲是曲线：防御 0 不减伤、防御 = δ 时减半、防御更高但仍有下限 1。
#[test]
fn armor_is_a_curve_not_subtraction() {
    let no_armor = melee(input(10.0, 0.0));
    assert_eq!(no_armor.armor_multiplier, 1.0, "防御 0 不减伤");
    assert_eq!(no_armor.damage, 10.0);

    let delta_armor = melee(input(10.0, ARMOR_DELTA));
    assert_eq!(
        delta_armor.armor_multiplier,
        0.5,
        "防御 = δ 时护甲域应为 50%"
    );
    assert_eq!(delta_armor.damage, 5.0);

    let deep_armor = melee(input(10.0, 12.0));
    assert_eq!(deep_armor.armor_multiplier, 0.25);
    assert_eq!(deep_armor.damage, 2.5);

    // 防御再高也只压到 1，不会出现 0/负伤害。
    let wall = melee(input(1.0, 1e9));
    assert_eq!(wall.damage, 1.0, "护甲域只压到下限 1");
}

/// 力量按武器上限封顶后转成基础伤害；空手用空手上限。
#[test]
fn strength_is_capped_by_the_weapon() {
    let capped = melee(MeleeInput {
        strength: 10.0,
        strength_cap: 3.0,
        ..input(8.0, 0.0)
    });
    assert_eq!(capped.strength_bonus, 3.0 * STRENGTH_TO_DAMAGE);
    assert_eq!(capped.base, 8.0 + 3.0 * STRENGTH_TO_DAMAGE);
    assert_eq!(capped.damage, capped.base);

    let weak = melee(MeleeInput {
        strength: 2.0,
        strength_cap: 10.0,
        ..input(8.0, 0.0)
    });
    assert_eq!(weak.strength_bonus, 2.0 * STRENGTH_TO_DAMAGE);

    let unarmed = melee(MeleeInput {
        strength: 99.0,
        strength_cap: UNARMED_STRENGTH_CAP,
        ..input(8.0, 0.0)
    });
    assert_eq!(
        unarmed.strength_bonus,
        UNARMED_STRENGTH_CAP * STRENGTH_TO_DAMAGE
    );
}

/// 增伤与抗性是独立的乘法域，互不干扰。
#[test]
fn damage_bonus_and_resistance_are_separate_zones() {
    let boosted = melee(MeleeInput {
        damage_bonus: 0.5,
        ..input(10.0, 4.0)
    });
    assert_eq!(boosted.damage_multiplier, 1.5);
    assert_eq!(boosted.damage, 7.5);

    let resistant = melee(MeleeInput {
        physical_resist: 0.5,
        ..input(10.0, 4.0)
    });
    assert_eq!(resistant.resistance_multiplier, 0.5);
    assert_eq!(resistant.damage, 2.5);

    let both = melee(MeleeInput {
        damage_bonus: 1.0,
        physical_resist: 0.5,
        ..input(10.0, 4.0)
    });
    assert_eq!(both.damage, 5.0, "基础 10 × 1.5(护甲0.5) 再 ×2 ×0.5");

    // 抗性夹到 75%，不会出现"免疫"。
    let overcapped = melee(MeleeInput {
        physical_resist: 0.99,
        ..input(10.0, 0.0)
    });
    assert_eq!(overcapped.resistance_multiplier, 0.25);
    assert_eq!(overcapped.damage, 2.5);
}

/// 未暴击时倍率为中性 `1.0`、暴击时等于 `1 + 暴击伤害加成`（负数吃 `max(0.0)`）。
#[test]
fn crit_multiplier_is_neutral_or_one_plus_bonus() {
    let no_crit = melee(MeleeInput {
        crit: CritProfile {
            rate: 0.0,
            damage: 5.0,
        },
        crit_roll: 0.9,
        ..input(10.0, 0.0)
    });
    assert!(!no_crit.is_crit);
    assert_eq!(no_crit.crit_multiplier, 1.0);

    let crit = melee(MeleeInput {
        crit: CritProfile {
            rate: 1.0,
            damage: 0.5,
        },
        crit_roll: 0.0,
        ..input(10.0, 0.0)
    });
    assert!(crit.is_crit);
    assert_eq!(crit.crit_multiplier, 1.5);
    assert_eq!(crit.damage, 15.0);

    let negative = melee(MeleeInput {
        crit: CritProfile {
            rate: 1.0,
            damage: -3.0,
        },
        crit_roll: 0.0,
        ..input(10.0, 0.0)
    });
    assert!(negative.is_crit);
    assert_eq!(negative.crit_multiplier, 1.0);
}

/// 暴击判定是**严格大于**（`crit_rate > crit_roll`），不是大于等于。
#[test]
fn crit_threshold_is_strictly_greater() {
    let at_threshold = melee(MeleeInput {
        crit: CritProfile {
            rate: 0.25,
            damage: 1.0,
        },
        crit_roll: 0.25,
        ..input(10.0, 0.0)
    });
    assert!(!at_threshold.is_crit, "掷数等于暴击率时不算暴击");
}

/// 非有限输入不会产生 NaN 伤害：NaN 落到 0，无限防御把护甲域压到 0 后吃下限 1。
#[test]
fn non_finite_inputs_stay_finite_or_floor_at_one() {
    let nan = melee(MeleeInput {
        attack: f64::NAN,
        defense: f64::NAN,
        ..input(0.0, 0.0)
    });
    assert_eq!(nan.base, 0.0);
    assert_eq!(nan.damage, 1.0);

    let infinite_defense = melee(input(10.0, f64::INFINITY));
    assert_eq!(infinite_defense.armor_multiplier, 0.0);
    assert_eq!(infinite_defense.damage, 1.0);
}
