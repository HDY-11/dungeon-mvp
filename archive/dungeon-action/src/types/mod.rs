//! Action domain types.
//!
//! This module contains data types / interfaces used by the action layer.

mod components;
mod input;
mod ui;

pub use components::*;
pub use input::*;
pub use ui::*;

/// Reaction time derived from agility.
pub fn agility_to_reaction(agility: u32) -> f32 {
    (100.0 - agility as f32 * 3.0).max(20.0)
}

/// Speed factor derived from agility.
pub fn agility_speed_factor(agility: u32) -> f32 {
    (1.0 - agility as f32 * 0.02).max(0.5)
}
