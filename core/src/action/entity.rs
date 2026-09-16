//! 行动实体 PoC（REFACTOR.md §11.3 Phase B；目标设计见 §3.6 / DESIGN Dsn27）。
//!
//! **一个行动 = 一个 actor 的瞬态子实体**，取代中央分派的 `ActionKind`：
//!
//! ```text
//! Generation（每个行为一个生成系统，只 spawn 候选，不碰 actor 状态）
//!     ↓ ApplyDeferred
//! Arbitration（唯一写入 actor 行动状态的系统）
//!     ↓
//! Tick（推进 ActiveAction 的 ActionTimer，归零加 Ready）
//!     ↓
//! Execution（每个行动一个专用 query 系统，零中央 match；只发事件）
//!     ↓ ActionSucceeded / ActionFailedEvent
//! Completion（唯一 despawn action 实体 + 回转 actor `Idle` / `Failure` 的系统）
//! ```
//!
//! # 本阶段范围（Phase B）
//!
//! - 只证明链路可行：`Wander`（AI 侧）+ `Move`（payload 侧）走完整链路；
//! - **不接主循环**：[`build_action_poc_schedule`] 只在测试里使用；
//!   `world/loop_.rs` 仍走旧的 `decide_monster_actions` + `mount_action`；
//! - **不删旧系统**：`ActionKind` / `mount_action` / `choose_action` 留到 Phase C；
//! - 死抽象清单（§10.8 / A43）在 Phase B 期间一律不动。
//!
//! # 与 actor 组件模型的关系
//!
//! `Can*` 仍是 actor 上的 ZST 组件（回答“能不能做”）；action 实体只回答
//! “正在考虑/执行什么”。actor 的 `Idle`/`Active`/`Failure` 仍是行动状态，
//! 但**只有仲裁与 completion 可以写**：生成系统一律不碰。
//!
//! # 不存档
//!
//! action 实体是瞬态子实体；存档只存 actor 状态，读档后重新生成行动
//! （见 REFACTOR.md §10.7）。

use crate::balance::{FLEE_DURATION, FLEE_HP_RATIO, WAIT_DURATION, WANDER_DURATION, action_av};
use crate::components::{
    ActionTimer, Active, Agility, CanFlee, CanWait, CanWander, Failure, Flee, Health, Idle, Move,
    Position, Ready, Wait, Wander,
};
use crate::entity_cls::Monster;
use crate::events::{ActionFailedEvent, ActionSucceededEvent};
use crate::map::Map;
use crate::resources::{GameRng, OccupancyMap};
use crate::schedule::ActionPocSchedule;
use bevy_ecs::prelude::*;
use bevy_ecs::query::Or;
use std::collections::{HashMap, HashSet};

// ── 优先级表（REFACTOR.md §3.5） ──────────────────────
//
// 仲裁取**最小** `(ActionPriority, to_bits())`，所以优先级越高 = 数值越小。
// 与 §3.5 一一对应：Flee 200 > Chase 100 > Wander 50 > Wait 0。

pub const PRIORITY_FLEE: i32 = -200;
pub const PRIORITY_CHASE: i32 = -100;
pub const PRIORITY_WANDER: i32 = -50;
pub const PRIORITY_WAIT: i32 = 0;

// ── action 实体组件 ──────────────────────────────────

/// 仲裁排序键。**越小越优先**；同值由 `action_entity.to_bits()` 升序打破平局。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ActionPriority(pub i32);

/// 行动来源。玩家行动不参与 AI 仲裁（Phase C 才做），这里先只用于调试/日志。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionSource {
    Ai,
    Player,
}

/// 生成系统产出、等待仲裁的候选。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Candidate;

/// 仲裁选中、正在计时/执行的行动。一个 actor 至多一个。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ActiveAction;

/// 人类可读的行动名，只用于日志（§3.6.9 调试建议）。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionName(pub &'static str);

// ── 生成系统 ─────────────────────────────────────────
//
// 生成系统只 spawn 候选：不写 actor 的 `Idle/Active/Failure`，不写计时器。

/// 游荡候选：空闲/失败且具 `CanWander` 的怪物各产出一个 `Wander` 候选。
pub fn wander_generation_system(
    mut commands: Commands,
    actors: Query<
        (Entity, &Agility),
        (
            With<Monster>,
            With<CanWander>,
            Without<Active>,
            Or<(With<Idle>, With<Failure>)>,
        ),
    >,
) {
    for (actor, agility) in &actors {
        commands.spawn((
            ChildOf(actor),
            ActionPriority(PRIORITY_WANDER),
            ActionSource::Ai,
            ActionName("Wander"),
            ActionTimer {
                remaining_av: action_av(WANDER_DURATION, agility.0),
            },
            Wander,
            Candidate,
        ));
    }
}

/// 逃跑候选：生命占比低于进入阈值且具 `CanFlee`。
///
/// 存在意义有两个：让仲裁真的需要比较优先级（Flee 200 > Wander 50），
/// 以及给 §3.6.4「每个行为一个生成系统」提供第二个样本。
pub fn flee_generation_system(
    mut commands: Commands,
    actors: Query<
        (Entity, &Agility, &Health),
        (
            With<Monster>,
            With<CanFlee>,
            Without<Active>,
            Or<(With<Idle>, With<Failure>)>,
        ),
    >,
) {
    for (actor, agility, health) in &actors {
        if health.ratio() >= FLEE_HP_RATIO {
            continue;
        }
        commands.spawn((
            ChildOf(actor),
            ActionPriority(PRIORITY_FLEE),
            ActionSource::Ai,
            ActionName("Flee"),
            ActionTimer {
                remaining_av: action_av(FLEE_DURATION, agility.0),
            },
            Flee,
            Candidate,
        ));
    }
}

/// 等待候选（C1）：兜底行为，任何空闲且具 `CanWait` 的 actor 都会产出一个 `Wait`。
///
/// 与 `Wander` / `Flee` 的差别：**永远有候选**，因此仲裁必然有结果，
/// 有能力的 actor 不会出现「没有任何行动」的空转（§3.3 的兜底语义）。
pub fn wait_generation_system(
    mut commands: Commands,
    actors: Query<
        (Entity, &Agility),
        (
            With<Monster>,
            With<CanWait>,
            Without<Active>,
            Or<(With<Idle>, With<Failure>)>,
        ),
    >,
) {
    for (actor, agility) in &actors {
        commands.spawn((
            ChildOf(actor),
            ActionPriority(PRIORITY_WAIT),
            ActionSource::Ai,
            ActionName("Wait"),
            ActionTimer {
                remaining_av: action_av(WAIT_DURATION, agility.0),
            },
            Wait,
            Candidate,
        ));
    }
}

// ── 仲裁系统 ─────────────────────────────────────────

/// 每个 actor 至多留下一个 `ActiveAction`，其余候选与落后候选一律 despawn。
///
/// 比较器是 `(ActionPriority, action_entity.to_bits())` 的**全序**：不含随机数。
/// 同优先级时 entity bits 小者胜出，保证结果可复现（REFACTOR.md §3.4）。
pub fn action_arbitration_system(
    mut commands: Commands,
    candidates: Query<
        (Entity, &ActionPriority, &ChildOf),
        (With<Candidate>, Without<ActiveAction>),
    >,
    active_actions: Query<&ChildOf, With<ActiveAction>>,
) {
    // 已经持有 `ActiveAction` 的 actor：本轮不得再被授予行动。
    // （候选查询里的 `Without<ActiveAction>` 只保证候选自身未激活，挡不住这种情况。）
    let busy_actors: HashSet<Entity> = active_actions.iter().map(ChildOf::parent).collect();

    // 每个 actor 选一个赢家：(priority, bits) 最小者。
    let mut winners: HashMap<Entity, (Entity, i32, u64)> = HashMap::new();
    let mut losers: Vec<Entity> = Vec::new();

    for (action, priority, child_of) in &candidates {
        let actor = child_of.parent();
        if busy_actors.contains(&actor) {
            losers.push(action);
            continue;
        }
        let bits = action.to_bits();
        match winners.get(&actor) {
            Some((_, best_priority, best_bits))
                if (*best_priority, *best_bits) <= (priority.0, bits) =>
            {
                losers.push(action);
            }
            Some((previous, ..)) => {
                losers.push(*previous);
                winners.insert(actor, (action, priority.0, bits));
            }
            None => {
                winners.insert(actor, (action, priority.0, bits));
            }
        }
    }

    for (actor, (winner, ..)) in &winners {
        commands
            .entity(*winner)
            .remove::<Candidate>()
            .insert(ActiveAction);
        let mut actor_cmd = commands.entity(*actor);
        actor_cmd.remove::<Idle>();
        actor_cmd.remove::<Failure>();
        actor_cmd.insert(Active);
    }
    for loser in losers {
        commands.entity(loser).despawn();
    }
}

// ── Tick ─────────────────────────────────────────────

/// 本轮要推进的最小正 AV（只统计 `ActiveAction`）。
///
/// 与 Phase A 的 `execution::positive_timer_delta` 同源；Phase C 会用本函数替换它。
pub fn active_action_timer_delta(world: &mut World) -> f64 {
    let mut query = world.query_filtered::<&ActionTimer, With<ActiveAction>>();
    query
        .iter(world)
        .map(|timer| timer.remaining_av)
        .filter(|remaining| *remaining > 0.0)
        .min_by(|a, b| a.partial_cmp(b).expect("ActionTimer must not be NaN"))
        .unwrap_or(0.0)
}

/// 推进所有 `ActiveAction` 的计时器；归零者加 `Ready`（执行门禁）。
pub fn tick_action_timers_system(world: &mut World) {
    let delta = active_action_timer_delta(world);

    let mut ready_actions = Vec::new();
    {
        let mut query = world.query_filtered::<(Entity, &mut ActionTimer), With<ActiveAction>>();
        for (action, mut timer) in query.iter_mut(world) {
            if delta > 0.0 && timer.remaining_av > 0.0 {
                timer.remaining_av = (timer.remaining_av - delta).max(0.0);
            }
            if timer.remaining_av <= 0.0 {
                ready_actions.push(action);
            }
        }
    }

    for action in ready_actions {
        world.entity_mut(action).insert(Ready);
    }
}

// ── 执行系统 ─────────────────────────────────────────
//
// 每个行动一个专用 query，执行层零中央 match。执行系统只发事件：
// **不 despawn action 实体、不写 actor 的 `Idle`/`Failure`**——那是 completion 的事。

/// 执行到期的 `Wander`：随机选一个 8 方向走一步；被挡/越界则原地不动。
///
/// 与旧 `execution::execute_wander_system` 行为一致：随机方向**照常消耗随机数**
/// （保证 RNG 步数与旧实现同序），只有合法时才改 `Position`；被挡也算行动完成
/// （游荡本来就是随机试探）。
pub fn execute_wander_system(
    mut commands: Commands,
    mut rng: ResMut<GameRng>,
    actions: Query<(Entity, &ChildOf), (With<ActiveAction>, With<Ready>, With<Wander>)>,
    actors: Query<&Position>,
    map: Res<Map>,
    occupancy: Res<OccupancyMap>,
    mut succeeded: EventWriter<ActionSucceededEvent>,
    mut failed: EventWriter<ActionFailedEvent>,
) {
    use crate::action::execution::movement::moved_position;

    const DIRECTIONS: [(isize, isize); 8] = [
        (0, -1),
        (0, 1),
        (-1, 0),
        (1, 0),
        (-1, -1),
        (1, -1),
        (-1, 1),
        (1, 1),
    ];

    for (action, child_of) in &actions {
        let actor = child_of.parent();
        let Ok(position) = actors.get(actor) else {
            // actor 已消失：清掉残留 action 实体并结束行动。
            commands.entity(action).despawn();
            failed.write(ActionFailedEvent { entity: actor });
            continue;
        };

        let index =
            (rng.random_range(0, DIRECTIONS.len() as u64) as usize).min(DIRECTIONS.len() - 1);
        let (dx, dy) = DIRECTIONS[index];

        match moved_position(&map, &occupancy, *position, dx, dy) {
            Some(next) => {
                commands.entity(actor).insert(Position::new(next.x, next.y));
                log::debug!(
                    "PoC 游荡: actor={actor:?} 方向=({dx},{dy}) → ({},{})",
                    next.x,
                    next.y
                );
            }
            None => {
                log::debug!("PoC 游荡: actor={actor:?} 方向=({dx},{dy}) 被挡，原地不动");
            }
        }
        succeeded.write(ActionSucceededEvent { entity: actor });
    }
}

/// 执行到期的 `Wait`（C1）：什么都不做，直接算完成。
///
/// 等价旧 `execution::execute_wait_system` 的语义：**无条件成功**——
/// 回合照常推进（AV 已经付过），actor 回到 `Idle`。
///
/// 这里刻意不 `.remove::<Ready>()`：completion 会 `despawn` 整个 action 实体，
/// 它上面的 `Ready` 随之消失；而「执行器不得回收实体」这条分层约束
/// （见 [`execute_move_system`]）在 `Wait` 上同样成立。
pub fn execute_wait_system(
    actions: Query<(Entity, &ChildOf), (With<ActiveAction>, With<Ready>, With<Wait>)>,
    mut succeeded: EventWriter<ActionSucceededEvent>,
) {
    for (action, child_of) in &actions {
        let actor = child_of.parent();
        log::debug!("PoC 等待: actor={actor:?}（action={action:?}）");
        succeeded.write(ActionSucceededEvent { entity: actor });
    }
}

/// 执行到期的 `Move { dx, dy }`：**参数化普通系统**（不再独占 `&mut World`）。
///
/// 为什么这样写是安全的（Phase B 时这里曾是 exclusive，属过度保守）：
///
/// - 驱动实体是 **action 实体**（`ActiveAction + Ready + Move` 都在它身上），
///   `ChildOf` 只作**读**，用来拿 actor id；
/// - 被写的是**另一个实体**的 `Position`，因此 `Query<&mut Position>` 的
///   per-entity 唯一可变访问没有被违反；
/// - 规则由纯函数 [`crate::action::execution::movement::moved_position`] 提供，
///   不需要 `World` 句柄。
///
/// 之前必须 exclusive，唯一原因是复用了 `movement::execute_move(&mut World, ...)`
/// ——那个签名把「读资源 + 读组件 + 写组件」揉进一次 `&mut World` 调用。
/// 抽取纯规则后这个约束自动消失，A41 的边界随之收窄。
pub fn execute_move_system(
    mut commands: Commands,
    mut actions: Query<(Entity, &ChildOf, &Move), (With<ActiveAction>, With<Ready>)>,
    mut actors: Query<&mut Position>,
    map: Res<Map>,
    occupancy: Res<OccupancyMap>,
    mut succeeded: EventWriter<ActionSucceededEvent>,
    mut failed: EventWriter<ActionFailedEvent>,
) {
    use crate::action::execution::movement::moved_position;

    for (action, child_of, action_move) in &mut actions {
        let actor = child_of.parent();
        let Ok(mut position) = actors.get_mut(actor) else {
            // actor 已消失：清掉残留 action 实体并结束行动。
            commands.entity(action).despawn();
            failed.write(ActionFailedEvent { entity: actor });
            continue;
        };

        let moved = moved_position(&map, &occupancy, *position, action_move.dx, action_move.dy);
        match moved {
            Some(next) => {
                position.x = next.x;
                position.y = next.y;
                log::debug!(
                    "PoC 移动: actor={actor:?} 方向=({},{}) → ({},{})",
                    action_move.dx,
                    action_move.dy,
                    next.x,
                    next.y
                );
            }
            None => {
                log::debug!(
                    "PoC 移动: actor={actor:?} 方向=({},{}) 被挡，原地不动",
                    action_move.dx,
                    action_move.dy
                );
            }
        }

        commands.entity(action).remove::<Ready>();
        if moved.is_some() {
            succeeded.write(ActionSucceededEvent { entity: actor });
        } else {
            failed.write(ActionFailedEvent { entity: actor });
        }
    }
}

// ── Completion ───────────────────────────────────────

/// 消费 `ActionSucceeded` / `ActionFailedEvent`：despawn action 实体，actor 回 `Idle`/`Failure`。
///
/// 这是 `ActionSucceeded/FailedEvent` 从零消费者的死事件变成真实状态回转机制的地方
/// （§3.6.6）；它也是唯一允许写 actor `Idle`/`Failure` 的地方之一。
pub fn action_completion_system(
    mut commands: Commands,
    mut succeeded: EventReader<ActionSucceededEvent>,
    mut failed: EventReader<ActionFailedEvent>,
    actions: Query<(Entity, &ChildOf), With<ActiveAction>>,
) {
    let done: Vec<(Entity, bool)> = succeeded
        .read()
        .map(|event| (event.entity, true))
        .chain(failed.read().map(|event| (event.entity, false)))
        .collect();

    for (actor, is_success) in done {
        // 该 actor 的所有 action 子实体：despawn 的实体无法再查询，
        // 因此按“归属 + 仍是 ActiveAction”逐个回收。
        let targets: Vec<Entity> = actions
            .iter()
            .filter(|(_, child_of)| child_of.parent() == actor)
            .map(|(action, _)| action)
            .collect();
        for action in targets {
            commands.entity(action).despawn();
        }

        if let Ok(mut actor_cmd) = commands.get_entity(actor) {
            actor_cmd.remove::<Active>();
            actor_cmd.remove::<ActionTimer>();
            actor_cmd.remove::<Ready>();
            if is_success {
                actor_cmd.insert(Idle);
            } else {
                actor_cmd.insert(Failure);
            }
        }
    }
}

// ── PoC 调度 ─────────────────────────────────────────

/// Phase B 的完整 PoC 调度：生成 → 仲裁 → tick → 执行 → completion。
///
/// 全链路都是**普通系统**：没有 exclusive `&mut World` 系统，因此这条调度可以被
/// 自由组合（见 `entity_tests.rs` 里把它追加进 `CoreSettleSchedule` 的共存测试）。
///
/// **只给测试使用**（`world/loop_.rs` 仍走旧路径）；Phase C 才把它接进主循环并
/// 用真正的 `CoreSettleSchedule` 前后置系统替换这里的顺序。
pub fn build_action_poc_schedule() -> Schedule {
    let mut schedule = Schedule::new(ActionPocSchedule);
    schedule.add_systems(
        (
            (
                wait_generation_system,
                wander_generation_system,
                flee_generation_system,
            )
                .chain(),
            ApplyDeferred,
            action_arbitration_system,
            ApplyDeferred,
            tick_action_timers_system,
            execute_wait_system,
            execute_move_system,
            execute_wander_system,
            ApplyDeferred,
            action_completion_system,
        )
            .chain(),
    );
    schedule
}

// ── 测试 ─────────────────────────────────────────────

#[cfg(test)]
#[path = "entity_tests.rs"]
mod tests;
