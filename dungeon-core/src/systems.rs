use crate::{
    ai::{AiBehavior, MonsterBrain},
    components::*,
    items::{Equipment, Inventory, ItemPickup},
    pathfinding::find_path,
    resources::*,
    effective_attack, effective_defense, equipment_bonus,
    calculate_visible_tiles, MAP_HEIGHT, MAP_WIDTH, Tile, Map,
};
use bevy_ecs::prelude::*;
use rand::RngExt;

/// 玩家移动 + bump 攻击
pub fn movement_system(
    mut player_query: Query<
        (&mut crate::Position, &mut MovingDir, &Stats, &Inventory, &Equipment, Option<&Buffs>),
        (With<Player>, Without<Monster>),
    >,
    mut monster_query: Query<(&mut Stats, &EntityName, Entity), (With<Monster>, Without<Player>)>,
    mut item_query: Query<(Entity, &ItemPickup, &crate::Position), (Without<Player>, Without<Monster>)>,
    map: Res<Map>,
    occupancy: Res<OccupancyMap>,
    mut commands: Commands,
    mut pending: ResMut<PendingExp>,
    mut event_log: ResMut<EventLog>,
    mut pending_pickup: ResMut<PendingPickup>,
) {
    for (mut pos, mut dir, player_stats, inv, equip, buffs) in player_query.iter_mut() {
        if dir.dx == 0 && dir.dy == 0 { continue; }
        let new_x = pos.x.wrapping_add_signed(dir.dx);
        let new_y = pos.y.wrapping_add_signed(dir.dy);
        dir.dx = 0; dir.dy = 0;
        if new_x >= MAP_WIDTH || new_y >= MAP_HEIGHT { continue; }
        if map.tiles[new_y][new_x] != Tile::Floor { continue; }
        if let Some(entity) = &occupancy.cells[new_y][new_x] {
            if let Ok((mut monster_stats, mon_name, _monster_e)) = monster_query.get_mut(*entity) {
                let atk = effective_attack(player_stats, inv, equip, buffs) as i32;
                let dmg = (atk - monster_stats.defense() as i32).max(1);
                monster_stats.hp -= dmg;
                if monster_stats.hp <= 0 {
                    pending.amount += monster_stats.exp;
                    event_log.push(format!("你击杀了{}！获得{}经验", mon_name.0, monster_stats.exp));
                    commands.entity(*entity).despawn();
                } else {
                    event_log.push(format!("你攻击了{}，造成{}点伤害", mon_name.0, dmg));
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

/// FOV
pub fn fov_system(mut query: Query<(&crate::Position, &mut Viewshed)>, map: Res<Map>) {
    for (pos, mut viewshed) in query.iter_mut() {
        viewshed.visible_tiles = calculate_visible_tiles(pos.x, pos.y, viewshed.range, &map);
    }
}

/// 行动轴
pub fn tick_action_system(mut query: Query<&mut ActionPoints>) {
    for mut ap in query.iter_mut() { ap.points = (ap.points + ap.speed).min(200.0); }
}

/// 死亡检测
pub fn check_death_system(
    player_query: Query<&Stats, With<Player>>,
    mut turn_manager: ResMut<TurnManager>,
) {
    if let Ok(stats) = player_query.single() {
        if stats.hp <= 0 { turn_manager.game_over = true; }
    }
}

/// 怪物 AI（优先级行为链）
pub fn monster_ai_system(
    mut monster_query: Query<(
        &mut crate::Position, &mut Viewshed, &Stats, &MonsterBrain, &EntityName, &mut FleeLogState,
    ), (With<Monster>, Without<Player>)>,
    mut player_query: Query<(&crate::Position, &mut Stats), (With<Player>, Without<Monster>)>,
    map: Res<Map>, occupancy: Res<OccupancyMap>,
    mut game_rng: ResMut<GameRng>, mut event_log: ResMut<EventLog>,
) {
    let Ok((player_pos, mut player_stats)) = player_query.single_mut() else { return; };
    let dirs: [(isize, isize); 4] = [(0, -1), (0, 1), (-1, 0), (1, 0)];
    for (mut pos, mut viewshed, monster_stats, brain, name, mut flee_state) in monster_query.iter_mut() {
        viewshed.visible_tiles = calculate_visible_tiles(pos.x, pos.y, viewshed.range, &map);
        let can_see_player = viewshed.visible_tiles.contains(&(player_pos.x, player_pos.y));
        let mut acted = false;
        for behavior in &brain.behaviors {
            if acted { break; }
            match behavior {
                AiBehavior::FleeWhenHurt { hp_threshold } => {
                    if (monster_stats.hp as f32) >= (monster_stats.max_hp as f32) * hp_threshold {
                        flee_state.last_turn_was_flee = false; continue;
                    }
                    let mut best: Option<(usize, usize)> = None; let mut best_dist: usize = 0;
                    for &(dx, dy) in &dirs {
                        let nx = pos.x.wrapping_add_signed(dx); let ny = pos.y.wrapping_add_signed(dy);
                        if nx < MAP_WIDTH && ny < MAP_HEIGHT && map.tiles[ny][nx] == Tile::Floor && !occupancy.is_occupied(nx, ny) {
                            let d = nx.abs_diff(player_pos.x) + ny.abs_diff(player_pos.y);
                            if d > best_dist { best_dist = d; best = Some((nx, ny)); }
                        }
                    }
                    if let Some((nx, ny)) = best {
                        pos.x = nx; pos.y = ny;
                        if !flee_state.last_turn_was_flee { event_log.push(format!("{} 逃走了", name.0)); }
                        flee_state.last_turn_was_flee = true; acted = true;
                    } else { flee_state.last_turn_was_flee = false; }
                }
                AiBehavior::ChasePlayer => {
                    flee_state.last_turn_was_flee = false;
                    if !can_see_player { continue; }
                    let dist = pos.x.abs_diff(player_pos.x) + pos.y.abs_diff(player_pos.y);
                    if dist <= 1 {
                        let dmg = (monster_stats.attack() as i32 - player_stats.defense() as i32).max(1);
                        player_stats.hp -= dmg;
                        event_log.push(format!("{} 攻击了你，造成 {} 点伤害", name.0, dmg));
                        acted = true;
                    } else if let Some(path) = find_path((pos.x, pos.y), (player_pos.x, player_pos.y), &map, &occupancy, true) {
                        if path.len() > 1 {
                            let next = path[1];
                            if !occupancy.is_occupied(next.0, next.1) || (next.0, next.1) == (player_pos.x, player_pos.y) {
                                pos.x = next.0; pos.y = next.1; acted = true;
                            }
                        }
                    }
                }
                AiBehavior::Wander => {
                    flee_state.last_turn_was_flee = false;
                    let (dx, dy) = dirs[game_rng.rng.random_range(0..4)];
                    let nx = pos.x.wrapping_add_signed(dx); let ny = pos.y.wrapping_add_signed(dy);
                    if nx < MAP_WIDTH && ny < MAP_HEIGHT && map.tiles[ny][nx] == Tile::Floor && !occupancy.is_occupied(nx, ny) {
                        pos.x = nx; pos.y = ny;
                    }
                    acted = true;
                }
            }
        }
    }
}

/// 拾取物品
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

/// 经验发放 + 升级
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
            player.max_hp = crate::max_hp_for(player.level, player.vitality);
            player.max_mp = crate::max_mp_for(player.level, player.intelligence);
            player.hp = player.max_hp; player.mp = player.max_mp;
            player.exp_to_next = crate::exp_to_next_level(player.level);
            event_log.push(format!("升级！达到 Lv.{}", player.level));
        }
    }
}

/// Buff 递减
pub fn buff_tick_system(mut query: Query<&mut Buffs, With<Player>>) {
    for mut b in query.iter_mut() {
        if b.shield_turns > 0 { b.shield_turns -= 1; if b.shield_turns <= 0 { b.shield_def = 0; } }
        if b.berserk_turns > 0 { b.berserk_turns -= 1; if b.berserk_turns <= 0 { b.berserk_atk = 0; } }
    }
}

/// 技能执行
pub fn skill_tick_system(
    mut pending: ResMut<PendingSkill>,
    mut player: Query<(&mut Stats, &Skills, &mut Buffs, &crate::Position), (With<Player>, Without<Monster>)>,
    mut monsters: Query<(&mut Stats, &crate::Position, &EntityName), (With<Monster>, Without<Player>)>,
    mut event_log: ResMut<EventLog>,
) {
    let Some(skill_idx) = pending.idx.take() else { return; };
    let Ok((mut stats, skills, mut buffs, pp)) = player.get_single_mut() else { return; };
    let Some(sk) = skills.list.get(skill_idx) else { return; };
    if stats.mp < sk.cost_mp { event_log.push(format!("MP 不足，无法释放 {}", sk.name)); return; }
    stats.mp -= sk.cost_mp;
    match &sk.kind {
        SkillKind::Heal { amount } => {
            stats.hp = (stats.hp + amount).min(stats.max_hp);
            event_log.push(format!("释放 {}，HP+{}", sk.name, amount));
        }
        SkillKind::Firebolt { damage } => {
            let mut hit = false;
            for (mut ms, mp, mn) in monsters.iter_mut() {
                if pp.x.abs_diff(mp.x) + pp.y.abs_diff(mp.y) <= 1 {
                    let dmg = (damage - ms.defense() as i32).max(1); ms.hp -= dmg;
                    event_log.push(format!("火球击中 {}！造成 {} 点伤害", mn.0, dmg));
                    hit = true;
                }
            }
            if !hit { event_log.push(String::from("附近没有敌人可以攻击")); }
        }
        SkillKind::Shield { def_boost, duration } => {
            buffs.shield_turns = *duration; buffs.shield_def = *def_boost;
            event_log.push(format!("释放 {}，DEF+{} 持续{}回合", sk.name, def_boost, duration));
        }
        SkillKind::Berserk { atk_boost, duration } => {
            buffs.berserk_turns = *duration; buffs.berserk_atk = *atk_boost;
            event_log.push(format!("释放 {}，ATK+{} 持续{}回合", sk.name, atk_boost, duration));
        }
    }
}

/// 设置技能（按键调用）
pub fn apply_skill(world: &mut World, skill_idx: usize) {
    world.resource_mut::<PendingSkill>().idx = Some(skill_idx);
}
