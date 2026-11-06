//! `super`（`components`）里稠密属性表 `Stats` 的测试（DsnE22 阶段 ②b）。

use super::*;
use crate::rules::{STATS, StatId};

#[test]
fn new_stats_start_at_the_table_defaults() {
    let stats = Stats::new();
    assert_eq!(stats.get(StatId::ATTACK), 0.0);
    assert_eq!(stats.get(StatId::DEFENSE), 0.0);
    assert_eq!(stats.get(StatId::MOVE_SPEED), 1.0);
    assert_eq!(stats.get(StatId::ATTACK_SPEED), 1.0);
    assert_eq!(stats.get(StatId::CRIT_RATE), 0.0);
    assert_eq!(stats.get(StatId::CRIT_DAMAGE), 0.0);
}

#[test]
fn from_pairs_only_overrides_the_listed_stats() {
    let stats = Stats::from_pairs([(StatId::ATTACK, 8.0), (StatId::MOVE_SPEED, 1.6)]);
    assert_eq!(stats.get(StatId::ATTACK), 8.0);
    assert_eq!(stats.get(StatId::MOVE_SPEED), 1.6);
    assert_eq!(stats.get(StatId::DEFENSE), 0.0, "未列出的属性保持默认");
    assert_eq!(stats.get(StatId::CRIT_RATE), 0.0);
}

#[test]
fn set_clamps_each_stat_to_its_table_range() {
    let mut stats = Stats::new();
    stats.set(StatId::ATTACK, -5.0);
    assert_eq!(stats.get(StatId::ATTACK), 0.0, "攻击下界 0");
    stats.set(StatId::DEFENSE, 2000.0);
    assert_eq!(stats.get(StatId::DEFENSE), 1000.0, "防御上界 1000");
    stats.set(StatId::MOVE_SPEED, 100.0);
    assert_eq!(stats.get(StatId::MOVE_SPEED), 20.0, "速度上界 20");
    stats.set(StatId::MOVE_SPEED, 0.0);
    assert_eq!(stats.get(StatId::MOVE_SPEED), 0.05, "速度下界 0.05（防除零）");
    stats.set(StatId::CRIT_RATE, 2.0);
    assert_eq!(stats.get(StatId::CRIT_RATE), 1.0, "概率上界 1");
    stats.set(StatId::CRIT_DAMAGE, -1.0);
    assert_eq!(stats.get(StatId::CRIT_DAMAGE), 0.0, "暴击加成下界 0");
}

#[test]
fn set_ignores_nan_instead_of_clamping_it() {
    let mut stats = Stats::new();
    stats.set(StatId::ATTACK, 7.0);
    stats.set(StatId::ATTACK, f64::NAN);
    assert_eq!(stats.get(StatId::ATTACK), 7.0, "NaN 保留旧值，不静默变 0");
}

#[test]
fn with_chains_like_a_builder() {
    let stats = Stats::new()
        .with(StatId::ATTACK, 12.0)
        .with(StatId::CRIT_RATE, 0.25);
    assert_eq!(stats.get(StatId::ATTACK), 12.0);
    assert_eq!(stats.get(StatId::CRIT_RATE), 0.25);
}

#[test]
fn every_default_lies_inside_its_table_range() {
    for def in STATS {
        assert!(
            def.default >= def.min && def.default <= def.max,
            "{} 的默认值 {} 不在 [{}, {}] 内",
            def.name,
            def.default,
            def.min,
            def.max
        );
    }
}
