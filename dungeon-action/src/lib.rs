//! 行动层：玩家/怪物行为、行动队列、执行/校验、tap-tap 输入确认。
//!
//! 依赖 dungeon-core 的纯数据，向上为 dungeon-world 和 TUI 提供可复用行为入口。

pub mod actions;
pub mod execute;
pub mod monster;
pub mod player;
mod tick;
pub mod types;

pub use execute::advance_action_queue;
pub use execute::confirm_throw;
pub use monster::{
    arbitration_system, chase_decision_system, flee_decision_system, wander_decision_system,
};
pub use player::{handle_player_direction, handle_skill, handle_timed_action, handle_wait};
pub use tick::advance_until_player_acted;
pub use types::*;

#[cfg(test)]
mod tests;
