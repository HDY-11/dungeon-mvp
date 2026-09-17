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

/// **旧口径**行动 AV（Phase D1 起的对照实现，D3 删除）。
///
/// `反应时 + 耗时 × 敏捷系数`——两段式：反应时对所有行动等量叠加。
/// 保留它只为让「新旧 AV 对比」测试能在同一个 commit 里说明差异，生产代码
/// 已全部走 [`action_av`]（新口径）。
pub fn legacy_action_av(duration: f64, agility: f64) -> f64 {
    agility_to_reaction(agility) + duration * agility_speed_factor(agility)
}

// ── 速度倍率（Phase D / REFACTOR.md §2.6） ───────────────
//
// 速度是**倍率组件**（`MoveSpeed` / `AttackSpeed`），`1.0` 为基准，越高越快：
//
// ```text
// AV = base_duration / speed.clamp(MIN_SPEED, MAX_SPEED)
// ```
//
// 与旧口径的差别：旧 `AV = 反应时 + duration × 敏捷系数` 是「延迟 + 耗时」两段式，
// 反应时对所有行动等量叠加（最短的 `BasicAttack` 因此被拉长最多）。新口径只有
// 倍率一段，**已确认先删除反应时**（REFACTOR.md §11.6 第 3 项）；试玩若觉得需要
// 「出手前的固定延迟」，再统一加回一个常数项。

/// 速度倍率下限：`1/0.25 = 4×` 耗时。
pub const MIN_SPEED: f64 = 0.25;
/// 速度倍率上限：`4×` 快。防止增益叠乘把行动压成瞬时。
pub const MAX_SPEED: f64 = 4.0;

/// 玩家初始移动速度：旧敏捷 10 → `1/max(1-0.2, 0.5) = 1.25`。
///
/// 这不是"玩家天生快 25%"，而是**迁移映射的产物**（旧敏捷 10 的耗时系数 0.80
/// 的倒数）。GAME.md 用 `[试调]` 重新校准玩家基准时应把这里调回 1.0 并重新配平。
pub const PLAYER_MOVE_SPEED: f64 = 1.25;
/// 玩家初始攻击速度，同 [`PLAYER_MOVE_SPEED`]。
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

/// 行动 AV：`base_duration / clamp_speed(speed)`（Phase D 新口径，见上）。
///
/// 与旧 [`legacy_action_av`] 同名时期已经过去：Phase D3 删掉旧函数后，
/// 这个名字就是唯一口径。
pub fn action_av(base_duration: f64, speed: f64) -> f64 {
    base_duration / clamp_speed(speed)
}

/// 旧敏捷 `agility` 对应的速度倍率：`1 / max(1 - agility * 0.02, 0.5)`。
///
/// **这是 Phase D 的一次性迁移映射**，不是长期公式的一部分。它是对旧耗时系数
/// `agility_speed_factor` 的反解，因此
/// `base_duration / mapped_speed == base_duration * agility_speed_factor(agility)`：
/// 保留旧敏捷排序与各行动之间的相对快慢，丢掉的只是等量叠加的反应时常数项
/// （已确认删除）。等价地 `mapped_speed(agility) == agility / factor(10)`。
///
/// 保行为示例：敏捷 10（旧玩家）→ `1/0.80 = 1.25`；敏捷 14（洞穴鱼）→ `1/0.72 ≈ 1.3889`。
pub fn agility_to_speed(agility: f64) -> f64 {
    1.0 / agility_speed_factor(agility)
}

