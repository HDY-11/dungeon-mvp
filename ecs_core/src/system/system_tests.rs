//! `super`（`system`）的结算链路测试。
//!
//! 通过 `mod.rs` 末尾的 `#[cfg(test)] #[path = "system_tests.rs"] mod tests;` 引入，
//! 与 `ecs_core/src/action/entity_tests.rs` 同一形式（生产与测试分开）。
//!
//! 覆盖：A3 伤害只结算一次、A4 死亡→经验→升级、A5 视野/记忆/占用图、
//! 以及 I90 的事件生命周期（旧事件不得在下一轮被重读）。

use crate::system::run_settle_systems;
use crate::balance::exp_to_next_level;
use crate::rules::{hp_curve, mp_curve};
use crate::components::{
    EntityName, Experience, ExperienceReward, Health, Idle, Level, Mana, Position, Stats, Viewshed,
};
use crate::events::{AttackIntentEvent, DeathEvent};
use crate::map::Tile;
use crate::monster::MonsterKindId;
use crate::resources::{EventLog, MapMemory, OccupancyMap, VisibleMemory};
use crate::test_util::{
    fill_map, kill_entity, player_entity, player_health, player_pos, single_tile_scene,
    spawn_test_actor, spawn_test_monster, spawn_test_player, test_world,
};
use crate::world::init::StairsPos;
use bevy_ecs::prelude::*;

/// 测试用攻击者：高攻、无暴击（伤害确定 = max(attack - defense, 1)）。
fn spawn_test_attacker(world: &mut World, attack: f64) -> Entity {
    let entity = spawn_test_actor(world, (0, 0), 100.0, attack, 1.0, 1.0);
    world.entity_mut(entity).insert(Idle);
    entity
}

/// 清掉所有带 `T` 的实体。
///
/// 用例要验的不是世界演化时（占位、FOV、移动落点），先把会自己走动的实体
/// 移走，避免「怪物恰好走到目标格」这类与断言无关的偶发失败（LESSONS.md LSYN20）。
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
    let target = world.spawn((Health::new(100.0), Stats::new())).id();

    world
        .resource_mut::<Messages<AttackIntentEvent>>()
        .write(AttackIntentEvent { attacker, target });

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
    let target = world.spawn((Health::new(100.0), Stats::new())).id();

    world
        .resource_mut::<Messages<AttackIntentEvent>>()
        .write(AttackIntentEvent { attacker, target });
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
        .resource_mut::<Messages<AttackIntentEvent>>()
        .write(AttackIntentEvent { attacker, target });
    run_settle_systems(&mut world);
    assert_eq!(
        world.get::<Health>(target).unwrap().current,
        hp_after_first - 10.0,
        "每次意图恰好结算一次"
    );

    // 日志也应当是两条，不是四条。
    // 注意：`EventLog::push` 会把**连续相同**的消息合并成一条并计数（ISSUES SYN8），
    // 所以这里统计的是"伤害事件次数"（repeat 求和），不是行数——行数会因为合并而少。
    let damage_events: u32 = world
        .resource::<EventLog>()
        .messages
        .iter()
        .filter(|m| m.text.contains("点伤害"))
        .map(|m| m.repeat)
        .sum();
    assert_eq!(damage_events, 2, "两次攻击应当只结算两次伤害（不多不少）");
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
    // DsnE25：升级不再回满——记下升级前的当前值，升级后应当保持原值、只重算上限。
    let health_current_before = world.get::<Health>(player).unwrap().current;
    let mana_current_before = world.get::<Mana>(player).unwrap().current;
    assert!(kill_entity(&mut world, player, monster), "怪物应当被打死");

    assert!(world.get_entity(monster).is_err(), "死亡怪物必须被 despawn");
    // 经验链路的唯一入口是 `DeathEvent`（奖励随事件携带，无旁路资源；旧
    // `PendingExp` 已在 Phase E 删除）。这里不做「缓冲为空」的断言——那测的是
    // `Events::update()` 的轮转时机，不是本用例的业务契约。真正要钉住的是
    // **奖励只结算一次**：再跑一轮结算，等级与经验都不得再变（I90 契约）。
    world.resource_mut::<Messages<DeathEvent>>().update();
    let level_before = world.get::<Level>(player).unwrap().0;
    let exp_before = world.get::<Experience>(player).unwrap().exp;
    run_settle_systems(&mut world);
    assert_eq!(
        world.get::<Level>(player).unwrap().0,
        level_before,
        "重复结算不得再加经验（DeathEvent 只能被消费一次）"
    );
    assert_eq!(
        world.get::<Experience>(player).unwrap().exp,
        exp_before,
        "重复结算不得再加经验"
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
    assert_eq!(health.max, hp_curve(2), "升级必须按等级曲线重算最大生命");
    assert_eq!(
        health.current, health_current_before,
        "升级不回满：当前生命保持原值（DsnE25）"
    );
    let mana = world.get::<Mana>(player).unwrap();
    assert_eq!(mana.max, mp_curve(2), "升级必须按等级曲线重算最大法力");
    assert_eq!(
        mana.current, mana_current_before,
        "升级不回满：当前法力保持原值（DsnE25）"
    );

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

/// P1（AUDIT-data）：死亡与升级同轮结算时，尸体不得"升级回满血"。
///
/// 结算链顺序是 `death → exp`（`system/mod.rs`）：玩家被怪物打死与击杀怪物的
/// 经验奖励发生在同一轮。经验系统必须跳过死人，否则升级重算 HP 上限会把
/// `health.current` 写成满血，游戏结束画面出现"HP 38/38"的自相矛盾状态。
#[test]
fn a_dead_player_does_not_level_up_or_heal() {
    let mut world = test_world();
    fill_map(&mut world, Tile::Wall);
    let player = spawn_test_player(&mut world, (0, 0));

    // 临门一脚的经验 + 0 血：若没有存活过滤，这一轮会升到 2 级并回满。
    {
        let mut exp = world.get_mut::<Experience>(player).unwrap();
        exp.exp = 99.0;
        exp.exp_to_next = 100.0;
    }
    world.get_mut::<Health>(player).unwrap().current = 0.0;

    world
        .resource_mut::<Messages<DeathEvent>>()
        .write(DeathEvent {
            entity: Entity::PLACEHOLDER,
            reward: 50.0,
        });
    run_settle_systems(&mut world);

    let health = world.get::<Health>(player).unwrap();
    assert!(!health.is_alive(), "玩家应当已经死亡");
    assert_eq!(health.current, 0.0, "死人不该借升级回血");
    assert_eq!(world.get::<Level>(player).unwrap().0, 1, "死人不该升级");
    assert_eq!(
        world.get::<Experience>(player).unwrap().exp,
        99.0,
        "死人不该再吃经验"
    );
    assert!(
        world.resource::<crate::resources::TurnManager>().game_over,
        "玩家死亡必须置游戏结束"
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
    // 直接投喂一个没有实体的死亡事件：奖励 30 点（不足以升级）。
    world
        .resource_mut::<Messages<DeathEvent>>()
        .write(DeathEvent {
            entity: Entity::PLACEHOLDER,
            reward: 30.0,
        });
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
        if !world.resource::<crate::map::Map>().tiles[ny][nx].walkable() {
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
    // DsnE23：结算链里的上限写者会把测试场里手工塞的 100 血重算成 Lv.1 曲线，
    // 并把 current 夹到新上限（不回血）。
    let health = player_health(&world);
    assert_eq!(health.max, hp_curve(1), "上限必须按等级曲线重算");
    assert_eq!(health.current, hp_curve(1), "current 只能被夹取，不回血");
}

/// DsnE24 阶段 ②：造成的伤害会喂给主手武器的养成进度，升级时记一条事件。
#[test]
fn damage_feeds_the_main_hand_weapons_growth() {
    use crate::item::{BODY, Container, ItemTag, spawn_item_assembled};

    let mut world = crate::world_loop::new_game(42);
    let player = player_entity(&world).expect("新游戏必须有玩家");

    // 装备一把锈铁剑，并把它的经验预置到"再挨一下就该升级"的位置。
    let sword = spawn_item_assembled(&mut world, crate::item::ItemId(0), 1);
    let weapon_slot = BODY
        .slots
        .iter()
        .position(|slot| slot.accept.accepts(&[ItemTag::Weapon]))
        .expect("身体必须有一个武器槽");
    world
        .get_mut::<Container>(player)
        .expect("玩家有身体容器")
        .insert(weapon_slot, sword, crate::item::ItemId(0).def().tags)
        .expect("武器槽接受剑");
    world
        .get_mut::<crate::item::WeaponProgress>(sword)
        .expect("武器装配带养成进度")
        .exp = 25.0;

    let target = world
        .spawn((Health::new(100.0), Stats::new(), EntityName("木桩".into())))
        .id();
    world
        .resource_mut::<Messages<AttackIntentEvent>>()
        .write(AttackIntentEvent {
            attacker: player,
            target,
        });
    run_settle_systems(&mut world);

    let progress = world
        .get::<crate::item::WeaponProgress>(sword)
        .expect("剑还在主手上");
    assert_eq!(progress.level, 2, "一次伤害应该跨过 30 点门槛升到 2 级");
    assert!(progress.exp < 30.0, "升级后剩余经验应小于下一级门槛");
    assert!(
        world
            .resource::<EventLog>()
            .messages
            .iter()
            .any(|message| message.text.contains("升到了 2 级")),
        "升级应写进事件栏（玩家要能看见）"
    );
}
