//! 行动系统。
//!
//! **一个行动 = 一个 actor 的瞬态子实体**（REFACTOR §3.6 / DESIGN Dsn27）：
//!
//! ```text
//! generation（每个行为一个生成系统，只 spawn 候选）
//!     ↓ ApplyDeferred
//! arbitration（唯一写入 actor 行动状态的系统）
//!     ↓
//! tick（推进 ActionTimer，归零加 Ready）
//!     ↓
//! execution（每个行动一个专用 query 系统，零中央 match）
//!     ↓ ActionSucceeded / ActionFailedEvent
//! completion（despawn action 实体，actor 回 Idle / Failure）
//! ```
//!
//! 模块布局：
//!
//! - [`entity`]：action 实体链路本体（组件、系统、调度），C7 起已接进主循环；
//! - [`generation`]：生成侧的类型与条件（`PlayerCommand`、追击/逃跑条件）；
//! - [`execution`]：执行侧的**纯规则**（移动判定；`movement` 子模块）。
//!
//! C8 删除了旧的中央分派 `ActionKind`、`mount_action` 中央 match、actor 上的
//! 行动 ZST 与对应的 `execute_*_system` 独占实现。

pub mod entity;
pub mod execution;
pub mod generation;

pub use entity::{
    ActionName, ActionPriority, ActionSource, ActiveAction, Candidate, PRIORITY_CHASE,
    PRIORITY_FLEE, PRIORITY_PLAYER, PRIORITY_WAIT, PRIORITY_WANDER, PlayerActionRequest,
};
pub use execution::*;
pub use generation::*;
