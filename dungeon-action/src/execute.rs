//! 行动执行引擎：队列推进、保活检查、行动执行

use dungeon_core::{
    world, action_types::*, ops, components::*, items::*, resources::*,
    Map, Tile, MAP_WIDTH, MAP_HEIGHT,
};
use bevy_ecs::prelude::*;
use bevy_ecs::system::RunSystemOnce;

/// 推进行动队列，返回实际推进量
pub fn advance_action_queue() -> f32 {
    // 阶段 1：推进队列（持有写锁）
    let dist;
    let ready;
    {
        let mut w = world!(mut);
        dist = {
            let queue = w.resource::<ActionQueue>();
            queue.next_event_distance().unwrap_or(0.0)
        };
        if dist <= 0.0 { return 0.0; }
        w.resource_mut::<ActionQueue>().advance(dist);
        ready = w.resource_mut::<ActionQueue>().pop_ready();
    }

    // 阶段 2：保活检查 + 执行就绪条目（每次执行后重建碰撞图）
    for entry in &ready {
        if check_condition(entry) {
            execute_entry(entry);
            let _ = world!(mut).run_system_once(dungeon_core::systems::apply_exp_system);
            ops::rebuild_occupancy();
        } else {
            // 条件不再满足，丢弃行动（实体已损失 AV）
            world!(mut).resource_mut::<EventLog>().push(format!("行动被取消"));
        }
    }
    dist
}

/// 保活检查：执行前回调组件验证条件是否仍满足
fn check_condition(entry: &ActionEntry) -> bool {
    let w = world!();
    match &entry.kind {
        ActionKindV3::Chase => {
            let player_pos = w.try_query::<(&Player, &Position)>().unwrap().iter(&w).next().map(|(_, p)| (p.x, p.y));
            let Some((px, py)) = player_pos else { return false };
            w.get::<Viewshed>(entry.entity)
                .map(|v| v.visible_tiles.contains(&(px, py)))
                .unwrap_or(false)
        }
        ActionKindV3::Flee => {
            w.get::<Stats>(entry.entity)
                .map(|s| (s.hp as f32 / s.max_hp as f32) < 0.25)
                .unwrap_or(false)
        }
        ActionKindV3::Wander | ActionKindV3::Wait => true,
        ActionKindV3::Move { dx, dy } => {
            if let Some(pos) = w.get::<Position>(entry.entity) {
                let nx = pos.x.wrapping_add_signed(*dx);
                let ny = pos.y.wrapping_add_signed(*dy);
                if nx < MAP_WIDTH && ny < MAP_HEIGHT {
                    let map = w.resource::<Map>();
                    if map.tiles[ny][nx] != Tile::Floor { return false; }
                    let occ = w.resource::<OccupancyMap>();
                    if occ.is_occupied(nx, ny) { return false; }
                    true
                } else { false }
            } else { false }
        }
        ActionKindV3::Attack { target } => {
            w.get::<Monster>(*target).is_some()
        }
        ActionKindV3::Skill(_) => true,
    }
}

fn execute_entry(entry: &ActionEntry) {
    match &entry.kind {
        ActionKindV3::Chase => execute_chase(entry.entity),
        ActionKindV3::Flee => execute_flee(entry.entity),
        ActionKindV3::Wander => execute_wander(entry.entity),
        ActionKindV3::Wait => execute_wait(entry.entity),
        ActionKindV3::Move { dx, dy } => execute_player_move(entry.entity, *dx, *dy),
        ActionKindV3::Attack { target } => execute_attack(entry.entity, *target),
        ActionKindV3::Skill(idx) => execute_skill(entry.entity, *idx),
    }
}

fn execute_chase(entity: Entity) {
    let mut w = world!(mut);
    let Some(player_entity) = w.query::<(Entity, &Player)>().iter(&mut *w).next().map(|(e, _)| e) else { return };
    let player_pos = w.get::<Position>(player_entity).map(|p| (p.x, p.y));
    let Some((px, py)) = player_pos else { return };
    let pos = match w.get::<Position>(entity) {
        Some(p) => (p.x, p.y),
        None => return,
    };
    if pos.0.abs_diff(px) + pos.1.abs_diff(py) <= 1 {
        let dmg = w.get::<Stats>(entity).map(|s| s.attack as i32).unwrap_or(1);
        let name = w.get::<EntityName>(entity).map(|n| n.0.clone()).unwrap_or("怪物".into());
        if let Some(mut ps) = w.get_mut::<Stats>(player_entity) { ps.hp -= dmg.max(1); }
        w.resource_mut::<EventLog>().push(format!("{} 攻击了你，{}伤", name, dmg));
    } else {
        let dx = if px > pos.0 { 1 } else if px < pos.0 { -1 } else { 0 };
        let dy = if py > pos.1 { 1 } else if py < pos.1 { -1 } else { 0 };
        let attempts = if px.abs_diff(pos.0) >= py.abs_diff(pos.1) {
            vec![(dx, 0), (0, dy)]
        } else {
            vec![(0, dy), (dx, 0)]
        };
        for (ndx, ndy) in attempts {
            let nx = pos.0.wrapping_add_signed(ndx);
            let ny = pos.1.wrapping_add_signed(ndy);
            if nx < MAP_WIDTH && ny < MAP_HEIGHT
                && w.resource::<Map>().tiles[ny][nx] == Tile::Floor
                && !w.resource::<OccupancyMap>().is_occupied(nx, ny)
            {
                if let Some(mut p) = w.get_mut::<Position>(entity) { p.x = nx; p.y = ny; }
                break;
            }
        }
    }
}

fn execute_flee(entity: Entity) {
    let mut w = world!(mut);
    let player_pos = w.query::<(&Player, &Position)>().iter(&mut *w).next().map(|(_, p)| (p.x, p.y));
    let Some((px, py)) = player_pos else { return };
    let pos = match w.get::<Position>(entity) { Some(p) => (p.x, p.y), None => return };
    let dirs: [(isize, isize); 4] = [(0, -1), (0, 1), (-1, 0), (1, 0)];
    let mut best: Option<(usize, usize)> = None;
    let mut best_dist = 0usize;
    for &(dx, dy) in &dirs {
        let nx = pos.0.wrapping_add_signed(dx);
        let ny = pos.1.wrapping_add_signed(dy);
        if nx < MAP_WIDTH && ny < MAP_HEIGHT
            && w.resource::<Map>().tiles[ny][nx] == Tile::Floor
            && !w.resource::<OccupancyMap>().is_occupied(nx, ny)
        {
            let d = nx.abs_diff(px) + ny.abs_diff(py);
            if d > best_dist { best_dist = d; best = Some((nx, ny)); }
        }
    }
    if let Some((nx, ny)) = best {
        if let Some(mut p) = w.get_mut::<Position>(entity) { p.x = nx; p.y = ny; }
    }
}

fn execute_wander(entity: Entity) {
    let mut w = world!(mut);
    let dirs: [(isize, isize); 4] = [(0, -1), (0, 1), (-1, 0), (1, 0)];
    let r = (w.resource::<FloorNumber>().0 as usize + w.query::<(Entity, &Monster)>().iter(&mut *w).count()) % 4;
    let (dx, dy) = dirs[r];
    if let Some(pos) = w.get::<Position>(entity) {
        let nx = pos.x.wrapping_add_signed(dx);
        let ny = pos.y.wrapping_add_signed(dy);
        if nx < MAP_WIDTH && ny < MAP_HEIGHT
            && w.resource::<Map>().tiles[ny][nx] == Tile::Floor
            && !w.resource::<OccupancyMap>().is_occupied(nx, ny)
        {
            if let Some(mut p) = w.get_mut::<Position>(entity) { p.x = nx; p.y = ny; }
        }
    }
}

fn execute_wait(_entity: Entity) {}

fn execute_player_move(entity: Entity, dx: isize, dy: isize) {
    let (nx, ny) = {
        let w = world!();
        let p = match w.get::<Position>(entity) { Some(p) => (p.x, p.y), None => return };
        let nx = p.0.wrapping_add_signed(dx);
        let ny = p.1.wrapping_add_signed(dy);
        if nx >= MAP_WIDTH || ny >= MAP_HEIGHT { return; }
        if w.resource::<Map>().tiles[ny][nx] != Tile::Floor { return; }
        (nx, ny)
    };
    let mut w = world!(mut);
    if let Some(mut p) = w.get_mut::<Position>(entity) { p.x = nx; p.y = ny; }
}

fn execute_attack(attacker: Entity, target: Entity) {
    let (exp, name, atk_name, dmg, crit, target_pos);
    {
        let w = world!(mut);
        let Some(target_stats) = w.get::<Stats>(target).cloned() else { return };
        let Some(attacker_stats) = w.get::<Stats>(attacker).cloned() else { return };
        name = w.get::<EntityName>(target).map(|n| n.0.clone()).unwrap_or("怪物".into());
        atk_name = w.get::<AttackName>(attacker).map(|a| a.0.clone()).unwrap_or("攻击".into());
        target_pos = w.get::<Position>(target).map(|p| (p.x, p.y));
        let inventory = w.get::<Inventory>(attacker).unwrap();
        let equipment = w.get::<Equipment>(attacker).unwrap();
        let buffs = w.get::<Buffs>(attacker);
        let effective_atk = ops::effective_attack(&attacker_stats, inventory, equipment, buffs) as i32;
        let target_def = target_stats.defense as i32;
        let raw_dmg = (effective_atk - target_def).max(1);
        let is_crit = attacker_stats.crit_rate > rand::random::<f32>();
        dmg = if is_crit { (raw_dmg as f32 * (1.0 + attacker_stats.crit_damage)).round() as i32 } else { raw_dmg };
        crit = is_crit;
        exp = target_stats.exp;
    }
    {
        let mut w = world!(mut);
        let Some(mut target_stats) = w.get_mut::<Stats>(target) else { return };
        target_stats.hp -= dmg;
        if target_stats.hp <= 0 {
            w.resource_mut::<PendingExp>().amount += exp;
            w.resource_mut::<EventLog>().push(format!("你{}击杀了{}！获得{}经验", atk_name, name, exp));
            let loot_stacks = w.get::<LootTable>(target).map(|lt| lt.roll()).unwrap_or_default();
            if let Some((px, py)) = target_pos {
                for stack in &loot_stacks {
                    let sname = stack.name();
                    w.resource_mut::<EventLog>().push(format!("{}掉落{}x{}", name, sname, stack.count));
                    w.spawn((
                        ItemPickup { stack: stack.clone() },
                        Position { x: px, y: py },
                        Renderable { glyph: stack.glyph(), color: stack.color() },
                    ));
                }
            }
            w.entity_mut(target).despawn();
        } else {
            w.resource_mut::<EventLog>().push(format!("你{}了{}{}，造成{}点伤害", atk_name, name, if crit { "！暴击" } else { "" }, dmg));
        }
    }
}

fn execute_skill(entity: Entity, skill_idx: usize) {
    let (skill_kind, cost_mp, skill_name);
    {
        let w = world!();
        let Some(skills) = w.get::<Skills>(entity) else { return };
        let Some(skill) = skills.list.get(skill_idx) else { return };
        let Some(stats) = w.get::<Stats>(entity) else { return };
        if stats.mp < skill.cost_mp {
            let msg = format!("MP不足，无法施放{}", skill.name);
            drop(w);
            world!(mut).resource_mut::<EventLog>().push(msg);
            return;
        }
        skill_kind = skill.kind.clone();
        cost_mp = skill.cost_mp;
        skill_name = skill.name.to_string();
    }
    {
        let mut w = world!(mut);
        if let Some(mut stats) = w.get_mut::<Stats>(entity) { stats.mp -= cost_mp; }
    }
    match skill_kind {
        SkillKind::Firebolt { damage } => {
            let (pp, magic_bonus, crit_rate, crit_dmg);
            {
                let mut w = world!(mut);
                pp = w.query::<(&Player, &Position)>().iter(&mut *w).next().map(|(_, p)| (p.x, p.y));
                let stats = w.get::<Stats>(entity);
                magic_bonus = stats.map(|s| (s.magic_mastery as f32 * 0.5) as i32).unwrap_or(0);
                crit_rate = stats.map(|s| s.crit_rate).unwrap_or(0.0);
                crit_dmg = stats.map(|s| s.crit_damage).unwrap_or(0.0);
            }
            let mut hits: Vec<(Entity, i32)> = Vec::new();
            let mut hit_any = false;
            {
                let mut w = world!(mut);
                for (me, mut ms, mp, _mn) in w.query::<(Entity, &mut Stats, &Position, &EntityName)>().iter_mut(&mut *w) {
                    if let Some((px, py)) = pp {
                        if mp.x.abs_diff(px) + mp.y.abs_diff(py) <= 1 {
                            let is_crit = crit_rate > rand::random::<f32>();
                            let mut dmg = (damage + magic_bonus - ms.defense as i32).max(1);
                            if is_crit { dmg = (dmg as f32 * (1.0 + crit_dmg)).round() as i32; }
                            ms.hp -= dmg;
                            hits.push((me, dmg));
                            hit_any = true;
                        }
                    }
                }
            }
            {
                let mut w = world!(mut);
                for (me, dmg) in &hits {
                    let name = w.get::<EntityName>(*me).map(|n| n.0.clone()).unwrap_or("怪物".into());
                    let hp = w.get::<Stats>(*me).map(|s| s.hp).unwrap_or(0);
                    w.resource_mut::<EventLog>().push(format!("火球击中 {}！{}伤", name, dmg));
                    if hp <= 0 { w.entity_mut(*me).despawn(); }
                }
                if !hit_any { w.resource_mut::<EventLog>().push(String::from("附近没有敌人")); }
            }
        }
        SkillKind::Heal { amount } => {
            let mut w = world!(mut);
            if let Some(mut stats) = w.get_mut::<Stats>(entity) { stats.hp = (stats.hp + amount).min(stats.max_hp); }
            w.resource_mut::<EventLog>().push(format!("{}恢复了{}HP", skill_name, amount));
        }
        SkillKind::Shield { def_boost, duration } => {
            let mut w = world!(mut);
            if let Some(mut buffs) = w.get_mut::<dungeon_core::Buffs>(entity) {
                buffs.shield_turns = duration as i32; buffs.shield_def = def_boost;
            }
            w.resource_mut::<EventLog>().push(format!("{}施放了护盾，防御+{}持续{}回合", skill_name, def_boost, duration));
        }
        SkillKind::Berserk { atk_boost, duration } => {
            let mut w = world!(mut);
            if let Some(mut buffs) = w.get_mut::<dungeon_core::Buffs>(entity) {
                buffs.berserk_turns = duration as i32; buffs.berserk_atk = atk_boost;
            }
            w.resource_mut::<EventLog>().push(format!("{}进入狂暴，攻击+{}持续{}回合", skill_name, atk_boost, duration));
        }
    }
}
