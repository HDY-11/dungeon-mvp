//! 行动系统。
//!
//! **一个行动 = 一个 actor 的瞬态子实体**（REFACTOR §3.6 / DESIGN DsnE8）：
//!
//! ```text
//! Generate   每个行为一个生成系统，只 spawn 候选
//!     ↓ ApplyDeferred
//! Arbitrate  唯一写 actor 行动状态的系统
//!     ↓
//! Tick       推进 ActionTimer，归零加 Ready
//!     ↓
//! Validate   每个行动一个**校验器**：定案（落在哪 / 打谁 / 还该做吗）+ 预留目标格
//!     ↓ ApplyDeferred
//! Execute    **一个**通用落地器：只写结果，不再判定
//!     ↓ ActionSucceeded / ActionFailedEvent
//! Complete   despawn action 实体，actor 回 Idle / Failure
//! ```
//!
//! # 为什么"判定"与"落地"要分成两个阶段（DsnE14 / ISSUES ECS45）
//!
//! 判定与写入曾经粘在一起：写位置那次顺手再判一次。于是"谁有权判定"和"谁有权写"
//! 两件事混在一个系统里，四条移动路径各写一遍；同一轮两个 actor 抢同一格的问题
//! 也因此修了两次没修净。
//!
//! 现在：
//!
//! - **校验阶段是串行的**（`.chain()`），校验器判定通过就**立刻 `occupancy.set` 预留**，
//!   所以后面的校验器立刻能看到这一格已经被占；
//! - **执行阶段只落地**，没有判定可做 → `Execute` 只需要一个通用系统；
//! - 扩展点在**校验器**那一侧：加一个行动 = 加一个生成器 + 一个校验器（各一行）。
//!
//! 副产品：`ECS43`（保活覆盖范围）从此有了天然的落点——保活检查现在就在校验器里
//! （`Chase` 的"还看得见吗"、`Flee` 的"还在滞回区间吗"），只是目前仍只对**到点**的
//! 行动生效；若要对"排队中"的行动也生效，把校验器改成扫所有 `ActiveAction` 即可，
//! 那是行为决策（RULE §八 ③）。
//!
//! 模块布局（**一个阶段一个文件**，读目录就能看出流水线）：
//!
//! - [`entity`]：action 实体链路本体（组件、生成/仲裁/tick、完成、调度），C7 起已接进主循环；
//! - [`validation`]：**校验阶段**——每个行动一个校验器，定案 + 预留（`MoveTarget`/`AttackTarget`）；
//! - [`settle`]：**落地阶段**——全项目唯一写 `Position` 的系统（另一个写入点是生成期出生点）；
//! - [`ownership`]：行动归属关系（`ActionOf` / `ActionChildren`）——用它而**不用**
//!   通用 `ChildOf`，让"只有行动实体参与行动仲裁"成为类型保证（ISSUES ECS30/ECS41）；
//! - [`generation`]：生成侧的类型与条件（`PlayerCommand`、追击/逃跑条件）；
//! - [`execution`]：执行侧共用件——**纯规则**（`movement::plan_step` 等），不含系统。

pub mod entity;
pub mod execution;
pub mod generation;
pub mod ownership;
pub mod settle;
pub mod validation;

pub use entity::{
    ActionName, ActionPriority, ActionSource, ActiveAction, Candidate, PRIORITY_CHASE,
    PRIORITY_FLEE, PRIORITY_PLAYER, PRIORITY_WAIT, PRIORITY_WANDER, PlayerActionRequest,
};
pub use execution::*;
pub use generation::*;
pub use ownership::{ActionChildren, ActionOf};
pub use settle::settle_action_system;
pub use validation::{AttackTarget, MoveTarget};
