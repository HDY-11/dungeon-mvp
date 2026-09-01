//! Action execution engine: queue advancement, keep-alive check, dispatch.

mod combat;
mod monster;
mod movement;
mod skill;
mod throw;

pub use throw::confirm_throw;

pub(crate) use combat::{adjacent_8, execute_attack};
pub(crate) use monster::{chase_condition, execute_chase, execute_flee, execute_wander, flee_condition};
pub(crate) use movement::{can_move_to, execute_player_move};
pub(crate) use skill::execute_skill;
pub(crate) use throw::execute_throw;

use crate::types::*;
use bevy_ecs::prelude::*;
use bevy_ecs::system::RunSystemOnce;
use dungeon_core::OptionLogExt;
use dungeon_core::{Map, components::*, ops, resources::*};

/// Advances the action queue and returns the actual distance advanced.
pub fn advance_action_queue(world: &mut World) -> f32 {
    let dist;
    let ready;
    {
        dist = {
            let queue = world.resource::<ActionQueue>();
            queue.next_event_distance().unwrap_or(0.0)
        };
        if dist <= 0.0 {
            return 0.0;
        }
        world.resource_mut::<ActionQueue>().advance(dist);

        {
            let mut q = world.query::<&mut ActiveBuffs>();
            for mut buffs in q.iter_mut(world) {
                buffs.0.retain_mut(|b| {
                    b.remaining_av -= dist;
                    b.remaining_av > 0.0
                });
            }
        }

        let invalid: Vec<Entity> = {
            let queue = world.resource::<ActionQueue>();
            queue
                .entries
                .iter()
                .filter(|e| e.av_remaining > 0.0 && !check_condition(world, e))
                .map(|e| e.entity)
                .collect()
        };
        if !invalid.is_empty() {
            world
                .resource_mut::<ActionQueue>()
                .entries
                .retain(|e| !invalid.contains(&e.entity));
        }

        ready = world.resource_mut::<ActionQueue>().pop_ready();
    }

    for entry in &ready {
        if check_condition(world, entry) {
            execute_entry(world, entry);
            let _ = world.run_system_once(dungeon_core::systems::apply_exp_system);
            ops::rebuild_occupancy(world);
        } else {
            world
                .resource_mut::<EventLog>()
                .push(dungeon_core::EventMessage::system("行动被取消"));
        }
    }
    dist
}

fn check_condition(world: &World, entry: &ActionEntry) -> bool {
    if let Some(ref action) = entry.action {
        return action.check_condition(world, entry.entity);
    }
    match &entry.kind {
        ActionKindV3::Chase => {
            let player_pos = world
                .try_query::<(&Player, &Position)>()
                .expect_log("Player+Position registered at init")
                .iter(world)
                .next()
                .map(|(_, p)| (p.x, p.y));
            if let Some((px, py)) = player_pos
                && world
                    .get::<Viewshed>(entry.entity)
                    .map(|v| v.visible_tiles.contains(&(px, py)))
                    .unwrap_or(false)
            {
                return true;
            }
            world
                .get::<LastKnownPlayerPos>(entry.entity)
                .map(|l| l.0.is_some())
                .unwrap_or(false)
        }
        ActionKindV3::Flee => {
            world
                .get::<Stats>(entry.entity)
                .map(|s| (s.hp as f32 / s.max_hp as f32) < dungeon_core::FLEE_HP_RATIO_EXIT)
                .unwrap_or(false)
        }
        ActionKindV3::Wander | ActionKindV3::Wait => true,
        ActionKindV3::Move { dx, dy } => {
            if let Some(pos) = world.get::<Position>(entry.entity) {
                let map = world.resource::<Map>();
                let occ = world.resource::<OccupancyMap>();
                can_move_to(map, occ, pos.x, pos.y, *dx, *dy)
            } else {
                false
            }
        }
        ActionKindV3::Attack { target } => {
            world.get::<Monster>(*target).is_some() && adjacent_8(world, entry.entity, *target)
        }
        ActionKindV3::Skill(_) => true,
        ActionKindV3::Throw { .. } => true,
    }
}

fn execute_entry(world: &mut World, entry: &ActionEntry) {
    if let Some(ref action) = entry.action {
        action.execute(world, entry.entity);
        return;
    }
    match &entry.kind {
        ActionKindV3::Chase => execute_chase(world, entry.entity),
        ActionKindV3::Flee => execute_flee(world, entry.entity),
        ActionKindV3::Wander => execute_wander(world, entry.entity),
        ActionKindV3::Wait => execute_wait(entry.entity),
        ActionKindV3::Move { dx, dy } => execute_player_move(world, entry.entity, *dx, *dy),
        ActionKindV3::Attack { target } => execute_attack(world, entry.entity, *target),
        ActionKindV3::Skill(idx) => execute_skill(world, entry.entity, *idx),
        ActionKindV3::Throw { tx, ty } => execute_throw(world, entry.entity, *tx, *ty),
    }
}

fn execute_wait(_entity: Entity) {}
