//! `super`（`action::entity`）的 PoC 测试。
//!
//! 独立文件是为了让生产代码与测试分开、便于 Phase C 整块替换。
//! 通过 `entity.rs` 末尾的 `#[path = "entity_tests.rs"] mod tests;` 引入。

use super::*;
use crate::action::generation::player::PlayerCommand;
use crate::components::{CanBasicAttack, CanMove, CanWait, Experience, ExperienceReward, Viewshed};
use crate::map::{MAP_HEIGHT, MAP_WIDTH, Tile};
use crate::resources::PendingExp;
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
/// C1（C8 更新）：`Wait` 语义——执行器只发事件，状态回转由 completion 负责。
///
/// 旧链路（actor 上的 `Wait` + `execute_wait_system(&mut World)`）已在 C8 删除，
/// 因此本用例只验证新链路的语义：**什么都不做 + 无条件成功**，actor 回 `Idle`。
#[test]
fn wait_action_succeeds_and_returns_actor_to_idle() {
    let (mut world, player) = poc_world();
    let monster = poc_actor(&mut world, (5, 5));

    let wait = run_wait_candidate(&mut world, monster);
    let _ = world.run_system_once(tick_action_timers_system);
    let _ = world.run_system_once(execute_wait_system);

    assert!(
        world.get::<Idle>(monster).is_none(),
        "执行器只发事件，状态回转由 completion 负责"
    );
    assert!(
        world.get::<Idle>(player).is_some(),
        "没有对应 action 实体的实体不得被误伤"
    );
    world.run_schedule(ActionPocSchedule);
    assert!(world.get::<Idle>(monster).is_some(), "完成后必须回 Idle");
    assert!(world.get::<Active>(monster).is_none());
    assert!(
        world.get::<Failure>(monster).is_none(),
        "等待必须成功，不得落到 Failure"
    );
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

/// C3：玩家走向怪物 = 声明攻击（`BasicAttack` 挂 action 实体，payload 是目标）。
#[test]
fn player_move_into_monster_declares_attack() {
    let (mut world, player) = player_move_scene((10, 10));
    let monster = spawn_test_monster(
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
        "测试前提：目标格必须已被怪物占用"
    );

    world.resource_mut::<PlayerActionRequest>().command =
        Some(PlayerCommand::Move { dx: 1, dy: 0 });
    let _ = world.run_system_once(player_action_generation_system);

    let actions = action_entities_of(&mut world, player);
    assert_eq!(actions.len(), 1, "应当声明一个攻击行动");
    assert_eq!(
        world.get::<BasicAttack>(actions[0]),
        Some(&BasicAttack { target: monster }),
        "攻击 payload 必须指向那只怪物"
    );
    assert!(
        world.get::<Move>(actions[0]).is_none(),
        "声明攻击时不得同时挂 Move"
    );
    assert_eq!(
        world.get::<ActionName>(actions[0]).map(|name| name.0),
        Some("BasicAttack")
    );
}

/// C3：攻击执行只发 `AttackIntentEvent`，伤害仍由结算链路负责（只结算一次）。
#[test]
fn attack_action_emits_intent_and_damage_resolves_once() {
    use crate::events::AttackIntentEvent;
    use bevy_ecs::event::Events;

    let (mut world, player) = player_move_scene((10, 10));
    let monster = spawn_test_monster(
        &mut world,
        crate::monster::MonsterKindId::Goblin,
        (11, 10),
        30.0,
        4.0,
        5.0,
    );
    crate::system::run_settle_systems(&mut world);
    let monster_hp_before = world.get::<Health>(monster).unwrap().current;

    world.resource_mut::<PlayerActionRequest>().command =
        Some(PlayerCommand::Move { dx: 1, dy: 0 });
    let _ = world.run_system_once(player_action_generation_system);
    let action = action_entities_of(&mut world, player)[0];

    // 只跑执行器：应当恰好发一个 AttackIntentEvent，且不动血量。
    let _ = world.run_system_once(tick_action_timers_system);
    let _ = world.run_system_once(execute_basic_attack_system);
    assert_eq!(
        world.resource::<Events<AttackIntentEvent>>().len(),
        1,
        "执行器应当恰好发出一个攻击意图"
    );
    assert_eq!(
        world.get::<Health>(monster).unwrap().current,
        monster_hp_before,
        "执行器不得自己结算伤害"
    );

    // 再跑结算：伤害落地一次，重复结算不再扣。
    crate::system::run_settle_systems(&mut world);
    let after_first = world.get::<Health>(monster).unwrap().current;
    assert!(after_first < monster_hp_before, "结算后应当扣血");
    crate::system::run_settle_systems(&mut world);
    assert_eq!(
        world.get::<Health>(monster).unwrap().current,
        after_first,
        "重复结算不得再扣血（I90 保证）"
    );

    // completion 回收 action 实体，玩家回 Idle。
    world.run_schedule(ActionPocSchedule);
    assert!(world.get_entity(action).is_err());
    assert!(world.get::<Idle>(player).is_some());
}

/// C3：保活失败（目标已跑远）→ `ActionFailedEvent` → 玩家进 `Failure`。
#[test]
fn attack_action_fails_when_target_is_far() {
    let (mut world, player) = player_move_scene((10, 10));
    let monster = spawn_test_monster(
        &mut world,
        crate::monster::MonsterKindId::Goblin,
        (11, 10),
        30.0,
        4.0,
        5.0,
    );
    crate::system::run_settle_systems(&mut world);

    world.resource_mut::<PlayerActionRequest>().command =
        Some(PlayerCommand::Move { dx: 1, dy: 0 });
    let _ = world.run_system_once(player_action_generation_system);

    // 目标已经跑远：保活失败。
    {
        let mut pos = world.get_mut::<Position>(monster).unwrap();
        pos.x = 40;
        pos.y = 40;
    }
    let _ = world.run_system_once(tick_action_timers_system);
    let _ = world.run_system_once(execute_basic_attack_system);
    world.run_schedule(ActionPocSchedule);

    assert!(
        world.get::<Failure>(player).is_some(),
        "保活失败必须让 actor 落到 Failure"
    );
    assert!(world.get::<Active>(player).is_none());
    assert_eq!(
        world.get::<Health>(monster).unwrap().current,
        30.0,
        "保活失败不得造成伤害"
    );
}

/// 目标格被非怪物占用时仍然拒绝（等价旧实现：占用者不是怪物 → 请求无效）。
#[test]
fn player_move_into_non_monster_occupant_is_rejected() {
    let (mut world, player) = player_move_scene((10, 10));
    // 用一个没有 Monster 标记的实体占住目标格。
    let obstacle = world.spawn((Position::new(11, 10), Health::new(10.0))).id();
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
        "占用者不是怪物时不得挂载行动"
    );
    assert!(world.get::<Idle>(player).is_some());
    assert!(world.get_entity(obstacle).is_ok());
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

// ── C4：游荡（AI 侧）迁移对照 ────────────────────────

/// 造一个「全平地 + 一只具 CanWander 的怪 + 同 seed RNG」的世界。
///
/// RNG 状态由 `new_game(seed)` 决定，两个世界用同一 seed 即同一随机序列。
fn wander_parity_scene(seed: u64, pos: (usize, usize)) -> (World, Entity) {
    let mut world = crate::world_loop::new_game(seed);
    // 清掉地图生成的怪物，只留玩家 + 我们要测的那只怪。
    let monsters: Vec<Entity> = {
        let mut query = world.query_filtered::<Entity, With<Monster>>();
        query.iter(&world).collect()
    };
    for monster in monsters {
        world.despawn(monster);
    }
    world.resource_mut::<Map>().tiles = [[Tile::Floor; MAP_WIDTH]; MAP_HEIGHT];
    let actor = spawn_test_monster(
        &mut world,
        crate::monster::MonsterKindId::Rat,
        pos,
        10.0,
        4.0,
        5.0,
    );
    world.entity_mut(actor).insert((CanWander,));
    crate::system::run_settle_systems(&mut world);
    (world, actor)
}

/// C4（C8 更新）：游荡每步**恰好消耗一次**随机数，且落点只能是合法相邻格。
///
/// 旧 `execute_wander_system` 已在 C8 删除；这里保留原本要钉住的契约：
/// 「先抽方向、再判合法性」——顺序若变化，`GameRng::steps` 的消耗量会漂移，
/// 而它是存档回放的基础。
#[test]
fn wander_action_consumes_exactly_one_random_step() {
    let pos = (30, 30);

    for seed in [1u64, 2, 3, 4, 5, 6, 7, 8] {
        let (mut world, actor) = wander_parity_scene(seed, pos);
        let _action = world
            .spawn((
                ChildOf(actor),
                ActionPriority(PRIORITY_WANDER),
                ActionSource::Ai,
                ActionTimer { remaining_av: 0.0 },
                Wander,
                ActiveAction,
                Ready,
            ))
            .id();
        world.entity_mut(actor).remove::<Idle>().insert(Active);
        let rng_before = world.resource::<GameRng>().steps;

        let _ = world.run_system_once(execute_wander_system);

        let after = world.get::<Position>(actor).unwrap().to_tuple();
        assert_eq!(
            world.resource::<GameRng>().steps - rng_before,
            1,
            "seed={seed} 时一次游荡应当恰好抽一次方向"
        );
        assert!(
            pos.0.abs_diff(after.0) <= 1 && pos.1.abs_diff(after.1) <= 1,
            "seed={seed} 时游荡只能走一步：{pos:?} → {after:?}"
        );
        assert!(
            world.resource::<Map>().tiles[after.1][after.0].walkable(),
            "seed={seed} 时落点必须可行走: {after:?}"
        );
    }
}

/// C4：游荡被墙挡住时原地不动，但仍算“完成”（等价旧实现的无条件成功）。
#[test]
fn wander_blocked_still_succeeds() {
    let (mut world, _player) = poc_world();
    world.resource_mut::<Map>().tiles = [[Tile::Wall; MAP_WIDTH]; MAP_HEIGHT];
    world.resource_mut::<Map>().tiles[30][30] = Tile::Floor;
    let actor = spawn_test_monster(
        &mut world,
        crate::monster::MonsterKindId::Rat,
        (30, 30),
        10.0,
        4.0,
        5.0,
    );
    let action = world
        .spawn((
            ChildOf(actor),
            ActionPriority(PRIORITY_WANDER),
            ActionSource::Ai,
            ActionTimer { remaining_av: 0.0 },
            Wander,
            ActiveAction,
            Ready,
        ))
        .id();
    world.entity_mut(actor).remove::<Idle>().insert(Active);

    let _ = world.run_system_once(execute_wander_system);

    assert_eq!(
        world.get::<Position>(actor).unwrap().to_tuple(),
        (30, 30),
        "四周都是墙时不得移动"
    );
    world.run_schedule(ActionPocSchedule);
    assert!(
        world.get_entity(action).is_err(),
        "被挡也算完成，实体应被回收"
    );
    assert!(
        world.get::<Idle>(actor).is_some(),
        "被挡的游荡仍然回 Idle（不落 Failure）"
    );
}

// ── C5：追击迁移对照 ─────────────────────────────────

/// 追击对照场景：同 seed、全平地、玩家与怪物位置固定、怪物带视野与记忆。
fn chase_parity_scene(
    seed: u64,
    player_tile: (usize, usize),
    monster_tile: (usize, usize),
) -> (World, Entity, Entity) {
    let mut world = crate::world_loop::new_game(seed);
    let monsters: Vec<Entity> = {
        let mut query = world.query_filtered::<Entity, With<Monster>>();
        query.iter(&world).collect()
    };
    for monster in monsters {
        world.despawn(monster);
    }
    let player = {
        let mut query = world.query_filtered::<Entity, With<Player>>();
        query.iter(&world).next().expect("新游戏必须有玩家")
    };
    world.resource_mut::<Map>().tiles = [[Tile::Floor; MAP_WIDTH]; MAP_HEIGHT];
    {
        let mut pos = world.get_mut::<Position>(player).unwrap();
        pos.x = player_tile.0;
        pos.y = player_tile.1;
    }
    let monster = spawn_test_monster(
        &mut world,
        crate::monster::MonsterKindId::Rat,
        monster_tile,
        10.0,
        4.0,
        5.0,
    );
    world
        .entity_mut(monster)
        .insert((CanChase, Viewshed::new(10), LastKnownPlayerPos::default()));
    crate::system::run_settle_systems(&mut world);
    // `new_game` 只注册 init/settle 两条调度；用行动链路还需要 PoC 调度。
    world.add_schedule(build_action_poc_schedule());
    (world, player, monster)
}

/// 直接设定「能否看见玩家」，避免依赖 FOV 细节。
fn set_visibility(world: &mut World, monster: Entity, player_tile: (usize, usize), sees: bool) {
    let mut viewshed = world.get_mut::<Viewshed>(monster).unwrap();
    viewshed.visible_tiles = if sees { vec![player_tile] } else { Vec::new() };
}

/// 给怪物挂一个已到期的 action 实体（`Chase`），并让它进入 `Active`。
fn mount_ready_chase(world: &mut World, monster: Entity) -> Entity {
    let action = world
        .spawn((
            ChildOf(monster),
            ActionPriority(PRIORITY_CHASE),
            ActionSource::Ai,
            ActionTimer { remaining_av: 0.0 },
            Chase,
            ActiveAction,
            Ready,
        ))
        .id();
    world.entity_mut(monster).remove::<Idle>().insert(Active);
    action
}

/// C5（C8 更新）：追击执行器三情形的行为契约。
///
/// 旧 `execute_chase_system` 已在 C8 删除，因此这里不再做「新旧对照」，
/// 而是把当时对照出来的三条契约直接钉住：
///
/// | 情形 | 期望 |
/// |---|---|
/// | 可见、不相邻 | 朝玩家走一步（`astar`），写 `LastKnownPlayerPos`，无攻击意图 |
/// | 不可见、有记忆 | 朝记忆点走一步 |
/// | 不可见、无记忆 | 保活失败 → `Failure`，不移动 |
///
/// **只比较「一次执行」的结果**：跑完执行器就取快照，**不要再跑完整调度**
/// ——completion 会立刻把 actor 放回 `Idle`，下一轮生成系统又会给它挂新的追击，
/// 那样量到的是「两步」而不是「一步」（本用例第一版就踩了这个坑）。
#[test]
fn chase_action_three_outcome_contracts() {
    use crate::events::AttackIntentEvent;
    use bevy_ecs::event::Events;

    let player_tile = (40, 40);
    let monster_tile = (34, 40);

    for (name, sees, memory) in [
        ("可见走一步", true, None),
        ("不可见但有记忆", false, Some((35, 40))),
        ("不可见且无记忆", false, None),
    ] {
        let (mut world, _player, monster) = chase_parity_scene(11, player_tile, monster_tile);
        set_visibility(&mut world, monster, player_tile, sees);
        world.get_mut::<LastKnownPlayerPos>(monster).unwrap().0 = memory;
        mount_ready_chase(&mut world, monster);

        let _ = world.run_system_once(execute_chase_system);
        world_run_completion_only(&mut world);

        let after = world.get::<Position>(monster).unwrap().to_tuple();
        let memory_after = world
            .get::<LastKnownPlayerPos>(monster)
            .and_then(|known| known.0);
        let intents = world.resource::<Events<AttackIntentEvent>>().len();

        match (sees, memory) {
            (true, _) => {
                assert_eq!(
                    memory_after,
                    Some(player_tile),
                    "情形「{name}」可见时必须把玩家位置写入记忆"
                );
                assert_eq!(intents, 0, "情形「{name}」不相邻时不得攻击");
                assert_ne!(after, monster_tile, "情形「{name}」应当朝玩家走一步");
                assert!(
                    world.get::<Idle>(monster).is_some(),
                    "情形「{name}」追击应当成功结束"
                );
            }
            (false, Some(_)) => {
                assert_ne!(after, monster_tile, "情形「{name}」应当朝记忆点走一步");
                assert_eq!(intents, 0, "情形「{name}」不得攻击");
                assert!(world.get::<Idle>(monster).is_some());
            }
            (false, None) => {
                assert_eq!(after, monster_tile, "情形「{name}」保活失败时不得移动");
                assert_eq!(intents, 0, "情形「{name}」保活失败时不得攻击");
                assert!(
                    world.get::<Failure>(monster).is_some(),
                    "情形「{name}」保活失败必须落到 Failure"
                );
                assert!(world.get::<Idle>(monster).is_none());
            }
        }
    }
}

/// 只跑 completion（消费 `ActionSucceeded/FailedEvent`），不跑生成/仲裁/执行。
fn world_run_completion_only(world: &mut World) {
    let mut schedule = Schedule::new(ActionPocSchedule);
    schedule.add_systems(action_completion_system);
    world.add_schedule(schedule);
    world.run_schedule(ActionPocSchedule);
}

/// C5：可见 + 相邻 → 声明攻击而不是移动。
#[test]
fn chase_adjacent_visible_declares_attack() {
    use crate::events::AttackIntentEvent;
    use bevy_ecs::event::Events;

    let player_tile = (40, 40);
    let monster_tile = (41, 40);
    let (mut world, _player, monster) = chase_parity_scene(21, player_tile, monster_tile);
    set_visibility(&mut world, monster, player_tile, true);
    mount_ready_chase(&mut world, monster);

    let _ = world.run_system_once(execute_chase_system);

    assert_eq!(
        world.get::<Position>(monster).unwrap().to_tuple(),
        monster_tile,
        "相邻时不得移动（应当原地攻击）"
    );
    assert_eq!(
        world.resource::<Events<AttackIntentEvent>>().len(),
        1,
        "相邻且可见时应当发出一个攻击意图"
    );
}

/// C5：追击候选的生成条件（可见 或 有记忆）。
#[test]
fn chase_generation_requires_sight_or_memory() {
    let player_tile = (40, 40);
    let monster_tile = (30, 40);

    // 情形 A：看不见且无记忆 → 不生成。
    let (mut world, _player, monster) = chase_parity_scene(31, player_tile, monster_tile);
    set_visibility(&mut world, monster, player_tile, false);
    let _ = world.run_system_once(chase_generation_system);
    assert!(
        candidates(&mut world).is_empty(),
        "看不见又没记忆时不得生成追击候选"
    );

    // 情形 B：可见 → 生成，优先级 CHASE。
    let (mut world, _player, monster) = chase_parity_scene(31, player_tile, monster_tile);
    set_visibility(&mut world, monster, player_tile, true);
    let _ = world.run_system_once(chase_generation_system);
    let spawned = candidates(&mut world);
    assert_eq!(spawned.len(), 1, "可见时应当生成一个追击候选");
    assert_eq!(
        world.get::<ActionPriority>(spawned[0]).unwrap().0,
        PRIORITY_CHASE
    );
    assert_eq!(world.get::<ChildOf>(spawned[0]).unwrap().parent(), monster);

    // 情形 C：看不见但有记忆 → 生成。
    let (mut world, _player, monster) = chase_parity_scene(31, player_tile, monster_tile);
    set_visibility(&mut world, monster, player_tile, false);
    world.get_mut::<LastKnownPlayerPos>(monster).unwrap().0 = Some((35, 40));
    let _ = world.run_system_once(chase_generation_system);
    assert_eq!(
        candidates(&mut world).len(),
        1,
        "有最后已知位置时应当生成追击候选"
    );
}

/// C5：抵达最后已知位置后清空记忆（旧实现的「放弃搜索」语义）。
#[test]
fn chase_clears_memory_when_reaching_last_known_position() {
    let player_tile = (40, 40);
    let monster_tile = (36, 40);
    let (mut world, _player, monster) = chase_parity_scene(41, player_tile, monster_tile);
    set_visibility(&mut world, monster, player_tile, false);
    // 最后已知位置就在脚下（切比雪夫距离 0 ≤ 2）。
    world.get_mut::<LastKnownPlayerPos>(monster).unwrap().0 = Some(monster_tile);
    mount_ready_chase(&mut world, monster);

    let _ = world.run_system_once(execute_chase_system);

    assert_eq!(
        world
            .get::<LastKnownPlayerPos>(monster)
            .and_then(|known| known.0),
        None,
        "抵达最后已知位置后必须清空记忆"
    );
}

/// I91 回归：`Idle` 与 `Failure` 必须互斥（成功清 Failure，失败清 Idle）。
///
/// 旧 `finish_action_failure` 不清 `Idle`（C8 已随旧链路删除），新链路的
/// `action_completion_system` 一开始也照抄了这个疏漏，本用例钉住不变式。
#[test]
fn idle_and_failure_are_mutually_exclusive() {
    // 失败分支：actor 事先同时持有 Idle（spawn 时给的）与 Failure 的旧残留，
    // completion 必须把 Idle 清掉。
    let (mut world, _player) = poc_world();
    let monster = poc_actor(&mut world, (5, 5));
    // 追击需要视野与记忆组件；这里给空的（既看不见也没记忆）。
    world
        .entity_mut(monster)
        .insert((CanChase, Viewshed::new(0), LastKnownPlayerPos::default()));
    let action = mount_ready_chase(&mut world, monster);

    let _ = world.run_system_once(execute_chase_system);
    assert!(
        world.get::<Idle>(monster).is_none(),
        "执行器发失败事件前不得改动状态"
    );
    world_run_completion_only(&mut world);
    assert!(world.get::<Failure>(monster).is_some(), "必须落到 Failure");
    assert!(
        world.get::<Idle>(monster).is_none(),
        "失败后不得残留 Idle（I91）"
    );
    assert!(world.get_entity(action).is_err());

    // 成功分支：从 Failure 状态出发，completion 必须把 Failure 清掉。
    let (mut world, _player) = poc_world();
    let monster = poc_actor(&mut world, (5, 5));
    world.entity_mut(monster).remove::<Idle>().insert(Failure);
    let action = world
        .spawn((
            ChildOf(monster),
            ActionPriority(PRIORITY_WAIT),
            ActionSource::Ai,
            ActionTimer { remaining_av: 0.0 },
            Wait,
            ActiveAction,
            Ready,
        ))
        .id();

    let _ = world.run_system_once(execute_wait_system);
    world_run_completion_only(&mut world);
    assert!(world.get::<Idle>(monster).is_some(), "成功后必须回 Idle");
    assert!(
        world.get::<Failure>(monster).is_none(),
        "成功后不得残留 Failure（I91）"
    );
    assert!(world.get_entity(action).is_err());
}

// ── C6：逃跑迁移对照 ─────────────────────────────────

/// Bevy 陷阱之二：**参数校验失败是"报错并跳过系统"，不是 panic**。
///
/// 参数缺失/查询无法初始化时，`run_system_once` 走错误处理器（默认只打印），
/// 系统体一行都不执行，调用方只看返回值——极容易被当成"逻辑跑过了但没生效"。
/// 与 L49（查询静默返回空）是同一类问题的两个面。
///
/// 这里的触发方式是「查询依赖的组件类型从未注册」：空世界里没有任何实体带
/// `Player`，`Query<Entity, With<Player>>` 无法初始化。
#[test]
fn run_system_once_skips_system_when_param_validation_fails() {
    // 空世界：`Player` 组件类型未注册 → 追击系统无法初始化 → 返回 Err。
    let mut world = World::new();
    world.insert_resource(Map::new());
    world.insert_resource(OccupancyMap::new());
    let result = world.run_system_once(execute_chase_system);
    assert!(
        result.is_err(),
        "查询无法初始化时 run_system_once 必须返回 Err（而不是静默成功）"
    );

    // 同一个系统在类型就绪的世界里就能跑：证明上一步确实是参数校验拦下的。
    let (mut ready_world, player) = poc_world();
    assert!(ready_world.get::<Player>(player).is_some());
    assert!(
        ready_world.run_system_once(execute_chase_system).is_ok(),
        "类型就绪时系统应当正常执行"
    );
}

/// 逃跑场景：全平地、怪物低血、玩家位置给定。
fn flee_parity_scene(
    seed: u64,
    player_tile: (usize, usize),
    monster_tile: (usize, usize),
    monster_hp_ratio: f64,
    sees_player: bool,
) -> (World, Entity, Entity) {
    let mut world = crate::world_loop::new_game(seed);
    let monsters: Vec<Entity> = {
        let mut query = world.query_filtered::<Entity, With<Monster>>();
        query.iter(&world).collect()
    };
    for monster in monsters {
        world.despawn(monster);
    }
    let player = {
        let mut query = world.query_filtered::<Entity, With<Player>>();
        query.iter(&world).next().expect("新游戏必须有玩家")
    };
    world.resource_mut::<Map>().tiles = [[Tile::Floor; MAP_WIDTH]; MAP_HEIGHT];
    {
        let mut pos = world.get_mut::<Position>(player).unwrap();
        pos.x = player_tile.0;
        pos.y = player_tile.1;
    }
    let monster = spawn_test_monster(
        &mut world,
        crate::monster::MonsterKindId::Rat,
        monster_tile,
        100.0,
        4.0,
        5.0,
    );
    world
        .entity_mut(monster)
        .insert((CanFlee, Viewshed::new(10), LastKnownPlayerPos::default()));
    let hp = 100.0 * monster_hp_ratio;
    *world.get_mut::<Health>(monster).unwrap() = Health::full(hp, 100.0);
    crate::system::run_settle_systems(&mut world);
    // **必须在 settle 之后**设置视图：`fov_system` 会按真实位置重算 Viewshed，
    // 先设的话会被覆盖（本用例第一版就踩了这个坑，导致「不可见」情形其实也可见）。
    set_visibility(&mut world, monster, player_tile, sees_player);
    world.add_schedule(build_action_poc_schedule());
    (world, player, monster)
}

/// 给怪物挂一个已到期的 `Flee` action 实体。
fn mount_ready_flee(world: &mut World, monster: Entity) -> Entity {
    let action = world
        .spawn((
            ChildOf(monster),
            ActionPriority(PRIORITY_FLEE),
            ActionSource::Ai,
            ActionTimer { remaining_av: 0.0 },
            Flee,
            ActiveAction,
            Ready,
        ))
        .id();
    world.entity_mut(monster).remove::<Idle>().insert(Active);
    action
}

/// C6（C8 更新）：逃跑执行器四情形的行为契约。
///
/// 旧 `execute_flee_system` 已在 C8 删除，这里把当时对照出来的契约直接钉住：
///
/// | 情形 | 期望 |
/// |---|---|
/// | 低血、有路 | 走到「合法且离玩家曼哈顿距离最远」的格，无攻击意图 |
/// | 血量回到退出阈值以上 | 保活失败 → `Failure`，不移动 |
/// | 相邻被堵、可见 | 无处可逃 → 原地声明攻击 |
/// | 相邻被堵、不可见 | 无处可逃 → 原地不动，无攻击意图 |
#[test]
fn flee_action_four_outcome_contracts() {
    use crate::events::AttackIntentEvent;
    use bevy_ecs::event::Events;

    let player_tile = (40, 40);
    let monster_tile = (36, 40);

    // 情形 1：低血、有路 → 远离玩家。
    {
        let (mut world, _player, monster) =
            flee_parity_scene(51, player_tile, monster_tile, 0.10, true);
        mount_ready_flee(&mut world, monster);
        let _ = world.run_system_once(execute_flee_system);
        let after = world.get::<Position>(monster).unwrap().to_tuple();
        assert!(
            Position::new(after.0, after.1).manhattan(Position::new(player_tile.0, player_tile.1))
                > Position::new(monster_tile.0, monster_tile.1)
                    .manhattan(Position::new(player_tile.0, player_tile.1)),
            "逃跑必须增大与玩家的距离：{monster_tile:?} → {after:?}"
        );
        assert_eq!(
            world.resource::<Events<AttackIntentEvent>>().len(),
            0,
            "有路可逃时不得攻击"
        );
    }

    // 情形 2：血量回到退出阈值以上 → 保活失败。
    {
        let (mut world, _player, monster) =
            flee_parity_scene(51, player_tile, monster_tile, 0.50, true);
        mount_ready_flee(&mut world, monster);
        let _ = world.run_system_once(execute_flee_system);
        world_run_completion_only(&mut world);
        assert!(
            world.get::<Failure>(monster).is_some(),
            "回到退出阈值以上必须让逃跑行动失败"
        );
        assert!(world.get::<Idle>(monster).is_none(), "不得残留 Idle（I91）");
        assert_eq!(
            world.get::<Position>(monster).unwrap().to_tuple(),
            monster_tile,
            "保活失败时不得移动"
        );
    }

    // 情形 3 / 4：被堵在角落（可见 / 不可见）。
    //
    // 触发「反咬」需要玩家**相邻**（切比雪夫 =1）；同时玩家格本身也被封成墙，
    // 否则那个格子就是合法逃跑位，怪物会走过去而不是「无处可逃」。
    for (name, sees, expect_intent) in [("可见", true, 1usize), ("不可见", false, 0usize)] {
        let monster_tile = (30, 30);
        let player_tile = (31, 30);
        let (mut world, _player, monster) =
            flee_parity_scene(51, player_tile, monster_tile, 0.10, sees);
        // 把怪物周围（含玩家所在的相邻格）全部封成墙。
        for (dx, dy) in [
            (0isize, -1isize),
            (0, 1),
            (-1, 0),
            (1, 0),
            (-1, -1),
            (1, -1),
            (-1, 1),
            (1, 1),
        ] {
            let (nx, ny) = Position::new(monster_tile.0, monster_tile.1).offset(dx, dy);
            if nx < MAP_WIDTH && ny < MAP_HEIGHT {
                world.resource_mut::<Map>().tiles[ny][nx] = Tile::Wall;
            }
        }
        // 上一个情形可能留下未消费的意图事件；先清空再计数，
        // 否则 `Events::len()` 会把残留算进本情形。
        world.resource_mut::<Events<AttackIntentEvent>>().update();
        mount_ready_flee(&mut world, monster);
        let _ = world.run_system_once(execute_flee_system);

        // 可见性必须真的是本情形设定的那个：`set_visibility` 在场景构建里调用过，
        // 但这里再断言一次，避免「两个情形其实跑成了同一个」这类静默错误。
        assert_eq!(
            world
                .get::<Viewshed>(monster)
                .is_some_and(|viewshed| viewshed.can_see(player_tile)),
            sees,
            "情形「被堵{name}」的可见性设置不符"
        );

        assert_eq!(
            world.get::<Position>(monster).unwrap().to_tuple(),
            monster_tile,
            "情形「被堵{name}」时无处可逃，必须原地不动"
        );
        assert_eq!(
            world.resource::<Events<AttackIntentEvent>>().len(),
            expect_intent,
            "情形「被堵{name}」的攻击意图数不符（相邻且可见才反咬）"
        );
    }
}

/// C6：逃跑候选的生成条件（低血才产生）。
#[test]
fn flee_generation_requires_low_health() {
    let player_tile = (40, 40);
    let monster_tile = (36, 40);

    // 低血 → 生成。
    let (mut world, _player, _monster) =
        flee_parity_scene(81, player_tile, monster_tile, 0.10, true);
    let _ = world.run_system_once(flee_generation_system);
    let spawned = candidates(&mut world);
    assert_eq!(spawned.len(), 1, "低血时应当生成逃跑候选");
    assert_eq!(
        world.get::<ActionPriority>(spawned[0]).unwrap().0,
        PRIORITY_FLEE
    );

    // 高血 → 不生成。
    let (mut world, _player, _monster) =
        flee_parity_scene(81, player_tile, monster_tile, 0.60, true);
    let _ = world.run_system_once(flee_generation_system);
    assert!(
        candidates(&mut world).is_empty(),
        "血量充足时不得生成逃跑候选"
    );
}

// ── C9：六行动 parity 套件 ───────────────────────────

/// 一次「到期 → 执行 → 完成」的往返，返回 action 实体是否已被回收。
///
/// **执行器只负责发事件**：`Ready` 的清理分两条路——`execute_move_system`
/// 显式 `remove::<Ready>()`，其余执行器靠 completion despawn action 实体顺带清掉。
/// 因此这一层的断言是「事件已发出」，而不是「Ready 已被移除」。
/// 只跑该行动的专属执行器 + completion，避免整轮调度把「一步」变成多步。
fn run_one_action_roundtrip(world: &mut World, action: Entity) {
    use crate::events::{ActionFailedEvent, ActionSucceededEvent};
    use bevy_ecs::event::Events;

    let _ = world.run_system_once(tick_action_timers_system);
    assert!(
        world.get::<Ready>(action).is_some(),
        "AV 归零后 action 必须被标记 Ready"
    );

    // 清空两类事件缓冲，后续只数本轮产生的。
    world
        .resource_mut::<Events<ActionSucceededEvent>>()
        .update();
    world.resource_mut::<Events<ActionFailedEvent>>().update();

    let _ = world.run_system_once(execute_wait_system);
    let _ = world.run_system_once(execute_move_system);
    let _ = world.run_system_once(execute_basic_attack_system);
    let _ = world.run_system_once(execute_chase_system);
    let _ = world.run_system_once(execute_flee_system);
    let _ = world.run_system_once(execute_wander_system);

    let emitted = world.resource::<Events<ActionSucceededEvent>>().len()
        + world.resource::<Events<ActionFailedEvent>>().len();
    assert_eq!(
        emitted, 1,
        "每个到期的 action 必须恰好产生一个完成/失败事件"
    );

    world_run_completion_only(world);
    assert!(
        world.get_entity(action).is_err(),
        "completion 必须回收 action 实体"
    );
}

/// C9：`Wait` —— 什么都不做、无条件成功。
#[test]
fn parity_wait_scenario() {
    let (mut world, _player) = poc_world();
    let actor = poc_actor(&mut world, (10, 10));
    let start = world.get::<Position>(actor).unwrap().to_tuple();
    let action = run_wait_candidate(&mut world, actor);

    run_one_action_roundtrip(&mut world, action);
    assert_eq!(
        world.get::<Position>(actor).unwrap().to_tuple(),
        start,
        "Wait 不得移动"
    );
    assert!(world.get::<Idle>(actor).is_some());
}

/// C9：`Move` —— 恰好移动一格（玩家路径产出的 payload）。
#[test]
fn parity_move_scenario() {
    let (mut world, player) = player_move_scene((10, 10));
    world.resource_mut::<PlayerActionRequest>().command =
        Some(PlayerCommand::Move { dx: 1, dy: 0 });
    let _ = world.run_system_once(player_action_generation_system);
    let action = action_entities_of(&mut world, player)[0];

    run_one_action_roundtrip(&mut world, action);
    assert_eq!(
        world.get::<Position>(player).unwrap().to_tuple(),
        (11, 10),
        "Move 必须恰好移动一格"
    );
    assert!(world.get::<Idle>(player).is_some());
}

/// C9：`BasicAttack` —— 只发意图、伤害由结算链路落地、目标死亡后 despawn。
#[test]
fn parity_basic_attack_scenario() {
    use crate::events::AttackIntentEvent;
    use bevy_ecs::event::Events;

    let (mut world, player) = player_move_scene((10, 10));
    let monster = spawn_test_monster(
        &mut world,
        crate::monster::MonsterKindId::Goblin,
        (11, 10),
        6.0,
        1.0,
        1.0,
    );
    world.entity_mut(monster).insert(ExperienceReward(10.0));
    crate::system::run_settle_systems(&mut world);
    world.resource_mut::<Events<AttackIntentEvent>>().update();

    world.resource_mut::<PlayerActionRequest>().command =
        Some(PlayerCommand::Move { dx: 1, dy: 0 });
    let _ = world.run_system_once(player_action_generation_system);
    let action = action_entities_of(&mut world, player)[0];
    assert!(world.get::<BasicAttack>(action).is_some());

    // 执行器只发意图。
    let _ = world.run_system_once(tick_action_timers_system);
    let _ = world.run_system_once(execute_basic_attack_system);
    assert_eq!(
        world.resource::<Events<AttackIntentEvent>>().len(),
        1,
        "攻击执行器必须恰好发一个意图"
    );
    assert_eq!(
        world.get::<Health>(monster).unwrap().current,
        6.0,
        "执行器不得自己结算伤害"
    );

    world_run_completion_only(&mut world);
    assert!(world.get_entity(action).is_err());
    assert!(world.get::<Idle>(player).is_some());

    // 结算链路把伤害/死亡/经验走完。
    crate::system::run_settle_systems(&mut world);
    assert!(
        world.get_entity(monster).is_err(),
        "6 血 1 防的怪应当被一击打死并 despawn"
    );
    assert_eq!(
        world.resource::<PendingExp>().amount,
        0.0,
        "经验应当被 apply_exp_system 消费"
    );
    assert!(
        world.get::<Experience>(player).unwrap().exp > 0.0,
        "玩家应当得到经验"
    );
}

/// C9：`Wander` —— 走一步或原地不动，但一定完成。
#[test]
fn parity_wander_scenario() {
    let (mut world, _player) = poc_world();
    world.resource_mut::<Map>().tiles = [[Tile::Floor; MAP_WIDTH]; MAP_HEIGHT];
    let actor = poc_actor(&mut world, (20, 20));
    let start = world.get::<Position>(actor).unwrap().to_tuple();
    let action = world
        .spawn((
            ChildOf(actor),
            ActionPriority(PRIORITY_WANDER),
            ActionSource::Ai,
            ActionTimer { remaining_av: 0.0 },
            Wander,
            ActiveAction,
            Ready,
        ))
        .id();
    world.entity_mut(actor).remove::<Idle>().insert(Active);

    run_one_action_roundtrip(&mut world, action);
    let after = world.get::<Position>(actor).unwrap().to_tuple();
    assert!(
        start.0.abs_diff(after.0) <= 1 && start.1.abs_diff(after.1) <= 1,
        "Wander 只能走一步：{start:?} → {after:?}"
    );
    assert!(world.get::<Idle>(actor).is_some());
}

/// C9：`Chase` —— 可见且不相邻时朝玩家走一步。
#[test]
fn parity_chase_scenario() {
    let player_tile = (40, 40);
    let monster_tile = (34, 40);
    let (mut world, _player, monster) = chase_parity_scene(11, player_tile, monster_tile);
    set_visibility(&mut world, monster, player_tile, true);
    let action = mount_ready_chase(&mut world, monster);

    run_one_action_roundtrip(&mut world, action);
    assert_ne!(
        world.get::<Position>(monster).unwrap().to_tuple(),
        monster_tile,
        "Chase 应当朝玩家走一步"
    );
    assert_eq!(
        world
            .get::<LastKnownPlayerPos>(monster)
            .and_then(|known| known.0),
        Some(player_tile),
        "追击可见时必须更新最后已知位置"
    );
    assert!(world.get::<Idle>(monster).is_some());
}

/// C9：`Flee` —— 低血时逃离玩家。
#[test]
fn parity_flee_scenario() {
    let player_tile = (40, 40);
    let monster_tile = (36, 40);
    let (mut world, _player, monster) =
        flee_parity_scene(51, player_tile, monster_tile, 0.10, true);
    let action = mount_ready_flee(&mut world, monster);

    run_one_action_roundtrip(&mut world, action);
    let after = world.get::<Position>(monster).unwrap().to_tuple();
    assert!(
        Position::new(after.0, after.1).manhattan(Position::new(player_tile.0, player_tile.1))
            > Position::new(monster_tile.0, monster_tile.1)
                .manhattan(Position::new(player_tile.0, player_tile.1)),
        "Flee 必须增大与玩家的距离：{monster_tile:?} → {after:?}"
    );
    assert!(world.get::<Idle>(monster).is_some());
}

/// C9：六个行动共同的不变量——执行后不留 action 子实体、actor 不残留 `Active`、
/// `Idle`/`Failure` 恰好有一个。
///
/// 逐个行动在同一套观察口径下跑一遍（每个用例自己搭好所需的上下文：
/// 攻击要目标、追击要视野、逃跑要低血）。
#[test]
fn parity_all_actions_leave_no_residue() {
    use crate::events::AttackIntentEvent;
    use bevy_ecs::event::Events;

    // 每个元素：行动名 + 搭场景并返回 (world, actor, action)。
    type Case = (&'static str, fn() -> (World, Entity, Entity));

    fn wait_case() -> (World, Entity, Entity) {
        let (mut world, _player) = poc_world();
        let actor = poc_actor(&mut world, (10, 10));
        let action = run_wait_candidate(&mut world, actor);
        (world, actor, action)
    }
    fn move_case() -> (World, Entity, Entity) {
        let (mut world, player) = player_move_scene((10, 10));
        world.resource_mut::<PlayerActionRequest>().command =
            Some(PlayerCommand::Move { dx: 1, dy: 0 });
        let _ = world.run_system_once(player_action_generation_system);
        let action = action_entities_of(&mut world, player)[0];
        (world, player, action)
    }
    fn attack_case() -> (World, Entity, Entity) {
        let (mut world, player) = player_move_scene((10, 10));
        spawn_test_monster(
            &mut world,
            crate::monster::MonsterKindId::Goblin,
            (11, 10),
            30.0,
            1.0,
            1.0,
        );
        crate::system::run_settle_systems(&mut world);
        world.resource_mut::<Events<AttackIntentEvent>>().update();
        world.resource_mut::<PlayerActionRequest>().command =
            Some(PlayerCommand::Move { dx: 1, dy: 0 });
        let _ = world.run_system_once(player_action_generation_system);
        let action = action_entities_of(&mut world, player)[0];
        (world, player, action)
    }
    fn wander_case() -> (World, Entity, Entity) {
        let (mut world, _player) = poc_world();
        world.resource_mut::<Map>().tiles = [[Tile::Floor; MAP_WIDTH]; MAP_HEIGHT];
        let actor = poc_actor(&mut world, (20, 20));
        let action = world
            .spawn((
                ChildOf(actor),
                ActionPriority(PRIORITY_WANDER),
                ActionTimer { remaining_av: 0.0 },
                Wander,
                ActiveAction,
                Ready,
            ))
            .id();
        world.entity_mut(actor).remove::<Idle>().insert(Active);
        (world, actor, action)
    }
    fn chase_case() -> (World, Entity, Entity) {
        let (mut world, _player, monster) = chase_parity_scene(11, (40, 40), (34, 40));
        set_visibility(&mut world, monster, (40, 40), true);
        let action = mount_ready_chase(&mut world, monster);
        (world, monster, action)
    }
    fn flee_case() -> (World, Entity, Entity) {
        let (mut world, _player, monster) = flee_parity_scene(51, (40, 40), (36, 40), 0.10, true);
        let action = mount_ready_flee(&mut world, monster);
        (world, monster, action)
    }

    let cases: [Case; 6] = [
        ("Wait", wait_case),
        ("Move", move_case),
        ("BasicAttack", attack_case),
        ("Wander", wander_case),
        ("Chase", chase_case),
        ("Flee", flee_case),
    ];

    for (name, build) in cases {
        let (mut world, actor, action) = build();

        // 执行器必须消费 Ready（否则同一行动会被反复执行）。
        run_one_action_roundtrip(&mut world, action);

        // 共同不变量。
        assert!(
            world.get_entity(action).is_err(),
            "情形「{name}」：action 实体必须被回收"
        );
        assert!(
            world.get::<Active>(actor).is_none(),
            "情形「{name}」：actor 不得残留 Active"
        );
        let idle = world.get::<Idle>(actor).is_some();
        let failure = world.get::<Failure>(actor).is_some();
        assert!(
            idle ^ failure,
            "情形「{name}」：Idle/Failure 必须恰好有一个（I91），实际 idle={idle} failure={failure}"
        );
        assert!(
            action_entities_of(&mut world, actor).is_empty(),
            "情形「{name}」：不得给 actor 留子实体"
        );
    }
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
