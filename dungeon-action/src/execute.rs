//! 行动执行引擎：队列推进、保活检查、行动执行

use dungeon_core::{
    action_types::*, ops, components::*, items::*, resources::*,
    Map, MAP_WIDTH, MAP_HEIGHT,
};
use bevy_ecs::prelude::*;
use bevy_ecs::system::RunSystemOnce;

/// 推进行动队列，返回实际推进量
pub fn advance_action_queue(world: &mut World) -> f32 {
    let dist;
    let ready;
    {
        dist = {
            let queue = world.resource::<ActionQueue>();
            queue.next_event_distance().unwrap_or(0.0)
        };
        if dist <= 0.0 { return 0.0; }
        world.resource_mut::<ActionQueue>().advance(dist);

        // P1: 保活检查所有 av_remaining > 0 的条目，剔除条件不满足的
        // 防止 Chase/Flee/Move 等在等待期间条件已失效的条目白耗 AV
        let invalid: Vec<Entity> = {
            let queue = world.resource::<ActionQueue>();
            queue.entries.iter()
                .filter(|e| e.av_remaining > 0.0 && !check_condition(world, e))
                .map(|e| e.entity)
                .collect()
        };
        if !invalid.is_empty() {
            world.resource_mut::<ActionQueue>().entries.retain(|e| !invalid.contains(&e.entity));
        }

        ready = world.resource_mut::<ActionQueue>().pop_ready();
    }

    for entry in &ready {
        if check_condition(world, entry) {
            execute_entry(world, entry);
            let _ = world.run_system_once(dungeon_core::systems::apply_exp_system);
            ops::rebuild_occupancy(world);
        } else {
            world.resource_mut::<EventLog>().push(format!("行动被取消"));
        }
    }
    dist
}

/// 检查 (x,y) 能否走到 (x+dx, y+dy)，对角线额外验证不穿墙角
fn can_move_to(map: &Map, occ: &OccupancyMap, x: usize, y: usize, dx: isize, dy: isize) -> bool {
    let nx = x.wrapping_add_signed(dx);
    let ny = y.wrapping_add_signed(dy);
    if nx >= MAP_WIDTH || ny >= MAP_HEIGHT { return false; }
    if !map.tiles[ny][nx].walkable() { return false; }
    if occ.is_occupied(nx, ny) { return false; }
    true
}

fn check_condition(world: &World, entry: &ActionEntry) -> bool {
    match &entry.kind {
        ActionKindV3::Chase => {
            let player_pos = world.try_query::<(&Player, &Position)>().unwrap().iter(world).next().map(|(_, p)| (p.x, p.y));
            let Some((px, py)) = player_pos else { return false };
            world.get::<Viewshed>(entry.entity)
                .map(|v| v.visible_tiles.contains(&(px, py)))
                .unwrap_or(false)
        }
        ActionKindV3::Flee => {
            // 滞回区间：进入逃跑 HP<25%，退出逃跑 HP>30%
            world.get::<Stats>(entry.entity)
                .map(|s| (s.hp as f32 / s.max_hp as f32) < 0.30)
                .unwrap_or(false)
        }
        ActionKindV3::Wander | ActionKindV3::Wait => true,
        ActionKindV3::Move { dx, dy } => {
            if let Some(pos) = world.get::<Position>(entry.entity) {
                let map = world.resource::<Map>();
                let occ = world.resource::<OccupancyMap>();
                can_move_to(map, occ, pos.x, pos.y, *dx, *dy)
            } else { false }
        }
        ActionKindV3::Attack { target } => {
            world.get::<Monster>(*target).is_some()
        }
        ActionKindV3::Skill(_) => true,
    }
}

fn execute_entry(world: &mut World, entry: &ActionEntry) {
    match &entry.kind {
        ActionKindV3::Chase => execute_chase(world, entry.entity),
        ActionKindV3::Flee => execute_flee(world, entry.entity),
        ActionKindV3::Wander => execute_wander(world, entry.entity),
        ActionKindV3::Wait => execute_wait(entry.entity),
        ActionKindV3::Move { dx, dy } => execute_player_move(world, entry.entity, *dx, *dy),
        ActionKindV3::Attack { target } => execute_attack(world, entry.entity, *target),
        ActionKindV3::Skill(idx) => execute_skill(world, entry.entity, *idx),
    }
}

fn execute_chase(world: &mut World, entity: Entity) {
    let Some(player_entity) = world.query::<(Entity, &Player)>().iter(world).next().map(|(e, _)| e) else { return };
    let player_pos = world.get::<Position>(player_entity).map(|p| (p.x, p.y));
    let Some((px, py)) = player_pos else { return };
    let pos = match world.get::<Position>(entity) { Some(p) => (p.x, p.y), None => return };
    // 邻接时攻击（含对角）
    if pos.0.abs_diff(px) <= 1 && pos.1.abs_diff(py) <= 1 && (pos.0 != px || pos.1 != py) {
        let monster_atk = world.get::<Stats>(entity).map(|s| s.attack as i32).unwrap_or(1);
        let player_def = world.query::<(&Stats, &Inventory, &Equipment, Option<&Buffs>)>().iter(world).next()
            .map(|(ps, inv, eq, buffs)| ops::effective_defense(ps, inv, eq, buffs) as i32)
            .unwrap_or(0);
        let dmg = (monster_atk - player_def).max(1);
        let name = world.get::<EntityName>(entity).map(|n| n.0.clone()).unwrap_or("怪物".into());
        if let Some(mut ps) = world.get_mut::<Stats>(player_entity) { ps.hp -= dmg; }
        world.resource_mut::<EventLog>().push(format!("{} 攻击了你，{}伤", name, dmg));
    } else {
        // A* 寻路至玩家，取第一步
        let next_step = {
            let map = world.resource::<Map>();
            let occ = world.resource::<OccupancyMap>();
            ops::astar(pos, (px, py), &map.tiles, Some(occ))
                .and_then(|path| {
                    // 跳过第一步如果它等于当前位置（A* 不含起点）
                    path.first().copied()
                })
        };
        if let Some((nx, ny)) = next_step {
            if let Some(mut p) = world.get_mut::<Position>(entity) { p.x = nx; p.y = ny; }
        }
    }
}

fn execute_flee(world: &mut World, entity: Entity) {
    let player_pos = world.query::<(&Player, &Position)>().iter(world).next().map(|(_, p)| (p.x, p.y));
    let Some((px, py)) = player_pos else { return };
    let pos = match world.get::<Position>(entity) { Some(p) => (p.x, p.y), None => return };
    let dirs: [(isize, isize); 8] = [
        (0, -1), (0, 1), (-1, 0), (1, 0),
        (-1, -1), (1, -1), (-1, 1), (1, 1),
    ];
    let best = {
        let map = world.resource::<Map>();
        let occ = world.resource::<OccupancyMap>();
        let mut best: Option<(usize, usize)> = None;
        let mut best_dist = 0usize;
        for &(dx, dy) in &dirs {
            if !can_move_to(map, occ, pos.0, pos.1, dx, dy) { continue; }
            let nx = pos.0.wrapping_add_signed(dx);
            let ny = pos.1.wrapping_add_signed(dy);
            let d = nx.abs_diff(px) + ny.abs_diff(py);
            if d > best_dist { best_dist = d; best = Some((nx, ny)); }
        }
        best
    };
    if let Some((nx, ny)) = best {
        if let Some(mut p) = world.get_mut::<Position>(entity) { p.x = nx; p.y = ny; }
    }
}

fn execute_wander(world: &mut World, entity: Entity) {
    let dirs: [(isize, isize); 8] = [
        (0, -1), (0, 1), (-1, 0), (1, 0),
        (-1, -1), (1, -1), (-1, 1), (1, 1),
    ];
    let r = (world.resource::<FloorNumber>().0 as usize + world.query::<(Entity, &Monster)>().iter(world).count()) % 8;
    let (dx, dy) = dirs[r];
    let target = if let Some(pos) = world.get::<Position>(entity) {
        let map = world.resource::<Map>();
        let occ = world.resource::<OccupancyMap>();
        can_move_to(map, occ, pos.x, pos.y, dx, dy)
            .then_some((pos.x.wrapping_add_signed(dx), pos.y.wrapping_add_signed(dy)))
    } else { None };
    if let Some((nx, ny)) = target {
        if let Some(mut p) = world.get_mut::<Position>(entity) { p.x = nx; p.y = ny; }
    }
}

fn execute_wait(_entity: Entity) {}

fn execute_player_move(world: &mut World, entity: Entity, dx: isize, dy: isize) {
    let (nx, ny) = {
        let ppos = match world.get::<Position>(entity) { Some(p) => (p.x, p.y), None => return };
        let map = world.resource::<Map>();
        let occ = world.resource::<OccupancyMap>();
        if !can_move_to(map, occ, ppos.0, ppos.1, dx, dy) { return; }
        (ppos.0.wrapping_add_signed(dx), ppos.1.wrapping_add_signed(dy))
    };
    if let Some(mut p) = world.get_mut::<Position>(entity) { p.x = nx; p.y = ny; }
}

fn execute_attack(world: &mut World, attacker: Entity, target: Entity) {
    let (exp, name, atk_name, dmg, crit, target_pos);
    {
        let Some(target_stats) = world.get::<Stats>(target).cloned() else { return };
        let Some(attacker_stats) = world.get::<Stats>(attacker).cloned() else { return };
        name = world.get::<EntityName>(target).map(|n| n.0.clone()).unwrap_or("怪物".into());
        atk_name = world.get::<AttackName>(attacker).map(|a| a.0.clone()).unwrap_or("攻击".into());
        target_pos = world.get::<Position>(target).map(|p| (p.x, p.y));
        let inventory = world.get::<Inventory>(attacker).unwrap();
        let equipment = world.get::<Equipment>(attacker).unwrap();
        let buffs = world.get::<Buffs>(attacker);
        let effective_atk = ops::effective_attack(&attacker_stats, inventory, equipment, buffs) as i32;
        let target_def = {
            let eq = world.get::<Equipment>(target);
            let buffs = world.get::<Buffs>(target);
            ops::effective_defense(&target_stats, &world.get::<Inventory>(target).cloned().unwrap_or_default(), &eq.cloned().unwrap_or_default(), buffs) as i32
        };
        let raw_dmg = (effective_atk - target_def).max(1);
        let is_crit = attacker_stats.crit_rate > rand::random::<f32>();
        dmg = if is_crit { (raw_dmg as f32 * (1.0 + attacker_stats.crit_damage)).round() as i32 } else { raw_dmg };
        crit = is_crit;
        exp = target_stats.exp;
    }
    {
        let Some(mut target_stats) = world.get_mut::<Stats>(target) else { return };
        target_stats.hp -= dmg;
        if target_stats.hp <= 0 {
            world.resource_mut::<PendingExp>().amount += exp;
            world.resource_mut::<EventLog>().push(format!("你{}击杀了{}！获得{}经验", atk_name, name, exp));
            let loot_stacks = world.get::<LootTable>(target).map(|lt| lt.roll()).unwrap_or_default();
            if let Some((px, py)) = target_pos {
                for stack in &loot_stacks {
                    let sname = stack.name();
                    world.resource_mut::<EventLog>().push(format!("{}掉落{}x{}", name, sname, stack.count));
                    world.spawn((
                        ItemPickup { stack: stack.clone() },
                        Position { x: px, y: py },
                        Renderable { glyph: stack.glyph(), color: stack.color() },
                    ));
                }
            }
            world.entity_mut(target).despawn();
        } else {
            world.resource_mut::<EventLog>().push(format!("你{}了{}{}，造成{}点伤害", atk_name, name, if crit { "！暴击" } else { "" }, dmg));
        }
    }
}

fn execute_skill(world: &mut World, entity: Entity, skill_idx: usize) {
    let (skill_kind, cost_mp, skill_name);
    {
        let Some(skills) = world.get::<dungeon_core::Skills>(entity) else { return };
        let Some(skill) = skills.list.get(skill_idx) else { return };
        let Some(stats) = world.get::<Stats>(entity) else { return };
        if stats.mp < skill.cost_mp {
            let msg = format!("MP不足，无法施放{}", skill.name);
            world.resource_mut::<EventLog>().push(msg);
            return;
        }
        skill_kind = skill.kind.clone();
        cost_mp = skill.cost_mp;
        skill_name = skill.name.to_string();
    }
    {
        if let Some(mut stats) = world.get_mut::<Stats>(entity) { stats.mp -= cost_mp; }
    }
    match skill_kind {
        dungeon_core::SkillKind::Heal { amount } => {
            if let Some(mut stats) = world.get_mut::<Stats>(entity) { stats.hp = (stats.hp + amount).min(stats.max_hp); }
            world.resource_mut::<EventLog>().push(format!("{}恢复了{}HP", skill_name, amount));
        }
        dungeon_core::SkillKind::Shield { def_boost, duration } => {
            if let Some(mut buffs) = world.get_mut::<dungeon_core::Buffs>(entity) {
                buffs.shield_turns = duration as i32; buffs.shield_def = def_boost;
            }
            world.resource_mut::<EventLog>().push(format!("{}施放了护盾，防御+{}持续{}回合", skill_name, def_boost, duration));
        }
        dungeon_core::SkillKind::Berserk { atk_boost, duration } => {
            if let Some(mut buffs) = world.get_mut::<dungeon_core::Buffs>(entity) {
                buffs.berserk_turns = duration as i32; buffs.berserk_atk = atk_boost;
            }
            world.resource_mut::<EventLog>().push(format!("{}进入狂暴，攻击+{}持续{}回合", skill_name, atk_boost, duration));
        }
    }
}
