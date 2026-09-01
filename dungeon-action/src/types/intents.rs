//! Monster intent buffers used by parallel decision systems.

use super::action::ActionKindV3;
use bevy_ecs::prelude::*;

#[derive(Resource, Default)]
pub struct ChaseIntents(pub Vec<(Entity, u32, f32, ActionKindV3)>);

#[derive(Resource, Default)]
pub struct FleeIntents(pub Vec<(Entity, u32, f32, ActionKindV3)>);

#[derive(Resource, Default)]
pub struct WanderIntents(pub Vec<(Entity, u32, f32, ActionKindV3)>);
