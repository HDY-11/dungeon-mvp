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

// ── 速度倍率（Phase D / REFACTOR.md §2.6） ───────────────
//
// 速度是**倍率组件**（`MoveSpeed` / `AttackSpeed`），`1.0` 为基准，越高越快：
//
// ```text
// AV = base_duration / speed.clamp(MIN_SPEED, MAX_SPEED)
// ```
//
// 与 Phase D 之前的旧口径的差别：旧式 `AV = 反应时 + duration × 敏捷系数` 是
// 「延迟 + 耗时」两段式，反应时对所有行动等量叠加（最短的 `BasicAttack` 因此
// 被拉长最多）。新口径只有倍率一段，**已确认删除反应时**
// （REFACTOR.md §11.6 第 3 项）；试玩若觉得需要「出手前的固定延迟」，
// 再统一加回一个常数项。

/// 速度倍率下限：`1/0.25 = 4×` 耗时。
pub const MIN_SPEED: f64 = 0.25;
/// 速度倍率上限：`4×` 快。防止增益叠乘把行动压成瞬时。
pub const MAX_SPEED: f64 = 4.0;

/// 玩家初始移动速度。
///
/// 这个 `1.25` 不是"玩家天生快 25%"，而是 Phase D 迁移映射的产物
/// （旧敏捷 10 → 旧耗时系数 0.80 → 倒数 1.25）。GAME.md 用 `[试调]` 重新
/// 校准玩家基准时应把这里改回 `1.0`，并连怪物速度一起重新配平。
pub const PLAYER_MOVE_SPEED: f64 = 1.25;
/// 玩家初始攻击速度，来源同 [`PLAYER_MOVE_SPEED`]。
pub const PLAYER_ATTACK_SPEED: f64 = 1.25;

/// 把速度倍率夹进 `[MIN_SPEED, MAX_SPEED]`。
///
/// 非有限值（NaN / inf）会被夹成 `MIN_SPEED`，避免 `AV` 变成 NaN 后
/// 让 `ActionTimer` 的比较器 panic。
pub fn clamp_speed(speed: f64) -> f64 {
    if speed.is_finite() {
        speed.clamp(MIN_SPEED, MAX_SPEED)
    } else {
        MIN_SPEED
    }
}

/// 行动 AV：`base_duration / clamp_speed(speed)`。
///
/// 这是 Phase D 之后的**唯一** AV 口径：速度越高 AV 越短，且 AV 与
/// `base_duration` 严格成正比（旧口径里那个等量叠加的反应时常数项已删除）。
pub fn action_av(base_duration: f64, speed: f64) -> f64 {
    base_duration / clamp_speed(speed)
}

