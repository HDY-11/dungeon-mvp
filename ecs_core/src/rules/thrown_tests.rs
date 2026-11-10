//! `rules::thrown` 的测试：投掷五域口径 + 因子分解可复算（Gm9 / m10559 Q4）。
//!
//! 验收线：
//! 1. 基值只来自物品，不掺力量/攻击力；
//! 2. 增伤、暴击、护甲、抗性四个域与近战同口径；
//! 3. 下限在最终值上（防御/抗性再高也只压到 1）；
//! 4. 因子分解可复算。

use super::*;

fn input(base: f64) -> ThrownInput {
    ThrownInput {
        base,
        damage_bonus: 0.0,
        crit: CritProfile {
            rate: 0.0,
            damage: 0.0,
        },
        crit_roll: 1.0,
        defense: 0.0,
        physical_resist: 0.0,
    }
}

#[test]
fn a_plain_throw_multiplies_by_the_armor_domain_only() {
    let breakdown = thrown(ThrownInput {
        defense: 4.0,
        ..input(3.0)
    });
    // 防御 4、δ=4 → 护甲域 0.5；3 × 0.5 = 1.5。
    assert_eq!(breakdown.armor_multiplier, 0.5);
    assert_eq!(breakdown.resistance_multiplier, 1.0);
    assert!((breakdown.damage - 1.5).abs() < 1e-9);
    assert!(!breakdown.is_crit);
}

#[test]
fn the_base_is_the_thrown_item_value_not_a_strength_term() {
    // 同为 base=3，与近战 input.attack=3 的结果一致；这里钉住"没有力量项"。
    let breakdown = thrown(input(3.0));
    assert_eq!(breakdown.base, 3.0);
    assert!((breakdown.damage - 3.0).abs() < 1e-9);
}

#[test]
fn damage_bonus_and_crit_compose_multiplicatively() {
    let breakdown = thrown(ThrownInput {
        damage_bonus: 0.2,
        crit: CritProfile {
            rate: 0.5,
            damage: 0.5,
        },
        crit_roll: 0.1,
        ..input(3.0)
    });
    assert!(breakdown.is_crit);
    assert!((breakdown.damage_multiplier - 1.2).abs() < 1e-9);
    assert!((breakdown.crit_multiplier - 1.5).abs() < 1e-9);
    assert!((breakdown.damage - 5.4).abs() < 1e-9);
}

#[test]
fn a_non_crit_roll_leaves_the_crit_factor_at_one() {
    let breakdown = thrown(ThrownInput {
        crit: CritProfile {
            rate: 0.25,
            damage: 1.0,
        },
        crit_roll: 0.25,
        ..input(3.0)
    });
    assert!(!breakdown.is_crit);
    assert_eq!(breakdown.crit_multiplier, 1.0);
}

#[test]
fn physical_resistance_is_clamped_to_three_quarters() {
    let breakdown = thrown(ThrownInput {
        defense: 0.0,
        physical_resist: 5.0,
        ..input(4.0)
    });
    assert!((breakdown.resistance_multiplier - 0.25).abs() < 1e-9);
    assert!((breakdown.damage - 1.0).abs() < 1e-9);
}

#[test]
fn the_final_damage_never_drops_below_one() {
    let breakdown = thrown(ThrownInput {
        defense: 1_000.0,
        physical_resist: 0.75,
        ..input(1.0)
    });
    assert_eq!(breakdown.damage, 1.0);
}

#[test]
fn non_finite_inputs_fall_back_to_the_conservative_zero() {
    let breakdown = thrown(ThrownInput {
        base: f64::NAN,
        damage_bonus: f64::NAN,
        defense: f64::NAN,
        physical_resist: f64::NAN,
        ..input(0.0)
    });
    assert_eq!(breakdown.base, 0.0);
    assert_eq!(breakdown.damage_multiplier, 1.0);
    assert_eq!(breakdown.resistance_multiplier, 1.0);
    assert_eq!(breakdown.damage, 1.0);
}

#[test]
fn factors_reconstruct_the_final_damage() {
    let breakdown = thrown(ThrownInput {
        base: 3.0,
        damage_bonus: 0.2,
        crit: CritProfile {
            rate: 0.5,
            damage: 0.5,
        },
        crit_roll: 0.1,
        defense: 7.0,
        physical_resist: 0.3,
    });
    assert!((breakdown.recompute() - breakdown.damage).abs() < 1e-9);
}
