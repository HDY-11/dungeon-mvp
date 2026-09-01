//! Action domain types.
//!
//! This module only contains data types / interfaces. Execution logic lives in `crate::execute`.
//! Submodules are an internal file organization detail; the public surface is re-exported here.

mod action;
mod components;
mod input;
mod intents;
mod queue;
mod ui;

pub use action::*;
pub use components::*;
pub use input::*;
pub use intents::*;
pub use queue::*;
pub use ui::*;
