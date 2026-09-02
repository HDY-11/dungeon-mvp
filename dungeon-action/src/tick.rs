//! Serial game loop for the ECS-native action model.

use bevy_ecs::prelude::*;
use dungeon_core::OptionLogExt;

/// Advances until the player's mounted action has executed.
pub fn advance_until_player_acted(world: &mut World) {
    loop {
        let dist = crate::state_action::next_action_distance(world).unwrap_or(0.0);
        if dist <= 0.0 {
            break;
        }

        crate::state_action::advance_action_timers(world, dist);

        // Keep buff timers synchronized with the same AV distance.
        {
            let mut query = world.query::<&mut dungeon_core::ActiveBuffs>();
            for mut buffs in query.iter_mut(world) {
                buffs.0.retain_mut(|buff| {
                    buff.remaining_av -= dist;
                    buff.remaining_av > 0.0
                });
            }
        }

        crate::state_action::execute_ready_actions(world);

        let player_done = {
            let player = world
                .try_query::<(Entity, &dungeon_core::Player)>()
                .expect_log("Entity+Player registered at init")
                .iter(world)
                .next()
                .map(|(entity, _)| entity);
            match player {
                Some(player) => world.get::<crate::state_action::Active>(player).is_none(),
                None => true,
            }
        };

        if player_done {
            break;
        }
    }
}
