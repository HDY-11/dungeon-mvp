//! 游戏循环编排

use dungeon_core::{
    world, action_types::*, components::*, ops,
};
use bevy_ecs::prelude::*;
use bevy_ecs::system::RunSystemOnce;
use crate::execute::advance_action_queue;
use crate::monster::run_monster_decision;

/// 持续推进直到玩家的行动被执行
pub fn advance_until_player_acted() {
    loop {
        let dist = advance_action_queue();
        if dist <= 0.0 { break; }
        let player_done = {
            let w = world!();
            let player = w.try_query::<(Entity, &Player)>().unwrap().iter(&w).next().map(|(e, _)| e);
            match player {
                Some(p) => !w.resource::<ActionQueue>().has_entity(p),
                None => true,
            }
        };
        if player_done { break; }
    }
}

/// 推进行动队列 → 怪物决策 → 刷新辅助系统
pub fn advance_and_settle() {
    advance_until_player_acted();
    run_monster_decision();

    ops::rebuild_occupancy();
    let _ = world!(mut).run_system_once(dungeon_core::systems::fov_system);
    ops::update_map_memory();
    ops::update_visible_memory();
    let _ = world!(mut).run_system_once(dungeon_core::systems::check_death_system);
    let _ = world!(mut).run_system_once(dungeon_core::systems::buff_tick_system);
}
