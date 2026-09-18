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

#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Health {
    pub current: f64,
    pub max: f64,
}

impl Health {
    pub const fn new(max: f64) -> Self {
        Self { current: max, max }
    }

    pub const fn full(current: f64, max: f64) -> Self {
        Self { current, max }
    }

    pub fn with_current(mut self, current: f64) -> Self {
        self.current = current.clamp(0.0, self.max);
        self
    }

    /// 返回修正后的生命值；正值治疗，负值受伤，结果钳制在 `[0, max]`。
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
}

#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Magic {
    pub current: f64,
    pub max: f64,
}

impl Magic {
    pub const fn new(max: f64) -> Self {
        Self { current: max, max }
    }

    pub fn try_cost(&mut self, amount: f64) -> Result<(), &'static str> {
        if amount < 0.0 {
            return Err("法力消耗不能为负数");
        }
        if self.current - amount < 0.0 {
            return Err("法力不足");
        }
        self.current -= amount;
        Ok(())
    }

    pub fn restore(&mut self, amount: f64) -> f64 {
        let actual = amount.max(0.0).min(self.max - self.current);
        self.current += actual;
        actual
    }

    pub fn ratio(self) -> f64 {
        if self.max <= 0.0 {
            0.0
        } else {
            self.current / self.max
        }
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
pub struct Attack(pub f64);

#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Defense(pub f64);

#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct MagicMastery(pub f64);

/// 移动速度倍率（Phase D / REFACTOR.md §2.6）：`1.0` 为基准，越高越快。
///
/// 作用于 `Move` / `Chase` / `Flee` / `Wander` 四类行动：
/// `AV = base_duration / MoveSpeed`（clamp 见 [`crate::balance::MIN_SPEED`]）。
/// 地形/负重/Buff 未来通过增删或改写这个组件影响移动节奏，
/// 不需要再挤进一个聚合的“敏捷”数值。
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct MoveSpeed(pub f64);

/// 攻击速度倍率（Phase D / REFACTOR.md §2.6）：`1.0` 为基准，越高越快。
///
/// 作用于 `BasicAttack`：`AV = UNARMED_ATTACK_DURATION / AttackSpeed`。
/// 武器攻速（I67）未来接到这里，不改生成/执行链路。
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct AttackSpeed(pub f64);

/// 两个速度组件的装配束：**只是打包，不是新组件**。
///
/// 玩家组件束已经到 16 个元素，而 bevy_ecs 0.16 的元组 `Bundle` 只实现到
/// **15** 元（`bevy_ecs/src/bundle.rs` 的 `all_tuples!(tuple_impl, 0, 15, B)`），
/// 再加两个组件直接编译失败。用 derive 出来的具名 Bundle 打包，可以把
/// 「一个实体上的组件数」和「元组元数上限」解耦——实体上仍然挂着两个
/// 独立组件（`MoveSpeed` / `AttackSpeed`），查询照旧。
#[derive(Bundle, Debug, Clone, Copy)]
pub struct Speed {
    pub move_speed: MoveSpeed,
    pub attack_speed: AttackSpeed,
}

impl Speed {
    /// 移动与攻击同速（基准 `1.0`）。
    pub const fn uniform(speed: f64) -> Self {
        Self {
            move_speed: MoveSpeed(speed),
            attack_speed: AttackSpeed(speed),
        }
    }
}

#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct CritRate(pub f64);

#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct CritDamage(pub f64);

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

/// AI 追击记忆：最后已知的玩家位置。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LastKnownPlayerPos(pub Option<(usize, usize)>);

// ── 行动状态组件 ─────────────────────────────────────

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
/// 执行系统只处理 `With<Ready>` 的实体；`finish_action_*` / `mount_action`
/// 会清理它。
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

// ── 受击记录（供威胁/AI 反应使用） ─────────────────────

/// 标记“该实体需要记录受击来源”。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NeedRecordBeAttacked;

/// 最近一次受击记录。
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct BeAttacked {
    pub by: Entity,
    pub av_since_hit: f64,
}

impl BeAttacked {
    pub const fn new(by: Entity) -> Self {
        Self {
            by,
            av_since_hit: 0.0,
        }
    }
}
