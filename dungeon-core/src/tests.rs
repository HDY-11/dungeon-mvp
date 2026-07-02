use crate::*;
use bevy_ecs::prelude::*;
use bevy_ecs::system::RunSystemOnce;
use rand::rngs::SmallRng;
use rand::SeedableRng;

fn seeded_map(seed: u64) -> Map {
    let mut rng = SmallRng::seed_from_u64(seed);
    let mut map = Map::new();
    map.generate(&mut rng);
    map
}

fn test_world() -> World {
    let mut world = World::new();
    let mut rng = SmallRng::seed_from_u64(42);
    let mut map = Map::new();
    map.generate(&mut rng);
    let (spawn_x, spawn_y) = map.rooms[0].center();
    world.insert_resource(MapMemory::new());
    world.insert_resource(GameRng { rng: SmallRng::seed_from_u64(0) });
    world.insert_resource(OccupancyMap::new());
    world.insert_resource(PendingExp::default());
    world.insert_resource(PendingPickup::default());
    world.insert_resource(PendingSkill::default());
    world.insert_resource(EventLog::new());
    world.insert_resource(TurnManager::new());
    world.insert_resource(FloorNumber(1));
    world.insert_resource(PendingLevelUp::default());
    world.insert_resource(PendingPlayerAction::default());
    world.insert_resource(GamePacing::default());
    world.insert_resource(map);
    let e = world.spawn((
        Player, Position { x: spawn_x, y: spawn_y },
        Renderable { glyph: '@', color: (255, 255, 0) },
        MovingDir::default(), Viewshed { range: 8, visible_tiles: Vec::new() },
        Stats::player(), EntityName("冒险者".into()), ActionValue::new(10),
        ActionPrediction::new("移动", ActionKind::Move),
        Inventory::new(36), Equipment::new(), Buffs::new(), ActionPreview::new(),
        PlayerClass::Warrior, AttackName("斩击".into()),
    )).id();
    world.entity_mut(e).insert(Skills { list: PlayerClass::Warrior.skills() });
    world
}

// ── 地图 ──────────────────────────────────────────

#[test] fn test_generation_produces_rooms() { assert!(seeded_map(42).rooms.len() >= 3); }
#[test] fn test_no_overlapping_rooms() {
    let map = seeded_map(123);
    for i in 0..map.rooms.len() {
        for j in i + 1..map.rooms.len() {
            let a = &map.rooms[i]; let b = &map.rooms[j];
            assert!(!(a.x < b.x + b.w + 1 && a.x + a.w + 1 > b.x && a.y < b.y + b.h + 1 && a.y + a.h + 1 > b.y));
        }
    }
}
#[test] fn test_floor_tiles_exist() {
    let count = seeded_map(42).tiles.iter().flatten().filter(|t| **t == Tile::Floor).count();
    assert!(count > 0);
}
#[test] fn test_render_length() { let lines = seeded_map(42).render(); assert_eq!(lines.len(), MAP_HEIGHT); assert_eq!(lines[0].len(), MAP_WIDTH); }

// ── ECS ──────────────────────────────────────────

#[test] fn test_setup_world_has_player() {
    let mut world = test_world();
    assert_eq!(collect_renderables(&mut world).len(), 1);
}
#[test] fn test_player_spawned_on_floor() {
    let mut world = test_world();
    let pos = { let mut q = world.query::<&Position>(); *q.single(&world).unwrap() };
    assert_eq!(world.resource::<Map>().tiles[pos.y][pos.x], Tile::Floor);
}
#[test] fn test_movement_to_floor_succeeds() {
    let mut world = test_world();
    let sx = { let mut q = world.query::<&Position>(); q.single(&world).unwrap().x };
    set_player_dir(&mut world, 1, 0);
    rebuild_occupancy(&mut world);
    let _ = world.run_system_once(movement_system);
    let ex = { let mut q = world.query::<&Position>(); q.single(&world).unwrap().x };
    assert!(ex >= sx);
}
#[test] fn test_moving_dir_reset() {
    let mut world = test_world();
    set_player_dir(&mut world, 1, 0);
    rebuild_occupancy(&mut world);
    let _ = world.run_system_once(movement_system);
    assert_eq!(world.query::<&MovingDir>().single(&world).unwrap().dx, 0);
}
#[test] fn test_fov() {
    let mut world = test_world();
    let _ = world.run_system_once(fov_system);
    assert!(world.query::<(&Player, &Viewshed)>().single(&world).unwrap().1.visible_tiles.len() > 0);
}
#[test] fn test_setup_world_has_monsters() {
    let mut world = setup_world();
    assert!(world.query::<&Monster>().iter(&world).count() > 0);
}
#[test] fn test_floor_number() {
    let mut world = setup_world();
    assert_eq!(world.resource::<FloorNumber>().0, 1);
}
#[test] fn test_skills_exist() {
    let mut world = setup_world();
    assert_eq!(world.query::<&Skills>().iter(&world).next().unwrap().list.len(), 2); // 战士2个技能
}

// ── 曲线 ──────────────────────────────────────────

#[test] fn test_exp_curve() { assert_eq!(exp_to_next_level(1), 50); assert_eq!(exp_to_next_level(2), 140); }
#[test] fn test_hp_curve() { assert_eq!(max_hp_for(1, 10) - max_hp_for(2, 10), max_hp_for(2, 10) - max_hp_for(3, 10)); }
#[test] fn test_def_log() { assert_eq!(defense_bonus(1), 0); assert_eq!(defense_bonus(2), 1); assert_eq!(defense_bonus(4), 2); }
#[test] fn test_rat_goblin() {
    let rat = Stats::monster('r', 1); let gob = Stats::monster('g', 1);
    assert!(gob.max_hp > rat.max_hp); assert!(gob.exp > rat.exp);
}

// ── AV 引擎 ─────────────────────────────────────

fn av_test_world(player_av: f32, monster_av: f32) -> World {
    let mut world = World::new();
    world.insert_resource(EventLog::new());
    world.insert_resource(TurnManager::new());
    world.insert_resource(PendingExp::default());
    world.insert_resource(PendingPickup::default());
    world.insert_resource(PendingSkill::default());
    world.insert_resource(PendingLevelUp::default());
    world.insert_resource(PendingPlayerAction::default());
    let mut map = Map::new();
    let mut rng = SmallRng::seed_from_u64(42);
    map.generate(&mut rng);
    world.insert_resource(map);
    world.insert_resource(OccupancyMap::new());
    world.insert_resource(GameRng { rng: SmallRng::seed_from_u64(0) });
    world.insert_resource(GamePacing::default());
    world.insert_resource(FloorNumber(1));
    world.insert_resource(PendingInput::default());
    let mut p_av = ActionValue::new(10);
    p_av.current_av = player_av;
    world.spawn((
        Player, Position { x: 1, y: 1 },
        MovingDir::default(),
        Renderable { glyph: '@', color: (255, 255, 0) },
        Viewshed { range: 8, visible_tiles: Vec::new() },
        Stats::player(), EntityName("玩家".into()),
        p_av,
        ActionPrediction::new("移动", ActionKind::Move),
        Inventory::new(10), Equipment::new(), Buffs::new(),
        PlayerClass::Warrior, AttackName("斩击".into()),
    ));
    let mon_stats = Stats::monster('r', 1);
    let mut m_av = ActionValue::new(mon_stats.agility);
    m_av.current_av = monster_av;
    world.spawn((
        Monster, Position { x: 5, y: 5 },
        Renderable { glyph: 'r', color: (255, 0, 0) },
        Viewshed { range: 8, visible_tiles: Vec::new() },
        mon_stats, EntityName("老鼠".into()),
        m_av,
        ActionPrediction::new("追击", ActionKind::Chase),
        MonsterBrain::creature(),
        FleeLogState::default(), ActionPreview::new(),
        AttackName("撕咬".into()),
    ));
    world
}

#[test]
fn test_advance_by_reduces_all_av() {
    let mut world = av_test_world(300.0, 200.0);
    let r = advance_by(&mut world, 50.0);
    let player_av = world.query::<(&Player, &ActionValue)>().single(&world).unwrap().1.current_av;
    let monster_av = world.query::<(&Monster, &ActionValue)>().single(&world).unwrap().1.current_av;
    assert_eq!(player_av as u32, 250, "玩家 AV=300-50");
    assert_eq!(monster_av as u32, 150, "怪物 AV=200-50");
    assert!(!r.any_executed, "未到0不应执行");
    assert!(!r.any_locked, "AV>100不应锁定");
}

#[test]
fn test_advance_by_executes_at_zero() {
    let mut world = av_test_world(100.0, 200.0);
    let r = advance_by(&mut world, 100.0);
    let player_av = world.query::<(&Player, &ActionValue)>().single(&world).unwrap().1.current_av;
    // 玩家 AV=100-100=0 → 执行 → 重置到 ~125
    assert!(player_av > 100.0, "执行后玩家 AV 应重置 >100");
    assert!(r.any_executed, "玩家应被执行");
}

#[test]
fn test_advance_by_locks_at_threshold() {
    let mut world = av_test_world(150.0, 200.0);
    let r = advance_by(&mut world, 50.0);
    // 玩家 AV=150-50=100 → 进入锁定区
    let pred = world.query::<(&Player, &ActionPrediction)>().single(&world).unwrap().1;
    assert!(pred.locked, "玩家预测应锁定");
    assert!(r.player_locked, "返回值应标记玩家锁定");
    assert!(r.any_locked, "返回值应标记有锁定");
}

#[test]
fn test_advance_by_simultaneous_lock_and_execute() {
    let mut world = av_test_world(50.0, 150.0);
    let r = advance_by(&mut world, 50.0);
    // 玩家: AV=50-50=0 → 执行后重置
    let player_av = world.query::<(&Player, &ActionValue)>().single(&world).unwrap().1.current_av;
    assert!(player_av > 100.0, "玩家执行后重置");
    // 怪物: AV=150-50=100 → 锁定
    let monster_pred = world.query::<(&Monster, &ActionPrediction)>().single(&world).unwrap().1;
    assert!(monster_pred.locked, "怪物预测应锁定");
    assert!(r.any_executed, "应有执行");
    assert!(r.any_locked, "应有锁定");
}

#[test]
fn test_advance_by_nothing_happens() {
    let mut world = av_test_world(500.0, 400.0);
    let r = advance_by(&mut world, 10.0);
    // 玩家 AV=500-10=490, 怪物=400-10=390, 均未到阈值
    assert!(!r.any_executed);
    assert!(!r.any_locked);
    assert!(!r.player_locked);
}

// ═════════════════════════════════════════════════════
// 游戏逻辑集成测试
// ═════════════════════════════════════════════════════

/// 创建带完整组件的测试世界（玩家 + 1 只怪物）
fn full_test_world() -> World {
    let mut world = World::new();
    let mut rng = SmallRng::seed_from_u64(42);
    let mut map = Map::new();
    map.generate(&mut rng);
    let (sx, sy) = map.rooms[0].center();
    let (mx, my) = if map.rooms.len() > 1 { map.rooms[1].center() } else { (sx + 2, sy) };

    world.insert_resource(MapMemory::new());
    world.insert_resource(GameRng { rng: SmallRng::seed_from_u64(0) });
    world.insert_resource(OccupancyMap::new());
    world.insert_resource(PendingExp::default());
    world.insert_resource(PendingPickup::default());
    world.insert_resource(PendingSkill::default());
    world.insert_resource(EventLog::new());
    world.insert_resource(TurnManager::new());
    world.insert_resource(FloorNumber(1));
    world.insert_resource(PendingLevelUp::default());
    world.insert_resource(PendingPlayerAction::default());
    world.insert_resource(PendingInput::default());
    world.insert_resource(GamePacing::default());
    world.insert_resource(map);

    let player_e = world.spawn((
        Player, Position { x: sx, y: sy },
        Renderable { glyph: '@', color: (255, 255, 0) },
        MovingDir::default(), Viewshed { range: 8, visible_tiles: Vec::new() },
        Stats::player(), EntityName("冒险者".into()), ActionValue::new(10),
        ActionPrediction::new("移动", ActionKind::Move),
        Inventory::new(36), Equipment::new(), Buffs::new(), ActionPreview::new(),
        PlayerClass::Warrior, AttackName("斩击".into()),
    )).id();
    world.entity_mut(player_e).insert(Skills { list: PlayerClass::Warrior.skills() });

    world.spawn((
        Monster, MonsterBrain::creature(),
        Position { x: mx, y: my }, Renderable { glyph: 'r', color: (255, 0, 0) },
        Viewshed { range: 8, visible_tiles: Vec::new() },
        Stats::monster('r', 1), EntityName("老鼠".into()),
        ActionValue::new(5), // 老鼠敏捷 5
        ActionPrediction::new("追击", ActionKind::Chase),
        FleeLogState::default(), ActionPreview::new(),
        AttackName("撕咬".into()),
    ));

    // 初始 FOV
    let _ = world.run_system_once(fov_system);
    world
}

#[test]
fn test_monster_av_advances_after_player_action() {
    let mut world = full_test_world();

    // 记下怪物初始 AV
    let monster_av_before = world.query::<(&Monster, &ActionValue)>().iter(&world)
        .next().map(|(_, a)| a.current_av).unwrap();

    // 模拟玩家移动（方向键 → 移动 → AV 重置）
    set_player_dir(&mut world, 1, 0);
    rebuild_occupancy(&mut world);
    let _ = world.run_system_once(movement_system);
    // 玩家移动后重置 AV
    let player_entity = world.query::<(Entity, &Player)>().iter(&world).next().map(|(e, _)| e).unwrap();
    let agility = world.get::<Stats>(player_entity).unwrap().agility;
    let speed = agility_to_speed(agility);
    let new_av = effective_cost(action_cost::MOVE, speed);
    world.get_mut::<ActionValue>(player_entity).unwrap().current_av = new_av;

    // 以 MOVE 成本推进一次
    let r = advance_by(&mut world, effective_cost(action_cost::MOVE, agility_to_speed(10)));

    // 怪物 AV 应该变化了
    let monster_av_after = world.query::<(&Monster, &ActionValue)>().iter(&world)
        .next().map(|(_, a)| a.current_av).unwrap();
    assert!(monster_av_after != monster_av_before,
        "推进后怪物 AV 应变化: 前={}, 后={}", monster_av_before, monster_av_after);
}

#[test]
fn test_monster_eventually_executes() {
    let mut world = full_test_world();
    let monster_pos_before = world.query::<(&Monster, &Position)>().iter(&world)
        .next().map(|(_, p)| (p.x, p.y)).unwrap();
    let player_entity = world.query::<(Entity, &Player)>().iter(&world).next().map(|(e, _)| e).unwrap();

    for _ in 0..20 {
        let agility = world.get::<Stats>(player_entity).unwrap().agility;
        let speed = agility_to_speed(agility);
        world.get_mut::<ActionValue>(player_entity).unwrap().current_av = effective_cost(action_cost::WAIT, speed);
        world.get_mut::<ActionPrediction>(player_entity).unwrap().locked = false;
        advance_by(&mut world, effective_cost(action_cost::WAIT, speed));

        // 检查怪物是否移动了
        let (mx, my) = world.query::<(&Monster, &Position)>().iter(&world)
            .next().map(|(_, p)| (p.x, p.y)).unwrap();
        if (mx, my) != monster_pos_before {
            return; // 怪物动了 → 测试通过
        }
    }
    panic!("20 次玩家行动后怪物仍未移动");
}

#[test]
fn test_combat_active_triggers_on_attack() {
    let mut world = full_test_world();

    // 初始 combat_active = false
    assert!(!world.resource::<GamePacing>().combat_active);

    // 玩家攻击（将怪物放在玩家旁边）
    {
        let (mx, my) = world.query::<(&Monster, &Position)>().iter(&world)
            .next().map(|(_, p)| (p.x, p.y)).unwrap();
        // 移动玩家到怪物旁边
        let player_e = world.query::<(Entity, &Player)>().iter(&world).next().map(|(e, _)| e).unwrap();
        world.get_mut::<Position>(player_e).unwrap().x = mx + 1;
        world.get_mut::<Position>(player_e).unwrap().y = my;

        // 攻击
        set_player_dir(&mut world, -1, 0);
        rebuild_occupancy(&mut world);
        let _ = world.run_system_once(movement_system);
    };

    // 攻击后 combat_active 应该为 true
    assert!(world.resource::<GamePacing>().combat_active,
        "玩家攻击后 combat_active 应为 true");
}

#[test]
fn test_fov_computed_at_start() {
    let mut world = full_test_world();
    // FOV 应该在创建世界时已计算
    let player_viewshed = world.query::<(&Player, &Viewshed)>().iter(&world)
        .next().map(|(_, v)| v.visible_tiles.len()).unwrap_or(0);
    assert!(player_viewshed > 0, "初始 FOV 应计算完成，可见格子数={}", player_viewshed);
}

#[test]
fn test_monster_brain_chain_used_in_prediction() {
    let mut world = full_test_world();

    // 让怪物看不见玩家（把玩家移到很远）
    let player_e = world.query::<(Entity, &Player)>().iter(&world).next().map(|(e, _)| e).unwrap();
    world.get_mut::<Position>(player_e).unwrap().x = 100; // 超出地图
    world.get_mut::<Position>(player_e).unwrap().y = 100;

    // 重新预测
    let _ = world.run_system_once(predict_monster_actions_system);

    // 怪物看不到玩家 → 应该预测为 Wander（游荡）
    let pred = world.query::<(&Monster, &ActionPrediction)>().iter(&world)
        .next().map(|(_, p)| p.kind.clone()).unwrap();
    assert_eq!(pred, ActionKind::Wander, "看不到玩家时怪物应预测为 Wander");
}

#[test]
fn test_monster_chases_player() {
    let mut world = full_test_world();

    // 让怪物能看到玩家（放在同一个房间内，相距 3 格）
    let (cx, cy) = { let map = world.resource::<Map>(); map.rooms[0].center() };
    let player_e = world.query::<(Entity, &Player)>().iter(&world).next().map(|(e, _)| e).unwrap();
    let monster_e = world.query::<(Entity, &Monster)>().iter(&world).next().map(|(e, _)| e).unwrap();

    // 房间内保证是地板
    world.get_mut::<Position>(player_e).unwrap().x = cx;
    world.get_mut::<Position>(player_e).unwrap().y = cy;
    world.get_mut::<Position>(monster_e).unwrap().x = cx + 2;
    world.get_mut::<Position>(monster_e).unwrap().y = cy;

    // 重新计算 FOV
    let _ = world.run_system_once(fov_system);

    // 重新预测
    let _ = world.run_system_once(predict_monster_actions_system);

    // 怪物看得到玩家 → 预测为 Chase
    let pred = world.query::<(&Monster, &ActionPrediction)>().iter(&world)
        .next().map(|(_, p)| p.kind.clone()).unwrap();
    assert_eq!(pred, ActionKind::Chase, "能看到玩家时怪物应预测为 Chase");

    // 推进多次让怪物行动
    let start_pos = (cx + 2, cy);
    for _ in 0..20 {
        let agility = world.get::<Stats>(player_e).unwrap().agility;
        let speed = agility_to_speed(agility);
        let cost = effective_cost(action_cost::WAIT, speed);
        world.get_mut::<ActionValue>(player_e).unwrap().current_av = cost;
        advance_by(&mut world, cost);

        let mpos = world.get::<Position>(monster_e).unwrap();
        if (mpos.x, mpos.y) != start_pos {
            return; // 怪物动了，测试通过
        }
    }
    panic!("怪物未能向玩家移动");
}

#[test]
fn test_advance_zero_does_nothing() {
    // advance_by(0) 现在会处理 AV ≤ 0 的待执行实体
    // 测试：AV > 0 时 advance_by(0) 真正无事可做
    let mut world = av_test_world(200.0, 200.0);
    let r = advance_by(&mut world, 0.0);
    assert!(!r.any_executed);
    assert!(!r.any_locked);
}

#[test]
fn test_advance_zero_executes_pending() {
    // AV = 0 的实体应被 advance_by(0) 执行
    let mut world = av_test_world(0.0, 200.0);
    let r = advance_by(&mut world, 0.0);
    assert!(r.any_executed, "玩家 AV=0 应被执行");
    let player_av = world.query::<(&Player, &ActionValue)>().single(&world).unwrap().1.current_av;
    assert!(player_av > 100.0, "执行后玩家 AV 应重置");
}

// ═════════════════════════════════════════════════════
// 行为追踪测试：玩家移动 AV 时序
// ═════════════════════════════════════════════════════

/// 模拟玩家提交移动后的 AV 时序：
/// 1. commit 后 AV = MOVE 成本（不是 0）
/// 2. advance_by 递减 AV 到 0
/// 3. AV=0 时执行并重置，玩家位置变化
#[test]
fn test_player_move_av_timeline() {
    let mut world = av_test_world(125.0, 300.0);

    // 把玩家放到房间中心（确保移动目标格是地板）
    let center = world.resource::<Map>().rooms[0].center();
    {
        let mut q = world.query::<(&Player, &mut Position)>();
        let (_, mut pos) = q.iter_mut(&mut world).next().unwrap();
        pos.x = center.0;
        pos.y = center.1;
    }
    rebuild_occupancy(&mut world);

    let player_e = world.query::<(Entity, &Player)>().single(&world).unwrap().0;
    let agility = world.get::<Stats>(player_e).unwrap().agility;
    let expected_cost = effective_cost(action_cost::MOVE, agility_to_speed(agility));
    let start_x = world.get::<Position>(player_e).unwrap().x;

    // 提交：写预测 + 设 AV = 移动成本
    world.resource_mut::<PendingInput>().direction = Some((1, 0));
    world.get_mut::<ActionPrediction>(player_e).unwrap().desc = "移动".into();
    world.get_mut::<ActionPrediction>(player_e).unwrap().kind = ActionKind::Move;
    *world.get_mut::<ActionValue>(player_e).unwrap() = ActionValue::with_cost(action_cost::MOVE, agility);

    let av_after_commit = world.get::<ActionValue>(player_e).unwrap().current_av;
    assert!((av_after_commit - expected_cost).abs() < 1.0,
        "提交后 AV={:.0} 应为行动成本 {:.0}", av_after_commit, expected_cost);

    rebuild_occupancy(&mut world);

    // 一步推进到 AV=0
    advance_by(&mut world, av_after_commit);

    let av_after = world.get::<ActionValue>(player_e).unwrap().current_av;
    assert!(av_after > 100.0, "执行后 AV={:.0} 应 >100", av_after);

    let pos = world.query::<(&Player, &Position)>().single(&world).unwrap().1;
    assert_eq!(pos.x, start_x + 1, "玩家应右移一格");
}

/// 战斗场景：玩家攻击老鼠的完整时序
#[test]
fn test_combat_flow_player_vs_rat() {
    use bevy_ecs::system::RunSystemOnce;

    let mut world = av_test_world(125.0, 300.0);

    // 把玩家和老鼠放到房间中心相邻位置
    let center = world.resource::<Map>().rooms[0].center();
    {
        let mut q = world.query::<(&Player, &mut Position)>();
        let (_, mut pos) = q.iter_mut(&mut world).next().unwrap();
        pos.x = center.0;
        pos.y = center.1;
    }
    {
        let mut q = world.query::<(&Monster, &mut Position)>();
        let (_, mut pos) = q.iter_mut(&mut world).next().unwrap();
        pos.x = center.0 + 1;
        pos.y = center.1;
    }
    rebuild_occupancy(&mut world);
    let _ = world.run_system_once(fov_system);
    let _ = world.run_system_once(predict_monster_actions_system);

    // 记录初始生命
    let rat_hp_before = {
        let mut q = world.query::<(Entity, &Stats)>();
        q.iter(&world)
            .find(|(e, _)| world.get::<Monster>(*e).is_some())
            .map(|(_, s)| s.hp)
            .unwrap()
    };

    // 玩家提交移动（向老鼠方向）
    world.resource_mut::<PendingInput>().direction = Some((1, 0));
    let player_e = world.query::<(Entity, &Player)>().single(&world).unwrap().0;
    let agility = world.get::<Stats>(player_e).unwrap().agility;
    world.get_mut::<ActionPrediction>(player_e).unwrap().desc = "移动".into();
    world.get_mut::<ActionPrediction>(player_e).unwrap().kind = ActionKind::Move;
    *world.get_mut::<ActionValue>(player_e).unwrap() = ActionValue::with_cost(action_cost::MOVE, agility);

    rebuild_occupancy(&mut world);

    // 推进到锁定边界（玩家距锁定 25AV）
    let cost = world.get::<ActionValue>(player_e).unwrap().current_av;
    advance_by(&mut world, cost - 100.0);

    let player_av = world.get::<ActionValue>(player_e).unwrap().current_av;
    assert!((player_av - 100.0).abs() < 1.0, "锁定边界 AV=100, 实际 {:.0}", player_av);
    assert!(world.get::<ActionPrediction>(player_e).unwrap().locked);

    // 解锁并继续推进到执行
    world.get_mut::<ActionPrediction>(player_e).unwrap().locked = false;
    advance_by(&mut world, 100.0);

    // 玩家执行 → bump attack → 老鼠受伤
    let rat_hp_after = {
        let mut q = world.query::<(Entity, &Stats)>();
        q.iter(&world)
            .find(|(e, _)| world.get::<Monster>(*e).is_some())
            .map(|(_, s)| s.hp)
    };
    if let Some(hp) = rat_hp_after {
        assert!(hp < rat_hp_before, "老鼠应受伤: before={} after={}", rat_hp_before, hp);
    }

    assert!(world.resource::<GamePacing>().combat_active, "攻击后应进入战斗");
}

/// 验证：玩家和怪物按 AV 顺序行动（AV 小的先执行）。
/// 玩家 AV=200（后），怪物 AV=50（前）→ 怪物先执行。
#[test]
fn test_execution_order_respects_av() {
    // 玩家 AV=200，怪物 AV=50（怪物在前）
    let mut world = av_test_world(200.0, 50.0);

    // 把怪物移到房间中心（玩家旁边但不同格）
    let center = world.resource::<Map>().rooms[0].center();
    {
        let mut q = world.query::<(&Monster, &mut Position)>();
        let (_, mut pos) = q.iter_mut(&mut world).next().unwrap();
        pos.x = center.0 + 2; pos.y = center.1;
    }
    {
        let mut q = world.query::<(&Player, &mut Position)>();
        let (_, mut pos) = q.iter_mut(&mut world).next().unwrap();
        pos.x = center.0; pos.y = center.1;
    }
    rebuild_occupancy(&mut world);

    let player_start = world.query::<(&Player, &Position)>().single(&world).unwrap().1.x;
    let monster_start = world.query::<(&Monster, &Position)>().single(&world).unwrap().1.x;

    // 记录执行顺序
    let mut exec_order: Vec<&str> = Vec::new();

    // 推进：先到 AV=0 的先执行
    // 怪物 AV=50 先到 0 → 怪物执行 → AV 重置
    // 然后玩家 AV=150（200-50）→ 继续推进 150 → 玩家执行
    let dist = 50.0; // 怪物距执行点的距离
    let r = advance_by(&mut world, dist);
    assert!(r.any_executed, "应有实体执行");
    // 检查怪物位置变了
    let monster_after = world.query::<(&Monster, &Position)>().single(&world).unwrap().1.x;
    // 怪物是 Wander/Chase 行为，可能移动。检查其 AV 是否重置
    let monster_av = world.query::<(&Monster, &ActionValue)>().single(&world).unwrap().1.current_av;
    assert!(monster_av > 100.0, "怪物执行后 AV 应重置, 实际 {:.0}", monster_av);

    // 玩家应尚未执行（AV=200-50=150 > 0）
    let player_pos = world.query::<(&Player, &Position)>().single(&world).unwrap().1.x;
    assert_eq!(player_pos, player_start, "玩家不应移动（AV 未到 0）");

    // 继续推进到玩家执行
    advance_by(&mut world, 150.0);
    let player_av = world.query::<(&Player, &ActionValue)>().single(&world).unwrap().1.current_av;
    assert!(player_av > 100.0, "玩家执行后 AV 应重置, 实际 {:.0}", player_av);
}

/// 战斗暂停 → 确认 → 老鼠先执行（AV 更小），玩家后执行。
#[test]
fn test_combat_confirm_rat_executes_first() {
    let mut world = av_test_world(100.0, 39.0);
    let c = world.resource::<Map>().rooms[0].center();
    {
        let mut q = world.query::<(&Player, &mut Position)>();
        let (_, mut pos) = q.iter_mut(&mut world).next().unwrap();
        pos.x = c.0; pos.y = c.1;
    }
    {
        let mut q = world.query::<(&Monster, &mut Position)>();
        let (_, mut pos) = q.iter_mut(&mut world).next().unwrap();
        pos.x = c.0 + 1; pos.y = c.1;
    }
    rebuild_occupancy(&mut world);

    world.resource_mut::<GamePacing>().combat_active = true;

    // 模拟 Enter 确认：解锁 + just_confirmed
    let pe = world.query::<(Entity, &Player)>().single(&world).unwrap().0;
    world.get_mut::<ActionPrediction>(pe).unwrap().locked = false;
    world.get_mut::<ActionPrediction>(pe).unwrap().just_confirmed = true;

    // 老鼠 AV=39，玩家 AV=100。一次推进：老鼠先执行，玩家后执行。
    advance_to_next_decision_point(&mut world);
    let rat_av = world.query::<(&Monster, &ActionValue)>().single(&world).unwrap().1.current_av;
    let player_av = world.query::<(&Player, &ActionValue)>().single(&world).unwrap().1.current_av;
    assert!(rat_av > 0.0, "老鼠应已推进, AV={:.0}", rat_av);
    assert!(player_av > 100.0, "玩家应已执行, AV={:.0}", player_av);
}


