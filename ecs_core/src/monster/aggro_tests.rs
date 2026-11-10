//! 仇恨 / 威胁表（DsnE29）的规则与系统测试。
//!
//! 目标口径只有三条，测试按这三条组织：
//! 1. 打一下 = 记一笔威胁 + 记住来源位置；
//! 2. 目标选威胁最高者，同值按实体编号（确定性）；
//! 3. 威胁随时间衰减、来源死亡立刻清掉。
//!
//! 系统的协作口径（近战/法术/投掷都发 `DamageTakenEvent`）在各自的测试里钉。

use super::*;
use crate::components::{Health, Position};
use crate::entity_cls::{Monster, Player};
use crate::events::DamageTakenEvent;
use crate::resources::WorldClock;
use bevy_ecs::system::RunSystemOnce;

fn fresh_world() -> World {
    let mut world = World::new();
    world.insert_resource(Events::<DamageTakenEvent>::default());
    world.insert_resource(WorldClock::default());
    world
}

fn spawn_monster(world: &mut World, at: (usize, usize), hp: f64) -> Entity {
    world
        .spawn((
            Monster,
            Position::new(at.0, at.1),
            Health::full(hp, hp),
            Aggro::new(),
        ))
        .id()
}

fn hit(world: &mut World, source: Entity, target: Entity, amount: f64) {
    world.send_event(DamageTakenEvent {
        source: Some(source),
        target,
        amount,
    });
}

fn recorded(world: &mut World) {
    let _ = world.run_system_once(record_threat_system);
}

fn decayed(world: &mut World) {
    let _ = world.run_system_once(threat_decay_system);
}

#[test]
fn a_hit_records_threat_and_remembers_where_it_came_from() {
    let mut world = fresh_world();
    let victim = spawn_monster(&mut world, (10, 10), 30.0);
    let attacker = spawn_monster(&mut world, (14, 12), 30.0);

    hit(&mut world, attacker, victim, 6.0);
    recorded(&mut world);

    let aggro = world.get::<Aggro>(victim).unwrap();
    assert_eq!(
        aggro.threat_against(attacker),
        6.0,
        "伤害 6 应记 6 点威胁（伤害折算优先于每击固定值）"
    );
    assert_eq!(
        aggro.last_known(),
        Some((14, 12)),
        "受击时必须记住来源位置，用来代替旧的 LastKnownPlayerPos"
    );
    assert_eq!(aggro.target(), Some(attacker));
}

#[test]
fn a_zero_damage_hit_still_records_the_minimum_threat() {
    let mut world = fresh_world();
    let victim = spawn_monster(&mut world, (10, 10), 30.0);
    let attacker = spawn_monster(&mut world, (11, 10), 30.0);

    hit(&mut world, attacker, victim, 0.0);
    recorded(&mut world);

    assert_eq!(
        world.get::<Aggro>(victim).unwrap().threat_against(attacker),
        1.0,
        "0 伤害的命中也要记一笔（每击固定威胁）"
    );
}

#[test]
fn the_most_threatening_source_wins_and_ties_break_by_entity_id() {
    let mut world = fresh_world();
    let victim = spawn_monster(&mut world, (10, 10), 30.0);
    let weak = spawn_monster(&mut world, (11, 10), 30.0);
    let strong = spawn_monster(&mut world, (9, 10), 30.0);

    hit(&mut world, weak, victim, 2.0);
    hit(&mut world, strong, victim, 7.0);
    recorded(&mut world);
    assert_eq!(
        world.get::<Aggro>(victim).unwrap().target(),
        Some(strong),
        "威胁高者优先"
    );

    // 同值：先 spawn 的实体编号更小，应胜出。
    let mut world2 = fresh_world();
    let victim2 = spawn_monster(&mut world2, (10, 10), 30.0);
    let first = spawn_monster(&mut world2, (11, 10), 30.0);
    let second = spawn_monster(&mut world2, (9, 10), 30.0);
    hit(&mut world2, first, victim2, 3.0);
    hit(&mut world2, second, victim2, 3.0);
    recorded(&mut world2);
    assert_eq!(
        world2.get::<Aggro>(victim2).unwrap().target(),
        Some(first),
        "威胁同值时按实体编号升序（确定性）"
    );
}

#[test]
fn threat_decays_with_time_and_drops_zeroed_sources() {
    let mut world = fresh_world();
    let victim = spawn_monster(&mut world, (10, 10), 30.0);
    let attacker = spawn_monster(&mut world, (11, 10), 30.0);
    hit(&mut world, attacker, victim, 6.0);
    recorded(&mut world);

    world.resource_mut::<WorldClock>().last_step_av = 500.0;
    decayed(&mut world);
    assert_eq!(
        world.get::<Aggro>(victim).unwrap().threat_against(attacker),
        1.0,
        "500 AV 应衰减 5 点威胁"
    );

    world.resource_mut::<WorldClock>().last_step_av = 1000.0;
    decayed(&mut world);
    assert!(
        world.get::<Aggro>(victim).unwrap().is_empty(),
        "威胁归零后来源应从表里移除"
    );
}

#[test]
fn a_dead_source_is_dropped_even_without_elapsed_time() {
    let mut world = fresh_world();
    let victim = spawn_monster(&mut world, (10, 10), 30.0);
    let attacker = spawn_monster(&mut world, (11, 10), 30.0);
    hit(&mut world, attacker, victim, 4.0);
    recorded(&mut world);

    world.get_mut::<Health>(attacker).unwrap().current = 0.0;
    world.resource_mut::<WorldClock>().last_step_av = 0.0;
    decayed(&mut world);

    assert!(
        world.get::<Aggro>(victim).unwrap().is_empty(),
        "死亡来源必须清掉；清理不受本轮 AV 影响"
    );
}

#[test]
fn hits_on_the_player_do_not_create_threat() {
    let mut world = fresh_world();
    let player = world
        .spawn((
            Player,
            Position::new(10, 10),
            Health::full(80.0, 80.0),
        ))
        .id();
    let attacker = spawn_monster(&mut world, (11, 10), 30.0);

    hit(&mut world, attacker, player, 5.0);
    recorded(&mut world);

    assert_eq!(
        world.get::<Aggro>(player),
        None,
        "玩家没有 Aggro 组件，受击不产生仇恨表"
    );
}

#[test]
fn remembering_and_forgetting_are_independent_of_threat_entries() {
    let mut aggro = Aggro::with_memory((3, 4));
    assert_eq!(aggro.last_known(), Some((3, 4)));
    assert!(aggro.is_empty(), "记忆本身不是一条威胁");

    aggro.remember((5, 6));
    assert_eq!(aggro.last_known(), Some((5, 6)));
    aggro.forget();
    assert_eq!(aggro.last_known(), None);

    let mut world = fresh_world();
    let source = spawn_monster(&mut world, (9, 9), 10.0);
    aggro.record(source, 2.0);
    assert_eq!(aggro.target(), Some(source));
    aggro.drop_source(source);
    assert!(aggro.is_empty());
}
