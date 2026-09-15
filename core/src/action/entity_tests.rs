//! `super`（`action::entity`）的 PoC 测试。
//!
//! 独立文件是为了让生产代码与测试分开、便于 Phase C 整块替换。
//! 通过 `entity.rs` 末尾的 `#[path = "entity_tests.rs"] mod tests;` 引入。

use super::*;
use crate::action::generation::player::PlayerCommand;
use crate::components::{CanBasicAttack, CanMove, CanWait};
use crate::map::{MAP_HEIGHT, MAP_WIDTH, Tile};
use crate::test_util::{find_walkable_step, player_pos, single_tile_scene, spawn_test_monster};
use crate::world_loop::apply_player_command;
use bevy_ecs::system::RunSystemOnce;

/// 一个 actor 同时只能有一个 `ActiveAction`；违反即为实现 bug。
const MAX_ACTIONS_PER_ACTOR: usize = 1;

/// 测试世界 + 注册 PoC 调度（`insert_core_resources` 只注册 init/settle 两条）。
fn poc_world() -> (World, Entity) {
    let (mut world, player) = single_tile_scene();
    world.add_schedule(build_action_poc_schedule());
    (world, player)
}

/// spawn 一个已授予全部行动能力的怪物。
fn poc_actor(world: &mut World, pos: (usize, usize)) -> Entity {
    let actor = spawn_test_monster(
        world,
        crate::monster::MonsterKindId::Rat,
        pos,
        10.0,
        4.0,
        5.0,
    );
    world
        .entity_mut(actor)
        .insert((CanWander, CanFlee, CanMove, CanWait, CanBasicAttack));
    actor
}

/// 只跑生成系统（不经仲裁），用于验证“生成不写 actor 状态”。
fn run_generation_systems(world: &mut World) {
    let _ = world.run_system_once(wander_generation_system);
    let _ = world.run_system_once(flee_generation_system);
}

/// 只跑生成 + 仲裁，返回该 actor 当前的 `ActiveAction`。
fn run_generation_and_arbitration(world: &mut World, actor: Entity) -> Vec<Entity> {
    run_generation_systems(world);
    let _ = world.run_system_once(action_arbitration_system);
    let mut query = world.query_filtered::<Entity, (With<ActiveAction>, With<ChildOf>)>();
    query
        .iter(world)
        .filter(|action| {
            world
                .get::<ChildOf>(*action)
                .is_some_and(|child_of| child_of.parent() == actor)
        })
        .collect()
}

fn candidates(world: &mut World) -> Vec<Entity> {
    let mut query = world.query_filtered::<Entity, With<Candidate>>();
    query.iter(world).collect()
}

/// B7：actor + Wander 全链路。
///
/// 覆盖 §11.4 的 `action_entity_poc_round_trip`，逐相位观察：
/// 生成候选 → 仲裁选中（此刻恰有一个 `ActiveAction`）→ tick 到 `Ready` →
/// 走一步（位置只能变到合法相邻格）→ completion 回收并回 `Idle`。
///
/// 注意：`ActiveAction` 在 actor 行动的那一轮里就会被执行并回收，
/// 所以“看到 ActiveAction”只能在**仲裁之后、执行之前**观察。
#[test]
fn action_entity_poc_round_trip() {
    let (mut world, _player) = poc_world();
    // 铺成平地：让游荡有空间，也让 8 个方向都可走。
    world.resource_mut::<Map>().tiles = [[Tile::Floor; MAP_WIDTH]; MAP_HEIGHT];
    let (ax, ay) = player_pos(&world);
    let actor = poc_actor(&mut world, (ax + 2, ay));
    let mut position = (ax + 2, ay);

    let mut moves = 0usize;
    let mut saw_active_action = false;
    let mut saw_ready_action = false;

    for _ in 0..12 {
        // 相位 1：生成 + 仲裁（赢家此刻已是 ActiveAction，但还没执行）。
        let active_actions = run_generation_and_arbitration(&mut world, actor);
        assert_eq!(
            active_actions.len(),
            MAX_ACTIONS_PER_ACTOR,
            "仲裁必须给 actor 恰好一个 ActiveAction，实际 {active_actions:?}"
        );
        let active = active_actions[0];
        saw_active_action = true;
        assert!(
            world.get::<Active>(actor).is_some(),
            "仲裁后 actor 必须进入 Active"
        );
        assert!(
            world.get::<Candidate>(active).is_none(),
            "赢家必须去掉 Candidate"
        );

        // 相位 2：tick → 执行 → completion。
        let _ = world.run_system_once(tick_action_timers_system);
        saw_ready_action |= world.get::<Ready>(active).is_some();
        world.run_schedule(ActionPocSchedule);

        let now = world.get::<Position>(actor).unwrap().to_tuple();
        if now != position {
            assert!(
                position.0.abs_diff(now.0) <= 1 && position.1.abs_diff(now.1) <= 1,
                "游荡只能走一步：{position:?} → {now:?}"
            );
            assert!(
                world.resource::<Map>().tiles[now.1][now.0].walkable(),
                "落点必须可行走: {now:?}"
            );
            position = now;
            moves += 1;
        }
        assert!(
            world.get_entity(active).is_err(),
            "执行后的 action 实体必须被 completion 回收"
        );
        assert!(
            world.get::<Idle>(actor).is_some(),
            "actor 每轮结束都必须回到 Idle"
        );
    }

    assert!(saw_active_action, "链路中必须出现过 ActiveAction");
    assert!(saw_ready_action, "AV 归零后 action 必须被标记 Ready");
    assert!(moves > 0, "12 轮内游荡至少应当成功移动一次");
    assert!(world.get::<Active>(actor).is_none(), "不得残留 Active");
}

/// §11.4 的 `arbitration_priority_and_cleanup`：高优先级胜出、loser 被清理。
///
/// 直接调用仲裁系统（不经执行/完成），这样能在赢家被消费前看到它的状态。
#[test]
fn arbitration_picks_higher_priority_and_cleans_up_candidates() {
    let (mut world, _player) = poc_world();
    let actor = poc_actor(&mut world, (5, 5));

    let high = world
        .spawn((
            ChildOf(actor),
            ActionPriority(PRIORITY_FLEE),
            ActionName("Flee"),
            ActionTimer { remaining_av: 50.0 },
            Flee,
            Candidate,
        ))
        .id();
    let low = world
        .spawn((
            ChildOf(actor),
            ActionPriority(PRIORITY_WANDER),
            ActionName("Wander"),
            ActionTimer { remaining_av: 50.0 },
            Wander,
            Candidate,
        ))
        .id();

    let _ = world.run_system_once(action_arbitration_system);

    assert!(
        world.get::<ActiveAction>(high).is_some(),
        "Flee(200) 必须胜过 Wander(50)"
    );
    assert!(
        world.get::<Candidate>(high).is_none(),
        "赢家必须去掉 Candidate"
    );
    assert!(world.get_entity(low).is_err(), "落后候选必须被 despawn");
    assert!(
        world.get::<Active>(actor).is_some(),
        "actor 必须进入 Active"
    );
    assert!(world.get::<Idle>(actor).is_none());
}

/// 同优先级时按 `action_entity.to_bits()` 升序打破平局（全序，无 RNG）。
#[test]
fn arbitration_tie_break_is_deterministic() {
    let (mut world, _player) = poc_world();
    let actor = poc_actor(&mut world, (5, 5));

    let first = world
        .spawn((
            ChildOf(actor),
            ActionPriority(PRIORITY_WANDER),
            ActionTimer {
                remaining_av: 500.0,
            },
            Wander,
            Candidate,
        ))
        .id();
    let second = world
        .spawn((
            ChildOf(actor),
            ActionPriority(PRIORITY_WANDER),
            ActionTimer {
                remaining_av: 500.0,
            },
            Wander,
            Candidate,
        ))
        .id();
    assert!(
        first.to_bits() < second.to_bits(),
        "测试前提：先 spawn 的 bits 更小"
    );

    let _ = world.run_system_once(action_arbitration_system);

    assert!(
        world.get::<ActiveAction>(first).is_some(),
        "同优先级时 bits 小者胜出"
    );
    assert!(world.get_entity(second).is_err(), "另一个候选必须被清理");
}

/// B6 前置：actor 已有 `ActiveAction` 时，仲裁不得再授予新行动。
///
/// 一轮完整调度后原行动应当被正常执行并回收，而不是被新候选夺走。
#[test]
fn arbitration_skips_actor_that_already_has_an_active_action() {
    let (mut world, _player) = poc_world();
    let actor = poc_actor(&mut world, (5, 5));

    let existing = world
        .spawn((
            ChildOf(actor),
            ActionPriority(PRIORITY_WANDER),
            ActionSource::Ai,
            ActionTimer { remaining_av: 5.0 },
            Wander,
            ActiveAction,
        ))
        .id();
    world.entity_mut(actor).remove::<Idle>().insert(Active);

    world.run_schedule(ActionPocSchedule);

    assert!(
        world.get_entity(existing).is_err(),
        "原行动应当被正常执行并由 completion 回收"
    );
    assert!(
        world.get::<Idle>(actor).is_some(),
        "原行动完成后 actor 必须回到 Idle"
    );
    assert!(world.get::<Active>(actor).is_none());
    assert!(
        candidates(&mut world).is_empty(),
        "每轮结束不得残留 Candidate"
    );
}

/// 生成系统只 spawn 候选：不得改动 actor 的 `Idle`/`Active`/`Failure`。
#[test]
fn generation_only_spawns_candidates() {
    let (mut world, _player) = poc_world();
    let actor = poc_actor(&mut world, (5, 5));

    run_generation_systems(&mut world);

    assert!(
        world.get::<Idle>(actor).is_some(),
        "生成阶段 actor 必须仍是 Idle"
    );
    assert!(
        world.get::<Active>(actor).is_none(),
        "生成系统不得写 Active"
    );
    let spawned = candidates(&mut world);
    assert_eq!(spawned.len(), 1, "一个具 CanWander 的怪物应恰好一个候选");
    let action = spawned[0];
    assert!(
        world.get::<ActiveAction>(action).is_none(),
        "候选不能被预先激活"
    );
    assert_eq!(
        world.get::<ActionPriority>(action).unwrap().0,
        PRIORITY_WANDER
    );
    assert_eq!(
        world.get::<ChildOf>(action).unwrap().parent(),
        actor,
        "候选必须挂在 actor 下"
    );
}

/// actor 已在行动中时不得再生成候选（避免实体 churn，§3.6.9）。
#[test]
fn generation_skips_actor_with_active_action() {
    let (mut world, _player) = poc_world();
    let actor = poc_actor(&mut world, (5, 5));

    let active = world
        .spawn((
            ChildOf(actor),
            ActionPriority(PRIORITY_WANDER),
            ActionTimer {
                remaining_av: 500.0,
            },
            Wander,
            ActiveAction,
        ))
        .id();
    world.entity_mut(actor).remove::<Idle>().insert(Active);

    run_generation_systems(&mut world);

    assert!(
        candidates(&mut world).is_empty(),
        "actor 已在行动中，生成系统不得再产出候选"
    );
    assert!(world.get_entity(active).is_ok());
}

/// 低血量且具 `CanFlee` 时同时产出 Flee 与 Wander 两个候选（生成系统的多样性）。
#[test]
fn low_health_actor_produces_flee_and_wander_candidates() {
    let (mut world, _player) = poc_world();
    let actor = poc_actor(&mut world, (5, 5));
    *world.get_mut::<Health>(actor).unwrap() = Health::full(1.0, 100.0);

    run_generation_systems(&mut world);

    let spawned = candidates(&mut world);
    assert_eq!(spawned.len(), 2, "应当同时产出 Flee 与 Wander 候选");
    let priorities: Vec<i32> = spawned
        .iter()
        .map(|action| world.get::<ActionPriority>(*action).unwrap().0)
        .collect();
    assert!(
        priorities.contains(&PRIORITY_FLEE),
        "缺 Flee 候选: {priorities:?}"
    );
    assert!(
        priorities.contains(&PRIORITY_WANDER),
        "缺 Wander 候选: {priorities:?}"
    );
}

/// 两个候选并存时，低血量怪物的逃跑候选必须赢过游荡候选（优先级真的生效）。
#[test]
fn low_health_actor_arbitrates_to_flee() {
    let (mut world, _player) = poc_world();
    let actor = poc_actor(&mut world, (5, 5));
    *world.get_mut::<Health>(actor).unwrap() = Health::full(1.0, 100.0);

    run_generation_systems(&mut world);
    let _ = world.run_system_once(action_arbitration_system);

    let mut query = world.query_filtered::<(&ActionPriority, &ChildOf), With<ActiveAction>>();
    let winners: Vec<i32> = query
        .iter(&world)
        .filter(|(_, child_of)| child_of.parent() == actor)
        .map(|(priority, _)| priority.0)
        .collect();
    assert_eq!(winners, vec![PRIORITY_FLEE], "赢家必须是 Flee 候选");
    assert!(
        world.get::<Flee>(actor).is_none(),
        "行动组件挂在 action 实体上，不得回到 actor"
    );
}

/// 玩家不参与 AI 生成：`Can*` 不足以让 AI 生成系统给玩家挂行动。
#[test]
fn player_is_not_generated_by_ai_systems() {
    let (mut world, player) = poc_world();
    world
        .entity_mut(player)
        .insert((CanWander, CanFlee, CanMove));

    world.run_schedule(ActionPocSchedule);

    assert!(
        world.get::<Active>(player).is_none(),
        "AI 生成系统必须用 With<Monster> 过滤，不得给玩家挂行动"
    );
    assert!(world.get::<Idle>(player).is_some());
    assert!(candidates(&mut world).is_empty(), "玩家不得产生候选");
}

/// 端到端对照：旧的玩家命令路径仍然工作（PoC 没接主循环）。
#[test]
fn legacy_player_path_still_works_alongside_poc() {
    let (mut world, player) = poc_world();
    // 先把出生点周围的合法格挖成平地，再按 `find_walkable_step` 选中的方向移动。
    let (px, py) = player_pos(&world);
    let directions: [(isize, isize); 8] = [
        (0, -1),
        (0, 1),
        (-1, 0),
        (1, 0),
        (-1, -1),
        (1, -1),
        (-1, 1),
        (1, 1),
    ];
    for (dx, dy) in directions {
        let (nx, ny) = Position::new(px, py).offset(dx, dy);
        if nx < MAP_WIDTH && ny < MAP_HEIGHT {
            world.resource_mut::<Map>().tiles[ny][nx] = Tile::Floor;
        }
    }
    crate::system::run_settle_systems(&mut world);

    let step = find_walkable_step(&world, (px, py)).expect("平地场景必有可走格");
    let (nx, ny) = Position::new(px, py).offset(step.0, step.1);
    assert!(
        apply_player_command(
            &mut world,
            PlayerCommand::Move {
                dx: step.0,
                dy: step.1
            }
        ),
        "旧路径的移动命令必须仍然可用"
    );
    assert_eq!(player_pos(&world), (nx, ny));
    assert!(world.get::<Idle>(player).is_some());
}
