use crate::{
    components::*,
    items::{Equipment, Inventory, ItemPickup},
    resources::*,
    effective_attack, effective_defense,
    calculate_visible_tiles, MAP_HEIGHT, MAP_WIDTH, Tile, Map,
};
use crate::world;
use bevy_ecs::prelude::*;
use bevy_ecs::system::RunSystemOnce;

/// 玩家移动 + bump 攻击（仅在 CanMove execute 中调用）
pub fn movement_system(
    mut player_query: Query<
        (&mut Position, &mut MovingDir, &Stats, &Inventory, &Equipment, Option<&Buffs>, &AttackName),
        (With<Player>, Without<Monster>),
    >,
    mut monster_query: Query<(&mut Stats, &EntityName, Entity), (With<Monster>, Without<Player>)>,
    item_query: Query<(Entity, &ItemPickup, &Position), (Without<Player>, Without<Monster>)>,
    map: Res<Map>,
    occupancy: Res<OccupancyMap>,
    mut commands: Commands,
    mut pending: ResMut<PendingExp>,
    mut event_log: ResMut<EventLog>,
    mut pending_pickup: ResMut<PendingPickup>,
) {
    for (mut pos, mut dir, player_stats, inv, equip, buffs, player_atk) in player_query.iter_mut() {
        if dir.dx == 0 && dir.dy == 0 { continue; }
        let new_x = pos.x.wrapping_add_signed(dir.dx);
        let new_y = pos.y.wrapping_add_signed(dir.dy);
        dir.dx = 0; dir.dy = 0;
        if new_x >= MAP_WIDTH || new_y >= MAP_HEIGHT { continue; }
        if map.tiles[new_y][new_x] != Tile::Floor { continue; }
        if let Some(entity) = &occupancy.cells[new_y][new_x] {
            if let Ok((mut monster_stats, mon_name, _monster_e)) = monster_query.get_mut(*entity) {
                let atk = effective_attack(player_stats, inv, equip, buffs) as i32;
                let def = monster_stats.defense as i32;
                let mut dmg = (atk - def).max(1);
                let is_crit = player_stats.crit_rate > rand::random::<f32>();
                if is_crit { dmg = (dmg as f32 * (1.0 + player_stats.crit_damage)).round() as i32; }
                monster_stats.hp -= dmg;
                if monster_stats.hp <= 0 {
                    pending.amount += monster_stats.exp;
                    event_log.push(format!("你{}击杀了{}！获得{}经验", player_atk.0, mon_name.0, monster_stats.exp));
                    commands.entity(*entity).despawn();
                } else {
                    let crit_tag = if is_crit { "！暴击" } else { "" };
                    event_log.push(format!("你{}了{}{}，造成{}点伤害", player_atk.0, mon_name.0, crit_tag, dmg));
                }
                continue;
            }
        }
        for (item_entity, pickup, item_pos) in item_query.iter() {
            if item_pos.x == new_x && item_pos.y == new_y && inv.items.len() < inv.capacity {
                pending_pickup.entries.push((item_entity, pickup.item.clone()));
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

pub fn pickup_system(
    mut player_query: Query<&mut Inventory, With<Player>>,
    mut pending: ResMut<PendingPickup>, mut commands: Commands, mut event_log: ResMut<EventLog>,
) {
    if pending.entries.is_empty() { return; }
    if let Ok(mut inv) = player_query.single_mut() {
        for (entity, item) in pending.entries.drain(..) {
            if inv.items.len() < inv.capacity {
                let name = item.name.clone(); inv.items.push(item);
                commands.entity(entity).despawn();
                event_log.push(format!("拾取了 {}", name));
            }
        }
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

pub fn skill_tick_system(
    mut pending: ResMut<PendingSkill>,
    mut player: Query<(&mut Stats, &Skills, &mut Buffs, &Position, &PlayerClass), (With<Player>, Without<Monster>)>,
    mut monsters: Query<(&mut Stats, &Position, &EntityName), (With<Monster>, Without<Player>)>,
    mut event_log: ResMut<EventLog>,
    mut game_rng: ResMut<GameRng>,
) {
    let Some(skill_idx) = pending.idx.take() else { return; };
    let Ok((mut stats, skills, mut buffs, pp, class)) = player.single_mut() else { return; };
    let Some(sk) = skills.list.get(skill_idx) else { return; };
    if !class.can_cast(sk) { return; }
    if stats.mp < sk.cost_mp { event_log.push(format!("MP 不足")); return; }
    stats.mp -= sk.cost_mp;
    match &sk.kind {
        SkillKind::Heal { amount } => {
            let total = amount + (stats.magic_mastery as f32 * 1.0) as i32;
            stats.hp = (stats.hp + total).min(stats.max_hp);
            event_log.push(format!("释放 {}，HP+{}", sk.name, total));
        }
        SkillKind::Firebolt { damage } => {
            let total_dmg = damage + (stats.magic_mastery as f32 * 0.5) as i32;
            let mut hit = false;
            for (mut ms, mp, mn) in monsters.iter_mut() {
                if pp.x.abs_diff(mp.x) + pp.y.abs_diff(mp.y) <= 1 {
                    let is_crit = stats.crit_rate > rand::random::<f32>();
                    let mut dmg = (total_dmg - ms.defense as i32).max(1);
                    if is_crit { dmg = (dmg as f32 * (1.0 + stats.crit_damage)).round() as i32; }
                    ms.hp -= dmg;
                    event_log.push(format!("火球击中 {}！{}伤", mn.0, dmg));
                    hit = true;
                }
            }
            if !hit { event_log.push(String::from("附近没有敌人")); }
        }
        _ => {}
    }
}

pub fn apply_skill(skill_idx: usize) {
    world!(mut).resource_mut::<PendingSkill>().idx = Some(skill_idx);
}
