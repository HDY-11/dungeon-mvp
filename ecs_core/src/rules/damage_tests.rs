//! `rules::damage` 的测试。
//!
//! Phase H2 的验收线是**两条**，这里各有一组用例：
//!
//! 1. **公式与数值不变**：下限、暴击阈值、倍率三个口径各自被钉住；
//!    另有一条用例保证"旧的标量签名（`combat::compute_melee_damage`）与新的
//!    结构化签名**接线一致**"——它是**适配器**的回归，不是公式的重复验证；
//! 2. **因子分解可复算**：`base * crit_multiplier` 必须精确等于 `damage`。

use super::*;
use crate::combat::compute_melee_damage;

/// 一组覆盖边界的输入：(攻击, 防御, 暴击率, 暴击伤害, 掷数)。
///
/// 刻意包含：防御高于攻击（吃下限 1）、暴击率恰好等于掷数（`>` 而非 `>=`）、
/// 掷数为 0（必定暴击）、暴击伤害为负（吃 `max(0.0)`）、以及非有限值。
const CASES: [(f64, f64, f64, f64, f64); 12] = [
    (10.0, 4.0, 0.0, 0.5, 0.99),
    (10.0, 4.0, 1.0, 0.5, 0.0),
    (10.0, 4.0, 0.5, 0.5, 0.5),
    (10.0, 4.0, 0.5, 0.0, 0.25),
    (10.0, 4.0, 0.5, -1.0, 0.25),
    (3.0, 10.0, 0.5, 0.5, 0.25),
    (3.0, 3.0, 0.5, 0.5, 0.25),
    (1.0, 1.0, 0.0, 0.0, 0.0),
    (0.0, 0.0, 0.5, 1.0, 0.1),
    (1e300, 0.0, 0.5, 0.0, 0.9),
    (f64::NAN, 1.0, 0.5, 0.5, 0.5),
    (1.0, f64::INFINITY, 0.5, 0.5, 0.5),
];

fn input_of(case: (f64, f64, f64, f64, f64)) -> MeleeInput {
    let (attack, defense, rate, damage, crit_roll) = case;
    MeleeInput {
        attack,
        defense,
        target_crit: CritProfile { rate, damage },
        crit_roll,
    }
}

/// 旧的标量签名与新的结构化签名**接线一致**（含 NaN/inf 的传播方式）。
///
/// 注意这条用例的性质：H2 之后 `compute_melee_damage` 是**适配器**（它内部调用
/// [`melee`]），所以它验证的是"标量 → 结构体"的字段映射没接错
/// （攻击/防御/暴击率/暴击伤害/掷数各就各位），**不是**公式的重复验证——
/// 公式本身由下面三条口径用例钉住。
///
/// "新旧公式逐值一致"这一点由构造保证（旧实现的表达式原样搬进 [`melee`]，
/// 见 commit 记录），不靠本用例；本用例保证的是这层壳不会将来接错。
///
/// 用 `to_bits` 比较而不是 `==`：NaN 参与时 `==` 恒假，会让"两边都返回 NaN"
/// 这种正确情形被误判为失败；而我们要断言的是**逐位相同**。
#[test]
fn legacy_scalar_adapter_maps_fields_correctly() {
    for case in CASES {
        let (attack, defense, rate, damage, crit_roll) = case;
        let legacy = compute_melee_damage(attack, defense, rate, damage, crit_roll);
        let new = melee(input_of(case));

        assert_eq!(
            new.damage.to_bits(),
            legacy.damage.to_bits(),
            "适配器必须把标量各就各位：case={case:?}"
        );
        assert_eq!(
            new.is_crit, legacy.is_crit,
            "适配器必须把标量各就各位：case={case:?}"
        );
    }
}

/// H2 验收 ②：**分解出的因子必须能复算出最终值**。
///
/// 这条是 `MeleeBreakdown::recompute` 存在的理由——没有它，"返回因子分解"
/// 只是一堆字段，无法保证它们与最终值一致。
#[test]
fn factors_reconstruct_the_final_damage() {
    for case in CASES {
        let breakdown = melee(input_of(case));
        assert_eq!(
            breakdown.recompute().to_bits(),
            breakdown.damage.to_bits(),
            "base × crit_multiplier 必须精确等于 damage：case={case:?}"
        );
    }
}

/// 未暴击时倍率为中性 `1.0`、暴击时等于 `1 + 暴击伤害加成`（负数吃 `max(0.0)`）。
#[test]
fn crit_multiplier_is_neutral_or_one_plus_bonus() {
    let no_crit = melee(MeleeInput {
        attack: 10.0,
        defense: 4.0,
        target_crit: CritProfile {
            rate: 0.0,
            damage: 5.0,
        },
        crit_roll: 0.9,
    });
    assert!(!no_crit.is_crit);
    assert_eq!(no_crit.crit_multiplier, 1.0);

    let crit = melee(MeleeInput {
        attack: 10.0,
        defense: 4.0,
        target_crit: CritProfile {
            rate: 1.0,
            damage: 0.5,
        },
        crit_roll: 0.0,
    });
    assert!(crit.is_crit);
    assert_eq!(crit.crit_multiplier, 1.5);
    assert_eq!(crit.base, 6.0);
    assert_eq!(crit.damage, 9.0);

    // 负加成被 `max(0.0)` 压成中性，不会出现"暴击反而更弱"。
    let negative = melee(MeleeInput {
        attack: 10.0,
        defense: 4.0,
        target_crit: CritProfile {
            rate: 1.0,
            damage: -3.0,
        },
        crit_roll: 0.0,
    });
    assert!(negative.is_crit);
    assert_eq!(negative.crit_multiplier, 1.0);
}

/// 伤害下限：防御高于攻击时仍打 1 点（既有口径）。
#[test]
fn damage_floor_is_one() {
    for (attack, defense) in [(3.0, 10.0), (0.0, 5.0), (-5.0, 0.0)] {
        let breakdown = melee(MeleeInput {
            attack,
            defense,
            target_crit: CritProfile {
                rate: 0.0,
                damage: 0.0,
            },
            crit_roll: 1.0,
        });
        assert_eq!(breakdown.base, 1.0, "攻{attack} vs 防{defense} 必须落到下限 1");
    }
}

/// 暴击判定是**严格大于**（`crit_rate > crit_roll`），不是大于等于。
///
/// 这条边界很容易在两处实现间漂移（一处写 `>`、一处写 `>=`），单独钉住。
#[test]
fn crit_threshold_is_strictly_greater() {
    let at_threshold = melee(MeleeInput {
        attack: 10.0,
        defense: 4.0,
        target_crit: CritProfile {
            rate: 0.25,
            damage: 1.0,
        },
        crit_roll: 0.25,
    });
    assert!(
        !at_threshold.is_crit,
        "掷数等于暴击率时不算暴击（保持既有 `>` 口径）"
    );
}
