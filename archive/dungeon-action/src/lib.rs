//! Action layer for the ECS-native action model.
//!
//! Depends on `dungeon-core` for pure data, and is consumed by `dungeon-world` and the TUI.

mod execute;
mod player;
pub mod state_action;
mod tick;
mod types;

pub use execute::confirm_throw;
pub use player::{handle_player_direction, handle_skill, handle_timed_action, handle_wait};
pub use state_action::*;
pub use tick::advance_until_player_acted;
pub use types::*;
