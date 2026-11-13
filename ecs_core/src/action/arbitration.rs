//! 仲裁与 tick：把候选收敛为每个 actor 的唯一 `ActiveAction`，并推进计时器。

use bevy_ecs::prelude::*;
use bevy_ecs::entity::{EntityHashMap, EntityHashSet};

use crate::action::entity::{ActionPriority, ActiveAction, Candidate, begin_action};
use crate::action::ownership::ActionOf;
use crate::components::{ActionTimer, Ready};

/// 仲裁候选查询的别名：(候选实体, 优先级, child_of)，只取未激活的候选。
type Candidates<'w, 's> = Query<'w, 's, (Entity, &'static ActionPriority, &'static ActionOf), (With<Candidate>, Without<ActiveAction>)>;

// ── 仲裁系统 ─────────────────────────────────────────

/// 每个 actor 至多留下一个 `ActiveAction`，其余候选与落后候选一律 despawn。
///
/// 比较器是 `(ActionPriority, action_entity.index_u32())` 的**全序**：不含随机数。
/// 同优先级时实体编号小者胜出，保证结果可复现（REFACTOR.md §3.4）。
///
/// 注意：bevy 0.20 的 `Entity::to_bits()` 用 NonMax 编码（数值 = u32::MAX − index），
/// 直接按 bits 排序会把生成顺序反过来，因此这里用 `index_u32()`。
///
/// # 发命令的顺序也必须确定（Phase H11 后的确定性修补）
///
/// 两个循环（授予、回收）都**不能**用 `EntityHashMap` 的迭代顺序：
/// `EntityHashMap` 的 `RandomState` 每进程种子不同，所以同一世界在两次进程里
/// 迭代顺序可能不同 → 命令发出顺序不同 → `insert(ActiveAction)` 造成的 **archetype 创建
/// 顺序**不同 → 之后所有系统对这些 archetype 的**查询迭代顺序**不同 → 若有系统在迭代中
/// x，随机流的分配就变了。
/// 回收循环同理：`despawn` 顺序决定实体 id 的复用顺序，从而影响未来行动的
/// `index_u32()` 分配顺序（它正是仲裁的平局打破键）。
///
/// 最终**状态**本身与顺序无关（每条命令作用在不同实体上），所以这类洞在单进程内、
/// 在现有测试里都不会显形——只在跨进程对比同一颗种子时才看得见。
pub fn action_arbitration_system(
    mut commands: Commands,
    candidates: Candidates<'_, '_>,
    active_actions: Query<&ActionOf, With<ActiveAction>>,
) {
    // 已经持有 `ActiveAction` 的 actor：本轮不得再被授予行动。
    // （候选查询里的 `Without<ActiveAction>` 只保证候选自身未激活，挡不住这种情况。）
    let busy_actors: EntityHashSet = active_actions.iter().map(ActionOf::parent).collect();

    // 每个 actor 选一个赢家：(priority, index) 最小者。
    let mut winners: EntityHashMap<(Entity, i32, u32)> = EntityHashMap::new();
    let mut losers: Vec<Entity> = Vec::new();

    for (action, priority, child_of) in &candidates {
        let actor = child_of.parent();
        if busy_actors.contains(&actor) {
            losers.push(action);
            continue;
        }
        let index = action.index_u32();
        match winners.get(&actor) {
            Some((_, best_priority, best_index))
                if (*best_priority, *best_index) <= (priority.0, index) =>
            {
                losers.push(action);
            }
            Some((previous, ..)) => {
                losers.push(*previous);
                winners.insert(actor, (action, priority.0, index));
            }
            None => {
                winners.insert(actor, (action, priority.0, index));
            }
        }
    }

    for (actor, winner) in ordered_grants(&winners) {
        commands.entity(winner).remove::<Candidate>().insert(ActiveAction);
        let mut actor_cmd = commands.entity(actor);
        begin_action(&mut actor_cmd);
    }
    // 回收顺序同样显式化：见函数文档（despawn 顺序影响实体 id 复用 → 影响 index_u32 分配）。
    losers.sort_unstable_by_key(|action| action.index_u32());
    for loser in losers {
        commands.entity(loser).despawn();
    }
}

/// 授予顺序：按 `(priority, action index)` **升序**——与仲裁键同序，且与哈希迭代顺序无关。
///
/// 抽成独立函数只为一件事：让"顺序规则"能被单测钉住。系统内联排序的话，
/// "这里要不要排序"就只剩注释在守（而注释不会失败）。
pub(crate) fn ordered_grants(
    winners: &EntityHashMap<(Entity, i32, u32)>,
) -> Vec<(Entity, Entity)> {
    let mut grants: Vec<(Entity, Entity, i32, u32)> = winners
        .iter()
        .map(|(actor, (action, priority, index))| (*actor, *action, *priority, *index))
        .collect();
    // index 唯一，所以这是严格全序（`sort_unstable` 的稳定性不构成依赖）。
    grants.sort_unstable_by_key(|&(_, _, priority, index)| (priority, index));
    grants
        .into_iter()
        .map(|(actor, action, ..)| (actor, action))
        .collect()
}

// ── Tick ─────────────────────────────────────────────

/// 本轮要推进的最小正 AV（只统计 `ActiveAction`）。
///
/// 这是推进世界时钟的唯一来源（旧 Phase A 的同类函数已随旧链路删除）。
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

    // 把"这一轮推进了多少 AV"记到世界时钟上：状态效果按 AV 计时（ISSUES ECS46），
    // 不能按"结算了多少轮"计——一次玩家行动会跨好几个世界轮。
    if let Some(mut clock) = world.get_resource_mut::<crate::resources::WorldClock>() {
        clock.last_step_av = delta;
        clock.total_av += delta;
    }

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
