//! Player tap-tap action handling for the ECS-native action model.

use crate::state_action::*;
use crate::types::*;
use bevy_ecs::prelude::*;
use dungeon_core::{Equipment, MAP_HEIGHT, MAP_WIDTH, Map, Monster, OccupancyMap, components::*};

fn mount_player_action(world: &mut World, entity: Entity, action: TimedPlayerAction, av: f32) {
    clear_concrete_actions(world, entity);
    if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
        entity_mut.remove::<Active>();
        entity_mut.remove::<ActionTimer>();
    }
    start_action(world, entity, av);

    let mut entity_mut = world.entity_mut(entity);
    match action {
        TimedPlayerAction::Move { dx, dy } => entity_mut.insert(Move { dx, dy }),
        TimedPlayerAction::Wait => entity_mut.insert(Wait),
        TimedPlayerAction::Attack { target } => entity_mut.insert(Attack { target }),
        TimedPlayerAction::Skill(index) => entity_mut.insert(Skill(index)),
    };
}

/// Tap-tap core: returns `true` when the action is confirmed and mounted.
pub fn handle_timed_action(
    world: &mut World,
    entity: Entity,
    action: TimedPlayerAction,
    av: f32,
) -> bool {
    let is_confirm = world.resource::<PlayerPreview>().action.as_ref() == Some(&action);

    if is_confirm {
        mount_player_action(world, entity, action, av);
        world.resource_mut::<PlayerPreview>().action = None;
        true
    } else {
        world.resource_mut::<PlayerPreview>().action = Some(action);
        false
    }
}

/// Direction key tap-tap: returns `true` when the action is confirmed and mounted.
pub fn handle_player_direction(world: &mut World, dx: isize, dy: isize) -> bool {
    let Some(entity) = dungeon_core::ops::player_entity(world) else {
        return false;
    };

    let action = {
        let Some(pos) = world.get::<Position>(entity) else {
            return false;
        };
        let nx = pos.x.wrapping_add_signed(dx);
        let ny = pos.y.wrapping_add_signed(dy);
        if nx >= MAP_WIDTH || ny >= MAP_HEIGHT {
            return false;
        }
        let tile = world.resource::<Map>().tiles[ny][nx];
        let has_enemy = world.resource::<OccupancyMap>().cells[ny][nx].and_then(|e| {
            if world.get::<Monster>(e).is_some() {
                Some(e)
            } else {
                None
            }
        });
        if !tile.walkable() && has_enemy.is_none() {
            return false;
        }
        if let Some(target) = has_enemy {
            TimedPlayerAction::Attack { target }
        } else {
            if !crate::execute::can_move_to(
                world.resource::<Map>(),
                world.resource::<OccupancyMap>(),
                pos.x,
                pos.y,
                dx,
                dy,
            ) {
                return false;
            }
            TimedPlayerAction::Move { dx, dy }
        }
    };

    let agility = world.get::<Stats>(entity).map(|s| s.agility).unwrap_or(10);
    let reaction_time = crate::types::agility_to_reaction(agility);
    let weapon_speed = world
        .get::<Equipment>(entity)
        .and_then(|eq| eq.main_hand.as_ref())
        .and_then(|s| s.def().and_then(|d| d.speed))
        .unwrap_or(300) as f32;
    let duration = weapon_speed * crate::types::agility_speed_factor(agility);
    let av = reaction_time + duration;

    handle_timed_action(world, entity, action, av)
}

/// Handles wait key.
pub fn handle_wait(world: &mut World) -> bool {
    if let Some(e) = dungeon_core::ops::player_entity(world) {
        let agility = world.get::<Stats>(e).map(|s| s.agility).unwrap_or(10);
        let reaction_time = crate::types::agility_to_reaction(agility);
        let duration = world
            .get::<CanWait>(e)
            .map(|w| w.duration * crate::types::agility_speed_factor(agility))
            .unwrap_or(800.0);
        handle_timed_action(world, e, TimedPlayerAction::Wait, reaction_time + duration)
    } else {
        false
    }
}

/// Handles skill key.
pub fn handle_skill(world: &mut World, idx: usize) -> bool {
    if let Some(e) = dungeon_core::ops::player_entity(world) {
        let key_char = char::from_digit(idx as u32 + 1, 10).unwrap_or('1');
        let real_idx = world
            .get::<dungeon_core::Skills>(e)
            .and_then(|s| s.index_of_key(key_char));
        let Some(real_idx) = real_idx else {
            world
                .resource_mut::<dungeon_core::EventLog>()
                .push(dungeon_core::EventMessage::skill(format!(
                    "技能 {} 未学习",
                    key_char
                )));
            return false;
        };
        let agility = world.get::<Stats>(e).map(|s| s.agility).unwrap_or(10);
        let reaction_time = crate::types::agility_to_reaction(agility);
        handle_timed_action(
            world,
            e,
            TimedPlayerAction::Skill(real_idx),
            reaction_time + 600.0 * crate::types::agility_speed_factor(agility),
        )
    } else {
        false
    }
}
