use crate::{
    ai::{AiBehavior, MonsterBrain},
    components::*,
    items::{Equipment, Inventory, ItemPickup},
    pathfinding::find_path,
    resources::*,
    effective_attack,
    calculate_visible_tiles, MAP_HEIGHT, MAP_WIDTH, Tile, Map,
};
use crate::world;
use bevy_ecs::prelude::*;
use bevy_ecs::system::RunSystemOnce;
use rand::RngExt;

/// ─────────────────────────────────────────────────────
/// 玩家移动 + bump 攻击
/// ─────────────────────────────────────────────────────
pub fn movement_system(
    mut player_query: Query<
        (&mut crate::Position, &mut MovingDir, &Stats, &Inventory, &Equipment, Option<&Buffs>, &AttackName),
        (With<Player>, Without<Monster>),
    >,
    mut monster_query: Query<(&mut Stats, &EntityName, Entity), (With<Monster>, Without<Player>)>,
    item_query: Query<(Entity, &ItemPickup, &crate::Position), (Without<Player>, Without<Monster>)>,
    map: Res<Map>,
    occupancy: Res<OccupancyMap>,
    mut commands: Commands,
    mut pending: ResMut<PendingExp>,
    mut event_log: ResMut<EventLog>,
    mut pending_pickup: ResMut<PendingPickup>,
    mut pacing: ResMut<GamePacing>,
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
                pacing.combat_active = true;
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

/// ─────────────────────────────────────────────────────
/// FOV
/// ─────────────────────────────────────────────────────
pub fn fov_system(mut query: Query<(&crate::Position, &mut Viewshed)>, map: Res<Map>) {
    for (pos, mut viewshed) in query.iter_mut() {
        viewshed.visible_tiles = calculate_visible_tiles(pos.x, pos.y, viewshed.range, &map);
    }
}

/// ─────────────────────────────────────────────────────
/// 死亡检测
/// ─────────────────────────────────────────────────────
pub fn check_death_system(
    player_query: Query<&Stats, With<Player>>,
    mut turn_manager: ResMut<TurnManager>,
) {
    if let Ok(stats) = player_query.single() {
        if stats.hp <= 0 { turn_manager.game_over = true; }
    }
}

/// ─────────────────────────────────────────────────────
/// 怪物 AI（全量执行版，用于批处理模式）
/// ─────────────────────────────────────────────────────
pub fn monster_ai_system(
    mut monster_query: Query<(
        &mut crate::Position, &mut Viewshed, &Stats, &MonsterBrain, &EntityName,
        &mut FleeLogState, &mut ActionPreview, &AttackName,
    ), (With<Monster>, Without<Player>)>,
    mut player_query: Query<(&crate::Position, &mut Stats, &Viewshed), (With<Player>, Without<Monster>)>,
    map: Res<Map>, occupancy: Res<OccupancyMap>,
    mut game_rng: ResMut<GameRng>, mut event_log: ResMut<EventLog>,
) {
    let Ok((player_pos, mut player_stats, player_viewshed)) = player_query.single_mut() else { return; };
    let dirs: [(isize, isize); 4] = [(0, -1), (0, 1), (-1, 0), (1, 0)];
    for (mut pos, mut viewshed, monster_stats, brain, name, mut flee_state, mut preview, atk_name) in monster_query.iter_mut() {
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
                    let in_fov = player_viewshed.visible_tiles.contains(&(pos.x, pos.y));
                    let preview_text = format!("{} 即将逃跑", name.0);
                    if in_fov && preview.last_preview.as_deref() != Some(&preview_text) {
                        event_log.push(preview_text.clone());
                        preview.last_preview = Some(preview_text);
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
                    let in_fov = player_viewshed.visible_tiles.contains(&(pos.x, pos.y));
                    if in_fov {
                        let preview_text = if dist <= 1 {
                            format!("{} 即将攻击你", name.0)
                        } else {
                            format!("{} 正在接近", name.0)
                        };
                        if preview.last_preview.as_deref() != Some(&preview_text) {
                            event_log.push(preview_text.clone());
                            preview.last_preview = Some(preview_text);
                        }
                    }
                    if dist <= 1 {
                        let dmg = (monster_stats.attack as i32 - player_stats.defense as i32).max(1);
                        let is_crit = monster_stats.crit_rate > game_rng.rng.random::<f32>();
                        let dmg = if is_crit { (dmg as f32 * (1.0 + monster_stats.crit_damage)).round() as i32 } else { dmg };
                        player_stats.hp -= dmg;
                        let crit_tag = if is_crit { "！暴击" } else { "" };
                        event_log.push(format!("{}{}了你{}，造成{}点伤害", name.0, atk_name.0, crit_tag, dmg));
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
                    let in_fov = player_viewshed.visible_tiles.contains(&(pos.x, pos.y));
                    let preview_text = format!("{} 正在游荡", name.0);
                    if in_fov && preview.last_preview.as_deref() != Some(&preview_text) {
                        event_log.push(preview_text.clone());
                        preview.last_preview = Some(preview_text);
                    }
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

/// ─────────────────────────────────────────────────────
/// 拾取物品
/// ─────────────────────────────────────────────────────
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

/// ─────────────────────────────────────────────────────
/// 经验发放 + 升级
/// ─────────────────────────────────────────────────────
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

/// ─────────────────────────────────────────────────────
/// Buff 递减
/// ─────────────────────────────────────────────────────
pub fn buff_tick_system(mut query: Query<&mut Buffs, With<Player>>) {
    for mut b in query.iter_mut() {
        if b.shield_turns > 0 { b.shield_turns -= 1; if b.shield_turns <= 0 { b.shield_def = 0; } }
        if b.berserk_turns > 0 { b.berserk_turns -= 1; if b.berserk_turns <= 0 { b.berserk_atk = 0; } }
    }
}

/// ─────────────────────────────────────────────────────
/// 技能执行
/// ─────────────────────────────────────────────────────
pub fn skill_tick_system(
    mut pending: ResMut<PendingSkill>,
    mut player: Query<(&mut Stats, &Skills, &mut Buffs, &crate::Position, &PlayerClass), (With<Player>, Without<Monster>)>,
    mut monsters: Query<(&mut Stats, &crate::Position, &EntityName), (With<Monster>, Without<Player>)>,
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
                    let is_crit = stats.crit_rate > game_rng.rng.random::<f32>();
                    let mut dmg = (total_dmg - ms.defense as i32).max(1);
                    if is_crit { dmg = (dmg as f32 * (1.0 + stats.crit_damage)).round() as i32; }
                    ms.hp -= dmg;
                    event_log.push(format!("火球击中 {}！{}伤", mn.0, dmg));
                    hit = true;
                }
            }
            if !hit { event_log.push(String::from("附近没有敌人")); }
        }
        SkillKind::Shield { def_boost, duration } => {
            // 同上...简化
        }
        SkillKind::Berserk { atk_boost, duration } => {
            // 同上...简化
        }
    }
}
pub fn apply_skill(skill_idx: usize) {
    world!(mut).resource_mut::<PendingSkill>().idx = Some(skill_idx);
}

// ══════════════════════════════════════════════════════
// 新 AV 引擎（Phase 1 — HSR 核心模型）
// ══════════════════════════════════════════════════════

/// 一次推进的结果
#[derive(Debug, Clone, PartialEq)]
pub struct AdvanceResult {
    /// 本次推进量
    pub amount: f32,
    /// 本轮执行了的实体
    pub executed: Vec<Entity>,
    /// 本轮新锁定的实体
    pub locked: Vec<Entity>,
    /// 是否有实体执行了动作
    pub any_executed: bool,
    /// 是否有实体（含玩家）进入了锁定区
    pub any_locked: bool,
    /// 玩家是否进入了锁定区
    pub player_locked: bool,
    /// 是否触发了战斗事件（进入/退出战斗）
    pub combat_event: Option<CombatEvent>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CombatEvent {
    Started,
    Ended,
}

/// 按给定量推进所有实体的 AV，处理执行（≤0）和锁定（≤reaction_time）。
///
/// 锁定阈值使用 `ActionValue.reaction_time`。
/// 即使 amount ≤ 0，也会执行当前 AV ≤ 0 的实体（处理"玩家刚提交、AV=0"的情况）。
pub fn advance_by(amount: f32) -> AdvanceResult {
    // amount < 0 视为 0（防御性处理），amount = 0 仍需要处理待执行实体
    let amount = amount.max(0.0);

    // 阶段 1：同步减去推进量，收集 AV ≤ 0 的实体
    let mut exec_set: Vec<Entity> = Vec::new();
    {
        let mut w = world!(mut);
        let mut q = w.query::<(Entity, &mut ActionValue)>();
        for (entity, mut av) in q.iter_mut(&mut *w) {
            if amount > 0.0 {
                av.current_av -= amount;
            }
            if av.current_av <= 0.0 {
                exec_set.push(entity);
            }
        }
    }

    // 阶段 2：排序 + 执行所有 AV≤0 的实体
    {
        let w = world!();
        exec_set.sort_by(|&a, &b| {
            let sa = w.get::<ActionValue>(a).map(|av| av.speed).unwrap_or(0.0);
            let sb = w.get::<ActionValue>(b).map(|av| av.speed).unwrap_or(0.0);
            sb.partial_cmp(&sa).unwrap()
        });
        // 玩家总是优先于同速怪物
        // (借用 w 会阻止排序，所以排序先完成)
    }
    exec_set.sort_by_key(|&e| world!().get::<Player>(e).is_none());

    for &entity in &exec_set {
        execute_entity(entity);
        tick_speed_modifier(entity);
    }

    // 阶段 3：锁定所有 AV ≤ reaction_time 的实体
    let mut lock_set: Vec<Entity> = Vec::new();
    let mut player_locked = false;
    {
        let mut w = world!(mut);
        let mut q = w.query::<(Entity, &ActionValue, &ActionPrediction)>();
        for (entity, av, pred) in q.iter(&*w) {
            if av.current_av <= av.reaction_time && !pred.locked && !pred.just_confirmed {
                lock_set.push(entity);
            }
        }
        for &entity in &lock_set {
            if let Some(mut pred) = w.get_mut::<ActionPrediction>(entity) {
                pred.locked = true;
            }
            if w.get::<Player>(entity).is_some() {
                player_locked = true;
            }
        }
    }

    // 阶段 4：有执行 → 批量重新预测
    if !exec_set.is_empty() {
        world!(mut).run_system_once(predict_monster_actions_system);
    }

    // 阶段 5：战斗事件检测（独立锁）
    let combat_event = {
        let mut w = world!(mut);
        detect_combat_event_inner(&mut *w, &exec_set)
    };

    AdvanceResult {
        amount,
        executed: exec_set.clone(),
        locked: lock_set.clone(),
        any_executed: !exec_set.is_empty(),
        any_locked: !lock_set.is_empty(),
        player_locked,
        combat_event,
    }
}

/// ─────────────────────────────────────────────────
/// HSR 标准操作：拉条 / 推条 / 加减速
/// ─────────────────────────────────────────────────

/// 拉条（行动提前）：直接扣减目标剩余 AV。
///
/// 公式：`new_av = current_av - base_av × ratio`
/// 若结果 ≤ 0 则目标本 tick 立即行动。
pub fn action_pull(entity: Entity, ratio: f32) {
    if let Some(mut av) = world!(mut).get_mut::<ActionValue>(entity) {
        let pull_amount = av.base_av * ratio;
        av.current_av = (av.current_av - pull_amount).max(0.0);
    }
}

/// 推条（行动延后）：直接增加目标剩余 AV。
///
/// 公式：`new_av = current_av + base_av × ratio`
/// 结算优先级通常先于拉条。
pub fn action_push(entity: Entity, ratio: f32) {
    if let Some(mut av) = world!(mut).get_mut::<ActionValue>(entity) {
        av.current_av += av.base_av * ratio;
    }
}

/// 加减速：修改速度属性，按比例缩放剩余 AV。
///
/// 公式：`new_av = current_av × (old_speed / new_speed)`
/// `new_base_av = base_av × (old_speed / new_speed)`
/// 离终点越近收益越低（天然成立）。
pub fn modify_speed(entity: Entity, factor: f32) {
    if let Some(mut av) = world!(mut).get_mut::<ActionValue>(entity) {
        let old_speed = av.speed;
        av.speed *= factor;
        let ratio = old_speed / av.speed;
        av.current_av *= ratio;
        av.base_av *= ratio;
    }
}

/// ─────────────────────────────────────────────────
/// 精确推进
/// ─────────────────────────────────────────────────

/// 计算到最近关键点的 AV 距离。
///
/// 执行点优先于锁定点——怪物先行动，玩家再决策。
pub fn next_key_point_distance() -> f32 {
    let mut min_exec = f32::MAX;
    let mut min_lock = f32::MAX;
    let mut has_pending = false;
    let mut w = world!(mut);
    let mut q = w.query::<(&ActionValue, &ActionPrediction)>();
    for (av, pred) in q.iter(&mut *w) {
        if av.current_av <= 0.0 {
            has_pending = true;
        }
        if av.current_av > 0.0 {
            min_exec = min_exec.min(av.current_av);
        }
        if !pred.locked && av.current_av > av.reaction_time {
            let d = av.current_av - av.reaction_time;
            min_lock = min_lock.min(d);
        }
    }
    if has_pending { return 0.0; }
    // 有实体可执行且执行距离 ≤ 最小锁定距离 → 优先执行
    if min_exec < f32::MAX && min_exec <= min_lock {
        return min_exec;
    }
    if min_lock < f32::MAX { return min_lock; }
    0.0
}

/// 循环推进，直到无可推进或战斗中遇到锁定。
///
/// - 非战斗时：一路推到 AV=0 执行，锁定不中断。
/// - 战斗时：遇到锁定立即返回，让玩家确认。
pub fn advance_to_next_decision_point() -> AdvanceResult {
    let mut last_result = AdvanceResult {
        amount: 0.0, executed: vec![], locked: vec![],
        any_executed: false, any_locked: false, player_locked: false,
        combat_event: None,
    };

    loop {
        let dist = next_key_point_distance();
        if dist < 0.0 {
            break;
        }
        if dist == 0.0 {
            // 有实体 AV≤0 待执行 → 用 advance_by(0) 处理它们
            let r = advance_by(0.0);
            let player_executed = r.executed.iter().any(|&e| world!().get::<crate::Player>(e).is_some());
            last_result.any_executed |= r.any_executed;
            last_result.any_locked |= r.any_locked;
            last_result.player_locked |= r.player_locked;
            last_result.executed.extend(r.executed);
            last_result.locked.extend(r.locked);
            if r.player_locked { break; }
            if player_executed { break; }
            continue;
        }
        let r = advance_by(dist);
        let player_executed = r.executed.iter().any(|&e| world!().get::<crate::Player>(e).is_some());
        last_result.amount += r.amount;
        last_result.executed.extend(r.executed);
        last_result.locked.extend(r.locked);
        last_result.any_executed |= r.any_executed;
        last_result.any_locked |= r.any_locked;
        last_result.player_locked |= r.player_locked;
        if r.combat_event.is_some() && last_result.combat_event.is_none() {
            last_result.combat_event = r.combat_event;
        }
        if r.player_locked {
            break;
        }
        if player_executed {
            break;
        }
    }

    last_result
}

/// ─────────────────────────────────────────────────
/// 内部辅助
/// ─────────────────────────────────────────────────

/// 递减 SpeedModifier 的剩余 tick，归零后移除。
fn tick_speed_modifier(entity: Entity) {
    let mut w = world!(mut);
    let should_remove = {
        if let Some(mut sm) = w.get_mut::<SpeedModifier>(entity) {
            if sm.remaining_ticks > 0 {
                sm.remaining_ticks -= 1;
            }
            sm.remaining_ticks == 0
        } else {
            false
        }
    };
    if should_remove {
        // 先提取 factor，再调用 modify_speed（避免双重借用）
        let factor = w.get::<SpeedModifier>(entity).map(|sm| sm.factor).unwrap_or(1.0);
        modify_speed(entity, 1.0 / factor);
        w.entity_mut(entity).remove::<SpeedModifier>();
    }
}

/// 检测是否有战斗事件的切换。
fn detect_combat_event_inner(world: &mut World, executed: &[Entity]) -> Option<CombatEvent> {
    let has_monsters = world.query::<&Monster>().iter(world).next().is_some();
    let pacing = world.resource::<GamePacing>();

    // 退出战斗：无怪物 + 当前标记为战斗中
    if !has_monsters && pacing.combat_active {
        return Some(CombatEvent::Ended);
    }

    // 进入战斗：执行了攻击动作（由外部通过 combat_active flag 处理，
    // 这里只检测"因攻击而触发"的场景）
    for &e in executed {
        if let Some(pred) = world.get::<ActionPrediction>(e) {
            if matches!(pred.kind, ActionKind::Attack | ActionKind::BumpAttack)
                || matches!(pred.kind, ActionKind::Skill(_))
            {
                if !pacing.combat_active {
                    return Some(CombatEvent::Started);
                }
            }
        }
    }

    None
}

/// 执行一个实体的已锁定动作（AV 引擎唯一执行入口）。
///
/// 执行后自动重置 AV、解锁预测，并处理 SpeedModifier 消耗。
fn execute_entity(entity: Entity) {
    let mut w = world!(mut);
    let pred = match w.get::<ActionPrediction>(entity) {
        Some(p) => p.clone(),
        None => return,
    };
    let is_player = w.get::<Player>(entity).is_some();
    let is_monster = w.get::<Monster>(entity).is_some();

    // 获取玩家位置（供怪物行动用）
    let player_pos = w.query::<(&Player, &Position)>().iter(&*w).next().map(|(_, p)| (p.x, p.y));

    // ── 执行行动 ──
    // 注意：这些内部函数会自己获取锁，所以需要先 drop w
    drop(w);
    match pred.kind {
        ActionKind::Chase if is_monster => execute_monster_chase(entity, player_pos),
        ActionKind::Flee if is_monster => execute_monster_flee(entity, player_pos),
        ActionKind::Wander if is_monster => execute_monster_wander(entity),
        ActionKind::Move if is_player => execute_player_move(),
        _ => {} // 其他类型暂时不处理
    }

    // ── 重置 AV ──
    let mut w = world!(mut);
    let agility = w.get::<Stats>(entity).map(|s| s.agility).unwrap_or(10);
    let base_cost = match pred.kind {
        ActionKind::Move | ActionKind::Chase | ActionKind::Flee | ActionKind::Wander => crate::action_cost::MOVE,
        ActionKind::Attack | ActionKind::BumpAttack => crate::action_cost::ATTACK,
        ActionKind::Wait => crate::action_cost::WAIT,
        ActionKind::Skill(_) => crate::action_cost::SKILL_CAST,
        _ => crate::action_cost::MOVE,
    };
    if let Some(mut av) = w.get_mut::<ActionValue>(entity) {
        let new = ActionValue::with_cost(base_cost, agility);
        *av = new;
    }

    // ── 重置预测 ──
    if let Some(mut p) = w.get_mut::<ActionPrediction>(entity) {
        p.locked = false;
        p.just_confirmed = false;
        p.desc = if is_player { "移动" } else { "追击" }.into();
        p.kind = if is_player { ActionKind::Move } else { ActionKind::Chase };
    }

    // ── 战斗事件日志 ──
    if matches!(pred.kind, ActionKind::Attack | ActionKind::BumpAttack | ActionKind::Skill(_)) {
        let msg = if is_player { "你发起了攻击" } else { "怪物攻击了你" };
        w.resource_mut::<EventLog>().push(msg);
    }
}

// ── 怪物行动执行（从 execute_entity 拆出，提高可读性） ──

fn execute_monster_chase(
    entity: Entity,
    player_pos: Option<(usize, usize)>,
) {
    let Some((px, py)) = player_pos else { return };
    let mut w = world!(mut);
    let (pos_x, pos_y) = match w.get::<Position>(entity) {
        Some(p) => (p.x, p.y),
        None => return,
    };
    let dist = pos_x.abs_diff(px) + pos_y.abs_diff(py);
    if dist > 1 {
        let dx = if px > pos_x { 1 } else if px < pos_x { -1 } else { 0 };
        let dy = if py > pos_y { 1 } else if py < pos_y { -1 } else { 0 };
        let attempts = if px.abs_diff(pos_x) >= py.abs_diff(pos_y) {
            vec![(dx, 0), (0, dy)]
        } else {
            vec![(0, dy), (dx, 0)]
        };
        for (ndx, ndy) in attempts {
            let nx = pos_x.wrapping_add_signed(ndx);
            let ny = pos_y.wrapping_add_signed(ndy);
            if nx < MAP_WIDTH && ny < MAP_HEIGHT
                && w.resource::<Map>().tiles[ny][nx] == Tile::Floor
                && !w.resource::<OccupancyMap>().is_occupied(nx, ny)
            {
                if let Some(mut pos_mut) = w.get_mut::<Position>(entity) {
                    pos_mut.x = nx;
                    pos_mut.y = ny;
                }
                break;
            }
        }
    } else {
        let dmg = w.get::<Stats>(entity).map(|s| s.attack as i32).unwrap_or(1);
        if let Some(player_e) = w.query::<(Entity, &Player)>().iter(&*w).next().map(|(e, _)| e) {
            if let Some(mut ps) = w.get_mut::<Stats>(player_e) {
                ps.hp -= dmg.max(1);
                let name = w.get::<EntityName>(entity).map(|n| n.0.clone()).unwrap_or("怪物".into());
                w.resource_mut::<EventLog>().push(format!("{} 攻击了你，{}伤", name, dmg));
                w.resource_mut::<GamePacing>().combat_active = true;
            }
        }
    }
}

fn execute_monster_flee(
    entity: Entity,
    player_pos: Option<(usize, usize)>,
) {
    let Some((px, py)) = player_pos else { return };
    let mut w = world!(mut);
    let (pos_x, pos_y) = match w.get::<Position>(entity) {
        Some(p) => (p.x, p.y),
        None => return,
    };
    let dirs: [(isize, isize); 4] = [(0, -1), (0, 1), (-1, 0), (1, 0)];
    let mut best: Option<(usize, usize)> = None;
    let mut best_dist = 0usize;
    for &(dx, dy) in &dirs {
        let nx = pos_x.wrapping_add_signed(dx);
        let ny = pos_y.wrapping_add_signed(dy);
        if nx < MAP_WIDTH && ny < MAP_HEIGHT
            && w.resource::<Map>().tiles[ny][nx] == Tile::Floor
            && !w.resource::<OccupancyMap>().is_occupied(nx, ny)
        {
            let d = nx.abs_diff(px) + ny.abs_diff(py);
            if d > best_dist { best_dist = d; best = Some((nx, ny)); }
        }
    }
    if let Some((nx, ny)) = best {
        if let Some(mut pos_mut) = w.get_mut::<Position>(entity) {
            pos_mut.x = nx;
            pos_mut.y = ny;
        }
    }
}

fn execute_monster_wander(entity: Entity) {
    let mut w = world!(mut);
    let dirs: [(isize, isize); 4] = [(0, -1), (0, 1), (-1, 0), (1, 0)];
    let r = (w.resource::<FloorNumber>().0 as usize
        + w.query::<(Entity, &Monster)>().iter(&*w).count()) % 4;
    let (dx, dy) = dirs[r];
    if let Some(pos) = w.get::<Position>(entity) {
        let nx = pos.x.wrapping_add_signed(dx);
        let ny = pos.y.wrapping_add_signed(dy);
        if nx < MAP_WIDTH && ny < MAP_HEIGHT
            && w.resource::<Map>().tiles[ny][nx] == Tile::Floor
            && !w.resource::<OccupancyMap>().is_occupied(nx, ny)
        {
            if let Some(mut pos_mut) = w.get_mut::<Position>(entity) {
                pos_mut.x = nx;
                pos_mut.y = ny;
            }
        }
    }
}

fn execute_player_move() {
    let dir = world!().resource::<PendingInput>().direction;
    if let Some((dx, dy)) = dir {
        crate::set_player_dir(dx, dy);
        crate::rebuild_occupancy();
        world!(mut).run_system_once(crate::movement_system);
        crate::rebuild_occupancy();
        world!(mut).resource_mut::<PendingInput>().direction = None;
    }
}

/// 使用脑链决定怪物下一步动作（纯预测，无副作用）
fn decide_monster_action(stats: &Stats, brain: &MonsterBrain, can_see_player: bool) -> (String, ActionKind) {
    for b in &brain.behaviors {
        match b {
            AiBehavior::FleeWhenHurt { hp_threshold } => {
                if (stats.hp as f32) < (stats.max_hp as f32) * hp_threshold {
                    return ("逃跑".into(), ActionKind::Flee);
                }
            }
            AiBehavior::ChasePlayer => {
                if can_see_player {
                    return ("追击".into(), ActionKind::Chase);
                }
            }
            AiBehavior::Wander => return ("游荡".into(), ActionKind::Wander),
        }
    }
    ("游荡".into(), ActionKind::Wander)
}

/// 批量预测：为所有未锁定的怪物写入 ActionPrediction（无副作用）。
///
/// 已锁定（`pred.locked == true`）的怪物跳过，保留其锁定时的预测不变。
/// 每轮写入当前动作和下轮动作，供行动轴展示两轮预测。
pub fn predict_monster_actions_system(
    mut monster_query: Query<(
        &Stats, &MonsterBrain, &Viewshed, &mut ActionPrediction,
    ), (With<Monster>, Without<Player>)>,
    player_query: Query<&Position, With<Player>>,
) {
    let player_pos = player_query.iter().next();

    for (stats, brain, viewshed, mut pred) in monster_query.iter_mut() {
        // 已锁定的实体不可修改预测
        if pred.locked {
            continue;
        }

        let can_see = player_pos.map_or(false, |pp| {
            viewshed.visible_tiles.contains(&(pp.x, pp.y))
        });

        // 本轮预测
        let (desc, kind) = decide_monster_action(stats, brain, can_see);
        pred.desc = desc;
        pred.kind = kind;

        // 下轮预测（假设执行当前动作后的状态再做一次决策）
        let (next_desc, next_kind) = decide_monster_action(stats, brain, can_see);
        pred.next_desc = next_desc;
        pred.next_kind = next_kind;
    }
}
