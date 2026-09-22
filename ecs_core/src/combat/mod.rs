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

/// 根据攻击/防御与暴击参数计算最终伤害（**兼容形状**）。
///
/// **Phase H2 之后真正的实现是 [`crate::rules::melee`]**（结构化输入 + 因子分解返回）。
/// 本函数保留为**薄适配器**，好处有二：
///
/// 1. 结算链路（`system/combat.rs`）不必为了"改形状"而一起改；
/// 2. 「新口径与旧口径逐值一致」这条 H2 验收，可以由**旧实现本身**当基准来断言
///    （见 `rules/damage_tests.rs`）——比拿一堆硬编码期望值更可靠。
///
/// 想拿因子分解（日志/调试/按因子触发）的调用方直接用 [`crate::rules::melee`]。
pub fn compute_melee_damage(
    attack: f64,
    defense: f64,
    crit_rate: f64,
    crit_damage: f64,
    crit_roll: f64,
) -> MeleeResult {
    let breakdown = crate::rules::melee(crate::rules::MeleeInput {
        attack,
        defense,
        target_crit: crate::rules::CritProfile {
            rate: crit_rate,
            damage: crit_damage,
        },
        crit_roll,
    });
    MeleeResult {
        damage: breakdown.damage,
        is_crit: breakdown.is_crit,
    }
}