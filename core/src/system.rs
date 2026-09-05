//! 核心 ECS 系统。
//!
//! 建议的系统顺序：
//! `execute_basic_attack_system -> apply_damage_system -> record_be_attacked_system
//!  -> check_death_system -> apply_exp_system -> fov_system -> memory -> occupancy`。

use crate::balance::{exp_to_next_level, max_hp_for, max_mp_for};
use crate::combat::compute_melee_damage;
use crate::components::*;
use crate::entity_cls::{EntityClass, Player, Stairs};
use crate::events::{AttackEvent, DeathEvent, LevelUpEvent};
use crate::fov::calculate_visible_tiles;
use crate::map::Map;
use crate::resources::{
    EventLog, EventMessage, GameRng, MapMemory, OccupancyMap, PendingExp, TurnManager,
    VisibleMemory,
};
use bevy_ecs::prelude::*;
use std::collections::HashSet;

// ── 行动执行 ─────────────────────────────────────────

/// 读取 `Active + BasicAttack`，计算伤害并写 `AttackEvent`，然后清行动状态。
///
/// 伤害应用交给 `apply_damage_system`，本系统不直接修改 `Health`。
pub fn execute_basic_attack_system(
    mut commands: Commands,
    actors: Query<(Entity, &BasicAttack), With<Active>>,
    positions: Query<&Position>,
    attacks: Query<&Attack>,
    defenses: Query<&Defense>,
    crits: Query<(&CritRate, &CritDamage)>,
    healths: Query<&Health>,
    mut rng: ResMut<GameRng>,
    mut attack_events: EventWriter<AttackEvent>,
) {
    for (entity, action) in actors.iter() {
        let (Some(apos), Some(tpos)) = (
            positions.get(entity).ok(),
            positions.get(action.target).ok(),
        ) else {
            continue;
        };

        let adjacent = apos.x.abs_diff(tpos.x) <= 1 && apos.y.abs_diff(tpos.y) <= 1;
        let target_alive = healths
            .get(action.target)
            .map(|h| h.is_alive())
            .unwrap_or(false);
        if !adjacent || !target_alive {
            commands.entity(entity).remove::<Active>();
            commands.entity(entity).remove::<BasicAttack>();
            commands.entity(entity).insert(Failure);
            continue;
        }

        let attack = attacks.get(entity).map(|a| a.0).unwrap_or(0.0);
        let defense = defenses.get(action.target).map(|d| d.0).unwrap_or(0.0);
        let (crit_rate, crit_damage) = crits
            .get(entity)
            .map(|(r, d)| (r.0, d.0))
            .unwrap_or((0.0, 0.0));
        let crit_roll = rng.random_f64();
        let result = compute_melee_damage(attack, defense, crit_rate, crit_damage, crit_roll);

        attack_events.write(AttackEvent {
            attacker: entity,
            target: action.target,
            damage: result.damage,
            is_crit: result.is_crit,
        });

        commands.entity(entity).remove::<Active>();
        commands.entity(entity).remove::<BasicAttack>();
        commands.entity(entity).insert(Idle);
    }
}

// ── 伤害与受击记录 ───────────────────────────────────

pub fn apply_damage_system(
    mut attack_events: EventReader<AttackEvent>,
    mut healths: Query<&mut Health>,
) {
    for event in attack_events.read() {
        if let Ok(mut health) = healths.get_mut(event.target) {
            *health = health.damage(event.damage);
        }
    }
}

pub fn record_be_attacked_system(
    mut commands: Commands,
    mut attack_events: EventReader<AttackEvent>,
    targets: Query<(Entity, Option<&BeAttacked>), (With<Health>, With<NeedRecordBeAttacked>)>,
) {
    for event in attack_events.read() {
        if let Ok((target, existing)) = targets.get(event.target) {
            let mut record = existing.copied().unwrap_or_else(|| BeAttacked::new(event.attacker));
            record.by = event.attacker;
            record.av_since_hit = 0.0;
            commands.entity(target).insert(record);
        }
    }
}

// ── 死亡 ─────────────────────────────────────────────

/// 必须最后执行：把死亡实体转为 `DeathEvent`，并处理玩家失败/怪物经验。
pub fn check_death_system(
    mut commands: Commands,
    query: Query<(
        Entity,
        &Health,
        Option<&Player>,
        Option<&ExperienceReward>,
    )>,
    mut death_events: EventWriter<DeathEvent>,
    mut pending_exp: ResMut<PendingExp>,
    mut turn_manager: ResMut<TurnManager>,
    mut event_log: ResMut<EventLog>,
) {
    for (entity, health, player, reward) in query.iter() {
        if health.is_alive() {
            continue;
        }

        death_events.write(DeathEvent { entity });

        if player.is_some() {
            turn_manager.game_over = true;
            event_log.push(EventMessage::danger("你死了"));
            continue;
        }

        if let Some(reward) = reward {
            pending_exp.amount += reward.0;
        }
        commands.entity(entity).despawn();
    }
}

// ── 经验与升级 ───────────────────────────────────────

pub fn apply_exp_system(
    mut players: Query<(
        Entity,
        &mut Experience,
        &mut Level,
        &mut Health,
        &mut Magic,
        &Defense,
        &MagicMastery,
    ), With<Player>>,
    mut pending_exp: ResMut<PendingExp>,
    mut event_log: ResMut<EventLog>,
    mut level_events: EventWriter<LevelUpEvent>,
) {
    if pending_exp.amount <= 0.0 {
        return;
    }
    let gained = pending_exp.amount;
    pending_exp.amount = 0.0;

    for (entity, mut exp, mut level, mut health, mut magic, defense, mastery) in players.iter_mut() {
        exp.add(gained);
        while exp.overflow() > 0.0 {
            exp.exp = exp.overflow();
            level.0 += 1;

            let max_hp = max_hp_for(level.0, defense.0);
            let max_mp = max_mp_for(level.0, mastery.0);
            health.max = max_hp;
            health.current = max_hp;
            magic.max = max_mp;
            magic.current = max_mp;
            exp.exp_to_next = exp_to_next_level(level.0);

            level_events.write(LevelUpEvent {
                entity,
                new_level: level.0,
            });
            event_log.push(EventMessage::system(format!(
                "升级！达到 Lv.{}",
                level.0
            )));
        }
    }
}

// ── 视野与记忆 ───────────────────────────────────────

pub fn fov_system(mut query: Query<(&Position, &mut Viewshed)>, map: Res<Map>) {
    for (pos, mut viewshed) in query.iter_mut() {
        viewshed.visible_tiles =
            calculate_visible_tiles(pos.x, pos.y, viewshed.range, &map);
    }
}

pub fn update_map_memory_system(
    players: Query<(&Player, &Viewshed)>,
    mut memory: ResMut<MapMemory>,
) {
    for (_, viewshed) in players.iter() {
        for &(x, y) in &viewshed.visible_tiles {
            memory.explored[y][x] = true;
        }
    }
}

pub fn update_visible_memory_system(
    players: Query<(&Player, &Viewshed)>,
    entities: Query<(Entity, &Position, Option<&Player>)>,
    alive: Query<(Entity,)>,
    mut memory: ResMut<VisibleMemory>,
) {
    let visible: HashSet<(usize, usize)> = players
        .iter()
        .next()
        .map(|(_, v)| v.visible_tiles.iter().copied().collect())
        .unwrap_or_default();

    let seen: Vec<(Entity, (usize, usize))> = entities
        .iter()
        .filter(|(_, pos, player)| {
            player.is_none() && visible.contains(&(pos.x, pos.y))
        })
        .map(|(entity, pos, _)| (entity, (pos.x, pos.y)))
        .collect();

    let alive: HashSet<Entity> = alive.iter().map(|(e,)| e).collect();
    for (entity, pos) in seen {
        memory.entries.insert(entity, pos);
    }
    memory.entries.retain(|e, _| alive.contains(e));
}

// ── 碰撞图 ───────────────────────────────────────────

pub fn rebuild_occupancy_system(
    entities: Query<(Entity, &Position, Option<&Stairs>, Option<&EntityClass>)>,
    mut occupancy: ResMut<OccupancyMap>,
) {
    occupancy.clear();
    for (entity, pos, stairs, class) in entities.iter() {
        if stairs.is_some() {
            continue;
        }
        if matches!(class, Some(EntityClass::Item)) {
            continue;
        }
        occupancy.set(pos.x, pos.y, entity);
    }
}

// ── Schedule 与便捷入口 ──────────────────────────────

/// 构建标准结算 Schedule。调用方可在前后插入自己的系统。
pub fn build_core_schedule() -> Schedule {
    let mut schedule = Schedule::default();
    schedule.add_systems((
        execute_basic_attack_system,
        apply_damage_system,
        record_be_attacked_system,
        check_death_system,
        apply_exp_system,
        fov_system,
        update_map_memory_system,
        update_visible_memory_system,
        rebuild_occupancy_system,
    ));
    schedule
}

/// 直接运行一次核心结算系统。
pub fn run_settle_systems(world: &mut World) {
    let mut schedule = build_core_schedule();
    schedule.run(world);
}
