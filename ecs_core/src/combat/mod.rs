//! 近战规则：相邻判定与伤害/暴击计算。
//!
//! 这里只有**纯函数**：不碰 `World`、不发事件、不改状态。结算链路是
//! `AttackIntentEvent -> resolve_attack_system`（`system/combat.rs`），
//! 行动执行器是 `action::entity::execute_basic_attack_system`；两者共用本模块的
//! 规则，因此「能不能打」与「打出多少」各只有一份实现。

use crate::components::*;

/// 伤害与暴击的计算结果。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeleeResult {
    pub damage: f64,
    pub is_crit: bool,
}

/// 目标当前是否可被近战攻击（纯组件版）。
///
/// 参数化执行器（`action::entity::execute_basic_attack_system`）不能对任意实体
/// 做 `World::get`，所以规则本身必须能只靠组件引用表达。规则由
/// [`Position::is_near`]（8 方向相邻）与 [`Health::is_alive`] 组成，
/// 与旧 `World` 版语义一致。
pub fn can_attack_positions(attacker: Position, target: Position, target_health: &Health) -> bool {
    attacker.is_near(target) && target_health.is_alive()
}

/// 根据攻击/防御与暴击参数计算最终伤害。
///
/// 纯函数：`crit_roll` 由调用方提供（结算系统从 `GameRng` 取），
/// 这样「随机数消耗点」留在系统层，公式本身可单测。
pub fn compute_melee_damage(
    attack: f64,
    defense: f64,
    crit_rate: f64,
    crit_damage: f64,
    crit_roll: f64,
) -> MeleeResult {
    let base = (attack - defense).max(1.0);
    let is_crit = crit_rate > crit_roll;
    let damage = if is_crit {
        base * (1.0 + crit_damage.max(0.0))
    } else {
        base
    };
    MeleeResult { damage, is_crit }
}