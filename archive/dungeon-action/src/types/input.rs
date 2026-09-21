//! Player input / preview types.

use bevy_ecs::prelude::*;

/// Player-triggered actions from keyboard.
#[derive(Clone, Debug, PartialEq)]
pub enum PlayerAction {
    Move(isize, isize),
    Wait,
    Skill(usize),
    Throw,
    OpenInventory,
    OpenLook,
    PickupGround,
    DescendStairs,
    SaveGame,
    LoadGame,
    Quit,
}

/// Timed action waiting for tap-tap confirmation.
#[derive(Clone, Debug, PartialEq)]
pub enum TimedPlayerAction {
    Move { dx: isize, dy: isize },
    Wait,
    Attack { target: Entity },
    Skill(usize),
}

/// tap-tap preview state.
#[derive(Resource, Default)]
pub struct PlayerPreview {
    pub action: Option<TimedPlayerAction>,
}
