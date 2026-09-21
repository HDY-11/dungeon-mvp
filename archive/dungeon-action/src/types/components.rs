//! ECS capability components for actions.

use bevy_ecs::prelude::*;

#[derive(Component, Clone, Debug)]
pub struct CanMove {
    pub duration: f32,
    pub priority: u32,
}

impl CanMove {
    pub fn new(priority: u32) -> Self {
        Self {
            duration: 300.0,
            priority,
        }
    }
}

#[derive(Component, Clone, Debug)]
pub struct CanChase {
    pub duration: f32,
    pub priority: u32,
}

impl CanChase {
    pub fn new(priority: u32) -> Self {
        Self {
            duration: 250.0,
            priority,
        }
    }

    pub fn condition(can_see_player: bool) -> bool {
        can_see_player
    }
}

#[derive(Component, Clone, Debug)]
pub struct CanFlee {
    pub duration: f32,
    pub priority: u32,
}

impl CanFlee {
    pub fn new(priority: u32) -> Self {
        Self {
            duration: 250.0,
            priority,
        }
    }

    pub fn condition(hp_ratio: f32) -> bool {
        hp_ratio < dungeon_core::FLEE_HP_RATIO
    }
}

#[derive(Component, Clone, Debug)]
pub struct CanWander {
    pub duration: f32,
    pub priority: u32,
}

impl CanWander {
    pub fn new(priority: u32) -> Self {
        Self {
            duration: 500.0,
            priority,
        }
    }

    pub fn condition() -> bool {
        true
    }
}

#[derive(Component, Clone, Debug)]
pub struct CanWait {
    pub duration: f32,
    pub priority: u32,
}

impl CanWait {
    pub fn new(priority: u32) -> Self {
        Self {
            duration: 800.0,
            priority,
        }
    }

    pub fn condition() -> bool {
        true
    }
}

#[derive(Component, Clone, Debug)]
pub struct CanThrow {
    pub priority: u32,
    pub duration: f32,
}

impl CanThrow {
    pub fn new(priority: u32) -> Self {
        Self {
            priority,
            duration: dungeon_core::THROW_DURATION,
        }
    }
}
