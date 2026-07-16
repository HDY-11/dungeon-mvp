//! 行动执行引擎：队列推进、保活检查、行动执行

use crate::types::*;
use dungeon_core::{
    ops, components::*, items::*, resources::*,
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

        // 推进所有实体的 ActiveBuffs 和 ActiveCooldowns（与队列同步使用同一 dist）
        {
            let mut q = world.query::<&mut ActiveBuffs>();
            for mut buffs in q.iter_mut(world) {
                buffs.0.retain_mut(|b| { b.remaining_av -= dist; b.remaining_av > 0.0 });
            }
        }
        {
            let mut q = world.query::<&mut ActiveCooldowns>();
            for mut cds in q.iter_mut(world) {
                cds.0.retain_mut(|c| { c.remaining_av -= dist; c.remaining_av > 0.0 });
            }
        }

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
            world.resource_mut::<EventLog>().push("行动被取消".to_string());
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
    if let Some(ref action) = entry.action {
        return action.check_condition(world, entry.entity);
    }
    match &entry.kind {
        ActionKindV3::Chase => {
            let player_pos = world.try_query::<(&Player, &Position)>().expect("Player+Position registered at init").iter(world).next().map(|(_, p)| (p.x, p.y));
            if let Some((px, py)) = player_pos
                && world.get::<Viewshed>(entry.entity)
                    .map(|v| v.visible_tiles.contains(&(px, py)))
                    .unwrap_or(false) { return true; }
            // 玩家不在视野内但有记忆位置 → 继续追击
            world.get::<LastKnownPlayerPos>(entry.entity)
                .map(|l| l.0.is_some())
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
        ActionKindV3::Throw { .. } => true,
    }
}

fn execute_entry(world: &mut World, entry: &ActionEntry) {
    if let Some(ref action) = entry.action {
        action.execute(world, entry.entity); return;
    }
    match &entry.kind {
        ActionKindV3::Chase => execute_chase(world, entry.entity),
        ActionKindV3::Flee => execute_flee(world, entry.entity),
        ActionKindV3::Wander => execute_wander(world, entry.entity),
        ActionKindV3::Wait => execute_wait(entry.entity),
        ActionKindV3::Move { dx, dy } => execute_player_move(world, entry.entity, *dx, *dy),
        ActionKindV3::Attack { target } => execute_attack(world, entry.entity, *target),
        ActionKindV3::Skill(idx) => execute_skill(world, entry.entity, *idx),
        ActionKindV3::Throw { tx, ty } => execute_throw(world, entry.entity, *tx, *ty),
    }
}

fn execute_chase(world: &mut World, entity: Entity) {
    let Some(player_entity) = world.query::<(Entity, &Player)>().iter(world).next().map(|(e, _)| e) else { return };
    let player_pos = world.get::<Position>(player_entity).map(|p| (p.x, p.y));
    let pos = match world.get::<Position>(entity) { Some(p) => (p.x, p.y), None => return };

    // 判断目标：玩家可见 → 玩家位置；不可见 → 记忆位置
    let (target_visible, target) = if let Some((ppx, ppy)) = player_pos {
        let can_see = world.get::<Viewshed>(entity)
            .map(|v| v.visible_tiles.contains(&(ppx, ppy)))
            .unwrap_or(false);
        if can_see { (true, Some((ppx, ppy))) }
        else { (false, world.get::<LastKnownPlayerPos>(entity).and_then(|l| l.0)) }
    } else {
        (false, world.get::<LastKnownPlayerPos>(entity).and_then(|l| l.0))
    };

    let Some((px, py)) = target else {
        // 无目标 → 清除记忆
        if let Some(mut lkp) = world.get_mut::<LastKnownPlayerPos>(entity) { lkp.0 = None; }
        return;
    };

    // 邻接时攻击（含对角，仅当目标是玩家时）
    if target_visible && pos.0.abs_diff(px) <= 1 && pos.1.abs_diff(py) <= 1 && (pos.0 != px || pos.1 != py) {
        let monster_atk = world.get::<Stats>(entity).map(|s| s.attack as i32).unwrap_or(1);
        let player_def = world.query::<(&Stats, &Inventory, &Equipment, Option<&ActiveBuffs>)>().iter(world).next()
            .map(|(ps, inv, eq, ab)| ops::effective_defense(ps, inv, eq, ab) as i32)
            .unwrap_or(0);
        let dmg = (monster_atk - player_def).max(1);
        let name = world.get::<EntityName>(entity).map(|n| n.0.clone()).unwrap_or("怪物".into());
        if let Some(mut ps) = world.get_mut::<Stats>(player_entity) { ps.hp -= dmg; }
        world.resource_mut::<EventLog>().push(format!("{} 攻击了你，{}伤", name, dmg));
    } else {
        // A* 寻路至目标，取第一步
        let next_step = {
            let map = world.resource::<Map>();
            let occ = world.resource::<OccupancyMap>();
            dungeon_core::pathfinding::astar(pos, (px, py), &map.tiles, Some(occ))
                .and_then(|path| path.first().copied())
        };
        if let Some((nx, ny)) = next_step
            && let Some(mut p) = world.get_mut::<Position>(entity) { p.x = nx; p.y = ny; }
    }

    // 到达记忆位置附近但仍未看到玩家 → 清除记忆，进入游荡
    if !target_visible
        && let Some(mut lkp) = world.get_mut::<LastKnownPlayerPos>(entity)
            && let Some((lkx, lky)) = lkp.0
                && pos.0.abs_diff(lkx) <= 2 && pos.1.abs_diff(lky) <= 2 {
                    lkp.0 = None;
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
    if let Some((nx, ny)) = best
        && let Some(mut p) = world.get_mut::<Position>(entity) { p.x = nx; p.y = ny; }
}

fn execute_wander(world: &mut World, entity: Entity) {
    let dirs: [(isize, isize); 8] = [
        (0, -1), (0, 1), (-1, 0), (1, 0),
        (-1, -1), (1, -1), (-1, 1), (1, 1),
    ];
    let r = world.resource_mut::<GameRng>().random_range(0, 8) as usize;
    let (dx, dy) = dirs[r];
    let target = if let Some(pos) = world.get::<Position>(entity) {
        let map = world.resource::<Map>();
        let occ = world.resource::<OccupancyMap>();
        can_move_to(map, occ, pos.x, pos.y, dx, dy)
            .then_some((pos.x.wrapping_add_signed(dx), pos.y.wrapping_add_signed(dy)))
    } else { None };
    if let Some((nx, ny)) = target
        && let Some(mut p) = world.get_mut::<Position>(entity) { p.x = nx; p.y = ny; }
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

/// 统一暴击计算（玩家和怪物共享路径）
fn calc_crit(stats: &Stats, bonus: &dungeon_core::StatBonus, crit_roll: f32) -> (bool, f32) {
    let total_crit_rate = (stats.crit_rate + bonus.crit_rate).min(1.0);
    let is_crit = total_crit_rate > crit_roll;
    let crit_mult = if is_crit { 1.0 + stats.crit_damage } else { 1.0 };
    (is_crit, crit_mult)
}

fn execute_attack(world: &mut World, attacker: Entity, target: Entity) {
    let (name, atk_name, dmg, crit);
    {
        let Some(target_stats) = world.get::<Stats>(target).cloned() else { return };
        let Some(attacker_stats) = world.get::<Stats>(attacker).cloned() else { return };
        name = world.get::<EntityName>(target).map(|n| n.0.clone()).unwrap_or("怪物".into());
        atk_name = world.get::<AttackName>(attacker).map(|a| a.0.clone()).unwrap_or("攻击".into());
        let inventory = world.get::<Inventory>(attacker)
            .expect("Attacker has Inventory");
        let equipment = world.get::<Equipment>(attacker)
            .expect("Attacker has Equipment");
        
        let ab = world.get::<ActiveBuffs>(attacker);
        let effective_atk = ops::effective_attack(&attacker_stats, inventory, equipment, ab) as i32;
        let target_def = {
            let eq = world.get::<Equipment>(target);
            ops::effective_defense(&target_stats, &world.get::<Inventory>(target).cloned().unwrap_or_default(), &eq.cloned().unwrap_or_default(), None) as i32
        };
        let raw_dmg = (effective_atk - target_def).max(1);
        let equip = world.get::<Equipment>(attacker);
        let inv = world.get::<Inventory>(attacker);
        let bonus = equip.zip(inv).map(|(eq, inv)| dungeon_core::equipment_bonus(inv, eq)).unwrap_or_default();
        let crit_roll = world.resource_mut::<GameRng>().random_f32();
        (dmg, crit) = {
            let (is_crit, crit_mult) = calc_crit(&attacker_stats, &bonus, crit_roll);
            let d = if is_crit { (raw_dmg as f32 * crit_mult).round() as i32 } else { raw_dmg };
            (d, is_crit)
        };
    }
    {
        let Some(mut target_stats) = world.get_mut::<Stats>(target) else { return };
        target_stats.hp -= dmg;
        if target_stats.hp <= 0 {
            handle_kill(world, target, &name);
        } else {
            world.resource_mut::<EventLog>().push(format!("你{}了{}{}，造成{}点伤害", atk_name, name, if crit { "！暴击" } else { "" }, dmg));
        }
    }
}

fn execute_skill(world: &mut World, entity: Entity, skill_idx: usize) {
    let (skill_kind, cost_mp, skill_name, skill_proficiency);
    {
        let has_skill = world.get::<dungeon_core::Skills>(entity)
            .map(|s| s.list.get(skill_idx).is_some())
            .unwrap_or(false);
        if !has_skill {
            world.resource_mut::<dungeon_core::EventLog>().push("技能未学习".to_string());
            return;
        }
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
        skill_proficiency = skill.proficiency;
    }
    {
        if let Some(mut stats) = world.get_mut::<Stats>(entity) { stats.mp -= cost_mp; }
    }
    match skill_kind {
        dungeon_core::SkillKind::Heal { amount } => {
            let effective = amount + skill_proficiency as i32 * 3;
            if let Some(mut stats) = world.get_mut::<Stats>(entity) { stats.hp = (stats.hp + effective).min(stats.max_hp); }
            world.resource_mut::<EventLog>().push(format!("{}恢复了{}HP（熟练度+{}）", skill_name, effective, skill_proficiency));
        }
        dungeon_core::SkillKind::Shield { def_boost, duration } => {
            // 新 AV Buff 系统
            let effective_def = def_boost + skill_proficiency as i32 * 2;
            if let Some(mut ab) = world.get_mut::<ActiveBuffs>(entity) {
                let av = duration as f32 * 1000.0;
                if let Some(existing) = ab.0.iter_mut().find(|b| b.kind == BuffKind::Shield) {
                    existing.remaining_av = av;
                    existing.magnitude = effective_def;
                } else {
                    ab.0.push(Buff { kind: BuffKind::Shield, remaining_av: av, magnitude: effective_def, stack_type: BuffStackType::None });
                }
            }
            world.resource_mut::<EventLog>().push(format!("{}施放了护盾，防御+{}持续{}秒（熟练度+{}）", skill_name, effective_def, duration, skill_proficiency));
        }
        dungeon_core::SkillKind::Berserk { atk_boost, duration } => {
            // 新 AV Buff 系统
            let effective_atk = atk_boost + skill_proficiency as i32 * 2;
            if let Some(mut ab) = world.get_mut::<ActiveBuffs>(entity) {
                let av = duration as f32 * 1000.0;
                if let Some(existing) = ab.0.iter_mut().find(|b| b.kind == BuffKind::Berserk) {
                    existing.remaining_av = av;
                    existing.magnitude = effective_atk;
                } else {
                    ab.0.push(Buff { kind: BuffKind::Berserk, remaining_av: av, magnitude: effective_atk, stack_type: BuffStackType::None });
                }
            }
            world.resource_mut::<EventLog>().push(format!("{}进入狂暴，攻击+{}持续{}秒（熟练度+{}）", skill_name, effective_atk, duration, skill_proficiency));
        }
    }
}

/// 投掷执行：石子伤害 + 暴击 + 掉落 + 副手消耗
/// 由 execute_entry 分派，走 AV 队列生命周期
fn execute_throw(world: &mut World, _attacker: Entity, tx: usize, ty: usize) {
    // 随机值收集
    let extra = {
        let mut rng = world.resource_mut::<GameRng>();
        rng.random_range(0u8, 2u8) as u32
    };
    let crit_roll = {
        let mut rng = world.resource_mut::<GameRng>();
        rng.random_f32()
    };

    // 查找目标格上的怪物
    let target = {
        let Some(mut q) = world.try_query::<(Entity, &Position, &Monster)>() else {
            world.resource_mut::<EventLog>().push("投掷内部错误：无法查询实体".to_string());
            return;
        };
        q.iter(world)
            .find(|(_, pos, _)| pos.x == tx && pos.y == ty)
            .map(|(e, _, _)| e)
    };

    let floor = world.resource::<FloorNumber>().0;
    let base_dmg = 3 + floor / 2;
    let (is_crit, crit_mult) = calc_player_crit(world, crit_roll);

    if let Some(target_entity) = target {
        let target_def = world.get::<Stats>(target_entity)
            .map(|s| s.defense as i32)
            .unwrap_or(0);
        let raw_dmg = ((base_dmg as i32 + extra as i32 - target_def).max(1)) as u32;
        let final_dmg = (raw_dmg as f32 * crit_mult).round() as i32;
        let target_name = world.get::<EntityName>(target_entity)
            .map(|n| n.0.clone())
            .unwrap_or("怪物".into());

        if let Some(mut s) = world.get_mut::<Stats>(target_entity) {
            s.hp -= final_dmg;
        }

        let dead = world.get::<Stats>(target_entity).map(|s| s.hp <= 0).unwrap_or(false);
        if dead {
            handle_kill(world, target_entity, &target_name);
        } else {
            world.resource_mut::<EventLog>()
                .push(format!("石子命中了{}！{}，造成{}点伤害", target_name, if is_crit { "暴击" } else { "" }, final_dmg));
        }
    } else {
        world.resource_mut::<EventLog>().push("石子落在地上".to_string());
    }

    // 消耗副手 1 颗石子
    if let Some(p) = ops::player_entity(world)
        && let Some(mut eq) = world.get_mut::<Equipment>(p)
            && let Some(ref mut stack) = eq.off_hand {
                stack.count = stack.count.saturating_sub(1);
                if stack.count == 0 {
                    eq.off_hand = None;
                }
            }
}

/// 计算玩家暴击率和暴击倍率，复用 calc_crit
fn calc_player_crit(world: &World, crit_roll: f32) -> (bool, f32) {
    let p = ops::player_entity(world);
    match p {
        Some(p) => {
            let p_stats = world.get::<Stats>(p);
            let inv = world.get::<Inventory>(p);
            let equip = world.get::<Equipment>(p);
            let bonus = equip.zip(inv)
                .map(|(eq, inv)| dungeon_core::equipment_bonus(inv, eq))
                .unwrap_or_default();
            match p_stats {
                Some(stats) => calc_crit(stats, &bonus, crit_roll),
                None => (false, 1.0),
            }
        }
        None => (false, 1.0),
    }
}

/// 处理实体死亡：经验、掉落生成、despawn。
/// 与 execute_attack 中的死亡处理共享同一模式。
fn handle_kill(world: &mut World, entity: Entity, name: &str) {
    let exp = world.get::<Stats>(entity).map(|s| s.exp).unwrap_or(0);
    world.resource_mut::<PendingExp>().amount += exp;
    world.resource_mut::<EventLog>()
        .push(format!("击杀了{}！获得{}经验", name, exp));
    let pos = world.get::<Position>(entity).map(|p| (p.x, p.y));
    let loot_stacks = {
        let lt = world.get::<LootTable>(entity).cloned();
        lt.map(|l| {
            let mut rng = world.resource_mut::<GameRng>();
            l.roll(&mut rng.rng)
        }).unwrap_or_default()
    };
    if let Some((px, py)) = pos {
        for stack in &loot_stacks {
            let sname = stack.name();
            world.resource_mut::<EventLog>()
                .push(format!("{}掉落{}x{}", name, sname, stack.count));
            world.spawn((
                ItemPickup { stack: stack.clone() },
                Position { x: px, y: py },
                Renderable { glyph: stack.glyph(), color: stack.color() },
            ));
        }
    }
    world.entity_mut(entity).despawn();
}
