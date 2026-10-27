//! `super`（`effect::status`）的测试：中毒的施加、刷新、运转与自我回收。

use super::*;
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
    assert_eq!(poisons[0].1.rounds_left, POISON_ROUNDS);
    assert_eq!(poisons[0].1.damage_per_round, POISON_DAMAGE_PER_ROUND);
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

/// 重复命中**刷新**而不是叠层：效果实体仍只有一条。
#[test]
fn reapplying_refreshes_instead_of_stacking() {
    let mut world = test_world();
    let scorpion = spawn_test_monster(&mut world, MonsterKindId::Scorpion, (5, 5), 10.0, 5.0);
    let player = spawn_test_player(&mut world, (6, 5));

    hit(&mut world, scorpion, player);
    world.run_system_once(poison_apply_system).expect("系统可运行");

    // 先把回合数打到只剩 1，再被咬一次：必须回到满值。
    {
        let (effect, _) = poison_of(&world, player)[0];
        world.get_mut::<Poison>(effect).unwrap().rounds_left = 1;
    }
    hit(&mut world, scorpion, player);
    world.run_system_once(poison_apply_system).expect("系统可运行");

    let poisons = poison_of(&world, player);
    assert_eq!(poisons.len(), 1, "刷新不得新增第二条中毒");
    assert_eq!(
        poisons[0].1.rounds_left, POISON_ROUNDS,
        "刷新必须把回合数补满"
    );
}

/// 每轮扣血；回合数归零时**自己消失**（owner 系统是唯一回收者）。
#[test]
fn poison_ticks_damage_and_recycles_itself() {
    let mut world = test_world();
    let player = spawn_test_player(&mut world, (6, 5));
    let before = *world.get::<Health>(player).expect("玩家有血量");

    let effect = world
        .spawn((
            EffectOf(player),
            Poison {
                damage_per_round: 5.0,
                rounds_left: 2,
            },
        ))
        .id();

    world.run_system_once(poison_tick_system).expect("系统可运行");
    let after_first = *world.get::<Health>(player).expect("玩家有血量");
    assert_eq!(after_first.current, before.current - 5.0, "每轮必须扣一次血");
    assert!(world.get_entity(effect).is_ok(), "还有回合时不得回收");
    assert_eq!(
        world.get::<Poison>(effect).map(|poison| poison.rounds_left),
        Some(1)
    );

    world.run_system_once(poison_tick_system).expect("系统可运行");
    assert!(
        world.get_entity(effect).is_err(),
        "回合数归零后效果实体必须被回收（否则每轮白扣血）"
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
