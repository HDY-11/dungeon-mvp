//! Player input / preview types.

use super::action::ActionKindV3;
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

/// tap-tap preview state.
#[derive(Resource, Default)]
pub struct PlayerPreview {
    pub kind: Option<ActionKindV3>,
}
