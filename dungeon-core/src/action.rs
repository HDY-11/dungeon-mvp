//! 行动系统 v3 执行引擎
//!
//! 类型定义移至 action_types.rs。
//! 本模块只保留执行逻辑：队列推进、保活检查、行动执行、怪物决策、玩家 tap-tap。

pub use crate::action_types::*;
use crate::world;
use crate::{Stats, Viewshed, Player, Position, EntityName, Monster, EventLog};
use bevy_ecs::prelude::*;
use bevy_ecs::system::RunSystemOnce;

// ══════════════════════════════════════════════════════
// 仲裁系统：从所有就绪行动中选优先级最高的入队
// ══════════════════════════════════════════════════════

/// 遍历所有怪物，检查各 Action 组件的条件，收集就绪行动，按优先级入队
pub fn run_monster_decision() {
    // 阶段 1：收集 (entity, priority, av, kind)
    let mut collected: Vec<(Entity, u32, f32, ActionKindV3)> = Vec::new();
    {
        let w = world!();
        let player_pos = w.try_query::<(&Player, &Position)>().unwrap().iter(&w).next().map(|(_, p)| (p.x, p.y));

        // 追击（读取 Reaction 获取反应时）
        for (entity, chase, _stats, view, reaction) in
            w.try_query::<(Entity, &CanChase, &Stats, &Viewshed, &Reaction)>().unwrap().iter(&w)
        {
            let can_see = player_pos.map_or(false, |pp| view.visible_tiles.contains(&pp));
            if CanChase::condition(can_see) {
                let av = reaction.time + chase.duration;
                collected.push((entity, chase.priority, av, ActionKindV3::Chase));
            }
        }

        // 逃跑
        for (entity, flee, stats, reaction) in
            w.try_query::<(Entity, &CanFlee, &Stats, &Reaction)>().unwrap().iter(&w)
        {
            let hp_ratio = stats.hp as f32 / stats.max_hp as f32;
            if CanFlee::condition(hp_ratio) {
                let av = reaction.time + flee.duration;
                collected.push((entity, flee.priority, av, ActionKindV3::Flee));
            }
        }

        // 游荡
        for (entity, wander, reaction) in
            w.try_query::<(Entity, &CanWander, &Reaction)>().unwrap().iter(&w)
        {
            if !collected.iter().any(|(e, _, _, _)| *e == entity) && CanWander::condition() {
                let av = reaction.time + wander.duration;
                collected.push((entity, wander.priority, av, ActionKindV3::Wander));
            }
        }
    }

    // 阶段 2：按 priority 排序，相同时随机
    collected.sort_by(|(_, pa, _, _), (_, pb, _, _)| {
        pb.cmp(pa).then_with(|| crate::global::rand_u8().cmp(&crate::global::rand_u8()))
    });

    // 阶段 3：入队（已在队列中的实体不再重复入队）
    let mut w = world!(mut);
    let mut queue = w.resource_mut::<ActionQueue>();
    for (entity, _priority, av, kind) in &collected {
        if !queue.has_entity(*entity) {
            queue.enqueue(*entity, kind.clone(), *av);
        }
    }
}

// ══════════════════════════════════════════════════════
// 行动执行引擎：推进队列 + 保活检查 + 执行
// ══════════════════════════════════════════════════════

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
            let _ = world!(mut).run_system_once(crate::systems::apply_exp_system);
            crate::rebuild_occupancy();
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
            // 玩家是否仍在视野内
            let player_pos = w.try_query::<(&Player, &Position)>().unwrap().iter(&w).next().map(|(_, p)| (p.x, p.y));
            let Some((px, py)) = player_pos else { return false };
            w.get::<Viewshed>(entry.entity)
                .map(|v| v.visible_tiles.contains(&(px, py)))
                .unwrap_or(false)
        }
        ActionKindV3::Flee => {
            // HP 比率是否仍低于阈值
            w.get::<Stats>(entry.entity)
                .map(|s| (s.hp as f32 / s.max_hp as f32) < 0.25)
                .unwrap_or(false)
        }
        ActionKindV3::Wander | ActionKindV3::Wait => true,
        ActionKindV3::Move { dx, dy } => {
            // 目标格是否仍是地板且未被占用
            if let Some(pos) = w.get::<Position>(entry.entity) {
                let nx = pos.x.wrapping_add_signed(*dx);
                let ny = pos.y.wrapping_add_signed(*dy);
                if nx < crate::MAP_WIDTH && ny < crate::MAP_HEIGHT {
                    let map = w.resource::<crate::Map>();
                    if map.tiles[ny][nx] != crate::Tile::Floor {
                        return false;
                    }
                    let occ = w.resource::<crate::OccupancyMap>();
                    if occ.is_occupied(nx, ny) {
                        // 有实体 → 转为 Attack
                        return false; // 放弃原来的 Move，等下次 tap-tap 重新判断
                    }
                    true
                } else {
                    false
                }
            } else {
                false
            }
        }
        ActionKindV3::Attack { target } => {
            // 目标是否仍存在且是怪物
            w.get::<Monster>(*target).is_some()
        }
        ActionKindV3::Skill(_) => true, // MP 检查在 execute 中
    }
}

/// 执行一个行动条目（保活检查通过后调用）
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
    use crate::{Map, MAP_WIDTH, MAP_HEIGHT, Tile, OccupancyMap};

    let mut w = world!(mut);
    let Some(player_entity) = w.query::<(Entity, &Player)>().iter(&mut *w).next().map(|(e, _)| e) else { return };
    let player_pos = w.get::<Position>(player_entity).map(|p| (p.x, p.y));
    let Some((px, py)) = player_pos else { return };
    let pos = match w.get::<Position>(entity) {
        Some(p) => (p.x, p.y),
        None => return,
    };
    let dist = pos.0.abs_diff(px) + pos.1.abs_diff(py);
    let map = w.resource::<Map>();
    let occupancy = w.resource::<OccupancyMap>();

    if dist <= 1 {
        // 近战攻击
        let dmg = w.get::<Stats>(entity).map(|s| s.attack as i32).unwrap_or(1);
        let name = w.get::<EntityName>(entity).map(|n| n.0.clone()).unwrap_or("怪物".into());
        if let Some(mut ps) = w.get_mut::<Stats>(player_entity) {
            ps.hp -= dmg.max(1);
        }
        w.resource_mut::<crate::EventLog>().push(format!("{} 攻击了你，{}伤", name, dmg));
    } else {
        // 向玩家移动一格
        let dx = if px > pos.0 { 1 } else if px < pos.0 { -1 } else { 0 };
        let dy = if py > pos.1 { 1 } else if py < pos.1 { -1 } else { 0 };
        let attempts = if px.abs_diff(pos.0) >= py.abs_diff(pos.1) {
            vec![(dx, 0), (0, dy)]
        } else {
            vec![(0, dy), (dx, 0)]
        };
        let _ = map; let _ = occupancy;
        for (ndx, ndy) in attempts {
            let nx = pos.0.wrapping_add_signed(ndx);
            let ny = pos.1.wrapping_add_signed(ndy);
            if nx < MAP_WIDTH && ny < MAP_HEIGHT
                && w.resource::<Map>().tiles[ny][nx] == Tile::Floor
                && !w.resource::<OccupancyMap>().is_occupied(nx, ny)
            {
                if let Some(mut p) = w.get_mut::<Position>(entity) {
                    p.x = nx; p.y = ny;
                }
                break;
            }
        }
    }
}

fn execute_flee(entity: Entity) {
    use crate::{Map, MAP_WIDTH, MAP_HEIGHT, Tile, OccupancyMap};
    let mut w = world!(mut);
    let player_pos = w.query::<(&Player, &Position)>().iter(&mut *w).next().map(|(_, p)| (p.x, p.y));
    let Some((px, py)) = player_pos else { return };
    let pos = match w.get::<Position>(entity) {
        Some(p) => (p.x, p.y),
        None => return,
    };
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
        if let Some(mut p) = w.get_mut::<Position>(entity) {
            p.x = nx; p.y = ny;
        }
    }
}

fn execute_wander(entity: Entity) {
    use crate::{Map, MAP_WIDTH, MAP_HEIGHT, Tile, OccupancyMap, FloorNumber};
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
            if let Some(mut p) = w.get_mut::<Position>(entity) {
                p.x = nx; p.y = ny;
            }
        }
    }
}

fn execute_wait(entity: Entity) {
    // 纯等待，无副作用
    let _ = entity;
}

fn execute_player_move(entity: Entity, dx: isize, dy: isize) {
    // 注意：Move 行动仅对空地板格入队（见 handle_player_direction），
    // 有怪物的格子会入队 Attack 行动走 execute_attack 路径。
    // 所以这里只需处理玩家移动，无需 bump 攻击逻辑。
    use crate::{Map, Tile, MAP_WIDTH, MAP_HEIGHT};
    let (nx, ny) = {
        let w = world!();
        let p = match w.get::<Position>(entity) {
            Some(p) => (p.x, p.y),
            None => return,
        };
        let nx = p.0.wrapping_add_signed(dx);
        let ny = p.1.wrapping_add_signed(dy);
        if nx >= MAP_WIDTH || ny >= MAP_HEIGHT { return; }
        if w.resource::<Map>().tiles[ny][nx] != Tile::Floor { return; }
        (nx, ny)
    };
    let mut w = world!(mut);
    if let Some(mut p) = w.get_mut::<Position>(entity) {
        p.x = nx;
        p.y = ny;
    }
}

fn execute_attack(attacker: Entity, target: Entity) {
    use crate::{Stats, EntityName, EventLog, AttackName, Inventory, Equipment, Buffs, PendingExp, LootTable, ItemPickup, Renderable, Position, effective_attack};
    // 先读取需要的数据
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
        let effective_atk = effective_attack(&attacker_stats, inventory, equipment, buffs) as i32;
        let target_def = target_stats.defense as i32;
        let raw_dmg = (effective_atk - target_def).max(1);
        let is_crit = attacker_stats.crit_rate > rand::random::<f32>();
        dmg = if is_crit { (raw_dmg as f32 * (1.0 + attacker_stats.crit_damage)).round() as i32 } else { raw_dmg };
        crit = is_crit;
        exp = target_stats.exp;
    }

    // 应用伤害 + 掉落
    {
        let mut w = world!(mut);
        let Some(mut target_stats) = w.get_mut::<Stats>(target) else { return };
        target_stats.hp -= dmg;
        if target_stats.hp <= 0 {
            w.resource_mut::<PendingExp>().amount += exp;
            w.resource_mut::<EventLog>().push(format!("你{}击杀了{}！获得{}经验", atk_name, name, exp));

            // 掉落
            let loot_stacks = w.get::<LootTable>(target)
                .map(|lt| lt.roll())
                .unwrap_or_default();
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
    };
}

fn execute_skill(entity: Entity, skill_idx: usize) {
    use crate::{Stats, Skills};
    // 读取技能数据和玩家属性
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

    // 扣 MP
    {
        let mut w = world!(mut);
        if let Some(mut stats) = w.get_mut::<Stats>(entity) {
            stats.mp -= cost_mp;
        }
    }

    // 执行技能效果
    match skill_kind {
        crate::SkillKind::Firebolt { damage } => {
            // 先读取需要的数据
            let (pp, magic_bonus, crit_rate, crit_dmg);
            {
                let mut w = world!(mut);
                pp = w.query::<(&Player, &Position)>().iter(&mut *w).next().map(|(_, p)| (p.x, p.y));
                let stats = w.get::<Stats>(entity);
                magic_bonus = stats.map(|s| (s.magic_mastery as f32 * 0.5) as i32).unwrap_or(0);
                crit_rate = stats.map(|s| s.crit_rate).unwrap_or(0.0);
                crit_dmg = stats.map(|s| s.crit_damage).unwrap_or(0.0);
            }
            // 计算伤害并收集目标
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
            // 日志和清理
            {
                let mut w = world!(mut);
                for (me, dmg) in &hits {
                    let name = w.get::<EntityName>(*me).map(|n| n.0.clone()).unwrap_or("怪物".into());
                    let hp = w.get::<Stats>(*me).map(|s| s.hp).unwrap_or(0);
                    w.resource_mut::<EventLog>().push(format!("火球击中 {}！{}伤", name, dmg));
                    if hp <= 0 {
                        w.entity_mut(*me).despawn();
                    }
                }
                if !hit_any { w.resource_mut::<EventLog>().push(String::from("附近没有敌人")); }
            }
        }
        crate::SkillKind::Heal { amount } => {
            let mut w = world!(mut);
            if let Some(mut stats) = w.get_mut::<Stats>(entity) {
                stats.hp = (stats.hp + amount).min(stats.max_hp);
            }
            w.resource_mut::<EventLog>().push(format!("{}恢复了{}HP", skill_name, amount));
        }
        crate::SkillKind::Shield { def_boost, duration } => {
            let mut w = world!(mut);
            if let Some(mut buffs) = w.get_mut::<crate::Buffs>(entity) {
                buffs.shield_turns = duration as i32;
                buffs.shield_def = def_boost;
            }
            w.resource_mut::<EventLog>().push(format!("{}施放了护盾，防御+{}持续{}回合", skill_name, def_boost, duration));
        }
        crate::SkillKind::Berserk { atk_boost, duration } => {
            let mut w = world!(mut);
            if let Some(mut buffs) = w.get_mut::<crate::Buffs>(entity) {
                buffs.berserk_turns = duration as i32;
                buffs.berserk_atk = atk_boost;
            }
            w.resource_mut::<EventLog>().push(format!("{}进入狂暴，攻击+{}持续{}回合", skill_name, atk_boost, duration));
        }
    }
}

// ══════════════════════════════════════════════════════
// 玩家行动处理（tap-tap 预览/确认）
// ══════════════════════════════════════════════════════

/// tap-tap 核心：返回 true 表示确认入队
pub fn handle_timed_action(entity: Entity, kind: ActionKindV3, av: f32) -> bool {
    let is_confirm = {
        let w = world!();
        let preview = w.resource::<PlayerPreview>();
        match (&preview.kind, &kind) {
            (Some(ActionKindV3::Move { dx: pd, dy: pd2 }), ActionKindV3::Move { dx, dy })
                if *pd == *dx && *pd2 == *dy => true,
            (Some(ActionKindV3::Wait), ActionKindV3::Wait) => true,
            (Some(ActionKindV3::Skill(a)), ActionKindV3::Skill(b)) if *a == *b => true,
            (Some(ActionKindV3::Attack { .. }), ActionKindV3::Attack { .. }) => true,
            _ => false,
        }
    };

    if is_confirm {
        world!(mut).resource_mut::<ActionQueue>().enqueue_if_absent(entity, kind, av);
        world!(mut).resource_mut::<PlayerPreview>().kind = None;
        true
    } else {
        world!(mut).resource_mut::<PlayerPreview>().kind = Some(kind);
        false
    }
}

/// 方向键 tap-tap：返回 true 表示确认了行动
pub fn handle_player_direction(dx: isize, dy: isize) -> bool {
    use crate::{Map, Tile, OccupancyMap, MAP_WIDTH, MAP_HEIGHT, Monster};

    let Some(entity) = crate::ops::player_entity() else { return false };

    let kind = {
        let w = world!();
        let Some(pos) = w.get::<Position>(entity) else { return false };
        let nx = pos.x.wrapping_add_signed(dx);
        let ny = pos.y.wrapping_add_signed(dy);
        if nx >= MAP_WIDTH || ny >= MAP_HEIGHT { return false; }
        let tile = w.resource::<Map>().tiles[ny][nx];
        let has_enemy = w.resource::<OccupancyMap>().cells[ny][nx]
            .and_then(|e| if w.get::<Monster>(e).is_some() { Some(e) } else { None });
        if tile != Tile::Floor && has_enemy.is_none() { return false; }
        if let Some(target) = has_enemy {
            ActionKindV3::Attack { target }
        } else {
            ActionKindV3::Move { dx, dy }
        }
    };

    let reaction_time = world!().get::<Reaction>(entity).map(|r| r.time).unwrap_or(50.0);
    let duration = world!().get::<CanMove>(entity).map(|m| m.duration).unwrap_or(300.0);
    handle_timed_action(entity, kind, reaction_time + duration)
}

/// 处理等待键
pub fn handle_wait() -> bool {
    if let Some(e) = crate::ops::player_entity() {
        let reaction_time = world!().get::<Reaction>(e).map(|r| r.time).unwrap_or(50.0);
        let duration = world!().get::<CanWait>(e).map(|w| w.duration).unwrap_or(800.0);
        handle_timed_action(e, ActionKindV3::Wait, reaction_time + duration)
    } else {
        false
    }
}

/// 处理技能键（idx: 技能索引 0..3）
pub fn handle_skill(idx: usize) -> bool {
    if let Some(e) = crate::ops::player_entity() {
        let reaction_time = world!().get::<Reaction>(e).map(|r| r.time).unwrap_or(50.0);
        handle_timed_action(e, ActionKindV3::Skill(idx), reaction_time + 600.0)
    } else {
        false
    }
}

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

    crate::ops::rebuild_occupancy();
    let _ = world!(mut).run_system_once(crate::systems::fov_system);
    crate::ops::update_map_memory();
    crate::ops::update_visible_memory();
    let _ = world!(mut).run_system_once(crate::systems::check_death_system);
    let _ = world!(mut).run_system_once(crate::systems::buff_tick_system);
}
