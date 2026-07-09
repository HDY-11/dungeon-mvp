//! 游戏循环编排（Schedule 并行版本）

use dungeon_core::ops;
use bevy_ecs::prelude::*;
use dungeon_action::{
    advance_until_player_acted,
    chase_decision_system, flee_decision_system,
    wander_decision_system, arbitration_system,
};
use dungeon_core::systems::{fov_system, check_death_system};

/// 构建并行调度器（每帧调用 — 开销 <1μs，测试兼容各 World）
fn build_parallel_schedule() -> Schedule {
    let mut schedule = Schedule::default();
    schedule.add_systems((
        chase_decision_system,
        flee_decision_system,
        wander_decision_system,
    ));
    schedule.add_systems(arbitration_system
        .after(chase_decision_system)
        .after(flee_decision_system)
        .after(wander_decision_system));
    schedule.add_systems((
        fov_system,
        check_death_system,
    ));
    schedule
}

/// 使用并行 Schedule 的 tick
pub fn advance_and_settle_parallel(world: &mut World) {
    advance_until_player_acted(world);  // 内部每 action 后已 rebuild_occupancy

    {
        let mut schedule = build_parallel_schedule();
        schedule.run(world);  // 调度器只产生意图，不改实体位置
    }

    // 碰撞图已在 advance_action_queue 中重建，此处不再重复
    ops::update_map_memory(world);
    ops::update_visible_memory(world);
}


