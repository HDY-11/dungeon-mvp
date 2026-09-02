//! Runtime lifecycle and execution for the ECS action model.

use super::components::*;
use bevy_ecs::prelude::*;

// ── Lifecycle helpers ──────────────────────────────────────────

/// Mounts `Active + ActionTimer` on an entity.
///
/// The concrete action component should be inserted by the caller before/after calling this.
/// The decision layer is responsible for removing any previous concrete action component.
pub fn start_action(world: &mut World, entity: Entity, av: f32) {
    let mut entity_mut = world.entity_mut(entity);
    entity_mut.remove::<Idle>();
    entity_mut.remove::<Failure>();
    entity_mut.insert(Active);
    entity_mut.insert(ActionTimer { remaining_av: av });
}

/// Marks the current action as successfully finished and returns the entity to `Idle`.
pub fn finish_action_success(world: &mut World, entity: Entity) {
    clear_action(world, entity);
    world.entity_mut(entity).insert(Idle);
}

/// Marks the current action as failed and returns the entity to `Failure`.
pub fn finish_action_failure(world: &mut World, entity: Entity) {
    clear_action(world, entity);
    world.entity_mut(entity).insert(Failure);
}

/// Removes every known concrete action component without touching state markers or timer.
pub fn clear_concrete_actions(world: &mut World, entity: Entity) {
    let mut entity_mut = world.entity_mut(entity);
    entity_mut.remove::<Chase>();
    entity_mut.remove::<Flee>();
    entity_mut.remove::<Wander>();
    entity_mut.remove::<Wait>();
    entity_mut.remove::<Move>();
    entity_mut.remove::<Attack>();
    entity_mut.remove::<Skill>();
    entity_mut.remove::<Throw>();
}

/// Removes all runtime action state and every known concrete action component.
fn clear_action(world: &mut World, entity: Entity) {
    clear_concrete_actions(world, entity);
    let mut entity_mut = world.entity_mut(entity);
    entity_mut.remove::<Active>();
    entity_mut.remove::<ActionTimer>();
}

// ── Timer advancement ──────────────────────────────────────────

/// Returns the smallest positive remaining AV among active entities.
pub fn next_action_distance(world: &mut World) -> Option<f32> {
    let mut query = world.query_filtered::<&ActionTimer, With<Active>>();
    query
        .iter(world)
        .map(|timer| timer.remaining_av)
        .filter(|remaining| *remaining > 0.0)
        .min_by(|a, b| a.partial_cmp(b).expect("AV values should never be NaN"))
}

/// Advances all active entities' timers by `amount`.
pub fn advance_action_timers(world: &mut World, amount: f32) {
    let mut query = world.query::<&mut ActionTimer>();
    for mut timer in query.iter_mut(world) {
        if timer.remaining_av > 0.0 {
            timer.remaining_av = (timer.remaining_av - amount).max(0.0);
        }
    }
}

/// Collects entities whose mounted action is ready to execute.
pub fn ready_entities(world: &mut World) -> Vec<Entity> {
    let mut query = world.query_filtered::<(Entity, &ActionTimer), With<Active>>();
    query
        .iter(world)
        .filter(|(_, timer)| timer.remaining_av <= 0.0)
        .map(|(entity, _)| entity)
        .collect()
}

/// Executes all ready actions and updates `Idle` / `Failure` markers.
pub fn execute_ready_actions(world: &mut World) {
    let ready = ready_entities(world);
    for entity in ready {
        let ok = execute_one(world, entity);
        if ok {
            finish_action_success(world, entity);
        } else {
            finish_action_failure(world, entity);
        }
    }
}

fn execute_one(world: &mut World, entity: Entity) -> bool {
    use crate::execute;
    use dungeon_core::{FLEE_HP_RATIO_EXIT, Map, Monster, OccupancyMap, Position, Stats};

    if world.get::<Chase>(entity).is_some() {
        if !execute::chase_condition(world, entity) {
            return false;
        }
        execute::execute_chase(world, entity);
        return true;
    }

    if world.get::<Flee>(entity).is_some() {
        let low_hp = world
            .get::<Stats>(entity)
            .map(|stats| stats.hp_ratio() < FLEE_HP_RATIO_EXIT)
            .unwrap_or(false);
        if !low_hp {
            return false;
        }
        execute::execute_flee(world, entity);
        return true;
    }

    if world.get::<Wander>(entity).is_some() {
        execute::execute_wander(world, entity);
        return true;
    }

    if world.get::<Wait>(entity).is_some() {
        return true;
    }

    if let Some(action) = world.get::<Move>(entity) {
        let can_move = world
            .get::<Position>(entity)
            .map(|pos| {
                let map = world.resource::<Map>();
                let occ = world.resource::<OccupancyMap>();
                execute::can_move_to(map, occ, pos.x, pos.y, action.dx, action.dy)
            })
            .unwrap_or(false);
        if !can_move {
            return false;
        }
        execute::execute_player_move(world, entity, action.dx, action.dy);
        return true;
    }

    if let Some(action) = world.get::<Attack>(entity) {
        let valid = world.get::<Monster>(action.target).is_some()
            && execute::adjacent_8(world, entity, action.target);
        if !valid {
            return false;
        }
        execute::execute_attack(world, entity, action.target);
        return true;
    }

    if let Some(action) = world.get::<Skill>(entity) {
        execute::execute_skill(world, entity, action.0);
        return true;
    }

    if let Some(action) = world.get::<Throw>(entity) {
        execute::execute_throw(world, entity, action.tx, action.ty);
        return true;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entity_with_idle(world: &mut World) -> Entity {
        world.spawn((Idle,)).id()
    }

    #[test]
    fn start_action_moves_idle_to_active() {
        let mut world = World::new();
        let entity = entity_with_idle(&mut world);

        start_action(&mut world, entity, 120.0);

        assert!(world.get::<Idle>(entity).is_none());
        assert!(world.get::<Active>(entity).is_some());
        assert_eq!(world.get::<ActionTimer>(entity).unwrap().remaining_av, 120.0);
    }

    #[test]
    fn finish_success_returns_to_idle_and_clears_action() {
        let mut world = World::new();
        let entity = entity_with_idle(&mut world);

        start_action(&mut world, entity, 80.0);
        world.entity_mut(entity).insert(Chase);
        finish_action_success(&mut world, entity);

        assert!(world.get::<Active>(entity).is_none());
        assert!(world.get::<ActionTimer>(entity).is_none());
        assert!(world.get::<Chase>(entity).is_none());
        assert!(world.get::<Idle>(entity).is_some());
    }

    #[test]
    fn finish_failure_returns_to_failure_and_clears_action() {
        let mut world = World::new();
        let entity = entity_with_idle(&mut world);

        start_action(&mut world, entity, 80.0);
        world.entity_mut(entity).insert(Flee);
        finish_action_failure(&mut world, entity);

        assert!(world.get::<Active>(entity).is_none());
        assert!(world.get::<Flee>(entity).is_none());
        assert!(world.get::<Failure>(entity).is_some());
    }

    #[test]
    fn execute_ready_wait_returns_to_idle() {
        let mut world = World::new();
        let entity = entity_with_idle(&mut world);

        start_action(&mut world, entity, 0.0);
        world.entity_mut(entity).insert(Wait);

        execute_ready_actions(&mut world);

        assert!(world.get::<Active>(entity).is_none());
        assert!(world.get::<Wait>(entity).is_none());
        assert!(world.get::<Idle>(entity).is_some());
    }

    #[test]
    fn timer_advances_to_next_event() {
        let mut world = World::new();
        let a = entity_with_idle(&mut world);
        let b = entity_with_idle(&mut world);

        start_action(&mut world, a, 100.0);
        start_action(&mut world, b, 300.0);

        let dist = next_action_distance(&mut world).unwrap();
        assert_eq!(dist, 100.0);

        advance_action_timers(&mut world, dist);
        assert_eq!(world.get::<ActionTimer>(a).unwrap().remaining_av, 0.0);
        assert_eq!(world.get::<ActionTimer>(b).unwrap().remaining_av, 200.0);

        let ready = ready_entities(&mut world);
        assert_eq!(ready, vec![a]);
    }
}
