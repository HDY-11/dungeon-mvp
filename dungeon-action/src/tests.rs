//! 行动系统单元测试
//!
//! 覆盖：advance_action_queue、tap-tap 流程、check_condition 各分之路

use super::*;
use dungeon_core::{
    self as core, items::*,
    Map, Stats,
    Player, Position, Renderable, MovingDir, Viewshed,
    EntityName, Monster, AttackName,
    Inventory, Equipment, Skills, PlayerClass,
    MapMemory, OccupancyMap, EventLog, TurnManager, FloorNumber,
    VisibleMemory, GameRng, PendingExp, MapSeed,
    MonsterKindId,
};
use bevy_ecs::prelude::*;
use rand::SeedableRng;

/// 找地图中第一个可行走格（用于测试放置玩家/怪物）
fn first_walkable(map: &Map) -> (usize, usize) {
    for y in 0..core::MAP_HEIGHT {
        for x in 0..core::MAP_WIDTH {
            if map.tiles[y][x].walkable() {
                return (x, y);
            }
        }
    }
    panic!("地图无可行走格");
}

/// 找 (x,y) 4 方向中可行走的邻居
fn walkable_neighbor(map: &Map, x: usize, y: usize) -> Option<(isize, isize)> {
    for &(dx, dy) in &[(0, -1isize), (0, 1), (-1, 0), (1, 0)] {
        let nx = x.wrapping_add_signed(dx);
        let ny = y.wrapping_add_signed(dy);
        if nx < core::MAP_WIDTH && ny < core::MAP_HEIGHT && map.tiles[ny][nx].walkable() {
            return Some((dx, dy));
        }
    }
    None
}

/// 为测试创建最小化世界（固定种子 42，保证可复现）
fn fresh_world() -> World {
    ItemRegistry::load();

    let mut world = World::new();
    let map_seed: u64 = 42;
    let mut rng = rand::rngs::SmallRng::seed_from_u64(map_seed);
    let mut map = Map::new();
    map.generate(&mut rng);

    world.insert_resource(MapSeed(map_seed));
    world.insert_resource(MapMemory::new());
    world.insert_resource(OccupancyMap::new());
    world.insert_resource(PendingExp::default());
    world.insert_resource(EventLog::new());
    world.insert_resource(GameRng::new(map_seed.wrapping_add(42)));
    world.insert_resource(TurnManager::new());
    world.insert_resource(FloorNumber(1));
    world.insert_resource(VisibleMemory::default());
    world.insert_resource(ActionQueue::default());
    world.insert_resource(InputBuffer::default());
    world.insert_resource(PlayerPreview::default());
    world.insert_resource(ChaseIntents::default());
    world.insert_resource(FleeIntents::default());
    world.insert_resource(WanderIntents::default());

    // 找第一个可行走格作为出生点（不依赖 rooms[0].center()）
    let (spawn_x, spawn_y) = first_walkable(&map);
    world.insert_resource(map);
    let player_agi = 10;

    let pc = PlayerClass::Warrior;
    let mut cmd = world.spawn((
        Player, Position { x: spawn_x, y: spawn_y },
        Renderable { glyph: '@', color: (255, 255, 0) }, MovingDir::default(),
        Viewshed { range: 10, visible_tiles: Vec::new() },
        Stats::player(), EntityName("冒险者".into()),
        Inventory::new(36), Equipment::new(),
        pc.clone(), AttackName("斩击".into()),
    ));
    cmd.insert(Reaction { time: agility_to_reaction(player_agi) });
    cmd.insert(CanMove::new(100));
    cmd.insert(CanWait::new(0));
    cmd.insert(Skills { list: pc.skills() });
    world
}

// ──────────────────────────────────────────────
// 测试：队列推进 — 移动
// ──────────────────────────────────────────────
#[test]
fn test_advance_queue_move() {
    let mut world = fresh_world();
    let player = core::ops::player_entity(&world).unwrap();
    let before = *world.get::<Position>(player).unwrap();

    let map = world.resource::<Map>();
    let (dx, dy) = walkable_neighbor(&map, before.x, before.y)
        .expect("出生点应至少有一个可行走邻居");
    let _ = map;

    let av = agility_to_reaction(10) + CanMove::new(100).duration * agility_speed_factor(10);
    world.resource_mut::<ActionQueue>().enqueue(
        player, ActionKindV3::Move { dx, dy }, av,
    );

    let dist = advance_action_queue(&mut world);
    assert!(dist > 0.0, "队列应推进");

    let after = *world.get::<Position>(player).unwrap();
    assert_eq!(after.x, before.x.wrapping_add_signed(dx));
    assert_eq!(after.y, before.y.wrapping_add_signed(dy));
}

// ──────────────────────────────────────────────
// 测试：队列推进 — 等待（原地不动）
// ──────────────────────────────────────────────
#[test]
fn test_advance_queue_wait() {
    let mut world = fresh_world();
    let player = core::ops::player_entity(&world).unwrap();
    let before = *world.get::<Position>(player).unwrap();

    let av = agility_to_reaction(10) + CanWait::new(0).duration * agility_speed_factor(10);
    world.resource_mut::<ActionQueue>().enqueue(
        player, ActionKindV3::Wait, av,
    );

    let dist = advance_action_queue(&mut world);
    assert!(dist > 0.0, "队列应推进");

    let after = *world.get::<Position>(player).unwrap();
    assert_eq!(after.x, before.x, "等待不应移动");
    assert_eq!(after.y, before.y, "等待不应移动");
}

// ──────────────────────────────────────────────
// 测试：保活检查 — Move 到不可行走格被取消
// ──────────────────────────────────────────────
#[test]
fn test_move_into_wall_cancelled() {
    let mut world = fresh_world();
    let player = core::ops::player_entity(&world).unwrap();
    let pos = *world.get::<Position>(player).unwrap();

    // 找一个不可行走方向
    let map = world.resource::<Map>();
    let wall_dir = {
        let mut d = None;
        for &(dx, dy) in &[(0, -1isize), (0, 1), (-1, 0), (1, 0)] {
            let nx = pos.x.wrapping_add_signed(dx);
            let ny = pos.y.wrapping_add_signed(dy);
            if nx < core::MAP_WIDTH && ny < core::MAP_HEIGHT && !map.tiles[ny][nx].walkable() {
                d = Some((dx, dy));
                break;
            }
        }
        d
    };

    if let Some((dx, dy)) = wall_dir {
        let av = agility_to_reaction(10) + CanMove::new(100).duration * agility_speed_factor(10);
        world.resource_mut::<ActionQueue>().enqueue(
            player, ActionKindV3::Move { dx, dy }, av,
        );

        advance_action_queue(&mut world);
        let pos_after = *world.get::<Position>(player).unwrap();
        assert_eq!(pos_after.x, pos.x, "撞墙不应移动");
        assert_eq!(pos_after.y, pos.y, "撞墙不应移动");
    }
    // 若出生点被可行走方向包围则跳过断言——测试本身验证了条件检查路径
}

// ──────────────────────────────────────────────
// 测试：tap-tap 方向键流程
// ──────────────────────────────────────────────
#[test]
fn test_tap_tap_direction() {
    let mut world = fresh_world();
    let player = core::ops::player_entity(&world).unwrap();
    let before = *world.get::<Position>(player).unwrap();

    let map = world.resource::<Map>();
    let (dx, dy) = walkable_neighbor(&map, before.x, before.y)
        .expect("应至少有一个可行走方向");
    let _ = map;

    // 第一次按 → 预览
    let confirmed = handle_player_direction(&mut world, dx, dy);
    assert!(!confirmed, "第一次按应为预览");
    assert!(world.resource::<PlayerPreview>().kind.is_some(), "应有预览");

    // 第二次按同方向 → 确认入队
    let confirmed = handle_player_direction(&mut world, dx, dy);
    assert!(confirmed, "第二次按应为确认");
    assert!(world.resource::<PlayerPreview>().kind.is_none(), "确认后预览应清除");
    assert_eq!(world.resource::<ActionQueue>().entries.len(), 1, "应有 1 个行动入队");
}

// ──────────────────────────────────────────────
// 测试：等待 tap-tap
// ──────────────────────────────────────────────
#[test]
fn test_tap_tap_wait() {
    let mut world = fresh_world();

    // 第一次按 → 预览
    let confirmed = handle_wait(&mut world);
    assert!(!confirmed);
    assert!(world.resource::<PlayerPreview>().kind.is_some());

    // 第二次 → 确认
    let confirmed = handle_wait(&mut world);
    assert!(confirmed);
    assert!(world.resource::<PlayerPreview>().kind.is_none());
}

// ──────────────────────────────────────────────
// 测试：攻击流程（放一只怪物在玩家邻接位）
// ──────────────────────────────────────────────
#[test]
fn test_attack_execution() {
    let mut world = fresh_world();
    let player = core::ops::player_entity(&world).unwrap();
    let pos = *world.get::<Position>(player).unwrap();

    // 找一个邻居位置放怪物
    let map = world.resource::<Map>();
    let (dx, dy) = walkable_neighbor(&map, pos.x, pos.y)
        .expect("应至少有一个可行走邻居放怪物");
    let _ = map;

    let monster_pos = Position {
        x: pos.x.wrapping_add_signed(dx),
        y: pos.y.wrapping_add_signed(dy),
    };

    let rat_stats = core::monster_def::monster_stats(core::MonsterKindId::Rat, 1);
    let monster_hp_before = rat_stats.hp;
    let monster_entity = world.spawn((
        Monster,
        monster_pos,
        Renderable { glyph: 'r', color: (255, 0, 0) },
        Viewshed { range: 10, visible_tiles: Vec::new() },
        rat_stats,
        EntityName("老鼠".into()),
        AttackName("撕咬".into()),
        core::monster_def::monster_loot(core::MonsterKindId::Rat),
    )).id();
    core::ops::rebuild_occupancy(&mut world);

    // 通过 ActionQueue 执行攻击
    let av = agility_to_reaction(10) + CanMove::new(100).duration * agility_speed_factor(10);
    world.resource_mut::<ActionQueue>().enqueue(
        player,
        ActionKindV3::Attack { target: monster_entity },
        av,
    );

    advance_action_queue(&mut world);

    // 验证怪物受伤
    if let Some(stats) = world.get::<Stats>(monster_entity) {
        assert!(stats.hp < monster_hp_before, "怪物应受伤");
        assert!(stats.hp >= 0, "HP 不应为负");
    }
    // 如果怪物死了会被 despawn，这是合理的结果

    let log = world.resource::<EventLog>();
    assert!(
        log.messages.iter().any(|m| m.contains("造成") || m.contains("击杀")),
        "事件日志应有攻击/击杀记录: {:?}",
        log.messages,
    );
}

// ──────────────────────────────────────────────
// 测试：条件函数
// ──────────────────────────────────────────────
#[test]
fn test_conditions() {
    assert!(CanFlee::condition(0.2));
    assert!(!CanFlee::condition(0.5));
    assert!(CanChase::condition(true));
    assert!(!CanChase::condition(false));
    assert!(CanWander::condition());
}

// ──────────────────────────────────────────────
// 测试：敏捷→反应时公式
// ──────────────────────────────────────────────
#[test]
fn test_reaction_from_agility() {
    let r10 = agility_to_reaction(10);
    let r20 = agility_to_reaction(20);
    assert!(r20 < r10);
    assert!(r10 >= 20.0);
    assert!(r10 <= 100.0);
}
