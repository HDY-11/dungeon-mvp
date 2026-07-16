//! GameAction 实现 — ChaseAction/FleeAction/WanderAction
//!
//! 放在独立模块中，所有 impl 在 compile 时可被 crate 内其他模块发现。

use bevy_ecs::prelude::*;
use crate::types::*;
use crate::execute;

impl GameAction for ChaseAction {
    fn execute(&self, world: &mut World, entity: Entity) {
        execute::execute_chase(world, entity);
    }
    fn check_condition(&self, world: &World, entity: Entity) -> bool {
        execute::chase_condition(world, entity)
    }
    fn display_name(&self) -> &'static str { "追击" }
    fn priority(&self) -> u32 { 100 }
    fn av_cost(&self, agility: u32) -> f32 {
        250.0 * agility_speed_factor(agility)
    }
    fn clone_box(&self) -> Box<dyn GameAction> { Box::new(self.clone()) }
    fn as_any(&self) -> &dyn std::any::Any { self }
}

impl GameAction for FleeAction {
    fn execute(&self, world: &mut World, entity: Entity) {
        execute::execute_flee(world, entity);
    }
    fn check_condition(&self, world: &World, entity: Entity) -> bool {
        execute::flee_condition(world, entity)
    }
    fn display_name(&self) -> &'static str { "逃跑" }
    fn priority(&self) -> u32 { 200 }
    fn av_cost(&self, agility: u32) -> f32 {
        250.0 * agility_speed_factor(agility)
    }
    fn clone_box(&self) -> Box<dyn GameAction> { Box::new(self.clone()) }
    fn as_any(&self) -> &dyn std::any::Any { self }
}

impl GameAction for WanderAction {
    fn execute(&self, world: &mut World, entity: Entity) {
        execute::execute_wander(world, entity);
    }
    fn check_condition(&self, _world: &World, _entity: Entity) -> bool { true }
    fn display_name(&self) -> &'static str { "游荡" }
    fn priority(&self) -> u32 { 50 }
    fn av_cost(&self, agility: u32) -> f32 {
        500.0 * agility_speed_factor(agility)
    }
    fn clone_box(&self) -> Box<dyn GameAction> { Box::new(self.clone()) }
    fn as_any(&self) -> &dyn std::any::Any { self }
}
