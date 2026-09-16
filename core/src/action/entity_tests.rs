//! `super`（`action::entity`）的 PoC 测试。
//!
//! 独立文件是为了让生产代码与测试分开、便于 Phase C 整块替换。
//! 通过 `entity.rs` 末尾的 `#[path = "entity_tests.rs"] mod tests;` 引入。

use super::*;
use crate::action::generation::player::PlayerCommand;
use crate::components::{CanBasicAttack, CanMove, CanWait, Viewshed};
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
///
/// **显式列出三个生成系统**：Phase C 会逐个加进来，漏加一处会让相应用例在
/// 「没产生候选」上失败而不是静默通过。
fn run_generation_systems(world: &mut World) {
    let _ = world.run_system_once(wait_generation_system);
    let _ = world.run_system_once(wander_generation_system);
    let _ = world.run_system_once(flee_generation_system);
}

/// 回归陷阱记录：**Bevy 的 `query_filtered` 在组件类型从未注册时会静默返回空**，
/// 不会报错。这条测试把该行为钉住，避免以后有人以为「跑过了 = 查到了」。
#[test]
fn unregistered_component_query_returns_empty_without_panic() {
    let mut world = World::new();
    let actor = world.spawn((crate::components::Active, Wait)).id();
    // `Ready` 在这个世界里从未注册过。
    let mut query = world.query_filtered::<Entity, (With<Active>, With<Wait>, With<Ready>)>();
    assert_eq!(query.iter(&world).count(), 0);
    assert!(world.get::<crate::components::Ready>(actor).is_none());
    assert!(world.get_entity(actor).is_ok(), "实体仍然活着");
}

/// 找出「有行动能力但没在行动」的 actor 的等待候选（C1）。
fn wait_candidate_for(world: &mut World, actor: Entity) -> Option<Entity> {
    let mut query = world.query_filtered::<(Entity, &ChildOf), With<Wait>>();
    query
        .iter(world)
        .find(|(_, child_of)| child_of.parent() == actor)
        .map(|(action, _)| action)
}

/// 只跑生成 + 等待仲裁，返回该 actor 的 `Wait` 行动实体。
fn run_wait_candidate(world: &mut World, actor: Entity) -> Entity {
    let _ = world.run_system_once(wait_generation_system);
    let _ = world.run_system_once(action_arbitration_system);
    wait_candidate_for(world, actor).expect("等待候选应当被生成并被仲裁选中")
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
///
/// 用「游荡能力」计数（等待是兜底行为，见 `wait_generation_*` 用例），
/// 因此这里只跑游荡生成系统，避免把兜底候选算进来。
#[test]
fn generation_only_spawns_candidates() {
    let (mut world, _player) = poc_world();
    let actor = poc_actor(&mut world, (5, 5));

    let _ = world.run_system_once(wander_generation_system);

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

    let _ = world.run_system_once(wander_generation_system);
    let _ = world.run_system_once(flee_generation_system);

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

/// C1 变体：`Wait` 与 `Wander` 并存时，`Wander`(50) 胜出、等待候选被清理。
#[test]
fn wait_loses_to_wander_and_candidate_is_cleaned_up() {
    let (mut world, _player) = poc_world();
    let actor = poc_actor(&mut world, (5, 5));

    let _ = world.run_system_once(wait_generation_system);
    let _ = world.run_system_once(wander_generation_system);
    let wait = wait_candidate_for(&mut world, actor).expect("应当生成 Wait 候选");
    let wander = {
        let mut query = world.query_filtered::<Entity, With<Wander>>();
        query
            .iter(&world)
            .find(|action| {
                world
                    .get::<ChildOf>(*action)
                    .is_some_and(|child_of| child_of.parent() == actor)
            })
            .expect("应当生成 Wander 候选")
    };

    let _ = world.run_system_once(action_arbitration_system);

    assert!(
        world.get::<ActiveAction>(wander).is_some(),
        "Wander(50) 必须胜过兜底 Wait(0)"
    );
    assert!(
        world.get_entity(wait).is_err(),
        "落败的等待候选必须被 despawn"
    );
}

/// C1 迁移记录：`Wait` 与旧 `World` 版执行器语义一致（无条件成功 + 回 Idle）。
///
/// **Bevy 陷阱（本次踩到）**：`query_filtered` 对「从未注册过的组件类型」静默返回空
/// 而不报错。所以「跑过旧执行器，实体没变」既可能是行为不同，也可能只是类型没注册。
/// 这里用 `poc_world()`（Phase B 的 PoC 链路已经把 `Active`/`Wait`/`Ready` 注册齐）
/// 来隔离这个变量。
#[test]
fn new_wait_semantics_match_legacy_wait() {
    let (mut world, player) = poc_world();
    let monster = poc_actor(&mut world, (5, 5));

    // 旧实现：action 组件挂在 actor 上，`execute_wait_system(&mut World)` 对
    // 命中实体无条件 `remove::<Wait>()` + `finish_action_success` → 回 `Idle`。
    //
    // 注意两点（都容易让这类「跑旧代码做对照」的测试假失败）：
    // 1. Bevy 的 `query_filtered` 对**从未注册过的组件类型**静默返回空，且按
    //    **archetype** 精确匹配——实体必须带上查询要求的所有组件（含 `Ready`）；
    // 2. 因此这里显式给出 `Active + Wait + Ready`，只验证旧执行器的语义本身。
    let legacy_actor = world
        .spawn((Active, Wait, Ready, ActionTimer { remaining_av: 0.0 }))
        .id();
    crate::action::execution::execute_wait_system(&mut world);
    assert!(
        world.get::<Idle>(legacy_actor).is_some(),
        "旧执行器必须把命中实体送回 Idle"
    );
    assert!(world.get::<Wait>(legacy_actor).is_none());
    assert!(world.get::<Active>(legacy_actor).is_none());
    assert!(
        world.get::<Idle>(monster).is_some(),
        "没有 Wait 组件的实体不得被误伤"
    );
    assert!(world.get::<Idle>(player).is_some());

    // 新实现：同样「什么都不做 + 无条件成功」，由 completion 收回 action 实体。
    let wait = run_wait_candidate(&mut world, monster);
    let _ = world.run_system_once(tick_action_timers_system);
    let _ = world.run_system_once(execute_wait_system);

    assert!(
        world.get::<Idle>(monster).is_none(),
        "执行器只发事件，状态回转由 completion 负责"
    );
    world.run_schedule(ActionPocSchedule);
    assert!(world.get::<Idle>(monster).is_some(), "完成后必须回 Idle");
    assert!(world.get::<Active>(monster).is_none());
    assert!(world.get_entity(wait).is_err(), "action 实体必须被回收");
}

// ── C2：玩家 Move 路径 ───────────────────────────────

/// 玩家请求用的场景：全平地 + 玩家位置，返回 `(world, player)`。
fn player_move_scene(player_pos: (usize, usize)) -> (World, Entity) {
    let (mut world, player) = poc_world();
    world.resource_mut::<Map>().tiles = [[Tile::Floor; MAP_WIDTH]; MAP_HEIGHT];
    {
        let mut pos = world.get_mut::<Position>(player).unwrap();
        pos.x = player_pos.0;
        pos.y = player_pos.1;
    }
    crate::system::run_settle_systems(&mut world);
    (world, player)
}

/// 当前挂在某个 actor 名下的 action 实体。
fn action_entities_of(world: &mut World, actor: Entity) -> Vec<Entity> {
    let mut query = world.query_filtered::<Entity, With<ChildOf>>();
    query
        .iter(world)
        .filter(|action| {
            world
                .get::<ChildOf>(*action)
                .is_some_and(|child_of| child_of.parent() == actor)
        })
        .collect()
}

/// 玩家 `Wait` 命令：直接产出 active action（不进仲裁），完成后回 `Idle`。
#[test]
fn player_wait_spawns_active_action_without_arbitration() {
    let (mut world, player) = player_move_scene((10, 10));

    world.resource_mut::<PlayerActionRequest>().command = Some(PlayerCommand::Wait);
    let _ = world.run_system_once(player_action_generation_system);

    let actions = action_entities_of(&mut world, player);
    assert_eq!(actions.len(), 1, "玩家应当恰好有一个 action 实体");
    let action = actions[0];
    assert!(
        world.get::<ActiveAction>(action).is_some(),
        "玩家行动必须直接是 ActiveAction（不经仲裁）"
    );
    assert!(
        world.get::<Candidate>(action).is_none(),
        "玩家行动不得留在候选态"
    );
    assert_eq!(
        world.get::<ActionSource>(action),
        Some(&ActionSource::Player)
    );
    assert_eq!(
        world.get::<ActionPriority>(action).unwrap().0,
        PRIORITY_PLAYER
    );
    assert!(world.get::<Wait>(action).is_some());
    assert!(
        world.get::<Wait>(player).is_none(),
        "行动组件必须挂在 action 实体上，不能回到 actor"
    );
    assert!(world.get::<Active>(player).is_some());
    assert!(world.get::<Idle>(player).is_none());

    // 一次完整往返：tick → 执行 → completion。
    world.run_schedule(ActionPocSchedule);
    assert!(world.get::<Idle>(player).is_some(), "完成后回 Idle");
    assert!(world.get::<Active>(player).is_none());
    assert!(
        action_entities_of(&mut world, player).is_empty(),
        "completion 之后不得残留 action 实体"
    );
}

/// 玩家 `Move` 命令：payload 挂在 action 实体上，执行后恰好移动一格。
#[test]
fn player_move_spawns_move_action_and_moves_one_tile() {
    let (mut world, player) = player_move_scene((0, 0));

    world.resource_mut::<PlayerActionRequest>().command =
        Some(PlayerCommand::Move { dx: 1, dy: 0 });
    let _ = world.run_system_once(player_action_generation_system);

    let actions = action_entities_of(&mut world, player);
    assert_eq!(actions.len(), 1);
    assert_eq!(
        world.get::<Move>(actions[0]),
        Some(&Move { dx: 1, dy: 0 }),
        "payload 必须挂在 action 实体上"
    );

    world.run_schedule(ActionPocSchedule);

    assert_eq!(
        world.get::<Position>(player).unwrap().to_tuple(),
        (1, 0),
        "执行后玩家必须恰好移动一格"
    );
    assert!(world.get::<Idle>(player).is_some());
}

/// 玩家移动与旧 `apply_player_command` 的落点一致（迁移期一致性）。
#[test]
fn player_move_via_action_entity_matches_legacy_command() {
    // 旧路径
    let (mut legacy, legacy_player) = player_move_scene((0, 0));
    assert!(
        apply_player_command(&mut legacy, PlayerCommand::Move { dx: 1, dy: 0 }),
        "旧路径应当接受该移动"
    );

    // 新路径（action 实体）
    let (mut modern, modern_player) = player_move_scene((0, 0));
    modern.resource_mut::<PlayerActionRequest>().command =
        Some(PlayerCommand::Move { dx: 1, dy: 0 });
    let _ = modern.run_system_once(player_action_generation_system);
    modern.run_schedule(ActionPocSchedule);

    assert_eq!(
        modern.get::<Position>(modern_player).unwrap().to_tuple(),
        legacy.get::<Position>(legacy_player).unwrap().to_tuple(),
        "新旧路径的玩家落点必须一致"
    );
}

/// 非法移动（撞墙 / 越界）必须被拒绝：不挂载 action，玩家保持 `Idle`。
#[test]
fn player_invalid_move_is_rejected() {
    let (mut world, player) = player_move_scene((10, 10));

    // 撞墙：目标格改成墙。
    world.resource_mut::<Map>().tiles[10][11] = Tile::Wall;
    world.resource_mut::<PlayerActionRequest>().command =
        Some(PlayerCommand::Move { dx: 1, dy: 0 });
    let _ = world.run_system_once(player_action_generation_system);
    assert!(
        action_entities_of(&mut world, player).is_empty(),
        "撞墙的移动不得挂载 action"
    );
    assert!(world.get::<Idle>(player).is_some());
    assert!(world.get::<Active>(player).is_none());

    // 越界：移到 (0,0) 后往负方向。
    {
        let mut pos = world.get_mut::<Position>(player).unwrap();
        pos.x = 0;
        pos.y = 0;
    }
    world.resource_mut::<PlayerActionRequest>().command =
        Some(PlayerCommand::Move { dx: -1, dy: 0 });
    let _ = world.run_system_once(player_action_generation_system);
    assert!(
        action_entities_of(&mut world, player).is_empty(),
        "越界的移动不得挂载 action"
    );
    assert_eq!(world.get::<Position>(player).unwrap().to_tuple(), (0, 0));
}

/// 目标格被占用时本轮不挂载（「走向怪物 = 攻击」属 C3）。
#[test]
fn player_move_into_occupied_tile_is_deferred_to_c3() {
    let (mut world, player) = player_move_scene((10, 10));
    let blocker = spawn_test_monster(
        &mut world,
        crate::monster::MonsterKindId::Goblin,
        (11, 10),
        10.0,
        4.0,
        5.0,
    );
    crate::system::run_settle_systems(&mut world);
    assert!(
        world.resource::<OccupancyMap>().is_occupied(11, 10),
        "测试前提：目标格必须已被占用"
    );

    world.resource_mut::<PlayerActionRequest>().command =
        Some(PlayerCommand::Move { dx: 1, dy: 0 });
    let _ = world.run_system_once(player_action_generation_system);

    assert!(
        action_entities_of(&mut world, player).is_empty(),
        "被占用的目标格本轮不得挂载 action（攻击属 C3）"
    );
    assert!(world.get::<Idle>(player).is_some());
    assert!(world.get_entity(blocker).is_ok());
}

/// 球员行动与 AI 行动互不覆盖：仲裁不得动玩家的 active action。
#[test]
fn arbitration_does_not_touch_player_active_action() {
    let (mut world, player) = player_move_scene((10, 10));
    let monster = poc_actor(&mut world, (20, 20));

    world.resource_mut::<PlayerActionRequest>().command =
        Some(PlayerCommand::Move { dx: 1, dy: 0 });
    let _ = world.run_system_once(player_action_generation_system);
    let player_actions = action_entities_of(&mut world, player);
    assert_eq!(player_actions.len(), 1);
    let player_action = player_actions[0];

    // 只跑 AI 生成 + 仲裁：玩家已经 active，AI 生成系统会跳过它。
    let _ = world.run_system_once(wait_generation_system);
    let _ = world.run_system_once(wander_generation_system);
    let _ = world.run_system_once(action_arbitration_system);

    assert!(
        world.get::<ActiveAction>(player_action).is_some(),
        "玩家的 active action 不得被仲裁夺走"
    );
    assert_eq!(
        world.get::<ActionSource>(player_action),
        Some(&ActionSource::Player)
    );
    // 对照：怪物确实拿到了 AI 行动（说明仲裁跑过并正常授予）。
    assert!(
        !action_entities_of(&mut world, monster).is_empty()
            || world.get::<Active>(monster).is_some(),
        "AI 侧仲裁应当照常给怪物授予行动"
    );
}

/// 玩家命令被消费后不会残留（`PlayerActionRequest` 必须被 take 掉）。
#[test]
fn player_request_is_consumed_even_when_rejected() {
    let (mut world, player) = player_move_scene((10, 10));
    world.resource_mut::<Map>().tiles[10][11] = Tile::Wall;
    world.resource_mut::<PlayerActionRequest>().command =
        Some(PlayerCommand::Move { dx: 1, dy: 0 });

    let _ = world.run_system_once(player_action_generation_system);
    assert!(
        world.resource::<PlayerActionRequest>().command.is_none(),
        "被拒绝的请求也必须被消费，否则会每轮重试"
    );
    assert!(world.get::<Idle>(player).is_some());
}

/// 玩家已有 action 时，第二个请求不得再挂载第二个 action（防重复行动）。
#[test]
fn player_second_request_is_ignored_while_busy() {
    let (mut world, player) = player_move_scene((10, 10));

    world.resource_mut::<PlayerActionRequest>().command =
        Some(PlayerCommand::Move { dx: 1, dy: 0 });
    let _ = world.run_system_once(player_action_generation_system);
    let first = action_entities_of(&mut world, player);
    assert_eq!(first.len(), 1);

    // 玩家还在 Active 中时再发一个请求：必须被忽略，且请求要被消费掉。
    world.resource_mut::<PlayerActionRequest>().command =
        Some(PlayerCommand::Move { dx: 0, dy: 1 });
    let _ = world.run_system_once(player_action_generation_system);

    let after = action_entities_of(&mut world, player);
    assert_eq!(after.len(), 1, "忙碌状态下不得再挂载 action：{after:?}");
    assert_eq!(after[0], first[0], "原有 action 不得被替换");
    assert!(
        world.resource::<PlayerActionRequest>().command.is_none(),
        "请求必须被消费，不能留到下一轮"
    );
}

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

// ── 参数化执行器与旧 `World` 执行器的 parity ─────────────

/// 构造「actor + action 实体（Move，已 Ready）」，返回 `(world, actor, action, observer)`。
///
/// `observer` 是观察者的 Entity id：它占据某个格子，用来让「被占用」与「空闲」
/// 两种情形落在同一个 actor 位置上。
fn move_parity_scene(
    actor_pos: (usize, usize),
    observer_pos: Option<(usize, usize)>,
    dx: isize,
    dy: isize,
) -> (World, Entity, Entity, Option<Entity>) {
    let (mut world, _player) = single_tile_scene();
    // 全平地：越界之外全是可走格，便于隔离「占用」这一个变量。
    world.resource_mut::<Map>().tiles = [[Tile::Floor; MAP_WIDTH]; MAP_HEIGHT];

    let actor = spawn_test_monster(
        &mut world,
        crate::monster::MonsterKindId::Rat,
        actor_pos,
        10.0,
        4.0,
        5.0,
    );
    let observer = observer_pos.map(|pos| {
        spawn_test_monster(
            &mut world,
            crate::monster::MonsterKindId::Goblin,
            pos,
            10.0,
            4.0,
            5.0,
        )
    });
    crate::system::run_settle_systems(&mut world);

    let action = world
        .spawn((
            ChildOf(actor),
            ActionPriority(PRIORITY_WANDER),
            ActionSource::Ai,
            ActionTimer { remaining_av: 0.0 },
            Move { dx, dy },
            ActiveAction,
            Ready,
        ))
        .id();
    world.entity_mut(actor).remove::<Idle>().insert(Active);

    (world, actor, action, observer)
}

/// 参数化执行器（`entity::execute_move_system`）与旧 `World` 执行器
/// （`movement::execute_move`）在同样场景下必须给出同样的结果。
///
/// 覆盖三情形：合法移动、目标格被占用、越界。比较的是
/// 「actor 最终位置 + 是否有 `ActionSucceeded` 事件 + 是否成功」。
#[test]
fn parameterized_move_matches_world_based_move() {
    use crate::action::execution::movement::execute_move as world_based_move;
    use crate::events::ActionSucceededEvent;
    use bevy_ecs::event::Events;

    let cases: [(&str, (usize, usize), Option<(usize, usize)>, (isize, isize)); 4] = [
        ("合法", (10, 10), None, (1, 0)),
        ("对角合法", (10, 10), None, (1, 1)),
        ("目标被占用", (10, 10), Some((11, 10)), (1, 0)),
        ("越界", (0, 0), None, (-1, 0)),
    ];

    for (name, actor_pos, observer_pos, (dx, dy)) in cases {
        // A：旧的 &mut World 执行器。
        let (mut world_a, actor_a, _action_a, _obs_a) =
            move_parity_scene(actor_pos, observer_pos, dx, dy);
        let moved_a = world_based_move(&mut world_a, actor_a, dx, dy);
        let pos_a = world_a.get::<Position>(actor_a).unwrap().to_tuple();

        // B：新的参数化执行器（只跑这一个系统）。
        let (mut world_b, actor_b, action_b, _obs_b) =
            move_parity_scene(actor_pos, observer_pos, dx, dy);
        let _ = world_b.run_system_once(execute_move_system);
        let pos_b = world_b.get::<Position>(actor_b).unwrap().to_tuple();
        let succeeded_b = world_b.resource::<Events<ActionSucceededEvent>>().len() > 0;

        assert_eq!(
            pos_a, pos_b,
            "情形「{name}」位置不一致：{pos_a:?} vs {pos_b:?}"
        );
        assert_eq!(
            moved_a, succeeded_b,
            "情形「{name}」成功判定不一致：World 版 {moved_a} vs 参数化版 {succeeded_b}"
        );
        assert_eq!(
            world_b.get_entity(action_b).is_ok(),
            true,
            "参数化执行器不得回收 action 实体（那是 completion 的职责）"
        );
        assert!(
            world_b.get::<Ready>(action_b).is_none(),
            "参数化执行器必须清掉 Ready，避免同一行动被执行两次"
        );
    }
}

/// 参数化执行器不再是 exclusive：能与 `core` 既有结算系统挂在**同一条调度**里。
///
/// `Schedule::initialize`（首次运行时触发）会做组件访问冲突检查——如果执行器真的
/// 与世界独占型访问冲突，这里会直接 panic。同时顺带验证两者都照常工作：
/// 行动被消耗（位置改变），结算系统也跑过（FOV 重算）。
#[test]
fn parameterized_move_executor_coexists_with_settle_systems() {
    use crate::schedule::CoreSettleSchedule;

    let (mut world, actor) = {
        let (mut world, _player) = single_tile_scene();
        world.resource_mut::<Map>().tiles = [[Tile::Floor; MAP_WIDTH]; MAP_HEIGHT];
        let actor = poc_actor(&mut world, (10, 10));
        // 让 actor 带 Viewshed，以便观察结算系统确实跑过。
        world.entity_mut(actor).insert(Viewshed::new(3));
        (world, actor)
    };

    let action = world
        .spawn((
            ChildOf(actor),
            ActionPriority(PRIORITY_WANDER),
            ActionSource::Ai,
            ActionTimer { remaining_av: 0.0 },
            Move { dx: 1, dy: 0 },
            ActiveAction,
            Ready,
        ))
        .id();
    world.entity_mut(actor).remove::<Idle>().insert(Active);

    // 把参数化执行器**追加**到既有的 core 结算调度里（不新建、不替换调度）：
    // 执行器必须能与 FOV / 占用图 / 事件更新等系统挂在一起而不冲突。
    world
        .resource_mut::<Schedules>()
        .get_mut(CoreSettleSchedule)
        .expect("core 结算调度已注册")
        .add_systems((execute_move_system, ApplyDeferred, action_completion_system).chain());
    world.run_schedule(CoreSettleSchedule);

    assert_eq!(
        world.get::<Position>(actor).unwrap().to_tuple(),
        (11, 10),
        "执行器应当把 actor 向右移动一格"
    );
    assert!(
        world
            .get::<Viewshed>(actor)
            .is_some_and(|viewshed| !viewshed.visible_tiles.is_empty()),
        "同一条调度里的既有结算系统（FOV）必须照常执行"
    );
    assert!(
        world.get_entity(action).is_err(),
        "同一条调度里的 completion 应当已经回收 action 实体"
    );
    assert!(
        world.get::<Idle>(actor).is_some(),
        "移动成功后 actor 必须回到 Idle"
    );
}
