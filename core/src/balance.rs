//! 数值公式与平衡常量。
//!
//! 数值统一使用 `f64`；此前的 i32/u32 数值不再作为行为基准。

/// 升到下一级所需经验：`25 * level^1.5 + 10 * level`。
pub fn exp_to_next_level(level: u64) -> f64 {
    let lv = level as f64;
    25.0 * lv.powf(1.5) + 10.0 * lv
}

/// 最大生命：`20 + level * 5 + defense * 2`。
pub fn max_hp_for(level: u64, defense: f64) -> f64 {
    20.0 + level as f64 * 5.0 + defense * 2.0
}

/// 最大法力：`5 + level * 3 + magic_mastery`。
pub fn max_mp_for(level: u64, magic_mastery: f64) -> f64 {
    5.0 + level as f64 * 3.0 + magic_mastery
}

/// 反应时：`max(100 - agility * 3, 20)`。
pub fn agility_to_reaction(agility: f64) -> f64 {
    (100.0 - agility * 3.0).max(20.0)
}

/// 耗时修正系数：`max(1.0 - agility * 0.02, 0.5)`。
pub fn agility_speed_factor(agility: f64) -> f64 {
    (1.0 - agility * 0.02).max(0.5)
}

/// 行动耗时基准（AV 毫秒）。
pub const UNARMED_ATTACK_DURATION: f64 = 300.0;
pub const CHASE_DURATION: f64 = 250.0;
pub const FLEE_DURATION: f64 = 250.0;
pub const WANDER_DURATION: f64 = 500.0;
pub const WAIT_DURATION: f64 = 800.0;

/// 低血量显示阈值。
pub const LOW_HP_RATIO: f64 = 0.30;
/// 逃跑触发阈值。
pub const FLEE_HP_RATIO: f64 = 0.25;
/// 逃跑退出阈值（滞回）。
pub const FLEE_HP_RATIO_EXIT: f64 = 0.30;

/// 计算行动 AV：反应时 + 耗时 * 敏捷系数。
pub fn action_av(duration: f64, agility: f64) -> f64 {
    agility_to_reaction(agility) + duration * agility_speed_factor(agility)
}
