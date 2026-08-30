//! 行动执行引擎：队列推进、保活检查、行动执行

use crate::types::*;
use bevy_ecs::prelude::*;
use bevy_ecs::system::RunSystemOnce;
use dungeon_core::OptionLogExt;
use dungeon_core::{MAP_HEIGHT, MAP_WIDTH, Map, components::*, items::*, ops, resources::*};

/// 推进行动队列，返回实际推进量
pub fn advance_action_queue(world: &mut World) -> f32 {
    let dist;
    let ready;
    {
        dist = {
            let queue = world.resource::<ActionQueue>();
            queue.next_event_distance().unwrap_or(0.0)
        };
        if dist <= 0.0 {
            return 0.0;
        }
        world.resource_mut::<ActionQueue>().advance(dist);

        // 推进所有实体的 ActiveBuffs（与队列同步使用同一 dist）
        {
            let mut q = world.query::<&mut ActiveBuffs>();
            for mut buffs in q.iter_mut(world) {
                buffs.0.retain_mut(|b| {
                    b.remaining_av -= dist;
                    b.remaining_av > 0.0
                });
            }
        }

        // P1: 保活检查所有 av_remaining > 0 的条目，剔除条件不满足的
        // 防止 Chase/Flee/Move 等在等待期间条件已失效的条目白耗 AV
        let invalid: Vec<Entity> = {
            let queue = world.resource::<ActionQueue>();
            queue
                .entries
                .iter()
                .filter(|e| e.av_remaining > 0.0 && !check_condition(world, e))
                .map(|e| e.entity)
                .collect()
        };
        if !invalid.is_empty() {
            world
                .resource_mut::<ActionQueue>()
                .entries
                .retain(|e| !invalid.contains(&e.entity));
        }

        ready = world.resource_mut::<ActionQueue>().pop_ready();
    }

    for entry in &ready {
        if check_condition(world, entry) {
            execute_entry(world, entry);
            let _ = world.run_system_once(dungeon_core::systems::apply_exp_system);
            ops::rebuild_occupancy(world);
        } else {
            world
                .resource_mut::<EventLog>()
                .push(dungeon_core::EventMessage::system("行动被取消"));
        }
    }
    dist
}

/// 检查 (x,y) 能否走到 (x+dx, y+dy)，对角线额外验证不穿墙角（G31）
pub(crate) fn can_move_to(
    map: &Map,
    occ: &OccupancyMap,
    x: usize,
    y: usize,
    dx: isize,
    dy: isize,
) -> bool {
    let nx = x.wrapping_add_signed(dx);
    let ny = y.wrapping_add_signed(dy);
    if nx >= MAP_WIDTH || ny >= MAP_HEIGHT {
        return false;
    }
    if !map.tiles[ny][nx].walkable() {
        return false;
    }
    if occ.is_occupied(nx, ny) {
        return false;
    }
    // G31: 对角移动需两侧正交格均可通行且未被占用（防 corner-cutting 斜穿墙角）
    if dx != 0 && dy != 0 {
        let sx = x.wrapping_add_signed(dx);
        let wy = y.wrapping_add_signed(dy);
        if sx >= MAP_WIDTH || wy >= MAP_HEIGHT {
            return false;
        }
        if !map.tiles[y][sx].walkable() || occ.is_occupied(sx, y) {
            return false;
        }
        if !map.tiles[wy][x].walkable() || occ.is_occupied(x, wy) {
            return false;
        }
    }
    true
}

/// 两个实体是否 8 方向邻接（G29: 攻击距离校验与怪物侧规则对称）
fn adjacent_8(world: &World, a: Entity, b: Entity) -> bool {
    world
        .get::<Position>(a)
        .zip(world.get::<Position>(b))
        .map(|(pa, pb)| pa.x.abs_diff(pb.x) <= 1 && pa.y.abs_diff(pb.y) <= 1)
        .unwrap_or(false)
}

pub(crate) fn chase_condition(world: &World, entity: Entity) -> bool {
    let player_pos = world
        .try_query::<(&Player, &Position)>()
        .expect_log("Player+Position registered at init")
        .iter(world)
        .next()
        .map(|(_, p)| (p.x, p.y));
    if let Some((px, py)) = player_pos
        && world
            .get::<Viewshed>(entity)
            .map(|v| v.visible_tiles.contains(&(px, py)))
            .unwrap_or(false)
    {
        return true;
    }
    world
        .get::<LastKnownPlayerPos>(entity)
        .map(|l| l.0.is_some())
        .unwrap_or(false)
}

pub(crate) fn flee_condition(world: &World, entity: Entity) -> bool {
    world
        .get::<Stats>(entity)
        .map(|s| (s.hp as f32 / s.max_hp as f32) < dungeon_core::FLEE_HP_RATIO)
        .unwrap_or(false)
}

fn check_condition(world: &World, entry: &ActionEntry) -> bool {
    if let Some(ref action) = entry.action {
        return action.check_condition(world, entry.entity);
    }
    match &entry.kind {
        ActionKindV3::Chase => {
            let player_pos = world
                .try_query::<(&Player, &Position)>()
                .expect_log("Player+Position registered at init")
                .iter(world)
                .next()
                .map(|(_, p)| (p.x, p.y));
            if let Some((px, py)) = player_pos
                && world
                    .get::<Viewshed>(entry.entity)
                    .map(|v| v.visible_tiles.contains(&(px, py)))
                    .unwrap_or(false)
            {
                return true;
            }
            // 玩家不在视野内但有记忆位置 → 继续追击
            world
                .get::<LastKnownPlayerPos>(entry.entity)
                .map(|l| l.0.is_some())
                .unwrap_or(false)
        }
        ActionKindV3::Flee => {
            // G30 滞回：进入逃跑 HP<25%，退出逃跑 HP≥30%——保活检查用退出阈值
            world
                .get::<Stats>(entry.entity)
                .map(|s| (s.hp as f32 / s.max_hp as f32) < dungeon_core::FLEE_HP_RATIO_EXIT)
                .unwrap_or(false)
        }
        ActionKindV3::Wander | ActionKindV3::Wait => true,
        ActionKindV3::Move { dx, dy } => {
            if let Some(pos) = world.get::<Position>(entry.entity) {
                let map = world.resource::<Map>();
                let occ = world.resource::<OccupancyMap>();
                can_move_to(map, occ, pos.x, pos.y, *dx, *dy)
            } else {
                false
            }
        }
        ActionKindV3::Attack { target } => {
            // G29: 目标仍是怪物且与攻击者 8 方向邻接（与怪物攻击规则对称）
            world.get::<Monster>(*target).is_some() && adjacent_8(world, entry.entity, *target)
        }
        ActionKindV3::Skill(_) => true,
        ActionKindV3::Throw { .. } => true,
    }
}

fn execute_entry(world: &mut World, entry: &ActionEntry) {
    if let Some(ref action) = entry.action {
        action.execute(world, entry.entity);
        return;
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

pub(crate) fn execute_chase(world: &mut World, entity: Entity) {
    let Some(player_entity) = world
        .query::<(Entity, &Player)>()
        .iter(world)
        .next()
        .map(|(e, _)| e)
    else {
        return;
    };
    let player_pos = world.get::<Position>(player_entity).map(|p| (p.x, p.y));
    let pos = match world.get::<Position>(entity) {
        Some(p) => (p.x, p.y),
        None => return,
    };

    // 判断目标：玩家可见 → 玩家位置；不可见 → 记忆位置
    let (target_visible, target) = if let Some((ppx, ppy)) = player_pos {
        let can_see = world
            .get::<Viewshed>(entity)
            .map(|v| v.visible_tiles.contains(&(ppx, ppy)))
            .unwrap_or(false);
        if can_see {
            (true, Some((ppx, ppy)))
        } else {
            (
                false,
                world.get::<LastKnownPlayerPos>(entity).and_then(|l| l.0),
            )
        }
    } else {
        (
            false,
            world.get::<LastKnownPlayerPos>(entity).and_then(|l| l.0),
        )
    };

    let Some((px, py)) = target else {
        // 无目标 → 清除记忆
        if let Some(mut lkp) = world.get_mut::<LastKnownPlayerPos>(entity) {
            lkp.0 = None;
        }
        return;
    };

    // 邻接时攻击（含对角，仅当目标是玩家时）
    if target_visible
        && pos.0.abs_diff(px) <= 1
        && pos.1.abs_diff(py) <= 1
        && (pos.0 != px || pos.1 != py)
    {
        monster_attack_player(world, entity, player_entity);
    } else {
        // A* 寻路至目标，取第一步
        let next_step = {
            let map = world.resource::<Map>();
            let occ = world.resource::<OccupancyMap>();
            dungeon_core::pathfinding::astar(pos, (px, py), &map.tiles, Some(occ))
                .and_then(|path| path.first().copied())
        };
        if let Some((nx, ny)) = next_step
            && let Some(mut p) = world.get_mut::<Position>(entity)
        {
            p.x = nx;
            p.y = ny;
        }
    }

    // 到达记忆位置附近但仍未看到玩家 → 清除记忆，进入游荡
    if !target_visible
        && let Some(mut lkp) = world.get_mut::<LastKnownPlayerPos>(entity)
        && let Some((lkx, lky)) = lkp.0
        && pos.0.abs_diff(lkx) <= 2
        && pos.1.abs_diff(lky) <= 2
    {
        lkp.0 = None;
    }
}

/// 怪物近战攻击玩家（G30: chase 邻接攻击与 flee 无路可逃兜底共用；G19: 怪物暴击走通用公式）
fn monster_attack_player(world: &mut World, entity: Entity, player_entity: Entity) {
    let (monster_atk, mon_stats) = world
        .get::<Stats>(entity)
        .map(|s| (s.attack as i32, s.clone()))
        .unwrap_or((1, dungeon_core::Stats::player()));
    let player_def = world
        .query::<(&Player, &Stats, &Equipment, Option<&ActiveBuffs>)>()
        .iter(world)
        .next()
        .map(|(_, ps, eq, ab)| ops::effective_defense(ps, eq, ab) as i32)
        .unwrap_or(0);
    let crit_roll = world.resource_mut::<GameRng>().random_f32();
    let (is_crit, crit_mult) =
        calc_crit(&mon_stats, &dungeon_core::StatBonus::default(), crit_roll);
    let base_dmg = (monster_atk - player_def).max(1);
    let dmg = if is_crit {
        (base_dmg as f32 * crit_mult).round() as i32
    } else {
        base_dmg
    };
    let name = world
        .get::<EntityName>(entity)
        .map(|n| n.0.clone())
        .unwrap_or("怪物".into());
    if let Some(mut ps) = world.get_mut::<Stats>(player_entity) {
        ps.hp -= dmg;
    }
    let crit_suffix = if is_crit { "（暴击）" } else { "" };
    world
        .resource_mut::<EventLog>()
        .push(dungeon_core::EventMessage::danger(format!(
            "{}{} 攻击了你，{}伤",
            name, crit_suffix, dmg
        )));
}

pub(crate) fn execute_flee(world: &mut World, entity: Entity) {
    let player_pos = world
        .query::<(&Player, &Position)>()
        .iter(world)
        .next()
        .map(|(_, p)| (p.x, p.y));
    let Some((px, py)) = player_pos else { return };
    let pos = match world.get::<Position>(entity) {
        Some(p) => (p.x, p.y),
        None => return,
    };
    let dirs: [(isize, isize); 8] = [
        (0, -1),
        (0, 1),
        (-1, 0),
        (1, 0),
        (-1, -1),
        (1, -1),
        (-1, 1),
        (1, 1),
    ];
    let best = {
        let map = world.resource::<Map>();
        let occ = world.resource::<OccupancyMap>();
        let mut best: Option<(usize, usize)> = None;
        let mut best_dist = 0usize;
        for &(dx, dy) in &dirs {
            if !can_move_to(map, occ, pos.0, pos.1, dx, dy) {
                continue;
            }
            let nx = pos.0.wrapping_add_signed(dx);
            let ny = pos.1.wrapping_add_signed(dy);
            let d = nx.abs_diff(px) + ny.abs_diff(py);
            if d > best_dist {
                best_dist = d;
                best = Some((nx, ny));
            }
        }
        best
    };
    if let Some((nx, ny)) = best
        && let Some(mut p) = world.get_mut::<Position>(entity)
    {
        p.x = nx;
        p.y = ny;
    } else {
        // G30: 无路可逃（被逼入死角）→ 邻接玩家时反击，不再原地挨打
        if let Some(pe) = dungeon_core::ops::player_entity(world)
            && adjacent_8(world, entity, pe)
            && monster_atk_visible(world, entity, pe)
        {
            monster_attack_player(world, entity, pe);
        }
    }
}

/// 怪物能否"看见"目标实体（Flee 兜底反击需要视野内才能出手，避免隔墙反击）
fn monster_atk_visible(world: &World, entity: Entity, target: Entity) -> bool {
    let Some((px, py)) = world.get::<Position>(target).map(|p| (p.x, p.y)) else {
        return false;
    };
    world
        .get::<Viewshed>(entity)
        .map(|v| v.visible_tiles.contains(&(px, py)))
        .unwrap_or(false)
}

pub(crate) fn execute_wander(world: &mut World, entity: Entity) {
    let dirs: [(isize, isize); 8] = [
        (0, -1),
        (0, 1),
        (-1, 0),
        (1, 0),
        (-1, -1),
        (1, -1),
        (-1, 1),
        (1, 1),
    ];
    let r = world.resource_mut::<GameRng>().random_range(0, 8) as usize;
    let (dx, dy) = dirs[r];
    let target = if let Some(pos) = world.get::<Position>(entity) {
        let map = world.resource::<Map>();
        let occ = world.resource::<OccupancyMap>();
        can_move_to(map, occ, pos.x, pos.y, dx, dy)
            .then_some((pos.x.wrapping_add_signed(dx), pos.y.wrapping_add_signed(dy)))
    } else {
        None
    };
    if let Some((nx, ny)) = target
        && let Some(mut p) = world.get_mut::<Position>(entity)
    {
        p.x = nx;
        p.y = ny;
    }
}

fn execute_wait(_entity: Entity) {}

fn execute_player_move(world: &mut World, entity: Entity, dx: isize, dy: isize) {
    let (nx, ny) = {
        let ppos = match world.get::<Position>(entity) {
            Some(p) => (p.x, p.y),
            None => return,
        };
        let map = world.resource::<Map>();
        let occ = world.resource::<OccupancyMap>();
        if !can_move_to(map, occ, ppos.0, ppos.1, dx, dy) {
            return;
        }
        (
            ppos.0.wrapping_add_signed(dx),
            ppos.1.wrapping_add_signed(dy),
        )
    };
    if let Some(mut p) = world.get_mut::<Position>(entity) {
        p.x = nx;
        p.y = ny;
    }
}

/// 统一暴击计算（玩家和怪物共享路径）
fn calc_crit(stats: &Stats, bonus: &dungeon_core::StatBonus, crit_roll: f32) -> (bool, f32) {
    let total_crit_rate = (stats.crit_rate + bonus.crit_rate).min(1.0);
    let is_crit = total_crit_rate > crit_roll;
    let crit_mult = if is_crit {
        1.0 + stats.crit_damage
    } else {
        1.0
    };
    (is_crit, crit_mult)
}

fn execute_attack(world: &mut World, attacker: Entity, target: Entity) {
    // G29: 执行入口兜底——目标已离开 8 邻接格则攻击取消（L48: 执行层验证不可绕过）
    if !adjacent_8(world, attacker, target) {
        return;
    }
    let (name, atk_name, dmg, crit);
    {
        let Some(target_stats) = world.get::<Stats>(target).cloned() else {
            return;
        };
        let Some(attacker_stats) = world.get::<Stats>(attacker).cloned() else {
            return;
        };
        name = world
            .get::<EntityName>(target)
            .map(|n| n.0.clone())
            .unwrap_or("怪物".into());
        atk_name = world
            .get::<AttackName>(attacker)
            .map(|a| a.0.clone())
            .unwrap_or("攻击".into());
        let equipment = world
            .get::<Equipment>(attacker)
            .expect_log("Attacker has Equipment");

        let ab = world.get::<ActiveBuffs>(attacker);
        let effective_atk = ops::effective_attack(&attacker_stats, equipment, ab) as i32;
        let target_def = {
            let eq = world.get::<Equipment>(target);
            ops::effective_defense(&target_stats, &eq.cloned().unwrap_or_default(), None) as i32
        };
        let raw_dmg = (effective_atk - target_def).max(1);
        let equip = world.get::<Equipment>(attacker);
        let bonus = equip.map(dungeon_core::equipment_bonus).unwrap_or_default();
        let crit_roll = world.resource_mut::<GameRng>().random_f32();
        (dmg, crit) = {
            let (is_crit, crit_mult) = calc_crit(&attacker_stats, &bonus, crit_roll);
            let d = if is_crit {
                (raw_dmg as f32 * crit_mult).round() as i32
            } else {
                raw_dmg
            };
            (d, is_crit)
        };
    }
    {
        let Some(mut target_stats) = world.get_mut::<Stats>(target) else {
            return;
        };
        target_stats.hp -= dmg;
        if target_stats.hp <= 0 {
            handle_kill(world, target, &name);
        } else {
            world
                .resource_mut::<EventLog>()
                .push(dungeon_core::EventMessage::combat(format!(
                    "你{}了{}{}，造成{}点伤害",
                    atk_name,
                    name,
                    if crit { "！暴击" } else { "" },
                    dmg
                )));
        }
    }
}

pub(crate) fn execute_skill(world: &mut World, entity: Entity, skill_idx: usize) {
    let (skill_kind, cost_mp, skill_name, skill_proficiency, magic_mastery);
    {
        let has_skill = world
            .get::<dungeon_core::Skills>(entity)
            .map(|s| s.list.get(skill_idx).is_some())
            .unwrap_or(false);
        if !has_skill {
            world
                .resource_mut::<dungeon_core::EventLog>()
                .push(dungeon_core::EventMessage::skill("技能未学习".to_string()));
            return;
        }
        let Some(skills) = world.get::<dungeon_core::Skills>(entity) else {
            return;
        };
        let Some(skill) = skills.list.get(skill_idx) else {
            return;
        };
        let Some(stats) = world.get::<Stats>(entity) else {
            return;
        };
        if stats.mp < skill.cost_mp {
            let msg = format!("MP不足，无法施放{}", skill.name);
            world
                .resource_mut::<EventLog>()
                .push(dungeon_core::EventMessage::skill(msg));
            return;
        }
        skill_kind = skill.kind.clone();
        cost_mp = skill.cost_mp;
        skill_name = skill.name.to_string();
        skill_proficiency = skill.proficiency;
        magic_mastery = stats.magic_mastery;
    }
    {
        if let Some(mut stats) = world.get_mut::<Stats>(entity) {
            stats.mp -= cost_mp;
        }
    }
    match skill_kind {
        dungeon_core::SkillKind::Heal { amount } => {
            // Gm6: 治愈量 = amount(15) + 法术精通×1 + 熟练度×3（G18 修复法术精通缺失）
            let effective = amount + magic_mastery as i32 + skill_proficiency as i32 * 3;
            if let Some(mut stats) = world.get_mut::<Stats>(entity) {
                stats.hp = (stats.hp + effective).min(stats.max_hp);
            }
            world
                .resource_mut::<EventLog>()
                .push(dungeon_core::EventMessage::skill(format!(
                    "{}恢复了{}HP（熟练度+{}）",
                    skill_name, effective, skill_proficiency
                )));
        }
        dungeon_core::SkillKind::Shield {
            def_boost,
            duration,
        } => {
            // 新 AV Buff 系统
            let effective_def = def_boost + skill_proficiency as i32 * 2;
            if let Some(mut ab) = world.get_mut::<ActiveBuffs>(entity) {
                let av = duration as f32 * 1000.0;
                if let Some(existing) = ab.0.iter_mut().find(|b| b.kind == BuffKind::Shield) {
                    existing.remaining_av = av;
                    existing.magnitude = effective_def;
                } else {
                    ab.0.push(Buff {
                        kind: BuffKind::Shield,
                        remaining_av: av,
                        magnitude: effective_def,
                        stack_type: BuffStackType::None,
                    });
                }
            }
            world
                .resource_mut::<EventLog>()
                .push(dungeon_core::EventMessage::skill(format!(
                    "{}施放了护盾，防御+{}持续{}秒（熟练度+{}）",
                    skill_name, effective_def, duration, skill_proficiency
                )));
        }
        dungeon_core::SkillKind::Berserk {
            atk_boost,
            duration,
        } => {
            // 新 AV Buff 系统
            let effective_atk = atk_boost + skill_proficiency as i32 * 2;
            if let Some(mut ab) = world.get_mut::<ActiveBuffs>(entity) {
                let av = duration as f32 * 1000.0;
                if let Some(existing) = ab.0.iter_mut().find(|b| b.kind == BuffKind::Berserk) {
                    existing.remaining_av = av;
                    existing.magnitude = effective_atk;
                } else {
                    ab.0.push(Buff {
                        kind: BuffKind::Berserk,
                        remaining_av: av,
                        magnitude: effective_atk,
                        stack_type: BuffStackType::None,
                    });
                }
            }
            world
                .resource_mut::<EventLog>()
                .push(dungeon_core::EventMessage::skill(format!(
                    "{}进入狂暴，攻击+{}持续{}秒（熟练度+{}）",
                    skill_name, effective_atk, duration, skill_proficiency
                )));
        }
    }
}

/// 投掷前置验证（I48 ① 拆分）：副手可投掷 + 射程 + 视线。
/// 返回 Err(取消原因) 时不消耗副手、不产生伤害。
fn validate_throw(
    world: &World,
    attacker: Entity,
    tx: usize,
    ty: usize,
) -> Result<(), &'static str> {
    // 验证 1：副手有可投掷物（防止非投掷物被消耗，I60）
    let has_throwable = world
        .get::<Equipment>(attacker)
        .and_then(|eq| eq.off_hand.as_ref())
        .map(|s| dungeon_core::is_throwable(s.item_id))
        .unwrap_or(false);
    if !has_throwable {
        return Err("没有可投掷的物品");
    }
    // 验证 2：射程（切比雪夫 ≤ 5）+ 视线畅通（I59；I68: 判定收敛到 core）
    let Some(pos) = world.get::<Position>(attacker).map(|p| (p.x, p.y)) else {
        return Err("无法定位投掷者");
    };
    if ops::chebyshev(pos, (tx, ty)) > dungeon_core::THROW_RANGE {
        return Err("目标超出射程");
    }
    let los_clear = {
        let map = world.resource::<Map>();
        ops::los_clear(map, pos, (tx, ty))
    };
    if !los_clear {
        return Err("视线受阻，无法投掷");
    }
    Ok(())
}

/// 投掷确认（I77/D20）：瞄准页 Enter 一次确认即入队。
/// 从 ThrowPreview 取目标 → 校验（L48: 执行入口兜底）→ 清理 UI 状态 → 入队。
/// 返回 true 表示已入队（调用方应推进世界）。
/// 直接入队而非 tap-tap 双确认——投掷已是专用瞄准页，无二次确认必要（与 README/Gm9「Enter 投掷」一致）。
pub fn confirm_throw(world: &mut World) -> bool {
    // 校验（与渲染层 valid_target 同一判定来源；防绕过 UI 直接调用）
    let valid = world.resource::<ThrowPreview>().valid_target;
    if !valid {
        world
            .resource_mut::<EventLog>()
            .push(dungeon_core::EventMessage::system("目标超出射程或视线受阻"));
        return false;
    }
    let (tx, ty) = {
        let tp = world.resource::<ThrowPreview>();
        tp.cursor
    };
    // 清理 UI 状态（I63: 防光标残留；同时清 PlayerPreview 防污染后续 tap-tap）
    world.resource_mut::<ThrowPreview>().active = false;
    world.resource_mut::<LookCursor>().active = false;
    world.resource_mut::<PlayerPreview>().kind = None;
    world.resource_mut::<PageStack>().pop();
    // 入队（enqueue_or_replace：若队列有玩家旧行动则替换，语义同其他行动确认）
    let Some(player) = dungeon_core::ops::player_entity(world) else {
        return false;
    };
    let agility = world.get::<Stats>(player).map(|s| s.agility).unwrap_or(10);
    let av =
        agility_to_reaction(agility) + dungeon_core::THROW_DURATION * agility_speed_factor(agility);
    world.resource_mut::<ActionQueue>().enqueue_or_replace(
        player,
        ActionKindV3::Throw { tx, ty },
        av,
    );
    true
}

/// 投掷执行：石子伤害 + 暴击 + 掉落 + 副手消耗
/// 由 execute_entry 分派，走 AV 队列生命周期
pub(crate) fn execute_throw(world: &mut World, attacker: Entity, tx: usize, ty: usize) {
    if let Err(reason) = validate_throw(world, attacker, tx, ty) {
        world
            .resource_mut::<EventLog>()
            .push(dungeon_core::EventMessage::system(reason.to_string()));
        return;
    }

    // 随机值收集（统一绑定 — G14）
    let (extra, crit_roll) = {
        let mut rng = world.resource_mut::<GameRng>();
        (rng.random_range(0u8, 2u8) as u32, rng.random_f32())
    };

    // 查找目标格上的怪物
    let target = {
        let Some(mut q) = world.try_query::<(Entity, &Position, &Monster)>() else {
            world
                .resource_mut::<EventLog>()
                .push(dungeon_core::EventMessage::system(
                    "投掷内部错误：无法查询实体".to_string(),
                ));
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
        let target_def = world
            .get::<Stats>(target_entity)
            .map(|s| s.defense as i32)
            .unwrap_or(0);
        let raw_dmg = ((base_dmg as i32 + extra as i32 - target_def).max(1)) as u32;
        let final_dmg = (raw_dmg as f32 * crit_mult).round() as i32;
        let target_name = world
            .get::<EntityName>(target_entity)
            .map(|n| n.0.clone())
            .unwrap_or("怪物".into());

        if let Some(mut s) = world.get_mut::<Stats>(target_entity) {
            s.hp -= final_dmg;
        }

        let dead = world
            .get::<Stats>(target_entity)
            .map(|s| s.hp <= 0)
            .unwrap_or(false);
        if dead {
            handle_kill(world, target_entity, &target_name);
        } else {
            world
                .resource_mut::<EventLog>()
                .push(dungeon_core::EventMessage::combat(format!(
                    "石子命中了{}！{}，造成{}点伤害",
                    target_name,
                    if is_crit { "暴击" } else { "" },
                    final_dmg
                )));
        }
    } else {
        world
            .resource_mut::<EventLog>()
            .push(dungeon_core::EventMessage::combat(
                "石子落在地上".to_string(),
            ));
    }

    // 消耗副手 1 颗石子（使用共享函数；验证已在上方通过）
    ops::consume_off_hand(world, attacker);
}

/// 计算玩家暴击率和暴击倍率，复用 calc_crit
fn calc_player_crit(world: &World, crit_roll: f32) -> (bool, f32) {
    let p = ops::player_entity(world);
    match p {
        Some(p) => {
            let p_stats = world.get::<Stats>(p);
            let equip = world.get::<Equipment>(p);
            let bonus = equip.map(dungeon_core::equipment_bonus).unwrap_or_default();
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
    world
        .resource_mut::<EventLog>()
        .push(dungeon_core::EventMessage::combat(format!(
            "击杀了{}！获得{}经验",
            name, exp
        )));
    let pos = world.get::<Position>(entity).map(|p| (p.x, p.y));
    let loot_stacks = {
        let lt = world.get::<LootTable>(entity).cloned();
        lt.map(|l| {
            let mut rng = world.resource_mut::<GameRng>();
            // G32: GameRng 自身实现 RngCore，掉落消耗计入可序列化状态
            l.roll(&mut rng)
        })
        .unwrap_or_default()
    };
    if let Some((px, py)) = pos {
        for stack in &loot_stacks {
            let sname = stack.name();
            world
                .resource_mut::<EventLog>()
                .push(dungeon_core::EventMessage::item(format!(
                    "{}掉落{}x{}",
                    name, sname, stack.count
                )));
            world.spawn((
                ItemPickup {
                    stack: stack.clone(),
                },
                Position { x: px, y: py },
                Renderable {
                    glyph: stack.glyph(),
                    color: stack.color(),
                },
            ));
        }
    }
    world.entity_mut(entity).despawn();
}
