//! `super`（`effect::status`）的测试：中毒的施加、刷新、运转与自我回收。
//!
//! 时间轴是 **AV**（世界时钟推进多少，毒就少多少），不是"结算了多少轮"——
//! 所以下面每个"推进时间"的地方都要显式给 `WorldClock.last_step_av`。
//! 这正是 ISSUES ECS46 修掉的那件事：按轮次计时会让"毒多久"随场上怪物数量漂移。

use super::*;
use crate::resources::WorldClock;
use crate::test_util::{spawn_test_monster, spawn_test_player, test_world};
use bevy_ecs::system::RunSystemOnce;

fn poison_of(world: &World, victim: Entity) -> Vec<(Entity, Poison)> {
    let Some(owned) = world.get::<OwnedEffects>(victim) else {
        return Vec::new();
    };
    let effects: Vec<Entity> = owned.iter().collect();
    effects
        .into_iter()
        .filter_map(|effect| world.get::<Poison>(effect).map(|poison| (effect, *poison)))
        .collect()
}

fn hit(world: &mut World, attacker: Entity, target: Entity) {
    world.send_event(AttackEvent {
        attacker,
        target,
        damage: 3.0,
        is_crit: false,
    });
}

/// 让世界时间前进 `av`，然后跑一次中毒结算。
fn advance_poison(world: &mut World, av: f64) {
    world.resource_mut::<WorldClock>().last_step_av = av;
    world.run_system_once(poison_tick_system).expect("系统可运行");
}

/// 蝎子命中 → 目标名下多一条中毒效果（`EffectOf` 关系真的被创建了）。
#[test]
fn a_scorpion_hit_poisons_its_target() {
    let mut world = test_world();
    let scorpion = spawn_test_monster(&mut world, MonsterKindId::Scorpion, (5, 5), 10.0, 5.0);
    let player = spawn_test_player(&mut world, (6, 5));

    hit(&mut world, scorpion, player);
    world.run_system_once(poison_apply_system).expect("系统可运行");

    let poisons = poison_of(&world, player);
    assert_eq!(poisons.len(), 1, "蝎子命中必须给目标挂一条中毒");
    assert_eq!(poisons[0].1.ticks_left, POISON_TICKS);
    assert_eq!(poisons[0].1.damage_per_tick, POISON_DAMAGE_PER_TICK);
    assert_eq!(poisons[0].1.remaining_av, POISON_TICK_AV);
}

/// 非蝎子命中不挂毒——否则"毒"就不是物种特性了。
#[test]
fn a_non_scorpion_hit_does_not_poison() {
    let mut world = test_world();
    let rat = spawn_test_monster(&mut world, MonsterKindId::Rat, (5, 5), 10.0, 5.0);
    let player = spawn_test_player(&mut world, (6, 5));

    hit(&mut world, rat, player);
    world.run_system_once(poison_apply_system).expect("系统可运行");

    assert!(
        poison_of(&world, player).is_empty(),
        "鼠命中不得挂毒（毒是蝎子的特性）"
    );
}

/// 重复命中**刷新**而不是叠层：效果实体仍只有一条，次数回满。
#[test]
fn reapplying_refreshes_instead_of_stacking() {
    let mut world = test_world();
    let scorpion = spawn_test_monster(&mut world, MonsterKindId::Scorpion, (5, 5), 10.0, 5.0);
    let player = spawn_test_player(&mut world, (6, 5));

    hit(&mut world, scorpion, player);
    world.run_system_once(poison_apply_system).expect("系统可运行");

    // 先推进一次，把次数消耗掉，再被咬一次：必须回满。
    advance_poison(&mut world, POISON_TICK_AV);
    assert_eq!(poison_of(&world, player)[0].1.ticks_left, POISON_TICKS - 1);

    hit(&mut world, scorpion, player);
    world.run_system_once(poison_apply_system).expect("系统可运行");

    let poisons = poison_of(&world, player);
    assert_eq!(poisons.len(), 1, "刷新不得新增第二条中毒");
    assert_eq!(
        poisons[0].1.ticks_left, POISON_TICKS,
        "刷新必须把发作次数补满"
    );
}

/// **时间轴是 AV**：不够 `POISON_TICK_AV` 的时间不发作；够了才发作一次。
///
/// 这条同时钉住"毒不随结算轮次漂移"：重复跑 `advance_poison(小步)` 很多次，
/// 伤害总量必须只由**累计 AV** 决定，而不是由跑了几次结算决定。
#[test]
fn poison_ticks_on_elapsed_av_not_on_rounds() {
    let mut world = test_world();
    let player = spawn_test_player(&mut world, (6, 5));
    let before = *world.get::<Health>(player).expect("玩家有血量");

    let effect = world.spawn((EffectOf(player), Poison::standard())).id();

    // 推进 1/3 个发作间隔：不该掉血。
    advance_poison(&mut world, POISON_TICK_AV / 3.0);
    assert_eq!(
        world.get::<Health>(player).map(|h| h.current),
        Some(before.current),
        "还没到发作点，不该掉血"
    );

    // 再推进 2/3：正好到点，掉一次。
    advance_poison(&mut world, POISON_TICK_AV * 2.0 / 3.0);
    assert_eq!(
        world.get::<Health>(player).map(|h| h.current),
        Some(before.current - POISON_DAMAGE_PER_TICK),
        "累计够一个间隔必须发作一次"
    );
    assert_eq!(
        world.get::<Poison>(effect).map(|p| p.ticks_left),
        Some(POISON_TICKS - 1)
    );

    // 一口气推进 10 个间隔：剩余两次发作全部结清，然后效果自己消失。
    advance_poison(&mut world, POISON_TICK_AV * 10.0);
    assert_eq!(
        world.get::<Health>(player).map(|h| h.current),
        Some(before.current - POISON_DAMAGE_PER_TICK * POISON_TICKS as f64),
        "跨过多个发作点时必须逐个结清（长行动不能漏毒）"
    );
    assert!(
        world.get_entity(effect).is_err(),
        "发作次数用完必须回收效果实体（否则每轮白扣血）"
    );
}

/// 没有时间流逝的一轮不发作、也不消耗次数（时间没走就没有中毒）。
#[test]
fn zero_elapsed_av_does_not_tick() {
    let mut world = test_world();
    let player = spawn_test_player(&mut world, (6, 5));
    let effect = world.spawn((EffectOf(player), Poison::standard())).id();

    advance_poison(&mut world, 0.0);
    assert_eq!(
        world.get::<Poison>(effect).map(|p| p.ticks_left),
        Some(POISON_TICKS),
        "时间没走时不得消耗发作次数"
    );
}

/// 受害者被 despawn 时，中毒由 `linked_spawn` 级联回收——不留孤儿效果实体。
#[test]
fn poison_dies_with_its_victim() {
    let mut world = test_world();
    let player = spawn_test_player(&mut world, (6, 5));
    let effect = world.spawn((EffectOf(player), Poison::standard())).id();

    world.despawn(player);
    assert!(
        world.get_entity(effect).is_err(),
        "状态效果必须随受害者一起消失（这正是 `EffectOf` 带 linked_spawn 的理由）"
    );
}
