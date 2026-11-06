//! 近战规则：相邻判定与目标存活。
//!
//! 这里只有**纯函数**：不碰 `World`、不发事件、不改状态。结算链路是
//! `AttackIntentEvent -> resolve_attack_system`（`system/combat.rs`）；
//! 「能不能打」由校验器 `action::validation::validate_basic_attack_system` 来问。
//! 两者共用本模块的规则，因此「能不能打」与「打出多少」各只有一份实现。
//!
//! 数值类规则（攻击/防御/力量/暴击/护甲/抗性的折叠）不在这里——DsnE24 阶段 ① 之后
//! 统一在 [`crate::rules::damage`]；旧的 `compute_melee_damage` 标量适配器随公式改写
//! 删除（DsnE24 决策 6）。
//!
//! [`crate::rules::damage`]: crate::rules::damage

use crate::components::*;

/// 目标当前是否可被近战攻击（纯组件版）。
///
/// 校验器与参数化执行器都不能对任意实体做 `World::get`，所以规则本身必须能
/// 只靠组件引用表达。规则由 [`Position::is_near`]（8 方向相邻）与
/// [`Health::is_alive`] 组成，与旧 `World` 版语义一致。
pub fn can_attack_positions(attacker: Position, target: Position, target_health: &Health) -> bool {
    attacker.is_near(target) && target_health.is_alive()
}