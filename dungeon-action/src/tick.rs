//! 游戏循环编排（串行版本，保留兼容）

use dungeon_core::ops;
use bevy_ecs::prelude::*;
use bevy_ecs::system::RunSystemOnce;
use crate::execute::advance_action_queue;
use crate::monster::run_monster_decision;

/// 持续推进直到玩家的行动被执行
pub fn advance_until_player_acted(world: &mut World) {
    loop {
        let dist = advance_action_queue(world);
        if dist <= 0.0 { break; }
        let player_done = {
            let player = world.try_query::<(Entity, &dungeon_core::Player)>().unwrap().iter(world).next().map(|(e, _)| e);
            match player {
                Some(p) => !world.resource::<dungeon_core::ActionQueue>().has_entity(p),
                None => true,
            }
        };
        if player_done { break; }
    }
}

/// 串行 tick（旧版，保持兼容）
pub fn advance_and_settle(world: &mut World) {
    advance_until_player_acted(world);
    run_monster_decision(world);

    ops::rebuild_occupancy(world);
    let _ = world.run_system_once(dungeon_core::systems::fov_system);
    ops::update_map_memory(world);
    ops::update_visible_memory(world);
    let _ = world.run_system_once(dungeon_core::systems::check_death_system);
    let _ = world.run_system_once(dungeon_core::systems::buff_tick_system);
}
