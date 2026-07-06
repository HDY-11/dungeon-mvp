//! 行动系统单元测试 — 验证逻辑正确性
use crate::*;
use bevy_ecs::prelude::*;
use bevy_ecs::system::RunSystemOnce;

fn fresh_world() -> World {
    setup_world()
}

#[test]
fn test_all_resources_exist() {
    let world = fresh_world();
    assert!(world.get_resource::<action_types::ActionQueue>().is_some());
    assert!(world.get_resource::<action_types::InputBuffer>().is_some());
    assert!(world.get_resource::<action_types::PlayerPreview>().is_some());
    assert!(world.get_resource::<TurnManager>().is_some());
    assert!(world.get_resource::<MapMemory>().is_some());
    assert!(world.get_resource::<OccupancyMap>().is_some());
    assert!(world.get_resource::<EventLog>().is_some());
    assert!(world.get_resource::<FloorNumber>().is_some());
}

#[test]
fn test_global_world_startup() {
    let mut world = fresh_world();
    assert!(!world.resource::<TurnManager>().game_over);
    assert!(world.resource::<action_types::ActionQueue>().entries.is_empty());
    assert!(world.resource::<action_types::PlayerPreview>().kind.is_none());
    assert!(world.resource::<EventLog>().messages.is_empty());

    let result = world.run_system_once(crate::systems::fov_system);
    assert!(result.is_ok());
}

#[test]
fn test_player_preview_tap_tap() {
    let mut world = fresh_world();
    let player = world.query::<(Entity, &Player)>().iter(&world).next().map(|(e, _)| e).unwrap();

    // 第一次 tap
    world.resource_mut::<action_types::PlayerPreview>().kind = Some(action_types::ActionKindV3::Move { dx: 1, dy: 0 });
    assert!(matches!(world.resource::<action_types::PlayerPreview>().kind, Some(action_types::ActionKindV3::Move { dx: 1, dy: 0 })));

    // 第二次 tap
    let reaction = world.get::<action_types::Reaction>(player).unwrap().time;
    let duration = world.get::<action_types::CanMove>(player).unwrap().duration;
    let av = reaction + duration;
    world.resource_mut::<action_types::ActionQueue>().enqueue(
        player, action_types::ActionKindV3::Move { dx: 1, dy: 0 }, av,
    );
    world.resource_mut::<action_types::PlayerPreview>().kind = None;
    assert_eq!(world.resource::<action_types::ActionQueue>().entries.len(), 1);
}

#[test]
fn test_action_queue_advance() {
    let mut world = fresh_world();
    let player = world.query::<(Entity, &Player)>().iter(&world).next().map(|(e, _)| e).unwrap();
    let reaction_time = world.get::<action_types::Reaction>(player).unwrap().time;
    let duration = world.get::<action_types::CanMove>(player).unwrap().duration;
    let av = reaction_time + duration;

    world.resource_mut::<action_types::ActionQueue>().enqueue(
        player, action_types::ActionKindV3::Move { dx: 1, dy: 0 }, av,
    );
    world.resource_mut::<action_types::ActionQueue>().advance(av / 2.0);
    assert_eq!(world.resource::<action_types::ActionQueue>().entries.len(), 1);

    world.resource_mut::<action_types::ActionQueue>().advance(av);
    let ready = world.resource_mut::<action_types::ActionQueue>().pop_ready();
    assert_eq!(ready.len(), 1);
}

#[test]
fn test_conditions() {
    assert!(action_types::CanMove::condition(true, false));
    assert!(action_types::CanMove::condition(true, true));
    assert!(!action_types::CanMove::condition(false, false));
    assert!(action_types::CanFlee::condition(0.2));
    assert!(!action_types::CanFlee::condition(0.5));
    assert!(action_types::CanChase::condition(true));
    assert!(!action_types::CanChase::condition(false));
    assert!(action_types::CanWander::condition());
}

#[test]
fn test_reaction_from_agility() {
    let r10 = action_types::agility_to_reaction(10);
    let r20 = action_types::agility_to_reaction(20);
    assert!(r20 < r10);
    assert!(r10 >= 20.0);
    assert!(r10 <= 100.0);
}
