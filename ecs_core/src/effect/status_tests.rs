//! `super`（`effect::status`）的测试：中毒的施加、刷新、运转与到期回收。
//!
//! 时间轴是 **AV**（世界时钟推进多少，毒就少多少），不是"结算了多少轮"——
//! 所以下面每个"推进时间"的地方都要显式给 `WorldClock.last_step_av`。
//! 这正是 ISSUES ECS46 修掉的那件事：按轮次计时会让"毒多久"随场上怪物数量漂移。
//!
//! DsnE12 的到期回收已拆到 `effect::duration`：本文件的 `advance_poison` 按
//! 结算链顺序跑"推进 → tick → 到期回收"三个系统。

use super::*;
use crate::effect::{advance_effect_durations_system, expire_effects_system};
use crate::resources::WorldClock;
use crate::test_util::{spawn_test_monster, spawn_test_player, test_world};
use bevy_ecs::system::RunSystemOnce;

/// 一条标准中毒效果（时长与 tick 共用同一个 `EffectDuration`）。
fn spawn_poison(world: &mut World, victim: Entity) -> Entity {
    world
        .spawn((
            EffectOf(victim),
            Poison::standard(),
            EffectDuration::new(POISON_TOTAL_AV),
        ))
        .id()
}

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
    world.write_message(AttackEvent {
        attacker,
        target,
        damage: 3.0,
        is_crit: false,
    });
}

/// 让世界时间前进 `av`，按结算链顺序跑一遍与效果生命周期有关的系统。
fn advance_poison(world: &mut World, av: f64) {
    world.resource_mut::<WorldClock>().last_step_av = av;
    world
        .run_system_once(advance_effect_durations_system)
        .expect("推进系统可运行");
    world
        .run_system_once(poison_tick_system)
        .expect("tick 系统可运行");
    world
        .run_system_once(expire_effects_system)
        .expect("回收系统可运行");
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
    assert_eq!(
        world
            .get::<EffectDuration>(poisons[0].0)
            .map(|duration| duration.remaining_av),
        Some(POISON_TOTAL_AV),
        "新中毒的剩余时长应当是整剂时长"
    );
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

/// 重复命中**刷新**而不是叠层：效果实体仍只有一条，次数与剩余时长都回满。
#[test]
fn reapplying_refreshes_instead_of_stacking() {
    let mut world = test_world();
    let scorpion = spawn_test_monster(&mut world, MonsterKindId::Scorpion, (5, 5), 10.0, 5.0);
    let player = spawn_test_player(&mut world, (6, 5));

    hit(&mut world, scorpion, player);
    world.run_system_once(poison_apply_system).expect("系统可运行");

    // 先推进一次，把次数与时长消耗掉，再被咬一次：必须回满。
    advance_poison(&mut world, POISON_TICK_AV);
    let poisons = poison_of(&world, player);
    assert_eq!(poisons[0].1.ticks_left, POISON_TICKS - 1);
    let effect = poisons[0].0;
    assert_eq!(
        world.get::<EffectDuration>(effect).map(|d| d.remaining_av),
        Some(POISON_TOTAL_AV - POISON_TICK_AV)
    );

    hit(&mut world, scorpion, player);
    world.run_system_once(poison_apply_system).expect("系统可运行");

    let poisons = poison_of(&world, player);
    assert_eq!(poisons.len(), 1, "刷新不得新增第二条中毒");
    assert_eq!(
        poisons[0].1.ticks_left, POISON_TICKS,
        "刷新必须把发作次数补满"
    );
    assert_eq!(
        world
            .get::<EffectDuration>(poisons[0].0)
            .map(|d| d.remaining_av),
        Some(POISON_TOTAL_AV),
        "刷新必须把剩余时长重置为整剂时长"
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

    let effect = spawn_poison(&mut world, player);

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

    // 一口气推进 10 个间隔：剩余两次发作全部结清，然后效果到期回收。
    advance_poison(&mut world, POISON_TICK_AV * 10.0);
    assert_eq!(
        world.get::<Health>(player).map(|h| h.current),
        Some(before.current - POISON_DAMAGE_PER_TICK * POISON_TICKS as f64),
        "跨过多个发作点时必须逐个结清（长行动不能漏毒）"
    );
    assert!(
        world.get_entity(effect).is_err(),
        "发作次数用完、时长归零后必须回收效果实体（由 expire_effects_system 统一回收）"
    );
}

/// 最后一次发作发生在时长归零的那一轮：payload 先结算，`expire` 再回收。
#[test]
fn the_final_tick_lands_before_the_effect_expires() {
    let mut world = test_world();
    let player = spawn_test_player(&mut world, (6, 5));
    let before = *world.get::<Health>(player).expect("玩家有血量");
    let effect = spawn_poison(&mut world, player);

    // 正好走完整剂时长（三个间隔）：三次发作都要落地，然后回收。
    advance_poison(&mut world, POISON_TOTAL_AV);
    assert_eq!(
        world.get::<Health>(player).map(|h| h.current),
        Some(before.current - POISON_DAMAGE_PER_TICK * POISON_TICKS as f64),
        "到期回收不得吞掉最后一次发作"
    );
    assert!(
        world.get_entity(effect).is_err(),
        "整剂时长走完后效果必须消失"
    );
}

/// 魔法抗性折进毒伤（m08882）：50% 抗性 → 每次只吃一半伤害。
#[test]
fn magic_resist_reduces_poison_damage() {
    let mut world = test_world();
    let player = spawn_test_player(&mut world, (6, 5));
    let before = *world.get::<Health>(player).expect("玩家有血量");
    world
        .get_mut::<Stats>(player)
        .expect("玩家有属性")
        .set(crate::rules::StatId::MAGIC_RESIST, 0.5);
    spawn_poison(&mut world, player);

    advance_poison(&mut world, POISON_TICK_AV);

    assert_eq!(
        world.get::<Health>(player).map(|h| h.current),
        Some(before.current - POISON_DAMAGE_PER_TICK * 0.5),
        "50% 魔法抗性应当把每次毒伤减半"
    );
}

/// 没有时间流逝的一轮不发作、也不消耗次数（时间没走就没有中毒）。
#[test]
fn zero_elapsed_av_does_not_tick() {
    let mut world = test_world();
    let player = spawn_test_player(&mut world, (6, 5));
    let effect = spawn_poison(&mut world, player);

    advance_poison(&mut world, 0.0);
    assert_eq!(
        world.get::<Poison>(effect).map(|p| p.ticks_left),
        Some(POISON_TICKS),
        "时间没走时不得消耗发作次数"
    );
    assert!(
        world.get_entity(effect).is_ok(),
        "时间没走时效果不得消失"
    );
}

/// 受害者被 despawn 时，中毒由 `linked_spawn` 级联回收——不留孤儿效果实体。
#[test]
fn poison_dies_with_its_victim() {
    let mut world = test_world();
    let player = spawn_test_player(&mut world, (6, 5));
    let effect = spawn_poison(&mut world, player);

    world.despawn(player);
    assert!(
        world.get_entity(effect).is_err(),
        "状态效果必须随受害者一起消失（这正是 `EffectOf` 带 linked_spawn 的理由）"
    );
}
