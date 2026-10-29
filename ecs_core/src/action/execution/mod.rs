//! 行动执行侧：**纯规则**只剩移动这一块。
//!
//! C8 删除了旧的独占执行系统（`tick_action_timers_system` / `execute_*_system` /
//! `run_action_cycle`）；Validate 阶段落地之后，**判定与写入都不再住在这里**：
//!
//! | 位置 | 内容 |
//! |---|---|
//! | [`movement`] | 移动规则 [`movement::plan_step`]（带原因的判定）及其 `bool` / `Option` 视图。**纯函数**，可脱离 `World` 单测 |
//! | `action::validation` | 判定 + 预留（`ActionPhase::Validate`），每个行动一个校验器 |
//! | `action::entity::settle_action_system` | **唯一**写 `Position` / 发攻击意图的地方（`ActionPhase::Execute`） |
//!
//! **为什么规则留在这里**：规则必须纯、必须能脱离 `World` 单测——`plan_step` 是唯一
//! 产生 [`movement::BlockReason`] 的地方，所以"为什么这一下没动"不可能出现两份不同答案。
//!
//! **为什么这里不再有"应用意图"的系统**：曾经有一条
//! `执行器 → MoveRequest → apply_move_system(判定 + 写入)` 的通道。判定搬进
//! `ActionPhase::Validate` 之后，`MoveRequest` 这个中间单元没有存在的理由了——
//! 判定发生的时刻（校验阶段，串行）就已经把格子占住，落地只是把定案写进 `Position`
//! （`settle_action_system`）。少一层事件 = 少一处"意图与写入不一致"的可能。

pub mod movement;
