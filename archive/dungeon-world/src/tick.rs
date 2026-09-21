//! World tick for the ECS-native action model.

use bevy_ecs::prelude::*;
use dungeon_action::{advance_until_player_acted, decide_monster_actions};
use dungeon_core::ops;
use dungeon_core::systems::{check_death_system, fov_system};

fn build_settle_schedule() -> Schedule {
    let mut schedule = Schedule::default();
    schedule.add_systems((fov_system, check_death_system));
    schedule
}

pub fn advance_and_settle_parallel(world: &mut World) {
    advance_until_player_acted(world);
    decide_monster_actions(world);

    let mut schedule = build_settle_schedule();
    schedule.run(world);

    ops::update_map_memory(world);
    ops::update_visible_memory(world);
}
