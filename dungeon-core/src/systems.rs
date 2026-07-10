use crate::{
    components::*,
    resources::*,
    Map,
};
use bevy_ecs::prelude::*;

pub fn fov_system(mut query: Query<(&Position, &mut Viewshed)>, map: Res<Map>) {
    for (pos, mut viewshed) in query.iter_mut() {
        viewshed.visible_tiles = crate::fov::calculate_visible_tiles(pos.x, pos.y, viewshed.range, &map);
    }
}

pub fn check_death_system(
    player_query: Query<&Stats, With<Player>>,
    mut turn_manager: ResMut<TurnManager>,
    mut event_log: ResMut<EventLog>,
) {
    if let Ok(stats) = player_query.single() {
        if stats.hp <= 0 {
            event_log.push("你死了".to_string());
            turn_manager.game_over = true;
        }
    }
}

pub fn apply_exp_system(
    mut player_query: Query<&mut Stats, With<Player>>,
    mut pending: ResMut<PendingExp>,
    mut event_log: ResMut<EventLog>,
) {
    if pending.amount == 0 { return; }
    let gained = pending.amount; pending.amount = 0;
    if let Ok(mut player) = player_query.single_mut() {
        player.exp += gained;
        loop {
            if player.exp < player.exp_to_next { break; }
            player.exp -= player.exp_to_next;
            player.level += 1;
            player.max_hp = crate::max_hp_for(player.level, player.defense);
            player.max_mp = crate::max_mp_for(player.level, player.magic_mastery);
            player.hp = player.max_hp; player.mp = player.max_mp;
            player.exp_to_next = crate::exp_to_next_level(player.level);
            event_log.push(format!("升级！达到 Lv.{}", player.level));
        }
    }
}

