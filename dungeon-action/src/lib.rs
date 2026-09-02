//! Action layer: player/monster behavior, action queue, execution/validation, tap-tap input.
//!
//! Depends on `dungeon-core` for pure data, and is consumed by `dungeon-world` and the TUI.
//!
//! The public API is intentionally re-exported at the crate root; internal module layout is not
//! part of the stable interface.

mod behavior;
mod decision;
mod execute;
mod player;
pub mod state_action;
mod tick;
mod types;

pub use decision::{
    arbitration_system, chase_decision_system, flee_decision_system, wander_decision_system,
};
pub use execute::{advance_action_queue, confirm_throw};
pub use player::{handle_player_direction, handle_skill, handle_timed_action, handle_wait};
pub use tick::advance_until_player_acted;
pub use types::*;

#[cfg(test)]
mod tests;
