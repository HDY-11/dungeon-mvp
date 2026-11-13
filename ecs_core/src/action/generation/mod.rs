//! 行动生成侧：AI 候选与玩家 active action。
//!
//! 生成系统**只 spawn 候选/active action**，不写 actor 的 `Idle`/`Active`/`Failure`
//! （玩家路径例外：它直接产出 active action，见 [`player`]）。

pub mod ai;
pub mod player;

pub use ai::*;
pub use player::*;
