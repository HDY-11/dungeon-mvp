//! 游戏循环编排（Schedule 并行版本）

use dungeon_core::ops;
use bevy_ecs::prelude::*;
use bevy_ecs::system::RunSystemOnce;
use dungeon_action::{
    advance_until_player_acted,
    chase_decision_system, flee_decision_system,
    wander_decision_system, arbitration_system,
};
use crate::systems::{fov_system, check_death_system, buff_tick_system};

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
        buff_tick_system,
    ));
    schedule
}

/// 使用并行 Schedule 的 tick
pub fn advance_and_settle_parallel(world: &mut World) {
    advance_until_player_acted(world);

    {
        let mut schedule = build_parallel_schedule();
        schedule.run(world);
    }

    ops::rebuild_occupancy(world);
    ops::update_map_memory(world);
    ops::update_visible_memory(world);
}

/// 旧版串行 tick（保持兼容，供对比）
pub fn advance_and_settle_serial(world: &mut World) {
    advance_until_player_acted(world);
    dungeon_action::run_monster_decision(world);

    ops::rebuild_occupancy(world);
    let _ = world.run_system_once(fov_system);
    ops::update_map_memory(world);
    ops::update_visible_memory(world);
    let _ = world.run_system_once(check_death_system);
    let _ = world.run_system_once(buff_tick_system);
}
