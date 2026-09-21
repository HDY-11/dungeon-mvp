//! Decision system for the ECS-native action model.
//!
//! This is the serial arbitration layer: it queries `Idle` / `Failure` entities that have
//! capability components (`Can*`), selects the best action, and mounts the concrete action
//! component plus `Active` and `ActionTimer`.

use super::components::*;
use super::runtime::{clear_concrete_actions, start_action};
use crate::types::*;
use bevy_ecs::prelude::*;
use bevy_ecs::query::Or;
use dungeon_core::{FLEE_HP_RATIO, LastKnownPlayerPos, Player, Position, Stats, Viewshed};

/// Selects and mounts actions for all eligible monsters.
pub fn decide_monster_actions(world: &mut World) {
    let candidates: Vec<Entity> = {
        let mut query = world.query_filtered::<Entity, (Or<(With<Idle>, With<Failure>)>, Or<(With<CanChase>, With<CanFlee>, With<CanWander>)>, Without<Active>)>();
        query.iter(world).collect()
    };

    for entity in candidates {
        if let Some(selected) = choose_best_action(world, entity) {
            clear_concrete_actions(world, entity);
            start_action(world, entity, selected.av);

            let mut entity_mut = world.entity_mut(entity);
            match selected.kind {
                SelectedAction::Chase => entity_mut.insert(Chase),
                SelectedAction::Flee => entity_mut.insert(Flee),
                SelectedAction::Wander => entity_mut.insert(Wander),
            };
        }
    }
}

enum SelectedAction {
    Chase,
    Flee,
    Wander,
}

struct Selected {
    kind: SelectedAction,
    av: f32,
}

fn choose_best_action(world: &mut World, entity: Entity) -> Option<Selected> {
    let agility = world.get::<Stats>(entity).map(|s| s.agility).unwrap_or(10);
    let player_pos = world
        .query_filtered::<&Position, With<Player>>()
        .iter(world)
        .next()
        .map(|p| p.to_tuple());

    // Chase
    let chase = world.get::<CanChase>(entity).and_then(|can_chase| {
        let can_see = player_pos.is_some_and(|pp| {
            world
                .get::<Viewshed>(entity)
                .map(|v| v.can_see(pp))
                .unwrap_or(false)
        });
        let has_memory = world
            .get::<LastKnownPlayerPos>(entity)
            .map(|l| l.0.is_some())
            .unwrap_or(false);
        if CanChase::condition(can_see) || has_memory {
            Some(Selected {
                kind: SelectedAction::Chase,
                av: action_av(can_chase.duration, agility),
            })
        } else {
            None
        }
    });

    // Flee
    let flee = world.get::<CanFlee>(entity).and_then(|can_flee| {
        let hp_ratio = world
            .get::<Stats>(entity)
            .map(|s| s.hp_ratio())
            .unwrap_or(1.0);
        if CanFlee::condition(hp_ratio) && hp_ratio < FLEE_HP_RATIO {
            Some(Selected {
                kind: SelectedAction::Flee,
                av: action_av(can_flee.duration, agility),
            })
        } else {
            None
        }
    });

    // Wander
    let wander = world.get::<CanWander>(entity).map(|can_wander| Selected {
        kind: SelectedAction::Wander,
        av: action_av(can_wander.duration, agility),
    });

    // Priority: Flee > Chase > Wander
    if flee.is_some() {
        flee
    } else if chase.is_some() {
        chase
    } else {
        wander
    }
}

fn action_av(duration: f32, agility: u32) -> f32 {
    crate::types::agility_to_reaction(agility) + duration * crate::types::agility_speed_factor(agility)
}
