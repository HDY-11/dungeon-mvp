//! 数值公式与平衡常量。
//!
//! 数值统一使用 `f64`；此前的 i32/u32 数值不再作为行为基准。

/// 升到下一级所需经验：`25 * level^1.5 + 10 * level`。
pub fn exp_to_next_level(level: u64) -> f64 {
    let lv = level as f64;
    25.0 * lv.powf(1.5) + 10.0 * lv
}

// ── 物理伤害（DsnE24 阶段 ①） ───────────────────────────
//
// 口径（GAME.md Gm2）：
//
// ```text
// 基础值   = 攻击力 + min(力量, 武器力量上限) × φ
// 最终伤害 = max(1, 基础值 × 增伤域 × 暴击域 × 护甲域 × 抗性域)
// ```

/// 护甲曲线常数 δ：`减伤 = δ / (防御 + δ)`。
///
/// 取 4 与 GAME.md Gm4 的"防御 4 → 减伤 50%"对齐；换成别的数时只需改这里。
pub const ARMOR_DELTA: f64 = 4.0;

/// 力量转化系数 φ：每点有效力量转成多少点基础伤害。
pub const STRENGTH_TO_DAMAGE: f64 = 0.5;

/// 空手时的力量加成上限 S_m（没有武器时不像武器那样封顶）。
pub const UNARMED_STRENGTH_CAP: f64 = 4.0;


// 生命/法力上限公式已迁到 `rules::vitals`（DsnE23）：等级曲线 + 属性修正，
// 并且只在那里算一次。旧的 `max_hp_for(level, defense)` / `max_mp_for(level, mastery)`
// 已删除——"防御顺带加血"是半成品口径。

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
// 速度是 **Stats 里的属性**（`MOVE_SPEED` / `ATTACK_SPEED`，DsnE22 阶段 ②b），
// `1.0` 为基准，越高越快：
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

// ── 资源回复与开局节奏（DsnE25） ─────────────────────
//
// 用户 m08053 拍板：法力随时间回；生命不回，只靠道具/可交互物。
// 数值全是 `[试调]`，改这里即可，不要在系统里写魔法数。

/// 每累计多少 AV 恢复 1 点法力（m08882 拍板：初始 10000，约 12 个玩家行动）。
pub const MANA_REGEN_AV: f64 = 10000.0;

/// 每层治疗泉数量。
pub const HEAL_SPRINGS: usize = 1;

/// 一座治疗泉恢复的生命。
pub const HEAL_SPRING_AMOUNT: f64 = 12.0;

/// 治疗泉离出生点的最近 / 最远距离（格）。
///
/// 太近等于开局白送，太远则开局压力没缓解；4–10 格大约是一间房到两间房的距离。
pub const HEAL_SPRING_MIN_RADIUS: usize = 4;
pub const HEAL_SPRING_MAX_RADIUS: usize = 10;

/// 第 `floor` 层的怪物数量区间 `(min, max)`（DsnE25 决策 4）。
///
/// 首层 4–6（旧口径 6–12）。随层线性增长，但单只数值不变：降的是"同时面对多少只"，
/// 不是把怪物变软。
pub fn monster_count_range(floor: u32) -> (usize, usize) {
    let step = floor.saturating_sub(1) as usize;
    (4 + step.saturating_mul(3), 6 + step.saturating_mul(4))
}

// ── 投掷（Gm9） ──────────────────────────────────────

/// 投掷行动的基础耗时（AV，固定耗时，不吃攻速）。
///
/// Gm9 旧案给 190；比徒手攻击（300）快，但每次要消耗一件可投掷物。
pub const THROW_DURATION_AV: f64 = 190.0;

/// 投掷射程（切比雪夫距离，格）。
pub const THROW_RANGE: usize = 5;

/// 投掷基础伤害的楼层加成步长：`floor(层数 / 步长)`。
///
/// Gm9 旧案是 `3 + floor(层/2)`；当前实现只取物品固定伤害，这条常量留给
/// "另设新计算方式"的预留（用户 m10559 Q4）。保持为 0 即不接楼层加成。
pub const THROWN_FLOOR_BONUS_STEP: u32 = 0;

// ── 仇恨 / 威胁（DsnE29） ────────────────────────────
//
// 威胁只用于"选谁当目标"与"记忆何时消退"，不进入任何伤害公式。
// 用户 m12161 要求先做仇恨、且要能扩展（多来源/群体仇恨），详见
// `ecs_core/src/monster/aggro.rs` 的模块文档。

/// 每次被击中的固定威胁：保证 0 伤害的命中也会记仇。
pub const THREAT_PER_HIT: f64 = 1.0;

/// 每点伤害折算的威胁。
pub const THREAT_PER_DAMAGE: f64 = 1.0;

/// 每 1000 AV 衰减的威胁值。
///
/// 1 点伤害 = 1 威胁；按此速率 1000 AV 后清空，约等于 2–4 个玩家行动，
/// 够让怪物"记仇"到追一两步，又不至于永远锁定。
pub const THREAT_DECAY_PER_1000_AV: f64 = 10.0;


