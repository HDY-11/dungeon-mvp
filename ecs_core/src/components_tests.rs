//! `super`（`components`）里稠密属性表 `Stats` 与通用资源池 `Pool` 的测试
//! （DsnE22 阶段 ②b / DsnE23）。

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
    // 上限修正默认 0：等级曲线就是裸上限（DsnE23）。
    assert_eq!(stats.get(StatId::MAX_HEALTH), 0.0);
    assert_eq!(stats.get(StatId::MAX_MANA), 0.0);
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

#[test]
fn a_new_pool_starts_full() {
    let pool = Pool::new(9.0);
    assert_eq!((pool.current, pool.max), (9.0, 9.0));
    assert!(pool.is_alive());
    assert_eq!(pool.ratio(), 1.0);
}

#[test]
fn pool_damage_and_heal_clamp_to_the_bounds() {
    let pool = Pool::new(10.0).damage(3.0);
    assert_eq!(pool.current, 7.0);
    let pool = pool.damage(100.0);
    assert_eq!(pool.current, 0.0, "伤害不会把 current 打进负数");
    assert!(!pool.is_alive());
    let pool = pool.heal(100.0);
    assert_eq!(pool.current, 10.0, "治疗不会超过上限");
}

#[test]
fn pool_with_current_clamps_to_the_maximum() {
    assert_eq!(Pool::full(5.0, 10.0).with_current(50.0).current, 10.0);
    assert_eq!(Pool::full(5.0, 10.0).with_current(-1.0).current, 0.0);
}

#[test]
fn try_cost_rejects_negative_and_unaffordable_amounts() {
    let mut mana = Mana::from(Pool::full(3.0, 12.0));
    assert!(mana.try_cost(2.0).is_ok());
    assert_eq!(mana.current, 1.0);
    assert_eq!(mana.try_cost(2.0), Err("资源不足"));
    assert_eq!(mana.try_cost(-1.0), Err("资源消耗不能为负数"));
    assert_eq!(mana.current, 1.0, "失败的消耗不改变资源");
}

#[test]
fn restore_returns_the_actual_amount_and_caps_at_max() {
    let mut health = Health::from(Pool::full(8.0, 10.0));
    assert_eq!(health.restore(1.0), 1.0);
    assert_eq!(health.restore(100.0), 1.0);
    assert_eq!(health.current, 10.0);
    assert_eq!(health.restore(5.0), 0.0, "满池没有可恢复量");
}

#[test]
fn health_and_mana_expose_their_pool_through_deref() {
    let health = Health::full(12.0, 40.0);
    assert_eq!(health.current, 12.0);
    assert_eq!(health.max, 40.0);
    assert!(health.is_alive());

    let mut mana = Mana::new(12.0);
    mana.current = 3.0;
    assert_eq!(mana.current, 3.0);
    assert_eq!(mana.ratio(), 0.25);
}
