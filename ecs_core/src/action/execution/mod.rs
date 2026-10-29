//! 行动执行侧：**纯规则** + **状态写入的唯一出口**。
//!
//! C8 删除了旧的独占执行系统（`tick_action_timers_system` / `execute_*_system` /
//! `run_action_cycle`）——它们已被 `action::entity` 里按行动类型拆分的参数化
//! 执行器取代。本模块保留两块：
//!
//! | 模块 | 内容 | 是否碰 `World` |
//! |---|---|---|
//! | [`movement`] | 移动规则 [`movement::plan_step`]（带原因的判定）及其 `bool`/`Option` 视图 | **不碰**，可单测 |
//! | [`apply_move`] | [`apply_move::MoveRequest`] → 判定 → **唯一**写 `Position` + 写穿占用图 | 是（系统） |
//!
//! **为什么要分成两块**（ISSUES ECS45 的教训）：规则必须纯、必须能脱离 `World` 单测；
//! 而"谁能写 `Position`"必须是**结构**而非约定。所以规则留在纯函数里，写入收进一个系统，
//! 四条移动路径只发意图——这与攻击链路（执行器发 `AttackIntentEvent`、扣血只有一处）同构。

pub mod apply_move;
pub mod movement;

pub use apply_move::{MoveRequest, apply_move_system};
