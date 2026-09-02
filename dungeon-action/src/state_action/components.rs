//! State markers and concrete action components.

use bevy_ecs::prelude::*;

// ── State markers ──────────────────────────────────────────────

/// Entity is ready for a new decision.
#[derive(Component, Clone, Debug, Default)]
pub struct Idle;

/// Last action failed because its condition became invalid.
#[derive(Component, Clone, Debug, Default)]
pub struct Failure;

/// Entity currently has exactly one mounted action.
#[derive(Component, Clone, Debug, Default)]
pub struct Active;

/// Remaining action value for the currently mounted action.
#[derive(Component, Clone, Debug)]
pub struct ActionTimer {
    pub remaining_av: f32,
}

// ── Concrete action components ─────────────────────────────────
// These are transient: they exist only while the entity is `Active`.

#[derive(Component, Clone, Debug)]
pub struct Chase;

#[derive(Component, Clone, Debug)]
pub struct Flee;

#[derive(Component, Clone, Debug)]
pub struct Wander;

#[derive(Component, Clone, Debug)]
pub struct Wait;

#[derive(Component, Clone, Debug)]
pub struct Move {
    pub dx: isize,
    pub dy: isize,
}

#[derive(Component, Clone, Debug)]
pub struct Attack {
    pub target: Entity,
}

#[derive(Component, Clone, Debug)]
pub struct Skill(pub usize);

#[derive(Component, Clone, Debug)]
pub struct Throw {
    pub tx: usize,
    pub ty: usize,
}
