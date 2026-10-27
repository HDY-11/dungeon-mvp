//! 结算系统：伤害、死亡、经验、视野、记忆、占用图，以及 Schedule 组装。
//!
//! 每个子模块只负责一条链路，`mod.rs` 只做模块声明与 Schedule 组装：
//!
//! ```text
//! action::entity::execute_basic_attack_system   ← 行动层只声明攻击意图
//!     ↓ AttackIntentEvent
//! combat::resolve_attack_system                 ← 算伤害（攻击/防御/暴击）
//!     ↓ AttackEvent
//! combat::apply_damage_system                   ← 扣血
//!     ↓ Health 归零
//! death::check_death_system                     ← 发 DeathEvent、despawn 实体
//!     ↓ DeathEvent { reward }
//! experience::apply_exp_system                  ← 发经验、升级、重算上限
//!     ↓
//! perception::{fov, update_map_memory, update_visible_memory}
//! occupancy::rebuild_occupancy_system
//! update_events_system                          ← 必须最后：交换事件双缓冲
//! ```
//!
//! 顺序由 [`build_core_schedule`] 的 `.chain()` 固定，改顺序前先想清楚事件
//! 缓冲的生命周期（I90：每轮必须 `update()`，否则下一轮会重读历史事件）。

pub mod combat;
pub mod death;
pub mod experience;
pub mod occupancy;
pub mod perception;

// 子模块的公共项在此重新导出：`lib.rs` 以 `pub use system::*` 暴露公共面，
// 而这些系统历史上都定义在 `system` 这一层。重导出保持
// `crate::system::fov_system` 这类调用路径不变（`world/init.rs` 等既有调用点
// 无需改动），也让「系统属于哪条链路」这件事在目录结构上可见。
pub use combat::{apply_damage_system, resolve_attack_system};
pub use death::check_death_system;
pub use experience::apply_exp_system;
pub use occupancy::rebuild_occupancy_system;
pub use perception::{fov_system, update_map_memory_system, update_visible_memory_system};

use crate::events::{
    ActionFailedEvent, ActionSucceededEvent, AttackEvent, AttackIntentEvent, DeathEvent,
    LevelUpEvent,
};
use crate::schedule::CoreSettleSchedule;
use bevy_ecs::prelude::*;

/// 每轮结算末尾更新所有事件缓冲。
///
/// 必须在所有 `EventReader` 之后运行：它交换双缓冲并清理旧事件，
/// 防止下一轮重新读取历史事件（I90）。
///
/// **新增事件时要同时加到这里**，否则该事件会跨轮残留。
pub fn update_events_system(
    mut attack_intents: ResMut<Events<AttackIntentEvent>>,
    mut attack_events: ResMut<Events<AttackEvent>>,
    mut death_events: ResMut<Events<DeathEvent>>,
    mut level_up_events: ResMut<Events<LevelUpEvent>>,
    mut action_succeeded: ResMut<Events<ActionSucceededEvent>>,
    mut action_failed: ResMut<Events<ActionFailedEvent>>,
) {
    attack_intents.update();
    attack_events.update();
    death_events.update();
    level_up_events.update();
    action_succeeded.update();
    action_failed.update();
}

/// 构建标准结算 Schedule（标签为 [`CoreSettleSchedule`]）。
///
/// `insert_core_resources` 会把它注册到 `World`；调用方可通过
/// `world.get_schedule_mut(CoreSettleSchedule)` 在前后插入自己的系统。
pub fn build_core_schedule() -> Schedule {
    // `core_schedule`（而不是 `Schedule::new`）：构建时强制无歧义，见 `schedule.rs`。
    let mut schedule = crate::schedule::core_schedule(CoreSettleSchedule);
    schedule.add_systems(
        (
            combat::resolve_attack_system,
            combat::apply_damage_system,
            // 中毒的顺序有语义（见 `effect/status.rs`）：先运转已有中毒，再让本轮的攻击
            // 施加新中毒（新毒下轮才发作），最后由死亡判定收口——否则"被毒死"会晚一轮。
            crate::effect::poison_tick_system,
            crate::effect::poison_apply_system,
            // 拾取放在中毒之后、死亡判定之前：捡到的东西当轮就生效，而"踩在物品上死掉"
            // 的边界不值得单独定义（本轮的物品没有任何死亡相关效果）。
            crate::item::pickup_system,
            death::check_death_system,
            experience::apply_exp_system,
            perception::fov_system,
            perception::update_map_memory_system,
            perception::update_visible_memory_system,
            occupancy::rebuild_occupancy_system,
            // 格索引与占用图并列：都是"每轮重建的派生数据"——没有增量更新，
            // 就没有漏改导致的幽灵条目。顺序无关（两者读的组件不同），
            // 放在占用图旁边只为让人一眼看到"这张图上有两个格索引"。
            crate::effect::rebuild_cell_effects_system,
            update_events_system,
        )
            .chain(),
    );
    schedule
}

/// 直接运行一次核心结算系统。
///
/// Schedule 已由 `insert_core_resources` 注册；这里只按 label 运行，
/// 不重新构建，因此 `EventReader` 游标等系统状态会跨轮保留。
pub fn run_settle_systems(world: &mut World) {
    world.run_schedule(CoreSettleSchedule);
}

// ── 测试 ─────────────────────────────────────────────

#[cfg(test)]
#[path = "system_tests.rs"]
mod tests;