use crate::{
    components::*,
    items::{Equipment, Inventory, ItemPickup},
    resources::*,
    effective_attack,
    calculate_visible_tiles, MAP_HEIGHT, MAP_WIDTH, Tile, Map,
};
// use crate::world; // 已移除
use bevy_ecs::prelude::*;

/// 玩家移动 + bump 攻击（仅在 CanMove execute 中调用）
pub fn movement_system(
    mut player_query: Query<
        (&mut Position, &mut MovingDir, &Stats, &Inventory, &Equipment, Option<&Buffs>, &AttackName),
        (With<Player>, Without<Monster>),
    >,
    mut monster_query: Query<(&mut Stats, &EntityName, &Position, Entity, Option<&LootTable>), (With<Monster>, Without<Player>)>,
    map: Res<Map>,
    occupancy: Res<OccupancyMap>,
    mut commands: Commands,
    mut pending: ResMut<PendingExp>,
    mut event_log: ResMut<EventLog>,
) {
    for (mut pos, mut dir, player_stats, inv, equip, buffs, player_atk) in player_query.iter_mut() {
        if dir.dx == 0 && dir.dy == 0 { continue; }
        let new_x = pos.x.wrapping_add_signed(dir.dx);
        let new_y = pos.y.wrapping_add_signed(dir.dy);
        dir.dx = 0; dir.dy = 0;
        if new_x >= MAP_WIDTH || new_y >= MAP_HEIGHT { continue; }
        if map.tiles[new_y][new_x] != Tile::Floor { continue; }
        if let Some(entity) = &occupancy.cells[new_y][new_x] {
            if let Ok((mut monster_stats, mon_name, monster_pos, mon_entity, loot_table)) = monster_query.get_mut(*entity) {
                let atk = effective_attack(player_stats, inv, equip, buffs) as i32;
                let def = monster_stats.defense as i32;
                let mut dmg = (atk - def).max(1);
                let is_crit = player_stats.crit_rate > rand::random::<f32>();
                if is_crit { dmg = (dmg as f32 * (1.0 + player_stats.crit_damage)).round() as i32; }
                monster_stats.hp -= dmg;
                if monster_stats.hp <= 0 {
                    // 经验
                    pending.amount += monster_stats.exp;
                    event_log.push(format!("你{}击杀了{}！获得{}经验", player_atk.0, mon_name.0, monster_stats.exp));

                    // 掉落
                    if let Some(loot_table) = loot_table {
                        let loot = loot_table.roll();
                        for stack in &loot {
                            let name = stack.name();
                            event_log.push(format!("{}掉落{}x{}", mon_name.0, name, stack.count));
                            commands.spawn((
                                ItemPickup { stack: stack.clone() },
                                Position { x: monster_pos.x, y: monster_pos.y },
                                Renderable { glyph: stack.glyph(), color: stack.color() },
                            ));
                        }
                    }

                    commands.entity(mon_entity).despawn();
                } else {
                    let crit_tag = if is_crit { "！暴击" } else { "" };
                    event_log.push(format!("你{}了{}{}，造成{}点伤害", player_atk.0, mon_name.0, crit_tag, dmg));
                }
                continue;
            }
        }
        pos.x = new_x; pos.y = new_y;
    }
}

pub fn fov_system(mut query: Query<(&Position, &mut Viewshed)>, map: Res<Map>) {
    for (pos, mut viewshed) in query.iter_mut() {
        viewshed.visible_tiles = calculate_visible_tiles(pos.x, pos.y, viewshed.range, &map);
    }
}

pub fn check_death_system(
    player_query: Query<&Stats, With<Player>>,
    mut turn_manager: ResMut<TurnManager>,
) {
    if let Ok(stats) = player_query.single() {
        if stats.hp <= 0 { turn_manager.game_over = true; }
    }
}



pub fn apply_exp_system(
    mut player_query: Query<&mut Stats, With<Player>>,
    mut pending: ResMut<PendingExp>,
    mut pending_lv: ResMut<PendingLevelUp>,
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
            pending_lv.points += 3;
            player.max_hp = crate::max_hp_for(player.level, player.defense);
            player.max_mp = crate::max_mp_for(player.level, player.magic_mastery);
            player.hp = player.max_hp; player.mp = player.max_mp;
            player.exp_to_next = crate::exp_to_next_level(player.level);
            event_log.push(format!("升级！达到 Lv.{}", player.level));
        }
    }
}

pub fn buff_tick_system(mut query: Query<&mut Buffs, With<Player>>) {
    for mut b in query.iter_mut() {
        if b.shield_turns > 0 { b.shield_turns -= 1; if b.shield_turns <= 0 { b.shield_def = 0; } }
        if b.berserk_turns > 0 { b.berserk_turns -= 1; if b.berserk_turns <= 0 { b.berserk_atk = 0; } }
    }
}




