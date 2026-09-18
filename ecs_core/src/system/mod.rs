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

// ── 查询别名 ─────────────────────────────────────────
//
// 结算链路里有三个「宽查询」：死亡候选、受击记录目标、升级玩家。字段多但语义
// 明确（都是按 entity 读一批组件），写成别名比每次内联更易核对字段有没有漏。

/// 受击记录目标：实体 + 可选的既有记录。
type RecordTarget = (Entity, Option<&'static BeAttacked>);

/// 死亡候选：实体 + 生命 + （玩家？经验奖励？名字？）。
type DeathCandidate = (
    Entity,
    &'static Health,
    Option<&'static Player>,
    Option<&'static ExperienceReward>,
    Option<&'static EntityName>,
);

/// 升级玩家：经验/等级/生命/法力 + 重算上限所需的防御与法术精通。
type ExpPlayer = (
    Entity,
    &'static mut Experience,
    &'static mut Level,
    &'static mut Health,
    &'static mut Magic,
    &'static Defense,
    &'static MagicMastery,
);
// ── 行动执行与伤害 ───────────────────────────────────
// 攻击执行系统位于 `action::execution::execute_basic_attack_system`。
// 这里只保留伤害结算与应用系统。

/// 消费 `AttackIntentEvent`，计算最终伤害并写 `AttackEvent`。
///
/// 参数偏多（8 个）是 ECS 系统的固有形状——`EventReader` / `Query` / `ResMut`
/// 每类都是一个独立入参，折成 DTO 会丢掉 Bevy 的参数校验（「可变访问是否冲突」
/// 这类错误会从编译期推迟到运行期）。这里保留显式签名。
///
/// 顺带记录一条实测结论：把这四个查询抽成 `type CombatQueries = (Query<..>, ..)`
/// **不行**——`Query<'w, 's, D, F>` 的 `'w`/`'s` 必须由系统参数推导，而 `type`
/// 别名里没有 `'w`/`'s` 可写（`Query<&'static Attack>` 会把 `'w`/`'s` 固定死，
/// 报 `not a valid SystemParam`）。要抽就只能抽成 `#[derive(SystemParam)]` 结构体。
#[allow(clippy::too_many_arguments)]
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
    targets: Query<RecordTarget, (With<Health>, With<NeedRecordBeAttacked>)>,
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
    query: Query<DeathCandidate>,
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
    mut players: Query<ExpPlayer, With<Player>>,
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
    use crate::components::{Defense, Health};
    use crate::events::AttackIntentEvent;
    use crate::map::Tile;
    use crate::monster::MonsterKindId;
    use crate::test_util::{
        fill_map, kill_entity, player_entity, player_health, player_pos, single_tile_scene,
        spawn_test_actor, spawn_test_monster, spawn_test_player, test_world,
    };
    use crate::world::init::StairsPos;

    /// 测试用攻击者：高攻、无暴击（伤害确定 = max(attack - defense, 1)）。
    fn spawn_test_attacker(world: &mut World, attack: f64) -> Entity {
        let entity = spawn_test_actor(world, (0, 0), 100.0, attack, 1.0, 1.0);
        world.entity_mut(entity).insert(Idle);
        entity
    }

    /// 清掉所有带 `T` 的实体。
    ///
    /// 用例要验的不是世界演化时（占位、FOV、移动落点），先把会自己走动的实体
    /// 移走，避免「怪物恰好走到目标格」这类与断言无关的偶发失败。
    fn despawn_all<T: Component>(world: &mut World) {
        let entities: Vec<Entity> = {
            let mut query = world.query_filtered::<Entity, With<T>>();
            query.iter(world).collect()
        };
        for entity in entities {
            world.despawn(entity);
        }
    }

    #[test]
    fn settle_does_not_reapply_old_events() {
        let mut world = crate::world_loop::new_game(42);
        let attacker = spawn_test_attacker(&mut world, 10.0);
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

    /// A3：攻击伤害精确只结算一次，且第二次结算不会重复扣血。
    #[test]
    fn attack_applies_damage_once() {
        let mut world = crate::world_loop::new_game(42);
        let attacker = spawn_test_attacker(&mut world, 10.0);
        let target = world.spawn((Health::new(100.0), Defense(0.0))).id();

        world
            .resource_mut::<Events<AttackIntentEvent>>()
            .send(AttackIntentEvent { attacker, target });
        run_settle_systems(&mut world);

        let hp_after_first = world.get::<Health>(target).expect("目标必须存活").current;
        assert_eq!(
            hp_after_first, 90.0,
            "一记 10 攻对 0 防应当只造成 10 点伤害"
        );

        // 第二轮：没有新事件，血量必须完全不变（I90 回归）。
        run_settle_systems(&mut world);
        assert_eq!(
            world.get::<Health>(target).unwrap().current,
            hp_after_first,
            "旧事件不得在下一轮重复结算"
        );

        // 再发一次同样的意图 → 恰好再扣一次。
        world
            .resource_mut::<Events<AttackIntentEvent>>()
            .send(AttackIntentEvent { attacker, target });
        run_settle_systems(&mut world);
        assert_eq!(
            world.get::<Health>(target).unwrap().current,
            hp_after_first - 10.0,
            "每次意图恰好结算一次"
        );

        // 日志也应当是两条，不是四条。
        let damage_lines = world
            .resource::<EventLog>()
            .messages
            .iter()
            .filter(|m| m.text.contains("点伤害"))
            .count();
        assert_eq!(damage_lines, 2, "两次攻击应当只产生两条伤害日志");
    }

    /// A4：怪物死亡 → despawn → 经验进入玩家 → 跨阈值升级并重算 HP/MP 上限。
    #[test]
    fn monster_death_rewards_exp_and_levels_up() {
        let mut world = test_world();
        fill_map(&mut world, Tile::Wall);

        let player = spawn_test_player(&mut world, (0, 0));
        let monster = spawn_test_monster(&mut world, MonsterKindId::Rat, (1, 0), 10.0, 4.0);
        world.entity_mut(monster).insert(ExperienceReward(50.0));

        // 升到 2 级需要 100 点：先给 99 点，再用一次击杀跨过阈值。
        {
            let mut exp = world.get_mut::<Experience>(player).unwrap();
            exp.exp = 99.0;
            exp.exp_to_next = 100.0;
        }
        run_settle_systems(&mut world);

        let monster_name = world.get::<EntityName>(monster).unwrap().0.clone();
        assert!(kill_entity(&mut world, player, monster), "怪物应当被打死");

        assert!(world.get_entity(monster).is_err(), "死亡怪物必须被 despawn");
        assert_eq!(
            world.resource::<PendingExp>().amount,
            0.0,
            "经验必须已被 apply_exp_system 消费"
        );

        let level = world.get::<Level>(player).unwrap().0;
        assert_eq!(level, 2, "99 + 50 经验应当让玩家升到 2 级");
        let exp = world.get::<Experience>(player).unwrap();
        assert!(
            (exp.exp - (99.0 + 50.0 - 100.0)).abs() < 1e-9,
            "升级后经验应为溢出值: 期望 49，实际 {}",
            exp.exp
        );
        assert_eq!(exp.exp_to_next, exp_to_next_level(2));

        let health = world.get::<Health>(player).unwrap();
        assert_eq!(health.max, max_hp_for(2, 4.0), "升级必须重算最大生命");
        assert_eq!(health.current, health.max, "升级必须回满生命");
        let magic = world.get::<Magic>(player).unwrap();
        assert_eq!(magic.max, max_mp_for(2, 8.0), "升级必须重算最大法力");
        assert_eq!(magic.current, magic.max, "升级必须回满法力");

        let log = world.resource::<EventLog>();
        assert!(
            log.messages
                .iter()
                .any(|m| m.text.contains(&monster_name) && m.text.contains("倒下了")),
            "死亡应当写入战斗日志: {:?}",
            log.messages.iter().map(|m| &m.text).collect::<Vec<_>>()
        );
        assert!(
            log.messages.iter().any(|m| m.text.contains("升级")),
            "升级应当写入系统日志"
        );
    }

    /// A4 补充：不够升级的经验只会累加，不会改等级/HP 上限。
    #[test]
    fn experience_below_threshold_does_not_level_up() {
        let mut world = test_world();
        fill_map(&mut world, Tile::Wall);
        let player = spawn_test_player(&mut world, (0, 0));
        run_settle_systems(&mut world);

        let max_hp_before = world.get::<Health>(player).unwrap().max;
        world.resource_mut::<PendingExp>().amount = 30.0;
        run_settle_systems(&mut world);

        assert_eq!(world.get::<Level>(player).unwrap().0, 1);
        assert_eq!(world.get::<Experience>(player).unwrap().exp, 30.0);
        assert_eq!(world.get::<Health>(player).unwrap().max, max_hp_before);
    }

    /// A5：FOV → 记忆 → 占用图在初始化后已经建立，移动后同步更新。
    #[test]
    fn fov_memory_and_occupancy_update() {
        let mut world = crate::world_loop::new_game(2026);
        let player = player_entity(&world).expect("新游戏必须有玩家");
        let start = player_pos(&world);

        // FOV：视野非空且包含自己所在格。
        let visible = world.get::<Viewshed>(player).unwrap().visible_tiles.clone();
        assert!(!visible.is_empty(), "玩家视野不得为空");
        assert!(visible.contains(&start), "视野必须包含玩家自身所在格");

        // 记忆：玩家所在格必须已探索。
        let explored_before = world
            .resource::<MapMemory>()
            .explored
            .iter()
            .flatten()
            .filter(|seen| **seen)
            .count();
        assert!(
            world.resource::<MapMemory>().explored[start.1][start.0],
            "玩家所在格必须已探索"
        );
        assert!(explored_before > 0, "已探索格数必须大于 0");

        // 占用图：玩家位置被自己占用。
        let occupancy = world.resource::<OccupancyMap>();
        assert_eq!(
            occupancy.entity_at(start.0, start.1),
            Some(player),
            "占用图必须记录玩家"
        );

        // 移动到相邻可走格：占用图旧格清空、新格写入。
        //
        // **先清场**：`apply_player_command` 会推进世界直到玩家行动做完，期间怪物
        // 可能游荡到玩家选定的目标格上，于是命令被改判成「走向怪物＝攻击」，
        // 位置断言就会随机失败。本用例要验的是 FOV/记忆/占用图，不是怪物交互，
        // 所以把怪物与楼梯移走，让目标格在整段推进期间保持空闲。
        //
        // 这个依赖在 Phase D 才暴露：速度组件改变了各行动的执行轮次，
        // 怪物消耗随机数的时机随之改变，原本"碰巧没人走过来"的假设随即失效。
        despawn_all::<crate::entity_cls::Monster>(&mut world);
        despawn_all::<crate::entity_cls::Stairs>(&mut world);
        run_settle_systems(&mut world);

        let mut moved = None;
        for (dx, dy) in [(0isize, 1isize), (1, 0), (0, -1), (-1, 0)] {
            let (nx, ny) = Position::new(start.0, start.1).offset(dx, dy);
            if nx >= crate::map::MAP_WIDTH || ny >= crate::map::MAP_HEIGHT {
                continue;
            }
            if !world.resource::<Map>().tiles[ny][nx].walkable() {
                continue;
            }
            if crate::world_loop::apply_player_command(
                &mut world,
                crate::action::generation::player::PlayerCommand::Move { dx, dy },
            ) {
                moved = Some((nx, ny));
                break;
            }
        }
        let Some(dest) = moved else {
            panic!("seed=2026 出生点 {start:?} 周围必须有可走格");
        };

        assert_eq!(player_pos(&world), dest, "玩家应当移动到 {dest:?}");
        let occupancy = world.resource::<OccupancyMap>();
        assert_eq!(
            occupancy.entity_at(dest.0, dest.1),
            Some(player),
            "移动后占用图必须在新格记录玩家"
        );
        assert_eq!(
            occupancy.entity_at(start.0, start.1),
            None,
            "移动后占用图必须清空旧格"
        );

        let visible = world.get::<Viewshed>(player).unwrap().visible_tiles.clone();
        assert!(!visible.is_empty(), "移动后视野必须重算且非空");
        assert!(
            world.resource::<MapMemory>().explored[dest.1][dest.0],
            "移动后新位置必须已探索"
        );
        assert!(
            world.resource::<VisibleMemory>().entries.len()
                <= crate::map::MAP_WIDTH * crate::map::MAP_HEIGHT,
            "可见记忆条数不得异常膨胀"
        );
    }

    /// A5 补充：占用图不记录楼梯，但记录玩家与怪物；一次性把三种实体都验到。
    #[test]
    fn occupancy_tracks_actors_but_not_stairs() {
        let (mut world, player) = single_tile_scene();
        let monster = {
            let mut query = world
                .try_query::<(Entity, &crate::entity_cls::Monster)>()
                .unwrap();
            query.iter(&world).next().unwrap().0
        };
        let monster_pos = *world.get::<Position>(monster).unwrap();

        world.resource_mut::<StairsPos>().0 = (0, 0);
        world.spawn((
            crate::entity_cls::Stairs,
            EntityClass::Field,
            Position::new(2, 2),
            EntityName("楼梯".into()),
        ));
        run_settle_systems(&mut world);

        let occupancy = world.resource::<OccupancyMap>();
        assert_eq!(
            occupancy.entity_at(0, 0),
            Some(player),
            "玩家必须在占用图里"
        );
        assert_eq!(
            occupancy.entity_at(monster_pos.x, monster_pos.y),
            Some(monster),
            "怪物必须在占用图里"
        );
        assert_eq!(
            occupancy.entity_at(2, 2),
            None,
            "楼梯不得进入占用图（否则挡路）"
        );
        assert_eq!(player_health(&world).current, 100.0);
    }
}
