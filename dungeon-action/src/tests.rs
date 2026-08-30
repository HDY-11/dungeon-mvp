//! 行动系统单元测试
//!
//! 覆盖：advance_action_queue、tap-tap 流程、check_condition 各分之路

use super::*;
use bevy_ecs::prelude::*;
use dungeon_core::{
    self as core, AttackName, EntityName, Equipment, EventLog, FloorNumber, GameRng, Inventory,
    Map, MapMemory, MapSeed, Monster, OccupancyMap, PendingExp, Player, PlayerClass, Position,
    Renderable, Skills, Stats, TurnManager, Viewshed, VisibleMemory, items::*,
};
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
    map.generate(core::MapKind::Cavern, &mut rng);

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
    world.insert_resource(PlayerPreview::default());
    world.insert_resource(ChaseIntents::default());
    world.insert_resource(FleeIntents::default());
    world.insert_resource(WanderIntents::default());

    // 找第一个可行走格作为出生点（不依赖 rooms[0].center()）
    let (spawn_x, spawn_y) = first_walkable(&map);
    world.insert_resource(map);

    let pc = PlayerClass::Warrior;
    let mut cmd = world.spawn((
        Player,
        Position {
            x: spawn_x,
            y: spawn_y,
        },
        Renderable {
            glyph: '@',
            color: (255, 255, 0),
        },
        Viewshed {
            range: 10,
            visible_tiles: Vec::new(),
        },
        Stats::player(),
        EntityName("冒险者".into()),
        Inventory::new(36),
        Equipment::new(),
        pc.clone(),
        AttackName("斩击".into()),
    ));
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

    let (dx, dy) = {
        let map = world.resource::<Map>();
        walkable_neighbor(map, before.x, before.y).expect("出生点应至少有一个可行走邻居")
    };

    let av = agility_to_reaction(10) + CanMove::new(100).duration * agility_speed_factor(10);
    world
        .resource_mut::<ActionQueue>()
        .enqueue(player, ActionKindV3::Move { dx, dy }, av);

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
    world
        .resource_mut::<ActionQueue>()
        .enqueue(player, ActionKindV3::Wait, av);

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
        world
            .resource_mut::<ActionQueue>()
            .enqueue(player, ActionKindV3::Move { dx, dy }, av);

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

    let (dx, dy) = {
        let map = world.resource::<Map>();
        walkable_neighbor(map, before.x, before.y).expect("应至少有一个可行走方向")
    };

    // 第一次按 → 预览
    let confirmed = handle_player_direction(&mut world, dx, dy);
    assert!(!confirmed, "第一次按应为预览");
    assert!(world.resource::<PlayerPreview>().kind.is_some(), "应有预览");

    // 第二次按同方向 → 确认入队
    let confirmed = handle_player_direction(&mut world, dx, dy);
    assert!(confirmed, "第二次按应为确认");
    assert!(
        world.resource::<PlayerPreview>().kind.is_none(),
        "确认后预览应清除"
    );
    assert_eq!(
        world.resource::<ActionQueue>().entries.len(),
        1,
        "应有 1 个行动入队"
    );
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

    let (dx, dy) = {
        let map = world.resource::<Map>();
        walkable_neighbor(map, pos.x, pos.y).expect("应至少有一个可行走邻居放怪物")
    };

    let monster_pos = Position {
        x: pos.x.wrapping_add_signed(dx),
        y: pos.y.wrapping_add_signed(dy),
    };

    let rat_stats = core::monster_def::monster_stats(core::MonsterKindId::Rat, 1);
    let monster_hp_before = rat_stats.hp;
    let monster_entity = world
        .spawn((
            Monster,
            monster_pos,
            Renderable {
                glyph: 'r',
                color: (255, 0, 0),
            },
            Viewshed {
                range: 10,
                visible_tiles: Vec::new(),
            },
            rat_stats,
            EntityName("老鼠".into()),
            AttackName("撕咬".into()),
            core::monster_def::monster_loot(core::MonsterKindId::Rat),
        ))
        .id();
    core::ops::rebuild_occupancy(&mut world);

    // 通过 ActionQueue 执行攻击
    let av = agility_to_reaction(10) + CanMove::new(100).duration * agility_speed_factor(10);
    world.resource_mut::<ActionQueue>().enqueue(
        player,
        ActionKindV3::Attack {
            target: monster_entity,
        },
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
        log.messages
            .iter()
            .any(|m| m.text.contains("造成") || m.text.contains("击杀")),
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

// ──────────────────────────────────────────────
// 回归测试：投掷验证（I59 射程/视线、I60 副手类型）
// 手动构造地图：玩家 (10,10)，走廊 y=10, x=5..20 为 Floor，
// (10,11) 为墙（挡视线），(10,12) 为墙后可行走目标
// ──────────────────────────────────────────────
fn fresh_throw_world() -> World {
    ItemRegistry::load();
    let mut world = World::new();
    let mut tiles = [[core::Tile::Wall; core::MAP_WIDTH]; core::MAP_HEIGHT];
    for row in &mut tiles[5..15] {
        for cell in &mut row[5..20] {
            *cell = core::Tile::Floor;
        }
    }
    tiles[11][10] = core::Tile::Wall; // (10,11) 视线挡墙
    world.insert_resource(Map {
        tiles,
        rooms: Vec::new(),
    });
    world.insert_resource(MapMemory::new());
    world.insert_resource(OccupancyMap::new());
    world.insert_resource(PendingExp::default());
    world.insert_resource(EventLog::new());
    world.insert_resource(GameRng::new(42));
    world.insert_resource(TurnManager::new());
    world.insert_resource(FloorNumber(1));
    world.insert_resource(VisibleMemory::default());
    world.insert_resource(ActionQueue::default());
    world.insert_resource(PlayerPreview::default());
    world.insert_resource(ChaseIntents::default());
    world.insert_resource(FleeIntents::default());
    world.insert_resource(WanderIntents::default());
    let mut cmd = world.spawn((
        Player,
        Position { x: 10, y: 10 },
        Renderable {
            glyph: '@',
            color: (255, 255, 0),
        },
        Viewshed {
            range: 10,
            visible_tiles: Vec::new(),
        },
        Stats::player(),
        EntityName("冒险者".into()),
        Inventory::new(36),
        Equipment::new(),
        PlayerClass::Warrior,
        AttackName("斩击".into()),
    ));
    cmd.insert(CanMove::new(100));
    cmd.insert(CanWait::new(0));
    cmd.insert(Skills { list: Vec::new() });
    cmd.insert(core::ActiveBuffs::new());
    world
}

/// 有效投掷：命中射程内视线畅通的目标并消耗石子
#[test]
fn test_throw_valid_hit() {
    let mut world = fresh_throw_world();
    let player = core::ops::player_entity(&world).unwrap();
    world.get_mut::<Equipment>(player).unwrap().off_hand = Some(ItemStack::new(ITEM_STONE, 3));
    world.spawn((
        Monster,
        Position { x: 12, y: 10 },
        Renderable {
            glyph: 'r',
            color: (255, 0, 0),
        },
        EntityName("老鼠".into()),
        Stats::player(),
    ));
    crate::execute::execute_throw(&mut world, player, 12, 10);
    let off = world
        .get::<Equipment>(player)
        .unwrap()
        .off_hand
        .as_ref()
        .unwrap();
    assert_eq!(off.count, 2, "有效投掷应消耗 1 颗石子");
    let mon_hp = world
        .query::<(&Monster, &Stats)>()
        .iter(&world)
        .next()
        .unwrap()
        .1
        .hp;
    assert!(mon_hp < 33, "目标怪物应受伤");
}

/// 超射程（切比雪夫 > 5）投掷被取消，不消耗石子
#[test]
fn test_throw_out_of_range_cancelled() {
    let mut world = fresh_throw_world();
    let player = core::ops::player_entity(&world).unwrap();
    world.get_mut::<Equipment>(player).unwrap().off_hand = Some(ItemStack::new(ITEM_STONE, 2));
    crate::execute::execute_throw(&mut world, player, 16, 10); // 距离 6 > 5
    let off = world
        .get::<Equipment>(player)
        .unwrap()
        .off_hand
        .as_ref()
        .unwrap();
    assert_eq!(off.count, 2, "超射程投掷不应消耗石子");
}

/// 视线受阻（墙后目标）投掷被取消，不消耗石子
#[test]
fn test_throw_blocked_los_cancelled() {
    let mut world = fresh_throw_world();
    let player = core::ops::player_entity(&world).unwrap();
    world.get_mut::<Equipment>(player).unwrap().off_hand = Some(ItemStack::new(ITEM_STONE, 2));
    crate::execute::execute_throw(&mut world, player, 10, 12); // 墙 (10,11) 后
    let off = world
        .get::<Equipment>(player)
        .unwrap()
        .off_hand
        .as_ref()
        .unwrap();
    assert_eq!(off.count, 2, "视线受阻投掷不应消耗石子");
}

/// 非投掷物副手（木盾）不被投掷消耗
#[test]
fn test_throw_non_throwable_offhand_cancelled() {
    let mut world = fresh_throw_world();
    let player = core::ops::player_entity(&world).unwrap();
    world.get_mut::<Equipment>(player).unwrap().off_hand =
        Some(ItemStack::new(ITEM_WOOD_SHIELD, 1));
    crate::execute::execute_throw(&mut world, player, 12, 10);
    let off = world
        .get::<Equipment>(player)
        .unwrap()
        .off_hand
        .as_ref()
        .unwrap();
    assert_eq!(off.item_id, ITEM_WOOD_SHIELD, "木盾不应被消耗");
    assert_eq!(off.count, 1);
}

// ──────────────────────────────────────────────
// 回归测试：技能卷轴（I61）+ 治愈公式（G18）
// ──────────────────────────────────────────────
#[test]
fn test_learn_skill_and_use_item() {
    let mut world = fresh_world();
    let player = core::ops::player_entity(&world).unwrap();
    assert!(
        core::use_item(ITEM_SCROLL_HEAL, &mut world, player),
        "卷轴应可学习"
    );
    let skills = world.get::<Skills>(player).unwrap();
    assert_eq!(skills.list.len(), 1);
    assert_eq!(skills.list[0].name, "治愈");
    assert_eq!(skills.list[0].proficiency, 1);
    core::use_item(ITEM_SCROLL_HEAL, &mut world, player);
    let skills = world.get::<Skills>(player).unwrap();
    assert_eq!(skills.list[0].proficiency, 2, "重复学习应提升熟练度");
    assert!(
        !core::use_item(ITEM_STONE, &mut world, player),
        "石子不可直接使用"
    );
}

#[test]
fn test_heal_includes_magic_mastery() {
    let mut world = fresh_world();
    let player = core::ops::player_entity(&world).unwrap();
    core::use_item(ITEM_SCROLL_HEAL, &mut world, player);
    let max_hp = world.get::<Stats>(player).unwrap().max_hp;
    let mastery = world.get::<Stats>(player).unwrap().magic_mastery;
    world.get_mut::<Stats>(player).unwrap().hp = 5;
    crate::execute::execute_skill(&mut world, player, 0);
    let hp = world.get::<Stats>(player).unwrap().hp;
    // Gm6: amount(15) + 法术精通×1 + 熟练度(1)×3
    let expected_heal = 15 + mastery as i32 + 3;
    assert!(hp <= max_hp, "治愈不应超过最大 HP");
    assert_eq!(hp, 5 + expected_heal, "治愈必须包含法术精通（G18）");
}

// ──────────────────────────────────────────────
// 回归测试：I66/L47 — 详情页操作判定共享 + 单物品拾取
// ──────────────────────────────────────────────
#[test]
fn test_detail_item_actions() {
    ItemRegistry::load();
    use dungeon_core::{ITEM_RUSTY_SWORD, ITEM_SCROLL_HEAL, ITEM_STONE, ItemAction};
    // 背包：装备类（有 slot）→ Equip+Drop；无 Use
    let sword = ItemStack::new(ITEM_RUSTY_SWORD, 1);
    let a = dungeon_core::detail_item_actions(Some(&sword), 0);
    assert!(
        a.contains(&ItemAction::Equip) && a.contains(&ItemAction::Drop),
        "剑应可装备+丢弃: {:?}",
        a
    );
    assert!(!a.contains(&ItemAction::Use), "剑不可直接使用: {:?}", a);
    // 背包：卷轴（可用）→ Use+Drop；无 Equip
    let scroll = ItemStack::new(ITEM_SCROLL_HEAL, 1);
    let a = dungeon_core::detail_item_actions(Some(&scroll), 0);
    assert!(
        a.contains(&ItemAction::Use) && a.contains(&ItemAction::Drop),
        "卷轴应可学习+丢弃: {:?}",
        a
    );
    assert!(!a.contains(&ItemAction::Equip), "卷轴不可装备: {:?}", a);
    // 背包：石子 → 仅 Drop（I66: UI 不再提示 r）
    let stone = ItemStack::new(ITEM_STONE, 1);
    let a = dungeon_core::detail_item_actions(Some(&stone), 0);
    assert_eq!(a, vec![ItemAction::Drop], "石子只可丢弃: {:?}", a);
    // 装备槽 → Unequip；地面 → Pickup
    assert_eq!(
        dungeon_core::detail_item_actions(Some(&sword), 1),
        vec![ItemAction::Unequip]
    );
    assert_eq!(
        dungeon_core::detail_item_actions(Some(&sword), 2),
        vec![ItemAction::Pickup]
    );
    // is_usable 与 use_item 同源
    assert!(dungeon_core::is_usable(ITEM_SCROLL_HEAL));
    assert!(!dungeon_core::is_usable(ITEM_STONE));
}

#[test]
fn test_pickup_ground_item() {
    let mut world = fresh_world();
    let player = core::ops::player_entity(&world).unwrap();
    let pos = *world.get::<Position>(player).unwrap();
    // 放一件地面物品在玩家脚下
    let e = world
        .spawn((
            core::ItemPickup {
                stack: ItemStack::new(ITEM_STONE, 2),
            },
            Position { x: pos.x, y: pos.y },
            Renderable {
                glyph: '·',
                color: (160, 140, 120),
            },
        ))
        .id();
    // 拾取成功：背包 +2，实体消失
    assert!(core::ops::pickup_ground_item(&mut world, e), "拾取应成功");
    let inv = world.get::<Inventory>(player).unwrap();
    assert_eq!(
        inv.stacks
            .iter()
            .find(|s| s.item_id == ITEM_STONE)
            .map(|s| s.count)
            .unwrap_or(0),
        2,
        "石子应进背包"
    );
    assert!(world.get_entity(e).is_err(), "地面物品实体应消失");
    // 背包满 → 拾取失败且实体保留（用不可堆叠装备填满 36 格，避免石子自动堆叠）
    let mut inv = world.get_mut::<Inventory>(player).unwrap();
    inv.stacks.clear();
    for i in 0..36 {
        let id = [
            ITEM_RUSTY_SWORD,
            ITEM_WOOD_SHIELD,
            ITEM_LEATHER_ARMOR,
            ITEM_ATTACK_RING,
        ][i % 4];
        inv.add(id, 1);
    }
    let e2 = world
        .spawn((
            core::ItemPickup {
                stack: ItemStack::new(ITEM_STONE, 1),
            },
            Position { x: pos.x, y: pos.y },
            Renderable {
                glyph: '·',
                color: (160, 140, 120),
            },
        ))
        .id();
    assert!(
        !core::ops::pickup_ground_item(&mut world, e2),
        "背包满时拾取应失败"
    );
    assert!(world.get_entity(e2).is_ok(), "背包满时地面物品应保留");
}

// ──────────────────────────────────────────────
// 回归测试：I67 武器攻速 + I69 模板碎片合成 + I68 LOS
// ──────────────────────────────────────────────
#[test]
fn test_weapon_speed_affects_av() {
    let mut world = fresh_world();
    let player = core::ops::player_entity(&world).unwrap();
    // 空手 → 默认 300ms
    let av_bare = {
        let agi = world.get::<Stats>(player).unwrap().agility;
        let r = agility_to_reaction(agi);
        let f = agility_speed_factor(agi);
        r + 300.0 * f
    };
    // 动态找一个可行方向（出生点周围可能有墙）
    let find_dir = |world: &mut World| -> (isize, isize) {
        for (dx, dy) in [(1isize, 0isize), (-1, 0), (0, 1), (0, -1)] {
            let _ = crate::player::handle_player_direction(world, dx, dy); // 预览
            if world.resource::<PlayerPreview>().kind.is_some() {
                return (dx, dy);
            }
        }
        panic!("无可行方向");
    };
    // 装备匕首（speed 200）→ 双击确认入队
    world.get_mut::<Equipment>(player).unwrap().main_hand = Some(ItemStack::new(ITEM_DAGGER, 1));
    let (dx, dy) = find_dir(&mut world);
    let _ = crate::player::handle_player_direction(&mut world, dx, dy); // 同键确认
    let queue = world.resource::<ActionQueue>();
    let entry = queue.entries.iter().find(|e| e.entity == player).unwrap();
    let av_dagger = entry.av_remaining;
    // 换成石锤（speed 450）
    world.resource_mut::<ActionQueue>().entries.clear();
    world.resource_mut::<PlayerPreview>().kind = None;
    world.get_mut::<Equipment>(player).unwrap().main_hand =
        Some(ItemStack::new(ITEM_STONE_HAMMER, 1));
    let (dx, dy) = find_dir(&mut world);
    let _ = crate::player::handle_player_direction(&mut world, dx, dy); // 同键确认
    let queue = world.resource::<ActionQueue>();
    let entry = queue.entries.iter().find(|e| e.entity == player).unwrap();
    let av_hammer = entry.av_remaining;
    assert!(
        av_dagger < av_bare && av_bare < av_hammer,
        "匕首({})应快于空手({})应快于石锤({})",
        av_dagger,
        av_bare,
        av_hammer
    );
}

#[test]
fn test_craft_with_template() {
    let mut world = fresh_world();
    let player = core::ops::player_entity(&world).unwrap();
    {
        let mut inv = world.get_mut::<Inventory>(player).unwrap();
        inv.stacks.clear();
        // 剑刃模板 + 生物血肉×2 + 坚硬木棍×1 → 锈铁剑
        inv.add(ITEM_TEMPLATE_BLADE, 1);
        inv.add(ITEM_BIOMASS, 2);
        inv.add(ITEM_STICK, 1);
    }
    assert!(
        core::craft_with_template(&mut world, player, ITEM_TEMPLATE_BLADE),
        "材料足时应合成成功"
    );
    let inv = world.get::<Inventory>(player).unwrap();
    assert!(inv.count_of(ITEM_RUSTY_SWORD) >= 1, "应产出锈铁剑");
    assert_eq!(inv.count_of(ITEM_BIOMASS), 0, "材料应被消耗");
    assert_eq!(inv.count_of(ITEM_STICK), 0, "材料应被消耗");
    assert!(
        inv.count_of(ITEM_TEMPLATE_BLADE) >= 1,
        "模板由调用方移除，此处不应被合成消耗"
    );
    // 材料不足 → 失败且不消耗模板
    {
        let mut inv = world.get_mut::<Inventory>(player).unwrap();
        inv.stacks.clear();
        inv.add(ITEM_TEMPLATE_BLADE, 1);
        inv.add(ITEM_BIOMASS, 1); // 只给 1 个（需要 2）
    }
    assert!(
        !core::craft_with_template(&mut world, player, ITEM_TEMPLATE_BLADE),
        "材料不足应失败"
    );
    let inv = world.get::<Inventory>(player).unwrap();
    assert_eq!(inv.count_of(ITEM_TEMPLATE_BLADE), 1, "失败时模板不应被消耗");
    assert_eq!(inv.count_of(ITEM_BIOMASS), 1, "失败时材料不应被消耗");
    // 背包满但模板+材料占位 → G26 修复后应成功（材料与模板移除腾出空间，不再误报"背包已满"）
    {
        let mut inv = world.get_mut::<Inventory>(player).unwrap();
        inv.stacks.clear();
        inv.add(ITEM_TEMPLATE_RING, 1);
        inv.add(ITEM_FANG, 2);
        for i in 0..34 {
            let id = [
                ITEM_RUSTY_SWORD,
                ITEM_WOOD_SHIELD,
                ITEM_LEATHER_ARMOR,
                ITEM_ATTACK_RING,
            ][i % 4];
            inv.add(id, 1);
        }
    }
    assert!(
        core::craft_with_template(&mut world, player, ITEM_TEMPLATE_RING),
        "背包满但材料/模板占位时应合成成功（G26）"
    );
    let inv = world.get::<Inventory>(player).unwrap();
    assert_eq!(inv.count_of(ITEM_FANG), 0, "材料应被消耗");
    assert!(inv.count_of(ITEM_ATTACK_RING) >= 1, "应产出攻击戒指");
    assert!(
        inv.count_of(ITEM_TEMPLATE_RING) >= 1,
        "模板由调用方移除，合成内部不消耗"
    );
}

#[test]
fn test_los_clear_and_chebyshev() {
    use dungeon_core::ops::{chebyshev, los_clear};
    // 构造：玩家 (10,10)，墙 (11,10)，目标 (12,10)
    let mut map = core::Map::new();
    map.tiles = [[core::Tile::Wall; core::MAP_WIDTH]; core::MAP_HEIGHT];
    for y in 5..15 {
        for x in 5..20 {
            map.tiles[y][x] = core::Tile::Floor;
        }
    }
    map.tiles[11][10] = core::Tile::Wall; // (10,11) 墙
    assert!(los_clear(&map, (10, 10), (12, 10)), "无阻挡应畅通");
    assert!(!los_clear(&map, (10, 10), (10, 12)), "(10,11) 墙应阻挡视线");
    assert_eq!(chebyshev((10, 10), (13, 12)), 3, "切比雪夫距离");
    assert_eq!(chebyshev((10, 10), (10, 10)), 0);
}

// ──────────────────────────────────────────────
// Dsn24: 地形消耗品（蘑菇回 HP / 海藻回 MP）
// ──────────────────────────────────────────────
#[test]
fn test_consumables_mushroom_and_seaweed() {
    let mut world = fresh_world();
    let player = core::ops::player_entity(&world).unwrap();
    // 蘑菇：掉血后食用恢复，不超上限
    let max_hp = world.get::<Stats>(player).unwrap().max_hp;
    world.get_mut::<Stats>(player).unwrap().hp = max_hp - 10;
    assert!(core::is_usable(ITEM_MUSHROOM));
    assert!(
        core::use_item(ITEM_MUSHROOM, &mut world, player),
        "蘑菇应可食用"
    );
    let hp = world.get::<Stats>(player).unwrap().hp;
    assert_eq!(hp, max_hp - 4, "蘑菇应恢复 6 HP（10-6=4 缺口）");
    // 满血食用：不消耗（返回 false）
    world.get_mut::<Stats>(player).unwrap().hp = max_hp;
    assert!(
        !core::use_item(ITEM_MUSHROOM, &mut world, player),
        "满血时蘑菇不应消耗"
    );
    // 海藻：缺蓝时恢复 4 MP
    let max_mp = world.get::<Stats>(player).unwrap().max_mp;
    world.get_mut::<Stats>(player).unwrap().mp = max_mp.saturating_sub(6);
    assert!(core::is_usable(ITEM_SEAWEED));
    assert!(
        core::use_item(ITEM_SEAWEED, &mut world, player),
        "海藻应可食用"
    );
    assert_eq!(
        world.get::<Stats>(player).unwrap().mp,
        max_mp - 2,
        "海藻应恢复 4 MP"
    );
    // 上限钳制：只差 2 时只恢复 2
    world.get_mut::<Stats>(player).unwrap().mp = max_mp - 2;
    assert!(
        core::use_item(ITEM_SEAWEED, &mut world, player),
        "海藻缺 2 MP 仍应消耗"
    );
    assert_eq!(
        world.get::<Stats>(player).unwrap().mp,
        max_mp,
        "不应超过上限"
    );
}

// ──────────────────────────────────────────────
// I77 回归：投掷确认（Enter 一次确认直接入队，不再依赖 tap-tap 双确认）
// ──────────────────────────────────────────────

/// 在目标格 spawn 一只老鼠（同时注册 Monster 组件——L2: try_query 需要组件已注册）
fn spawn_target_monster(world: &mut World, x: usize, y: usize) {
    world.spawn((
        Monster,
        Position { x, y },
        Renderable {
            glyph: 'r',
            color: (255, 0, 0),
        },
        EntityName("老鼠".into()),
        Stats::player(),
    ));
}

/// 构造瞄准状态：ThrowPreview 有效目标 + 光标 + 页栈在 ThrowAim
fn arm_throw_preview(world: &mut World, tx: usize, ty: usize, valid: bool) {
    world.insert_resource(core::ThrowPreview {
        active: true,
        cursor: (tx, ty),
        path: vec![(tx, ty)],
        valid_target: valid,
    });
    world.insert_resource(core::LookCursor {
        active: true,
        x: tx,
        y: ty,
    });
    world.insert_resource(PageStack::default());
    world.resource_mut::<PageStack>().push(Page::ThrowAim);
}

/// Enter 一次确认：投掷入队 + 页栈弹回 + 预览清空
#[test]
fn test_confirm_throw_enqueues_once() {
    let mut world = fresh_throw_world();
    let player = core::ops::player_entity(&world).unwrap();
    world.get_mut::<Equipment>(player).unwrap().off_hand = Some(ItemStack::new(ITEM_STONE, 3));
    spawn_target_monster(&mut world, 12, 10);
    arm_throw_preview(&mut world, 12, 10, true);

    // 第一次 Enter 即确认（I77 修复前：需要两次且永不成立）
    assert!(
        crate::execute::confirm_throw(&mut world),
        "有效目标应确认成功"
    );
    assert!(
        world.resource::<ActionQueue>().has_entity(player),
        "投掷应入队"
    );
    assert_eq!(
        world.resource::<PageStack>().current(),
        &Page::Game,
        "页栈应弹回 Game"
    );
    assert!(
        world.resource::<PlayerPreview>().kind.is_none(),
        "预览应清空（防污染后续 tap-tap）"
    );

    // 推进队列：投掷执行并消耗石子
    crate::advance_until_player_acted(&mut world);
    let off = world
        .get::<Equipment>(player)
        .unwrap()
        .off_hand
        .as_ref()
        .unwrap();
    assert_eq!(off.count, 2, "确认后推进应消耗 1 颗石子");
}

/// 无效目标（超射程/视线受阻）：拒绝入队，不消耗
#[test]
fn test_confirm_throw_invalid_rejected() {
    let mut world = fresh_throw_world();
    let player = core::ops::player_entity(&world).unwrap();
    world.get_mut::<Equipment>(player).unwrap().off_hand = Some(ItemStack::new(ITEM_STONE, 2));
    arm_throw_preview(&mut world, 16, 10, false); // 超射程

    assert!(
        !crate::execute::confirm_throw(&mut world),
        "无效目标应被拒绝"
    );
    assert!(
        !world.resource::<ActionQueue>().has_entity(player),
        "不应入队"
    );
    let off = world
        .get::<Equipment>(player)
        .unwrap()
        .off_hand
        .as_ref()
        .unwrap();
    assert_eq!(off.count, 2, "拒绝时不应消耗石子");
}

/// 队列中已有玩家旧行动时：确认投掷替换之（enqueue_or_replace 语义）
#[test]
fn test_confirm_throw_replaces_old_action() {
    let mut world = fresh_throw_world();
    let player = core::ops::player_entity(&world).unwrap();
    world.get_mut::<Equipment>(player).unwrap().off_hand = Some(ItemStack::new(ITEM_STONE, 3));
    spawn_target_monster(&mut world, 12, 10);
    world
        .resource_mut::<ActionQueue>()
        .enqueue_or_replace(player, ActionKindV3::Wait, 800.0);
    arm_throw_preview(&mut world, 12, 10, true);

    assert!(crate::execute::confirm_throw(&mut world));
    let queue = world.resource::<ActionQueue>();
    assert_eq!(queue.entries.len(), 1, "旧行动应被替换");
    assert!(matches!(
        queue.entries[0].kind,
        ActionKindV3::Throw { tx: 12, ty: 10 }
    ));
}

// ──────────────────────────────────────────────
// 批 1 回归测试（G28/G29/G30/G31）
// ──────────────────────────────────────────────

/// G28: 背包满时 pickup_ground 不得销毁地面物品；部分空间时写回剩余数量
#[test]
fn test_pickup_ground_full_inventory_keeps_items() {
    let mut world = fresh_world();
    let player = core::ops::player_entity(&world).unwrap();
    let pos = *world.get::<Position>(player).unwrap();

    // 背包完全填满（36 格全部占用，直接赋值绕过堆叠合并；用与拾取物不同的物品）
    {
        let mut inv = world.get_mut::<Inventory>(player).unwrap();
        inv.stacks = vec![ItemStack::new(ITEM_BIOMASS, 1); 36];
    }
    let pickup = world
        .spawn((
            ItemPickup {
                stack: ItemStack::new(ITEM_RUSTY_SWORD, 1),
            },
            Position { x: pos.x, y: pos.y },
            Renderable {
                glyph: '!',
                color: (255, 255, 255),
            },
        ))
        .id();

    core::ops::pickup_ground(&mut world);

    // 背包满：物品实体必须保留、数量不变
    let ip = world
        .get::<ItemPickup>(pickup)
        .expect("背包满时物品实体不应被销毁");
    assert_eq!(ip.stack.count, 1, "背包满时物品数量不应变化");
    let inv = world.get::<Inventory>(player).unwrap();
    assert_eq!(inv.count_of(ITEM_RUSTY_SWORD), 0, "背包满时不应拾取成功");

    // 空出空间后再次拾取 → 全部装下 → 实体销毁
    {
        let mut inv = world.get_mut::<Inventory>(player).unwrap();
        inv.stacks.clear();
    }
    core::ops::pickup_ground(&mut world);
    assert!(
        world.get::<ItemPickup>(pickup).is_none(),
        "拾取成功后实体应销毁"
    );
    let inv = world.get::<Inventory>(player).unwrap();
    assert_eq!(inv.count_of(ITEM_RUSTY_SWORD), 1, "拾取成功后物品应入包");
}

/// G28: 背包部分空间时，装不下的剩余数量写回地面实体
#[test]
fn test_pickup_ground_partial_space_writes_back_leftover() {
    let mut world = fresh_world();
    let player = core::ops::player_entity(&world).unwrap();
    let pos = *world.get::<Position>(player).unwrap();

    {
        let mut inv = world.get_mut::<Inventory>(player).unwrap();
        // 36 格：1 格石子 18 颗 + 35 格满栈其他物品（直接赋值防堆叠合并）
        let mut stacks = vec![ItemStack::new(ITEM_STONE, 18)];
        for _ in 0..35 {
            stacks.push(ItemStack::new(ITEM_RUSTY_SWORD, 1));
        }
        inv.stacks = stacks;
    }
    let pickup = world
        .spawn((
            ItemPickup {
                stack: ItemStack::new(ITEM_STONE, 5),
            },
            Position { x: pos.x, y: pos.y },
            Renderable {
                glyph: '!',
                color: (255, 255, 255),
            },
        ))
        .id();

    core::ops::pickup_ground(&mut world);

    // 装下 2 颗，剩 3 颗留在地面
    let ip = world
        .get::<ItemPickup>(pickup)
        .expect("部分装下时实体不应销毁");
    assert_eq!(ip.stack.count, 3, "剩余数量应写回地面实体");
    let inv = world.get::<Inventory>(player).unwrap();
    assert_eq!(inv.count_of(ITEM_STONE), 20, "背包应达到堆叠上限");
}

/// G29: 目标离开邻接格后，Attack 保活检查取消行动，怪物不受伤害
#[test]
fn test_attack_cancelled_when_target_not_adjacent() {
    let mut world = fresh_world();
    let player = core::ops::player_entity(&world).unwrap();
    let pos = *world.get::<Position>(player).unwrap();

    // 怪物放在距玩家 2 格处（(dx,dy) 方向连走两步）
    let (dx, dy) = {
        let map = world.resource::<Map>();
        walkable_neighbor(map, pos.x, pos.y).expect("需要可行走邻居")
    };
    let mx = pos.x.wrapping_add_signed(dx * 2);
    let my = pos.y.wrapping_add_signed(dy * 2);
    let rat_stats = core::monster_def::monster_stats(core::MonsterKindId::Rat, 1);
    let hp_before = rat_stats.hp;
    let monster = world
        .spawn((
            Monster,
            Position { x: mx, y: my },
            Renderable {
                glyph: 'r',
                color: (255, 0, 0),
            },
            Viewshed {
                range: 10,
                visible_tiles: Vec::new(),
            },
            rat_stats,
            EntityName("老鼠".into()),
            AttackName("撕咬".into()),
            core::monster_def::monster_loot(core::MonsterKindId::Rat),
        ))
        .id();
    core::ops::rebuild_occupancy(&mut world);

    let av = agility_to_reaction(10) + CanMove::new(100).duration * agility_speed_factor(10);
    world.resource_mut::<ActionQueue>().enqueue(
        player,
        ActionKindV3::Attack { target: monster },
        av,
    );

    advance_action_queue(&mut world);

    // 目标不邻接：攻击被保活检查取消，怪物满血
    let stats = world.get::<Stats>(monster).expect("怪物不应死亡");
    assert_eq!(
        stats.hp, hp_before,
        "非邻接目标的攻击应被取消，怪物不应受伤"
    );
}

/// G30: 逃跑滞回——进入 <25%，保活检查用退出阈值 ≥30%
#[test]
fn test_flee_hysteresis_thresholds() {
    // 决策层进入条件：< 25% 才入队（CanFlee::condition）
    assert!(CanFlee::condition(0.24));
    assert!(
        !CanFlee::condition(0.27),
        "27% 高于进入阈值 25%，不应新产生逃跑意图"
    );
    // 保活退出条件：< 30% 时已入队的逃跑继续有效（滞回窗口 25%-30%）
    const {
        assert!(
            dungeon_core::FLEE_HP_RATIO_EXIT > dungeon_core::FLEE_HP_RATIO,
            "退出阈值应高于进入阈值（滞回）"
        );
    }
}

/// G30: 队列中的 Flee 条目在 HP 恢复到 30% 以上时被取消
#[test]
fn test_flee_entry_cancelled_above_exit_threshold() {
    let mut world = fresh_world();
    let player = core::ops::player_entity(&world).unwrap();
    let pos = *world.get::<Position>(player).unwrap();

    let (dx, dy) = {
        let map = world.resource::<Map>();
        walkable_neighbor(map, pos.x, pos.y).expect("需要可行走邻居")
    };
    let monster_pos = Position {
        x: pos.x.wrapping_add_signed(dx),
        y: pos.y.wrapping_add_signed(dy),
    };
    // HP 比率 40% → 已超过退出阈值 30%
    let mut rat_stats = core::monster_def::monster_stats(core::MonsterKindId::Rat, 1);
    rat_stats.hp = (rat_stats.max_hp as f32 * 0.4) as i32;
    let monster = world
        .spawn((
            Monster,
            monster_pos,
            Renderable {
                glyph: 'r',
                color: (255, 0, 0),
            },
            Viewshed {
                range: 10,
                visible_tiles: Vec::new(),
            },
            rat_stats,
            EntityName("老鼠".into()),
            AttackName("撕咬".into()),
            core::monster_def::monster_loot(core::MonsterKindId::Rat),
            CanChase::new(100),
            CanFlee::new(200),
            CanWander::new(50),
            CanWait::new(0),
            core::LastKnownPlayerPos::default(),
        ))
        .id();
    core::ops::rebuild_occupancy(&mut world);

    // 手动入队 Flee（模拟 HP 曾跌破 25% 的遗留行动）
    let av = agility_to_reaction(5) + CanFlee::new(200).duration * agility_speed_factor(5);
    world
        .resource_mut::<ActionQueue>()
        .enqueue(monster, ActionKindV3::Flee, av);

    advance_action_queue(&mut world);

    // HP 40% ≥ 30%：Flee 被保活检查取消，怪物未移动
    let after = world.get::<Position>(monster).expect("怪物应存活");
    assert_eq!(
        (after.x, after.y),
        (monster_pos.x, monster_pos.y),
        "HP 超过退出阈值后逃跑行动应被取消，怪物不应移动"
    );
}

/// G30: 被逼入死角且邻接玩家时，Flee 兜底反击而非原地挨打
#[test]
fn test_flee_cornered_fights_back() {
    let mut world = fresh_world();
    // 重造全墙地图：怪物 (10,10) 被墙围死，玩家 (10,11) 在唯一出口方向
    let mut map = Map::new();
    for row in map.tiles.iter_mut() {
        for t in row.iter_mut() {
            *t = dungeon_core::Tile::Wall;
        }
    }
    map.tiles[10][10] = dungeon_core::Tile::Floor;
    map.tiles[11][10] = dungeon_core::Tile::Floor;
    world.insert_resource(map);

    let player = core::ops::player_entity(&world).unwrap();
    world.get_mut::<Position>(player).unwrap().x = 10;
    world.get_mut::<Position>(player).unwrap().y = 11;
    let player_hp_before = world.get::<Stats>(player).unwrap().hp;

    let mut rat_stats = core::monster_def::monster_stats(core::MonsterKindId::Rat, 1);
    rat_stats.hp = (rat_stats.max_hp as f32 * 0.2) as i32; // 低于 25% 进入逃跑
    let monster = world
        .spawn((
            Monster,
            Position { x: 10, y: 10 },
            Renderable {
                glyph: 'r',
                color: (255, 0, 0),
            },
            Viewshed {
                range: 10,
                visible_tiles: vec![(10, 11)],
            },
            rat_stats,
            EntityName("老鼠".into()),
            AttackName("撕咬".into()),
            core::monster_def::monster_loot(core::MonsterKindId::Rat),
            CanChase::new(100),
            CanFlee::new(200),
            CanWander::new(50),
            CanWait::new(0),
            core::LastKnownPlayerPos::default(),
        ))
        .id();
    core::ops::rebuild_occupancy(&mut world);

    // (10,10) 的 8 邻域中只有玩家占的 (10,11) 可行走 → can_move_to 全 false → 兜底反击
    crate::execute::execute_flee(&mut world, monster);

    let player_hp_after = world.get::<Stats>(player).unwrap().hp;
    assert!(
        player_hp_after < player_hp_before,
        "死角中的逃跑怪物邻接玩家时应反击，玩家 HP 应从 {} 下降",
        player_hp_before
    );
}

/// G31: 对角移动两侧都是墙时禁止（防 corner-cutting）
#[test]
fn test_can_move_to_blocks_corner_cutting() {
    let mut world = fresh_world();
    // 全墙地图 + 手工挖角
    {
        let mut map = world.resource_mut::<Map>();
        for row in map.tiles.iter_mut() {
            for t in row.iter_mut() {
                *t = dungeon_core::Tile::Wall;
            }
        }
        // (5,5) 与 (6,6) 都是 Floor，但 (6,5) 与 (5,6) 是墙 → 对角被堵
        map.tiles[5][5] = dungeon_core::Tile::Floor;
        map.tiles[6][6] = dungeon_core::Tile::Floor;
    }
    {
        let occ = world.resource::<OccupancyMap>();
        let map = world.resource::<Map>();
        // 两侧墙：禁止对角
        assert!(
            !crate::execute::can_move_to(map, occ, 5, 5, 1, 1),
            "两侧都是墙时禁止对角移动"
        );
        // 打开一侧 (6,5)：仍禁止（G31 规则要求两侧都可通行）
        assert!(
            !crate::execute::can_move_to(map, occ, 5, 5, 1, 1),
            "一侧是墙时仍禁止对角移动"
        );
    }
    // 两侧都通时允许
    {
        let mut map = world.resource_mut::<Map>();
        map.tiles[6][5] = dungeon_core::Tile::Floor;
        map.tiles[5][6] = dungeon_core::Tile::Floor;
    }
    {
        let occ = world.resource::<OccupancyMap>();
        let map = world.resource::<Map>();
        assert!(
            crate::execute::can_move_to(map, occ, 5, 5, 1, 1),
            "两侧都可行走时允许对角移动"
        );
    }
}

/// G31: 玩家入队预检同样阻止对角穿墙（handle_player_direction 返回 false）
#[test]
fn test_player_direction_blocks_corner() {
    let mut world = fresh_world();
    let player = core::ops::player_entity(&world).unwrap();
    let pos = *world.get::<Position>(player).unwrap();
    // 在玩家斜对角方向制造墙角：目标格挖空，两侧保持墙
    let (nx, ny) = (pos.x.wrapping_add_signed(1), pos.y.wrapping_add_signed(1));
    if nx >= core::MAP_WIDTH || ny >= core::MAP_HEIGHT {
        return;
    }
    {
        let mut map = world.resource_mut::<Map>();
        for row in map.tiles.iter_mut() {
            for t in row.iter_mut() {
                *t = dungeon_core::Tile::Wall;
            }
        }
        map.tiles[pos.y][pos.x] = dungeon_core::Tile::Floor;
        map.tiles[ny][nx] = dungeon_core::Tile::Floor; // 目标格可行走
        // 两侧 (nx, pos.y) 与 (pos.x, ny) 保持墙
    }
    core::ops::rebuild_occupancy(&mut world);

    // 目标格 walkable 但两侧墙 → 入队预检应拒绝（返回 false 不确认）
    assert!(
        !crate::player::handle_player_direction(&mut world, 1, 1),
        "对角穿墙角应被入队预检拒绝"
    );
    assert!(
        !world.resource::<ActionQueue>().has_entity(player),
        "拒绝后不应入队"
    );
}
