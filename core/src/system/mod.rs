//! 核心 ECS 系统。
//!
//! 建议的系统顺序：
//! `execute_basic_attack_system -> apply_damage_system -> record_be_attacked_system
//!  -> check_death_system -> apply_exp_system -> fov_system -> memory -> occupancy`。

use crate::balance::{exp_to_next_level, max_hp_for, max_mp_for};
use crate::combat::compute_melee_damage;
use crate::components::*;
use crate::entity_cls::{EntityClass, Player, Stairs};
use crate::events::{
    ActionFailedEvent, ActionSucceededEvent, AttackEvent, AttackIntentEvent, DeathEvent,
    LevelUpEvent, ThreatEvent,
};
use crate::spatial::fov::calculate_visible_tiles;
use crate::map::Map;
use crate::resources::{
    EventLog, EventMessage, GameRng, MapMemory, OccupancyMap, PendingExp, TurnManager,
    VisibleMemory,
};
use crate::schedule::CoreSettleSchedule;
use bevy_ecs::prelude::*;
use std::collections::HashSet;

// ── 行动执行与伤害 ───────────────────────────────────
// 攻击执行系统位于 `action::execution::execute_basic_attack_system`。
// 这里只保留伤害结算与应用系统。

/// 消费 `AttackIntentEvent`，计算最终伤害并写 `AttackEvent`。
pub fn resolve_attack_system(
    mut attack_intents: EventReader<AttackIntentEvent>,
    attacks: Query<&Attack>,
    defenses: Query<&Defense>,
    crits: Query<(&CritRate, &CritDamage)>,
    names: Query<&EntityName>,
    mut rng: ResMut<GameRng>,
    mut attack_events: EventWriter<AttackEvent>,
    mut event_log: ResMut<EventLog>,
) {
    for intent in attack_intents.read() {
        let attack = attacks.get(intent.attacker).map(|a| a.0).unwrap_or(0.0);
        let defense = defenses.get(intent.target).map(|d| d.0).unwrap_or(0.0);
        let (crit_rate, crit_damage) = crits
            .get(intent.attacker)
            .map(|(r, d)| (r.0, d.0))
            .unwrap_or((0.0, 0.0));
        let crit_roll = rng.random_f64();
        let result = compute_melee_damage(attack, defense, crit_rate, crit_damage, crit_roll);

        log::debug!(
            "伤害结算: attacker={:?}, target={:?}, attack={attack:.2}, defense={defense:.2}, damage={:.2}, crit={}",
            intent.attacker,
            intent.target,
            result.damage,
            result.is_crit
        );

        let target_name = names
            .get(intent.target)
            .map(|n| n.0.as_str())
            .unwrap_or("目标");
        event_log.push(EventMessage::combat(format!(
            "对{target_name}造成 {:.0} 点伤害{}",
            result.damage,
            if result.is_crit { "（暴击）" } else { "" }
        )));

        attack_events.write(AttackEvent {
            attacker: intent.attacker,
            target: intent.target,
            damage: result.damage,
            is_crit: result.is_crit,
        });
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
        Option<&EntityName>,
    )>,
    mut death_events: EventWriter<DeathEvent>,
    mut pending_exp: ResMut<PendingExp>,
    mut turn_manager: ResMut<TurnManager>,
    mut event_log: ResMut<EventLog>,
) {
    for (entity, health, player, reward, name) in query.iter() {
        if health.is_alive() {
            continue;
        }

        death_events.write(DeathEvent { entity });

        if player.is_some() {
            log::warn!("玩家死亡");
            turn_manager.game_over = true;
            event_log.push(EventMessage::danger("你死了"));
            continue;
        }

        let name = name.map(|n| n.0.as_str()).unwrap_or("怪物");
        log::info!("实体死亡: {name} ({entity:?})");
        event_log.push(EventMessage::combat(format!("{name} 倒下了")));

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

/// 每轮结算末尾更新所有事件缓冲。
///
/// 必须在所有 `EventReader` 之后运行：它交换双缓冲并清理旧事件，
/// 防止下一轮重新读取历史事件。
pub fn update_events_system(
    mut attack_intents: ResMut<Events<AttackIntentEvent>>,
    mut attack_events: ResMut<Events<AttackEvent>>,
    mut death_events: ResMut<Events<DeathEvent>>,
    mut level_up_events: ResMut<Events<LevelUpEvent>>,
    mut action_succeeded: ResMut<Events<ActionSucceededEvent>>,
    mut action_failed: ResMut<Events<ActionFailedEvent>>,
    mut threat_events: ResMut<Events<ThreatEvent>>,
) {
    attack_intents.update();
    attack_events.update();
    death_events.update();
    level_up_events.update();
    action_succeeded.update();
    action_failed.update();
    threat_events.update();
}

/// 构建标准结算 Schedule（标签为 [`CoreSettleSchedule`]）。
///
/// `insert_core_resources` 会把它注册到 `World`；调用方可通过
/// `world.get_schedule_mut(CoreSettleSchedule)` 在前后插入自己的系统。
pub fn build_core_schedule() -> Schedule {
    let mut schedule = Schedule::new(CoreSettleSchedule);
    schedule.add_systems(
        (
            resolve_attack_system,
            apply_damage_system,
            record_be_attacked_system,
            check_death_system,
            apply_exp_system,
            fov_system,
            update_map_memory_system,
            update_visible_memory_system,
            rebuild_occupancy_system,
            update_events_system,
        )
            .chain(),
    );
    schedule
}

/// 直接运行一次核心结算系统。
///
/// Schedule 已由 `insert_core_resources` 注册；这里只按 label 运行，
/// 不重新构建，因此 `EventReader` 游标等系统状态会跨轮保留。
pub fn run_settle_systems(world: &mut World) {
    world.run_schedule(CoreSettleSchedule);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::{Attack, CritDamage, CritRate, Defense, Health};
    use crate::events::AttackIntentEvent;

    #[test]
    fn settle_does_not_reapply_old_events() {
        let mut world = crate::world_loop::new_game(42);
        let attacker = world
            .spawn((Attack(10.0), CritRate(0.0), CritDamage(0.0)))
            .id();
        let target = world.spawn((Health::new(100.0), Defense(0.0))).id();

        world
            .resource_mut::<Events<AttackIntentEvent>>()
            .send(AttackIntentEvent { attacker, target });

        run_settle_systems(&mut world);
        let hp_after_first = world.get::<Health>(target).unwrap().current;
        run_settle_systems(&mut world);
        let hp_after_second = world.get::<Health>(target).unwrap().current;

        assert!(
            hp_after_first < 100.0,
            "first settle should apply the attack"
        );
        assert_eq!(
            hp_after_first, hp_after_second,
            "old AttackIntentEvent must not be re-read on the next settle"
        );
    }
}
