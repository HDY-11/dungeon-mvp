//! 游戏循环编排（串行版本，保留兼容）

use crate::execute::advance_action_queue;
use bevy_ecs::prelude::*;
use dungeon_core::OptionLogExt;

/// 持续推进直到玩家的行动被执行
pub fn advance_until_player_acted(world: &mut World) {
    loop {
        let dist = advance_action_queue(world);
        if dist <= 0.0 {
            break;
        }
        let player_done = {
            let player = world
                .try_query::<(Entity, &dungeon_core::Player)>()
                .expect_log("Entity+Player registered at init")
                .iter(world)
                .next()
                .map(|(e, _)| e);
            match player {
                Some(p) => !world.resource::<crate::types::ActionQueue>().has_entity(p),
                None => true,
            }
        };
        if player_done {
            break;
        }
    }
}
