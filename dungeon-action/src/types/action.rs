//! Core action value types and the behavior interface.

use bevy_ecs::prelude::*;

/// Reaction time derived from agility.
pub fn agility_to_reaction(agility: u32) -> f32 {
    (100.0 - agility as f32 * 3.0).max(20.0)
}

/// Speed factor derived from agility.
pub fn agility_speed_factor(agility: u32) -> f32 {
    (1.0 - agility as f32 * 0.02).max(0.5)
}

#[derive(Clone, Debug, PartialEq)]
pub enum ActionKindV3 {
    Move { dx: isize, dy: isize },
    Chase,
    Flee,
    Wander,
    Wait,
    Attack { target: Entity },
    Skill(usize),
    Throw { tx: usize, ty: usize },
}

/// Monster behavior trait.
///
/// This is the "new" behavior interface. It currently coexists with `ActionKindV3`;
/// the refactor should eventually make this the single runtime interface or remove it.
pub trait GameAction: Send + Sync + std::fmt::Debug {
    fn execute(&self, world: &mut World, entity: Entity);
    fn check_condition(&self, world: &World, entity: Entity) -> bool;
    fn display_name(&self) -> &'static str;
    fn priority(&self) -> u32;
    fn av_cost(&self, agility: u32) -> f32;
    fn clone_box(&self) -> Box<dyn GameAction>;
    fn as_any(&self) -> &dyn std::any::Any;
}

#[derive(Clone, Debug)]
pub struct ChaseAction;

#[derive(Clone, Debug)]
pub struct FleeAction;

#[derive(Clone, Debug)]
pub struct WanderAction;
