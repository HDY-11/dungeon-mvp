//! 生命/法力上限公式的单元测试（DsnE23），以及法力随时间的回复（DsnE25）。
//!
//! 这里钉的是"数值口径"：曲线、修正折叠、夹取、升级**不回满**（DsnE25）与
//! 法力回复的节奏/上限/死亡不回复。系统接线由集成测试覆盖。

use bevy_ecs::prelude::Entity;
use bevy_ecs::system::RunSystemOnce;
use bevy_ecs::world::World;

use super::*;
use crate::components::Pool;
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
fn apply_maxima_restores_only_when_asked() {
    let mut health = Health::full(1.0, 1.0);
    let mut mana = Mana::from(crate::components::Pool::full(0.0, 1.0));

    // `restore = true` 只给"出生满池"这类一次性初始化；升级走的是 `false`（DsnE25）。
    apply_maxima(3, &[], &mut health, &mut mana, true);
    assert_eq!((health.current, health.max), (56.0, 56.0));
    assert_eq!((mana.current, mana.max), (16.0, 16.0));
}

// ── DsnE25：法力随 AV 回复 ─────────────────────────────

/// 造一个只有玩家、且已设定"上一轮推进了多少 AV"的世界。
fn regen_world(mana: (f64, f64), step_av: f64) -> (World, Entity) {
    let mut world = World::new();
    world.insert_resource(WorldClock {
        total_av: step_av,
        last_step_av: step_av,
    });
    let entity = world
        .spawn((
            crate::entity_cls::Player,
            Health::full(40.0, 40.0),
            Mana::from(Pool::full(mana.0, mana.1)),
            ManaRegen::new(),
        ))
        .id();
    (world, entity)
}

fn run_regen(world: &mut World, step_av: f64) {
    world.resource_mut::<WorldClock>().last_step_av = step_av;
    world
        .run_system_once(mana_regen_system)
        .expect("法力回复系统必须能跑");
}

#[test]
fn mana_regenerates_one_point_per_interval() {
    let (mut world, entity) = regen_world((0.0, 12.0), 0.0);

    // 一个完整间隔：0 → 1。
    run_regen(&mut world, MANA_REGEN_AV);
    assert_eq!(world.get::<Mana>(entity).unwrap().current, 1.0);

    // 半个间隔不回复，但零头保留：连续两次半个间隔凑够一个点。
    run_regen(&mut world, MANA_REGEN_AV / 2.0);
    assert_eq!(world.get::<Mana>(entity).unwrap().current, 1.0);
    run_regen(&mut world, MANA_REGEN_AV / 2.0);
    assert_eq!(world.get::<Mana>(entity).unwrap().current, 2.0);

    // 一次跨过 1.5 个间隔：只回 1 点，余下 0.5 个间隔留到下一轮。
    run_regen(&mut world, MANA_REGEN_AV * 1.5);
    assert_eq!(world.get::<Mana>(entity).unwrap().current, 3.0);
    run_regen(&mut world, MANA_REGEN_AV / 2.0);
    assert_eq!(world.get::<Mana>(entity).unwrap().current, 4.0);
}

#[test]
fn mana_regen_does_not_bank_while_the_pool_is_full() {
    let (mut world, entity) = regen_world((12.0, 12.0), 0.0);

    // 满池挂机很久：进度必须清零，不能攒出一发大的。
    run_regen(&mut world, MANA_REGEN_AV * 10.0);
    assert_eq!(
        world.get::<ManaRegen>(entity).unwrap().progress_av,
        0.0,
        "满池时不得积攒进度"
    );

    // 花掉 1 点，等一个间隔只回 1 点——不会因为刚才挂机而连回。
    world.get_mut::<Mana>(entity).unwrap().current = 11.0;
    run_regen(&mut world, MANA_REGEN_AV);
    assert_eq!(world.get::<Mana>(entity).unwrap().current, 12.0);
    assert_eq!(
        world.get::<ManaRegen>(entity).unwrap().progress_av,
        0.0,
        "满池时溢出的进度要丢弃"
    );
}

#[test]
fn dead_players_do_not_regenerate_mana() {
    let (mut world, entity) = regen_world((0.0, 12.0), 0.0);
    {
        let mut health = world.get_mut::<Health>(entity).unwrap();
        health.current = 0.0;
    }

    run_regen(&mut world, MANA_REGEN_AV);
    assert_eq!(world.get::<Mana>(entity).unwrap().current, 0.0);
    assert_eq!(world.get::<ManaRegen>(entity).unwrap().progress_av, 0.0);
}

#[test]
fn zero_step_does_not_advance_the_regen_progress() {
    let (mut world, entity) = regen_world((0.0, 12.0), 0.0);
    world.get_mut::<ManaRegen>(entity).unwrap().progress_av = MANA_REGEN_AV - 1.0;

    // 没有行动到点（`last_step_av == 0`）的轮次不代表时间流逝。
    run_regen(&mut world, 0.0);
    assert_eq!(
        world.get::<ManaRegen>(entity).unwrap().progress_av,
        MANA_REGEN_AV - 1.0
    );
    assert_eq!(world.get::<Mana>(entity).unwrap().current, 0.0);
}
