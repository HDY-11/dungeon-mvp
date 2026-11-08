//! 法术注册表 / 公式 / 结算的测试（DsnE26）。

use super::*;
use crate::components::{Health, Mana, Position, Viewshed};
use crate::events::SpellIntentEvent;
use crate::monster::MonsterKindId;
use crate::resources::EventLog;
use crate::test_util::{spawn_test_monster, spawn_test_player, test_world};
use bevy_ecs::system::RunSystemOnce;

#[test]
fn the_spell_table_is_pinned() {
    let table = spells();
    assert_eq!(table.len(), 2);

    let spark = SpellId(0).def();
    assert_eq!(spark.name, "火花术");
    assert_eq!(spark.mp_cost, 3);
    assert_eq!(spark.cast_av, 600.0);
    assert_eq!(spark.range, 6);
    assert_eq!(spark.targeting, SpellTargeting::NearestEnemy);
    assert_eq!(spark.effect, SpellEffect::Damage { amount: 5.0 });

    let heal = SpellId(1).def();
    assert_eq!(heal.name, "治愈术");
    assert_eq!(heal.mp_cost, 6);
    assert_eq!(heal.range, 0);
    assert_eq!(heal.targeting, SpellTargeting::SelfOnly);
    assert_eq!(heal.effect, SpellEffect::Heal { amount: 12.0 });

    assert!(SpellId(0).exists());
    assert!(!SpellId(2).exists());
}

#[test]
fn the_starting_book_offers_two_slots() {
    let book = SpellBook::starting();
    assert_eq!(book.get(0), Some(SpellId(0)));
    assert_eq!(book.get(1), Some(SpellId(1)));
    assert_eq!(book.get(2), None);
    assert_eq!(book.known().len(), 2);
}

#[test]
fn spell_damage_adds_mastery_and_respects_resistance() {
    // 无抗性：5 威力 + 8 精通 = 13。
    assert_eq!(spell_damage(5.0, 8.0, 0.0), 13.0);
    // 50% 抗性：13 × 0.5 = 6.5。
    assert_eq!(spell_damage(5.0, 8.0, 0.5), 6.5);
    // 抗性上限 0.75：13 × 0.25 = 3.25；超过上限也按上限算。
    assert_eq!(spell_damage(5.0, 8.0, 0.75), 3.25);
    assert_eq!(spell_damage(5.0, 8.0, 9.0), 3.25);
    // 下限 1：抗性再高也不会被完全免疫。
    assert_eq!(spell_damage(0.0, 0.0, 0.75), 1.0);
}

#[test]
fn spell_heal_adds_mastery() {
    assert_eq!(spell_heal(12.0, 8.0), 20.0);
    assert_eq!(spell_heal(0.0, 8.0), 8.0);
}

#[test]
fn nearest_enemy_needs_visibility_and_range() {
    let mut world = test_world();
    let caster = Position::new(10, 10);
    let mut viewshed = Viewshed::new(3);
    viewshed.visible_tiles = vec![(11, 10), (12, 10), (12, 11)];
    // 实体编号按 spawn 顺序递增，用真实实体让排序测试可复现。
    let hidden = world.spawn_empty().id();
    let near = world.spawn_empty().id();
    let farther = world.spawn_empty().id();
    let candidates = vec![
        (hidden, Position::new(10, 11)),
        (farther, Position::new(12, 11)),
        (near, Position::new(11, 10)),
    ];
    let found = nearest_enemy_in_range(caster, &viewshed, 3, candidates.clone().into_iter());
    assert_eq!(found, Some(near), "最近的可见敌人应被选中");

    // 射程缩到 1：更远的那只被排除；仍然是最近的可见者。
    let found = nearest_enemy_in_range(caster, &viewshed, 1, candidates.clone().into_iter());
    assert_eq!(found, Some(near));
    // 射程 0（自施法）不选任何敌人。
    assert_eq!(
        nearest_enemy_in_range(caster, &viewshed, 0, candidates.into_iter()),
        None
    );
}

#[test]
fn nearest_enemy_breaks_ties_by_entity_id() {
    let mut world = test_world();
    let caster = Position::new(10, 10);
    let mut viewshed = Viewshed::new(2);
    viewshed.visible_tiles = vec![(11, 10), (9, 10)];
    let left = world.spawn_empty().id();
    let right = world.spawn_empty().id();
    let candidates = vec![
        (right, Position::new(11, 10)),
        (left, Position::new(9, 10)),
    ];
    let found = nearest_enemy_in_range(caster, &viewshed, 2, candidates.into_iter());
    assert_eq!(found, Some(left), "同距时应按实体编号升序（确定性）");
}

#[test]
fn casting_spends_mana_and_hurts_the_target() {
    let mut world = test_world();
    let player = spawn_test_player(&mut world, (10, 10));
    let monster = spawn_test_monster(&mut world, MonsterKindId::Rat, (11, 10), 30.0, 4.0);
    world
        .resource_mut::<bevy_ecs::event::Events<SpellIntentEvent>>()
        .send(SpellIntentEvent {
            caster: player,
            spell: SpellId(0),
            target: monster,
        });
    world.run_system_once(resolve_spell_system).unwrap();

    // 火花术 5 + 法术精通 8 = 13 点魔法伤害；耗蓝 3。
    let health = *world.get::<Health>(monster).expect("怪物还活着");
    assert_eq!(health.current, 17.0);
    let mana = *world.get::<Mana>(player).expect("玩家有法力");
    assert_eq!(mana.current, 17.0);

    let log = &world.resource::<EventLog>().messages;
    let last = log.last().expect("施法应写日志");
    assert!(last.text.contains("火花术"), "{}", last.text);
    assert!(last.text.contains("魔法伤害"), "{}", last.text);
}

#[test]
fn healing_restores_the_caster_and_spends_mana() {
    let mut world = test_world();
    let player = spawn_test_player(&mut world, (10, 10));
    world.get_mut::<Health>(player).unwrap().current = 40.0;
    world
        .resource_mut::<bevy_ecs::event::Events<SpellIntentEvent>>()
        .send(SpellIntentEvent {
            caster: player,
            spell: SpellId(1),
            target: player,
        });
    world.run_system_once(resolve_spell_system).unwrap();

    // 治愈术 12 + 精通 8 = 20 点回复；耗蓝 6。
    let health = *world.get::<Health>(player).unwrap();
    assert_eq!(health.current, 60.0);
    let mana = *world.get::<Mana>(player).unwrap();
    assert_eq!(mana.current, 14.0);
    let log = &world.resource::<EventLog>().messages;
    let last = log.last().expect("治疗应写日志");
    assert!(last.text.contains("恢复了"), "{}", last.text);
}

#[test]
fn a_heal_without_mana_fizzles_for_free() {
    let mut world = test_world();
    let player = spawn_test_player(&mut world, (10, 10));
    world.get_mut::<Health>(player).unwrap().current = 40.0;
    // 治愈术要 6 点蓝，只留 2 点。
    world.get_mut::<Mana>(player).unwrap().current = 2.0;
    world
        .resource_mut::<bevy_ecs::event::Events<SpellIntentEvent>>()
        .send(SpellIntentEvent {
            caster: player,
            spell: SpellId(1),
            target: player,
        });
    world.run_system_once(resolve_spell_system).unwrap();

    let health = *world.get::<Health>(player).unwrap();
    assert_eq!(health.current, 40.0, "法力不足时不能治疗");
    let mana = *world.get::<Mana>(player).unwrap();
    assert_eq!(mana.current, 2.0, "施放失败不应扣蓝");
    let log = &world.resource::<EventLog>().messages;
    let last = log.last().expect("应记录失败原因");
    assert!(last.text.contains("没有施放出来"), "{}", last.text);
}

#[test]
fn a_dead_target_wastes_the_mana() {
    let mut world = test_world();
    let player = spawn_test_player(&mut world, (10, 10));
    let monster = spawn_test_monster(&mut world, MonsterKindId::Rat, (11, 10), 30.0, 4.0);
    world.despawn(monster);
    world
        .resource_mut::<bevy_ecs::event::Events<SpellIntentEvent>>()
        .send(SpellIntentEvent {
            caster: player,
            spell: SpellId(0),
            target: monster,
        });
    world.run_system_once(resolve_spell_system).unwrap();

    // 锁定目标的成本在扣蓝那一刻就付了：目标已经倒下，法术落空但法力照扣。
    let mana = *world.get::<Mana>(player).unwrap();
    assert_eq!(mana.current, 17.0);
    let log = &world.resource::<EventLog>().messages;
    let last = log.last().expect("应记录落空");
    assert!(last.text.contains("落空"), "{}", last.text);
}
