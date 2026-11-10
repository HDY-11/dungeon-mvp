//! `super`（`effect::duration`）的测试：推进、到期回收、与 payload 的顺序契约。
//!
//! 时间轴是 AV（`WorldClock.last_step_av`），与 `effect/status_tests.rs` 同口径。

use super::*;
use crate::test_util::test_world;
use bevy_ecs::system::RunSystemOnce;

/// 模拟结算链里的一段：推进 → 到期回收。
fn advance(world: &mut World, av: f64) {
    world.resource_mut::<WorldClock>().last_step_av = av;
    world
        .run_system_once(advance_effect_durations_system)
        .expect("推进系统可运行");
    world
        .run_system_once(expire_effects_system)
        .expect("回收系统可运行");
}

#[test]
fn durations_advance_by_the_world_clock() {
    let mut world = test_world();
    let effect = world.spawn(EffectDuration::new(1000.0)).id();

    advance(&mut world, 400.0);
    assert_eq!(
        world.get::<EffectDuration>(effect).map(|d| d.remaining_av),
        Some(600.0),
        "一次推进 400 AV 后应剩 600"
    );

    advance(&mut world, 250.0);
    assert_eq!(
        world.get::<EffectDuration>(effect).map(|d| d.remaining_av),
        Some(350.0),
        "多次推进应连续扣减"
    );
    assert!(
        world.get_entity(effect).is_ok(),
        "还没到期不得回收"
    );
}

#[test]
fn expired_effects_are_reclaimed_by_the_single_owner() {
    let mut world = test_world();
    let effect = world.spawn(EffectDuration::new(400.0)).id();

    advance(&mut world, 400.0);
    assert!(
        world.get_entity(effect).is_err(),
        "remaining_av 归零后必须由 expire 系统回收"
    );
}

#[test]
fn fresh_effects_survive_the_round_they_are_created() {
    let mut world = test_world();

    // 这一轮的时间已经推进过了（模拟 payload 之后才施加的新效果）。
    advance(&mut world, 400.0);
    let fresh = world.spawn(EffectDuration::new(400.0)).id();

    world
        .run_system_once(expire_effects_system)
        .expect("回收系统可运行");
    assert!(
        world.get_entity(fresh).is_ok(),
        "本轮新挂上的效果不得在同轮被回收（它的计时还没开始走）"
    );
}

#[test]
fn entities_without_a_duration_are_left_alone() {
    let mut world = test_world();
    let permanent = world.spawn_empty().id();

    advance(&mut world, 10_000.0);
    assert!(
        world.get_entity(permanent).is_ok(),
        "没有 EffectDuration 的实体（如永久格子效果）不参与到期回收"
    );
}

#[test]
fn zero_elapsed_time_does_not_advance_durations() {
    let mut world = test_world();
    let effect = world.spawn(EffectDuration::new(400.0)).id();

    advance(&mut world, 0.0);
    assert_eq!(
        world.get::<EffectDuration>(effect).map(|d| d.remaining_av),
        Some(400.0),
        "时间没走时不得扣减"
    );
}
