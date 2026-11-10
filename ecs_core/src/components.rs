//! 业务域组件。
//!
//! 组件只保存数据；规则由 `system` / `action` / `ai` / `combat` 中的系统实现。
//! 数值统一使用 `f64`，为后续精度与数值规划预留空间。

use bevy_ecs::prelude::*;

// ── 基础数值组件 ─────────────────────────────────────

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    pub x: usize,
    pub y: usize,
}

impl Position {
    pub const fn new(x: usize, y: usize) -> Self {
        Self { x, y }
    }

    pub const fn to_tuple(&self) -> (usize, usize) {
        (self.x, self.y)
    }

    /// 判断八个方位（相邻且不同格）。
    pub fn is_near(self, other: Position) -> bool {
        let dx = self.x as isize - other.x as isize;
        let dy = self.y as isize - other.y as isize;
        (dx > -2 && dx < 2) && (dy > -2 && dy < 2) && (dx | dy) != 0
    }

    /// 切比雪夫距离（8 方向移动距离）。
    pub fn chebyshev(self, other: Position) -> usize {
        self.x.abs_diff(other.x).max(self.y.abs_diff(other.y))
    }

    /// 曼哈顿距离（4 方向移动距离）。
    pub fn manhattan(self, other: Position) -> usize {
        self.x.abs_diff(other.x) + self.y.abs_diff(other.y)
    }

    pub fn offset(self, dx: isize, dy: isize) -> (usize, usize) {
        (self.x.wrapping_add_signed(dx), self.y.wrapping_add_signed(dy))
    }
}

/// 通用资源池（DsnE23）：生命、法力，将来的气力也复用这一形状。
///
/// `current` 只被伤害/消耗/回复写；`max` 由 [`crate::rules::vitals`] 的唯一公式
/// 重算——`Health`/`Mana` 的字段访问经 `Deref` 暴露，`health.current` 照旧可读。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pool {
    pub current: f64,
    pub max: f64,
}

impl Pool {
    /// 满池。
    pub const fn new(max: f64) -> Self {
        Self { current: max, max }
    }

    /// 指定当前值与上限（读档/测试用；不做夹取，调用点自己保证语义）。
    pub const fn full(current: f64, max: f64) -> Self {
        Self { current, max }
    }

    /// 设置当前值（夹到 `[0, max]`）。
    pub fn with_current(mut self, current: f64) -> Self {
        self.current = current.clamp(0.0, self.max);
        self
    }

    /// 返回修正后的池值；正值治疗，负值受伤，结果钳制在 `[0, max]`。
    pub fn modified(mut self, amount: f64) -> Self {
        self.current = (self.current + amount).clamp(0.0, self.max);
        self
    }

    pub fn heal(mut self, amount: f64) -> Self {
        self.current = (self.current + amount.max(0.0)).min(self.max);
        self
    }

    pub fn damage(mut self, amount: f64) -> Self {
        self.current = (self.current - amount.max(0.0)).max(0.0);
        self
    }

    pub fn is_alive(self) -> bool {
        self.current > 0.0
    }

    pub fn ratio(self) -> f64 {
        if self.max <= 0.0 {
            0.0
        } else {
            self.current / self.max
        }
    }

    /// 尝试扣除（负数是调用方错误）；法力等"不够就不能用"的资源用它。
    pub fn try_cost(&mut self, amount: f64) -> Result<(), &'static str> {
        if amount < 0.0 {
            return Err("资源消耗不能为负数");
        }
        if self.current - amount < 0.0 {
            return Err("资源不足");
        }
        self.current -= amount;
        Ok(())
    }

    /// 回复（受上限钳制），返回实际回复量。
    pub fn restore(&mut self, amount: f64) -> f64 {
        let actual = amount.max(0.0).min(self.max - self.current);
        self.current += actual;
        actual
    }
}

/// 生命值池（DsnE23：`Health` 是 [`Pool`] 的 newtype）。
///
/// 字段经 `Deref`/`DerefMut` 暴露，所以既有代码的 `health.current` /
/// `health.is_alive()` 不用改；**上限**只能由 [`crate::rules::vitals::apply_maxima`]
/// 写（单一公式，DsnE23）。
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Health(pub Pool);

impl Health {
    /// 满血。
    pub const fn new(max: f64) -> Self {
        Self(Pool::new(max))
    }

    /// 指定当前值与上限。
    pub const fn full(current: f64, max: f64) -> Self {
        Self(Pool::full(current, max))
    }

    /// 设当前值（返回新 `Health`，与旧接口一致）。
    pub fn with_current(self, current: f64) -> Self {
        Self(self.0.with_current(current))
    }

    /// 治疗（返回新 `Health`，与旧接口一致）。
    pub fn heal(self, amount: f64) -> Self {
        Self(self.0.heal(amount))
    }

    /// 受伤（返回新 `Health`，与旧接口一致）。
    pub fn damage(self, amount: f64) -> Self {
        Self(self.0.damage(amount))
    }

    /// 增减（返回新 `Health`，与旧接口一致）。
    pub fn modified(self, amount: f64) -> Self {
        Self(self.0.modified(amount))
    }
}

impl std::ops::Deref for Health {
    type Target = Pool;

    fn deref(&self) -> &Pool {
        &self.0
    }
}

impl std::ops::DerefMut for Health {
    fn deref_mut(&mut self) -> &mut Pool {
        &mut self.0
    }
}

impl From<Pool> for Health {
    fn from(pool: Pool) -> Self {
        Self(pool)
    }
}

/// 法力值池（DsnE23；旧名 `Mana`）。
///
/// 改名是为了让"法力资源"与"魔法体系/法术"在讨论里不打架：`Mana` 是资源，
/// 法术与精通是别的东西（DsnE25）。同样经 `Deref` 暴露 [`Pool`] 的字段与方法。
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Mana(pub Pool);

impl Mana {
    /// 满池。
    pub const fn new(max: f64) -> Self {
        Self(Pool::new(max))
    }
}

impl std::ops::Deref for Mana {
    type Target = Pool;

    fn deref(&self) -> &Pool {
        &self.0
    }
}

impl std::ops::DerefMut for Mana {
    fn deref_mut(&mut self) -> &mut Pool {
        &mut self.0
    }
}

impl From<Pool> for Mana {
    fn from(pool: Pool) -> Self {
        Self(pool)
    }
}

/// 法力回复进度（DsnE25）：累计 AV，攒满 `MANA_REGEN_AV` 换 1 点 MP。
///
/// **为什么放组件而不是资源**：回复是"每个施法者自己的节奏"，怪物以后也可能有；
/// 挂在实体上就不必在资源里维护一张 `Entity -> 进度` 的散表。进度是运行时派生数据，
/// 不进存档（读档从 0 开始，与"随机流不进档"同一边界）。
#[derive(Component, Debug, Clone, Copy, PartialEq, Default)]
pub struct ManaRegen {
    /// 尚未兑换成 MP 的累计 AV。
    pub progress_av: f64,
}

impl ManaRegen {
    pub const fn new() -> Self {
        Self { progress_av: 0.0 }
    }
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Level(pub u64);

#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Experience {
    pub exp: f64,
    pub exp_to_next: f64,
}

impl Experience {
    pub const fn new(exp: f64, exp_to_next: f64) -> Self {
        Self { exp, exp_to_next }
    }

    pub fn add(&mut self, amount: f64) {
        self.exp += amount.max(0.0);
    }

    /// 溢出到下一级的经验；升级逻辑由经验系统负责。
    pub fn overflow(self) -> f64 {
        (self.exp - self.exp_to_next).max(0.0)
    }
}

/// 怪物死亡时奖励给玩家的经验值。玩家自身不使用此组件。
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct ExperienceReward(pub f64);

#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct MagicMastery(pub f64);

// ── 动态属性（DsnE22 阶段 ②b） ──────────────────────

/// actor 的**基础属性值**：按 [`StatId`] 稠密存放，是求值五步的 ① base。
///
/// # 为什么不是"每属性一个组件"
///
/// 阶段 ② 之前，攻击/防御/移动速度/攻击速度/暴击率/暴击伤害是六个独立组件。
/// 每加一项属性都要加组件、加查询、改所有生成点；属性是长尾内容，这条成本线会
/// 一直涨。现在换成一份稠密表：`get(StatId) -> f64` / `set(StatId, value)`，
/// 加属性只改词表（`rules/stat.rs`）与数据，查询与系统不动。
///
/// # 区间
///
/// [`Stats::set`] 按词表里的 `[min, max]` 夹取，并忽略 NaN——属性的合法区间是
/// **数据**（DsnE22 已拍板），夹取只有这一处，避免每个写入点各自 clamp。
/// `get` 返回的就是已夹取过的值；派生值（修正折叠后）不在本结构的职责内，
/// 它们由规则层与 `balance` 的 clamp 负责。
///
/// # 与"物品实体上的 Stats"的关系
///
/// 同一套词表将来既服务 actor 也服务物品实例（DsnE22 决策 3）。本阶段先落 actor
/// 侧：物品的加成仍走 `Modifier`，物品的基础值还不是数值（阶段 ③/④ 再迁）。
#[derive(Component, Debug, Clone, PartialEq)]
pub struct Stats {
    /// 按 `StatId.0` 下标存放，长度 = `STATS.len()`。
    values: Vec<f64>,
}

impl Stats {
    /// 全默认值（默认值来自词表 [`crate::rules::STATS`]）。
    pub fn new() -> Self {
        Self {
            values: crate::rules::STATS.iter().map(|def| def.default).collect(),
        }
    }

    /// 从若干 `(属性, 基础值)` 对构造；未列出的属性用词表默认值。
    pub fn from_pairs(pairs: impl IntoIterator<Item = (crate::rules::StatId, f64)>) -> Self {
        let mut stats = Self::new();
        for (stat, value) in pairs {
            stats.set(stat, value);
        }
        stats
    }

    /// 读一项属性（值已按词表区间夹取过）。
    pub fn get(&self, stat: crate::rules::StatId) -> f64 {
        let def = stat.def();
        self.values
            .get(stat.0 as usize)
            .copied()
            .unwrap_or(def.default)
    }

    /// 写一项属性：非有限值中的 NaN 被忽略，其余按 `[min, max]` 夹取。
    ///
    /// 忽略 NaN 而不是夹成 min：NaN 是"谁都没想到的写法"，静默变成 0 会掩盖 bug；
    /// 保留旧值至少让行为可解释，测试也能直接断言这条口径。
    pub fn set(&mut self, stat: crate::rules::StatId, value: f64) {
        if value.is_nan() {
            return;
        }
        let def = stat.def();
        let index = stat.0 as usize;
        if index >= self.values.len() {
            self.values.resize(index + 1, def.default);
        }
        self.values[index] = value.clamp(def.min, def.max);
    }

    /// 链式构造：`Stats::new().with(StatId::ATTACK, 8.0)`。
    pub fn with(mut self, stat: crate::rules::StatId, value: f64) -> Self {
        self.set(stat, value);
        self
    }
}

impl Default for Stats {
    fn default() -> Self {
        Self::new()
    }
}

// ── 通用数据组件 ─────────────────────────────────────

#[derive(Component, Debug, Clone, PartialEq, Eq)]
pub struct EntityName(pub String);

#[derive(Component, Debug, Clone, PartialEq, Eq)]
pub struct AttackName(pub String);

/// 视野组件。`visible_tiles` 由 `fov_system` 每轮重算。
#[derive(Component, Debug, Clone, PartialEq, Eq)]
pub struct Viewshed {
    pub range: usize,
    pub visible_tiles: Vec<(usize, usize)>,
}

impl Viewshed {
    pub fn new(range: usize) -> Self {
        Self {
            range,
            visible_tiles: Vec::new(),
        }
    }

    pub fn can_see(&self, pos: (usize, usize)) -> bool {
        self.visible_tiles.contains(&pos)
    }
}

// ── 行动状态组件 ─────────────────────────────────────

/// 这个实体**占格**：别人不能走进它所在的格子。
///
/// 占用图只登记带 `Blocks` 的实体（`rebuild_occupancy_system`），所以
/// "站在格子上"与"占住格子"从此是**两件事**：
/// - 怪物 / 玩家：挂 `Blocks`（并同时有 `Position`）；
/// - 楼梯：有 `Position` 但**不挂** `Blocks`——它仍可以被站上去（G31），
///   而这一条以前是占用图里写死的一个 `if stairs.is_some() { continue }`。
///
/// **漏挂的后果是静默的**（穿人 / 被穿，不报错），因此生成侧集中挂载，
/// 并由 `world/init_tests.rs` 断言"每个 actor 都有 `Blocks`"。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Blocks;

/// 没有行动，等待决策。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Idle;

/// 已挂载一个具体行动，正在等待 `ActionTimer` 归零。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Active;

/// 上次行动因保活条件失效而失败，等待下一轮决策。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Failure;

/// 当前行动的剩余 AV。只与 `Active` 同时存在。
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct ActionTimer {
    pub remaining_av: f64,
}

/// 当前行动的 AV 已归零，可以执行。
///
/// 由 `tick_action_timers_system` 在 `remaining_av <= 0` 时插入；
/// 执行系统只处理 `With<Ready>` 的实体。
///
/// **清理方是唯一的**（Phase H1）：执行器一律经 `ActionEvents::succeed` / `::fail`
/// 结束行动，那对方法同时清 `Ready` 并发终态事件——所以「谁负责清 `Ready`」不再是
/// 散落在 6 个执行器里的约定，而是终态出口的内部实现（ECS35 / ECS36）。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Ready;

// ── 能力组件（纯标记） ───────────────────────────────

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CanMove;

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CanWait;

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CanBasicAttack;

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CanChase;

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CanFlee;

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CanWander;

// ── 具体行动组件 ─────────────────────────────────────
// 这些组件是瞬态组件：只在实体处于 `Active` 期间存在。

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Wait;

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Move {
    pub dx: isize,
    pub dy: isize,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct BasicAttack {
    pub target: Entity,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Chase;

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Flee;

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Wander;

// ── 观察候选（DsnE21 阶段 3） ────────────────────────

/// "这个实体能产生事件、值得被玩家关注"的**显式标记**。
///
/// 信息栏的候选列表只列带这个标记的可见实体；规则层在 spawn 实体时决定要不要
/// 挂它（怪物、地面物品、尸体、楼梯），而不是靠"有没有 `Health`""有没有 `Item`"
/// 之类的字段猜测——猜测会在加新实体类型时悄悄漏掉或误收，标记则逼着生成方表态。
///
/// 标记与"玩家关注了谁"是两件事：关注集属于 UI 状态（`PageStack`），不写进世界。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Observable;

#[cfg(test)]
#[path = "components_tests.rs"]
mod tests;
