//! 生命/法力上限公式的单元测试（DsnE23）。
//!
//! 这里钉的是"数值口径"：曲线、修正折叠、夹取与升级回满。系统接线由集成测试覆盖。

use super::*;
use crate::rules::{EffectSource, Modifier};

fn add_health(value: f64) -> Modifier {
    Modifier::delta(StatId::MAX_HEALTH, EffectSource::Equipment, value)
}

fn add_mana(value: f64) -> Modifier {
    Modifier::delta(StatId::MAX_MANA, EffectSource::Equipment, value)
}

#[test]
fn the_level_curves_are_pinned() {
    assert_eq!(hp_curve(1), 40.0);
    assert_eq!(hp_curve(10), 112.0);
    assert_eq!(mp_curve(1), 12.0);
    assert_eq!(mp_curve(10), 30.0);
    // 0 级按 1 级算：读档/测试里出现 0 不应把上限算崩。
    assert_eq!(hp_curve(0), 40.0);
    assert_eq!(mp_curve(0), 12.0);
}

#[test]
fn max_health_folds_the_matching_stat_only() {
    let modifiers = [add_health(3.0), add_mana(99.0)];
    assert_eq!(max_health(1, &modifiers), 43.0);
    assert_eq!(max_mana(1, &modifiers), 111.0);
}

#[test]
fn multipliers_compose_on_the_maximum() {
    // (40 + 3) × 1.5 = 64.5：上限也是普通属性，走同一套折叠。
    let modifiers = [
        add_health(3.0),
        Modifier::scaled(StatId::MAX_HEALTH, EffectSource::Equipment, 1.5),
    ];
    assert_eq!(max_health(1, &modifiers), 64.5);
}

#[test]
fn maxima_stay_positive_with_negative_modifiers() {
    // 负修正不应把上限算成 0：零血上限会让任何伤害都直接致死。
    let modifiers = [add_health(-1000.0)];
    assert_eq!(max_health(1, &modifiers), 1.0);
}

#[test]
fn apply_maxima_clamps_without_healing() {
    let mut health = Health::full(50.0, 999.0);
    let mut mana = Mana::new(999.0);
    mana.current = 5.0;

    // 普通刷新：上限变小，current 只向下夹取，绝不上涨。
    apply_maxima(1, &[], &mut health, &mut mana, false);
    assert_eq!((health.current, health.max), (40.0, 40.0));
    assert_eq!((mana.current, mana.max), (5.0, 12.0));

    // 上限变大：current 保持原值，不跟着变满。
    let mut health = Health::full(10.0, 40.0);
    let mut mana = Mana::from(crate::components::Pool::full(1.0, 12.0));
    apply_maxima(10, &[], &mut health, &mut mana, false);
    assert_eq!((health.current, health.max), (10.0, 112.0));
    assert_eq!((mana.current, mana.max), (1.0, 30.0));
}

#[test]
fn apply_maxima_restores_on_level_up() {
    let mut health = Health::full(1.0, 1.0);
    let mut mana = Mana::from(crate::components::Pool::full(0.0, 1.0));

    apply_maxima(3, &[], &mut health, &mut mana, true);
    assert_eq!((health.current, health.max), (56.0, 56.0));
    assert_eq!((mana.current, mana.max), (16.0, 16.0));
}
