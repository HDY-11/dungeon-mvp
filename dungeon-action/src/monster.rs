//! 怪物决策 — 并行 System 版本
//!
//! 三个独立 system 分别检查追击/逃跑/游荡条件，
//! 各自写入意图缓冲区 → arbitration_system 合并入 ActionQueue。
//! bevy 调度器自动并行执行互不冲突的 system。

use crate::types::*;
use dungeon_core::{
    components::*,
};
use bevy_ecs::prelude::*;
use bevy_ecs::system::RunSystemOnce;

/// 追击决策：有 CanChase 的怪物是否看到玩家
pub fn chase_decision_system(
    player: Query<&Position, With<Player>>,
    monsters: Query<(Entity, &CanChase, &Stats, &Viewshed, &Reaction)>,
    mut out: ResMut<ChaseIntents>,
) {
    out.0.clear();
    let player_pos = player.iter().next().map(|p| (p.x, p.y));
    for (entity, chase, stats, view, _reaction) in &monsters {
        let can_see = player_pos.map_or(false, |pp| view.visible_tiles.contains(&pp));
        if CanChase::condition(can_see) {
            let av = agility_to_reaction(stats.agility) + chase.duration * agility_speed_factor(stats.agility);
            out.0.push((entity, chase.priority, av, ActionKindV3::Chase));
        }
    }
}

/// 逃跑决策：有 CanFlee 的怪物 HP 是否低于 25%
pub fn flee_decision_system(
    monsters: Query<(Entity, &CanFlee, &Stats, &Reaction)>,
    mut out: ResMut<FleeIntents>,
) {
    out.0.clear();
    for (entity, flee, stats, _reaction) in &monsters {
        let hp_ratio = stats.hp as f32 / stats.max_hp as f32;
        if CanFlee::condition(hp_ratio) {
            let av = agility_to_reaction(stats.agility) + flee.duration * agility_speed_factor(stats.agility);
            out.0.push((entity, flee.priority, av, ActionKindV3::Flee));
        }
    }
}

/// 游荡决策：有 CanWander 且尚未决定行动的怪物
pub fn wander_decision_system(
    monsters: Query<(Entity, &CanWander, &Stats, &Reaction)>,
    chase_out: Res<ChaseIntents>,
    flee_out: Res<FleeIntents>,
    mut out: ResMut<WanderIntents>,
) {
    out.0.clear();
    // 已有追击/逃跑意图的实体，不再纳入游荡
    let already_decided: Vec<Entity> = chase_out.0.iter().chain(flee_out.0.iter()).map(|(e, _, _, _)| *e).collect();
    for (entity, wander, stats, _reaction) in &monsters {
        if !already_decided.contains(&entity) && CanWander::condition() {
            let av = agility_to_reaction(stats.agility) + wander.duration * agility_speed_factor(stats.agility);
            out.0.push((entity, wander.priority, av, ActionKindV3::Wander));
        }
    }
}

/// 仲裁：合并所有意图，按优先级排序，入队 ActionQueue
pub fn arbitration_system(
    chase_out: Res<ChaseIntents>,
    flee_out: Res<FleeIntents>,
    wander_out: Res<WanderIntents>,
    mut queue: ResMut<ActionQueue>,
) {
    let mut all: Vec<(Entity, u32, f32, ActionKindV3)> = Vec::new();
    all.extend(chase_out.0.iter().cloned());
    all.extend(flee_out.0.iter().cloned());
    all.extend(wander_out.0.iter().cloned());

    // 按 priority 降序，相同 priority 保持插入顺序（stable sort）。移除随机 tiebreaker 避免违反全序契约
    all.sort_by(|(_, pa, _, _), (_, pb, _, _)| pb.cmp(pa));

    for (entity, _priority, av, kind) in &all {
        if !queue.has_entity(*entity) {
            queue.enqueue(*entity, kind.clone(), *av);
        }
    }
}

/// 向后兼容包装：顺序执行四个决策 system（非并行）
pub fn run_monster_decision(world: &mut World) {
    let _ = world.run_system_once(chase_decision_system);
    let _ = world.run_system_once(flee_decision_system);
    let _ = world.run_system_once(wander_decision_system);
    let _ = world.run_system_once(arbitration_system);
}
