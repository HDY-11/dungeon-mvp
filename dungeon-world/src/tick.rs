//! 游戏循环编排（Schedule 并行版本）
//!
//! 使用 bevy Schedule 自动并行执行互不冲突的 system：
//!   Phase 1 — chase_system || flee_system || wander_system
//!   Phase 2 — arbitration_system（等 Phase 1 完成）
//!   Phase 3 — fov_system || check_death_system || buff_tick_system（等 Phase 2 完成）

use dungeon_core::{
    world, ops,
};
use bevy_ecs::prelude::*;
use bevy_ecs::system::RunSystemOnce;
use dungeon_action::{
    advance_until_player_acted,
    chase_decision_system, flee_decision_system,
    wander_decision_system, arbitration_system,
};
use crate::systems::{fov_system, check_death_system, buff_tick_system};

/// 构建一次、重复使用的并行调度器
fn build_parallel_schedule() -> Schedule {
    use bevy_ecs::schedule::ExecutorKind;
    let mut schedule = Schedule::default();
    schedule.set_executor_kind(ExecutorKind::MultiThreaded);

    // Phase 1 + 2：怪物决策（三个可以并行）
    schedule.add_systems((
        chase_decision_system,   // 读 CanChase/Stats/Viewshed → 写 ChaseIntents
        flee_decision_system,    // 读 CanFlee/Stats → 写 FleeIntents
        wander_decision_system,  // 读 CanWander → 写 WanderIntents
    ));
    // 仲裁等所有决策完成（读三个意图 → 写 ActionQueue）
    schedule.add_systems(arbitration_system
        .after(chase_decision_system)
        .after(flee_decision_system)
        .after(wander_decision_system));

    // Phase 3：世界系统（可以互相并行，但与 Viewshed 读写串行）
    // fov_system 写 Viewshed，chase_decision_system 读 Viewshed → bevy 自动串行
    schedule.add_systems((
        fov_system,
        check_death_system,
        buff_tick_system,
    ));

    schedule
}

/// 使用并行 Schedule 的 tick
pub fn advance_and_settle_parallel() {
    advance_until_player_acted();

    // 用 Schedule 并行运行决策 + 视野 + 死亡检测 + Buff 衰减
    let mut schedule = build_parallel_schedule();
    let mut w = world!(mut);
    schedule.run(&mut w);

    // 以下依赖前面写入的数据，串行执行
    ops::rebuild_occupancy();
    ops::update_map_memory();
    ops::update_visible_memory();
}

/// 旧版串行 tick（保持兼容，供对比）
pub fn advance_and_settle_serial() {
    advance_until_player_acted();
    dungeon_action::run_monster_decision();

    ops::rebuild_occupancy();
    let _ = world!(mut).run_system_once(fov_system);
    ops::update_map_memory();
    ops::update_visible_memory();
    let _ = world!(mut).run_system_once(check_death_system);
    let _ = world!(mut).run_system_once(buff_tick_system);
}
