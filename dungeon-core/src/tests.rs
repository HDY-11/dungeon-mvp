//! 行动系统单元测试 — 验证逻辑正确性
use crate::*;
use crate::action::*;
use bevy_ecs::prelude::*;
use bevy_ecs::system::RunSystemOnce;

fn fresh_world() -> World {
    setup_world()
}

// ── 直接使用 World（不通过全局宏） ──

#[test]
fn test_all_resources_exist() {
    let world = fresh_world();
    assert!(world.get_resource::<ActionQueue>().is_some());
    assert!(world.get_resource::<InputBuffer>().is_some());
    assert!(world.get_resource::<PlayerPreview>().is_some());
    assert!(world.get_resource::<TurnManager>().is_some());
    assert!(world.get_resource::<MapMemory>().is_some());
    assert!(world.get_resource::<OccupancyMap>().is_some());
    assert!(world.get_resource::<EventLog>().is_some());
    assert!(world.get_resource::<FloorNumber>().is_some());
}

// ── 全局 World 两个步骤，分开作用域避免临时值 ──

#[test]
fn test_global_world_startup() {
    let world = setup_world();
    crate::global::set_world(world);

    {
        let w = world!();
        assert!(!w.resource::<TurnManager>().game_over);
        assert!(w.resource::<ActionQueue>().entries.is_empty());
        assert!(w.resource::<PlayerPreview>().kind.is_none());
        assert!(w.resource::<EventLog>().messages.is_empty());
    }
    // fov_system 测试
    let result = world!(mut).run_system_once(fov_system);
    assert!(result.is_ok());
}

// ── 双 tap 验证──

#[test]
fn test_player_preview_tap_tap() {
    crate::global::set_world(fresh_world());
    // 获取 player entity
    let player;
    {
        let mut w = world!(mut);
        player = w.query::<(Entity, &Player)>().iter(&mut w).next().map(|(e, _)| e).unwrap();
    }
    // 第一次 tap
    {
        let mut w = world!(mut);
        w.resource_mut::<PlayerPreview>().kind = Some(ActionKindV3::Move { dx: 1, dy: 0 });
    }
    // 验证预览
    {
        let w = world!();
        assert!(matches!(w.resource::<PlayerPreview>().kind, Some(ActionKindV3::Move { dx: 1, dy: 0 })));
    }
    // 第二次 tap
    let av;
    {
        let w = world!();
        let reaction = w.get::<Reaction>(player).unwrap().time;
        let duration = w.get::<CanMove>(player).unwrap().duration;
        av = reaction + duration;
    }
    {
        let mut w = world!(mut);
        w.resource_mut::<ActionQueue>().enqueue(
            player, ActionKindV3::Move { dx: 1, dy: 0 }, av,
        );
        w.resource_mut::<PlayerPreview>().kind = None;
    }
    // 验证
    {
        let w = world!();
        assert_eq!(w.resource::<ActionQueue>().entries.len(), 1);
    }
}

// ── ActionQueue 推进 ──

#[test]
fn test_action_queue_advance() {
    crate::global::set_world(fresh_world());
    // 获取 entity 和数据
    let player;
    let av;
    {
        let mut w = world!(mut);
        player = w.query::<(Entity, &Player)>().iter(&mut w).next().map(|(e, _)| e).unwrap();
        let reaction_time = w.get::<Reaction>(player).unwrap().time;
        let duration = w.get::<CanMove>(player).unwrap().duration;
        av = reaction_time + duration;
    }
    // 入队
    {
        let mut w = world!(mut);
        w.resource_mut::<ActionQueue>().enqueue(
            player, ActionKindV3::Move { dx: 1, dy: 0 }, av,
        );
    }
    // 推进一半
    {
        let mut w = world!(mut);
        w.resource_mut::<ActionQueue>().advance(av / 2.0);
    }
    {
        let w = world!();
        assert_eq!(w.resource::<ActionQueue>().entries.len(), 1);
    }
    // 推进全部 → pop_ready
    {
        let mut w = world!(mut);
        w.resource_mut::<ActionQueue>().advance(av);
        let ready = w.resource_mut::<ActionQueue>().pop_ready();
        assert_eq!(ready.len(), 1);
    }
}

// ── 怪物决策 ──

#[test]
fn test_monster_decision_produces_actions() {
    crate::global::set_world(fresh_world());
    {
        let w = world!();
        assert!(w.resource::<ActionQueue>().entries.is_empty());
    }
    run_monster_decision();
    {
        let w = world!();
        assert!(w.resource::<ActionQueue>().entries.len() > 0);
    }
}

// ── Action 条件 ──

#[test]
fn test_conditions() {
    assert!(CanMove::condition(true, false));
    assert!(CanMove::condition(true, true));
    assert!(!CanMove::condition(false, false));
    assert!(CanFlee::condition(0.2));
    assert!(!CanFlee::condition(0.5));
    assert!(CanChase::condition(true));
    assert!(!CanChase::condition(false));
    assert!(CanWander::condition());
}

#[test]
fn test_reaction_from_agility() {
    let r10 = agility_to_reaction(10);
    let r20 = agility_to_reaction(20);
    assert!(r20 < r10);
    assert!(r10 >= 20.0);
    assert!(r10 <= 100.0);
}

// ── set_world 二次调用 ──

#[test]
fn test_set_world_twice() {
    crate::global::set_world(setup_world());
    let old = crate::global::set_world(setup_world());
    assert!(old.is_some());
}
