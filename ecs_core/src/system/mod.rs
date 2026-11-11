//! 结算系统：伤害、死亡、经验、视野、记忆、占用图，以及 Schedule 组装。
//!
//! 每个子模块只负责一条链路，`mod.rs` 只做模块声明与 Schedule 组装：
//!
//! ```text
//! action::entity::settle_action_system          ← 行动层只声明攻击意图（不判定、不算伤害）
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
    ActionFailedEvent, ActionSucceededEvent, AttackEvent, AttackIntentEvent, DamageTakenEvent,
    DeathEvent, LevelUpEvent, SpellIntentEvent, ThrowIntentEvent,
};
use crate::schedule::CoreSettleSchedule;
use bevy_ecs::prelude::*;
use bevy_ecs::system::SystemParam;

/// 结算末尾要统一 `update()` 的事件缓冲集合。
///
/// 打包成 [`SystemParam`]：每新增一种事件，只需往这里加一个字段，
/// `update_events_system` 的签名不会随事件种类膨胀（否则很快会撞
/// `clippy::too_many_arguments`）。字段与 [`crate::events`] 一一对应。
#[derive(SystemParam)]
pub struct CoreEventBuffers<'w> {
    attack_intents: ResMut<'w, Messages<AttackIntentEvent>>,
    attack_events: ResMut<'w, Messages<AttackEvent>>,
    death_events: ResMut<'w, Messages<DeathEvent>>,
    level_up_events: ResMut<'w, Messages<LevelUpEvent>>,
    action_succeeded: ResMut<'w, Messages<ActionSucceededEvent>>,
    action_failed: ResMut<'w, Messages<ActionFailedEvent>>,
    spell_intents: ResMut<'w, Messages<SpellIntentEvent>>,
    throw_intents: ResMut<'w, Messages<ThrowIntentEvent>>,
    damage_taken: ResMut<'w, Messages<DamageTakenEvent>>,
}

/// 每轮结算末尾更新所有事件缓冲。
///
/// 必须在所有 `MessageReader` 之后运行：它交换双缓冲并清理旧事件，
/// 防止下一轮重新读取历史事件（I90）。
///
/// **新增事件时要同时加到 [`CoreEventBuffers`] 里**，否则该事件会跨轮残留。
pub fn update_events_system(mut events: CoreEventBuffers) {
    events.attack_intents.update();
    events.attack_events.update();
    events.death_events.update();
    events.level_up_events.update();
    events.action_succeeded.update();
    events.action_failed.update();
    events.spell_intents.update();
    events.throw_intents.update();
    events.damage_taken.update();
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
            (
                combat::resolve_attack_system,
                combat::apply_damage_system,
                // 法术（DsnE26）：settle 只发 SpellIntentEvent，效果在这里落地。
                // 放在 apply_damage_system 之后、中毒之前：法术伤害先扣血，
                // 本轮的死亡判定照常收口；法术不参与"中毒"这条链。
                crate::spell::resolve_spell_system,
                // 投掷（Gm9）：settle 只发 ThrowIntentEvent；伤害在这里落地，消耗系统
                // 随后从副手扣 1。放在死亡判定之前，投掷击杀能在本轮收口。
                crate::throw::resolve_throw_system,
                crate::throw::consume_thrown_item_system,
                // 效果时长（DsnE12 第 4 条）：先统一推进所有 `EffectDuration`；payload 系统
                // （如中毒 tick）读到的剩余时长必须是"这一轮已经走完"的值，否则发作晚一轮。
                crate::effect::advance_effect_durations_system,
                // 中毒的顺序有语义（见 `effect/status.rs`）：先运转已有中毒，再让本轮的攻击
                // 施加新中毒（新毒下轮才发作），最后由死亡判定收口——否则"被毒死"会晚一轮。
                crate::effect::poison_tick_system,
                crate::effect::poison_apply_system,
                // 到期回收必须在所有 payload 结算**之后**：否则"时长归零那一轮"的最后一次
                // 发作会被回收吞掉。它是按时间终结效果实体的唯一入口（DsnE12 第 4 条）。
                crate::effect::expire_effects_system,
                // 拾取**不在结算链里**：踩上物品不会自动捡，玩家在背包页「地面」栏
                // 按 Enter 调 `item::take_from_ground`（DsnE21 阶段 4）。结算链只留
                // 与回合推进相关的系统。
                death::check_death_system,
                experience::apply_exp_system,
                // 等级变化后重算生命/法力上限（DsnE23 的唯一上限公式）。
                // 放在升级之后、其它系统之前：本轮的观测值（HUD）已经是新上限。
                crate::rules::refresh_vitals_system,
                // 法力按 AV 回复（DsnE25）：放在上限刷新之后，保证本轮回的是新上限下的蓝。
                crate::rules::mana_regen_system,
                perception::fov_system,
                perception::update_map_memory_system,
                perception::update_visible_memory_system,
                occupancy::rebuild_occupancy_system,
                // 格索引与占用图并列：都是"每轮重建的派生数据"——没有增量更新，
                // 就没有漏改导致的幽灵条目。顺序无关（两者读的组件不同），
                // 放在占用图旁边只为让人一眼看到"这张图上有两个格索引"。
                crate::effect::rebuild_cell_effects_system,
                // 治疗泉（DsnE25）：必须在格索引重建之后——这一轮玩家可能刚走进泉眼，
                // 索引要先用当前位置重建，才能按格找到它。
                crate::effect::heal_spring_system,
                // 仇恨（DsnE29）：所有伤害来源都已发 `DamageTakenEvent`——近战在
                // `apply_damage_system`、法术/投掷分别在各自 resolve 里；这里收口成
                // 威胁表并按本轮 AV 衰减。与 update_events 分开是因为"一轮结束"的
                // 事件缓冲更新必须最后做，而仇恨只读本轮事件。
                crate::monster::threat_system,
            )
                .chain(),
            // 事件缓冲必须在一轮所有系统之后统一更新（否则跨轮残留）。
            update_events_system,
        )
            .chain(),
    );
    schedule
}

/// 直接运行一次核心结算系统。
///
/// Schedule 已由 `insert_core_resources` 注册；这里只按 label 运行，
/// 不重新构建，因此 `MessageReader` 游标等系统状态会跨轮保留。
pub fn run_settle_systems(world: &mut World) {
    world.run_schedule(CoreSettleSchedule);
}

// ── 测试 ─────────────────────────────────────────────

#[cfg(test)]
#[path = "system_tests.rs"]
mod tests;