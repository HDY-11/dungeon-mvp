//! New ECS-native action model.
//!
//! This module is the target runtime for replacing the old `ActionQueue` + `ActionKindV3`
//! + `GameAction` dual-track design.

mod components;
pub mod decision;
mod runtime;

pub use components::*;
pub use decision::decide_monster_actions;
pub use runtime::*;
