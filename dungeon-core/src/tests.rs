//! 行动系统单元测试 — 验证逻辑正确性
use crate::*;
use bevy_ecs::prelude::*;
use bevy_ecs::system::RunSystemOnce;
use rand::SeedableRng;

/// 为测试创建世界（与 dungeon-world::setup_world 一致）
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
    world.insert_resource(crate::action_types::ActionQueue::default());
    world.insert_resource(crate::action_types::InputBuffer::default());
    world.insert_resource(crate::action_types::PlayerPreview::default());

    let (spawn_x, spawn_y) = map.rooms[0].center();
    world.insert_resource(map);
    let player_agi = 10;

    let pc = PlayerClass::Warrior;
    let mut cmd = world.spawn((
        Player, Position { x: spawn_x, y: spawn_y },
        Renderable { glyph: '@', color: (255, 255, 0) }, MovingDir::default(),
        Viewshed { range: 10, visible_tiles: Vec::new() },
        Stats::player(), EntityName("冒险者".into()),
        Inventory::new(36), Equipment::new(), Buffs::new(),
        pc.clone(), AttackName("斩击".into()),
    ));
    cmd.insert(crate::action_types::Reaction { time: crate::action_types::agility_to_reaction(player_agi) });
    cmd.insert(crate::action_types::CanMove::new(100));
    cmd.insert(crate::action_types::CanWait::new(0));
    cmd.insert(Skills { list: pc.skills() });

    let map_tiles = world.resource::<Map>().tiles;
    let population = crate::monster_def::generate_monster_population(&map_tiles, 1, &mut rng);
    for &(kind, mx, my) in &population {
            let glyph = crate::monster_def::monster_glyph(kind);
            let color = crate::monster_def::monster_color(kind);
            let mon_agi = crate::monster_def::monster_stats(kind, 1).agility;
            let loot = crate::monster_def::monster_loot(kind);
            let attk = crate::monster_def::monster_attack_name(kind);
            let name = crate::monster_def::monster_name(kind);
            let mut cmd = world.spawn((
                Monster, Position { x: mx, y: my }, Renderable { glyph, color },
                Viewshed { range: 10, visible_tiles: Vec::new() },
                crate::monster_def::monster_stats(kind, 1), EntityName(name.into()),
                AttackName(attk.into()), loot,
            ));
            cmd.insert(crate::action_types::Reaction { time: crate::action_types::agility_to_reaction(mon_agi) });
            cmd.insert(crate::action_types::CanChase::new(100));
            cmd.insert(crate::action_types::CanFlee::new(200));
            cmd.insert(crate::action_types::CanWander::new(50));
            cmd.insert(crate::action_types::CanWait::new(0));
        }

    {
        let m = world.resource::<Map>();
        let last = m.rooms.len() - 1;
        let (sx, sy) = m.rooms[last].center();
        world.spawn((Stairs, Position { x: sx, y: sy }, Renderable { glyph: '>', color: (0, 255, 0) }));
    }

    let room_centers: Vec<(usize, usize)> = world.resource::<Map>().rooms.iter().skip(1).map(|r| r.center()).collect();
    let ground_item_ids = [0, 1, 2, 3, 0, 1, 3, 2];
    let item_count = room_centers.len().min(ground_item_ids.len());
    for (i, &item_id) in ground_item_ids[..item_count].iter().enumerate() {
        if let Some(&(ix, iy)) = room_centers.get(i) {
            let def = ItemRegistry::global().get(item_id).unwrap();
            world.spawn((
                ItemPickup { stack: ItemStack::new(item_id, 1) },
                Position { x: ix + 1, y: iy },
                Renderable { glyph: def.glyph, color: def.color },
            ));
        }
    }
    world
}

#[test]
fn test_all_resources_exist() {
    let world = fresh_world();
    assert!(world.get_resource::<action_types::ActionQueue>().is_some());
    assert!(world.get_resource::<action_types::InputBuffer>().is_some());
    assert!(world.get_resource::<action_types::PlayerPreview>().is_some());
    assert!(world.get_resource::<TurnManager>().is_some());
    assert!(world.get_resource::<MapMemory>().is_some());
    assert!(world.get_resource::<OccupancyMap>().is_some());
    assert!(world.get_resource::<EventLog>().is_some());
    assert!(world.get_resource::<FloorNumber>().is_some());
}

#[test]
fn test_global_world_startup() {
    let mut world = fresh_world();
    assert!(!world.resource::<TurnManager>().game_over);
    assert!(world.resource::<action_types::ActionQueue>().entries.is_empty());
    assert!(world.resource::<action_types::PlayerPreview>().kind.is_none());
    assert!(world.resource::<EventLog>().messages.is_empty());

    let result = world.run_system_once(crate::systems::fov_system);
    assert!(result.is_ok());
}

#[test]
fn test_player_preview_tap_tap() {
    let mut world = fresh_world();
    let player = world.query::<(Entity, &Player)>().iter(&world).next().map(|(e, _)| e).unwrap();

    world.resource_mut::<action_types::PlayerPreview>().kind = Some(action_types::ActionKindV3::Move { dx: 1, dy: 0 });
    assert!(matches!(world.resource::<action_types::PlayerPreview>().kind, Some(action_types::ActionKindV3::Move { dx: 1, dy: 0 })));

    let reaction = world.get::<action_types::Reaction>(player).unwrap().time;
    let duration = world.get::<action_types::CanMove>(player).unwrap().duration;
    let av = reaction + duration;
    world.resource_mut::<action_types::ActionQueue>().enqueue(
        player, action_types::ActionKindV3::Move { dx: 1, dy: 0 }, av,
    );
    world.resource_mut::<action_types::PlayerPreview>().kind = None;
    assert_eq!(world.resource::<action_types::ActionQueue>().entries.len(), 1);
}

#[test]
fn test_action_queue_advance() {
    let mut world = fresh_world();
    let player = world.query::<(Entity, &Player)>().iter(&world).next().map(|(e, _)| e).unwrap();
    let reaction_time = world.get::<action_types::Reaction>(player).unwrap().time;
    let duration = world.get::<action_types::CanMove>(player).unwrap().duration;
    let av = reaction_time + duration;

    world.resource_mut::<action_types::ActionQueue>().enqueue(
        player, action_types::ActionKindV3::Move { dx: 1, dy: 0 }, av,
    );
    world.resource_mut::<action_types::ActionQueue>().advance(av / 2.0);
    assert_eq!(world.resource::<action_types::ActionQueue>().entries.len(), 1);

    world.resource_mut::<action_types::ActionQueue>().advance(av);
    let ready = world.resource_mut::<action_types::ActionQueue>().pop_ready();
    assert_eq!(ready.len(), 1);
}

#[test]
fn test_conditions() {
    assert!(action_types::CanMove::condition(true, false));
    assert!(action_types::CanMove::condition(true, true));
    assert!(!action_types::CanMove::condition(false, false));
    assert!(action_types::CanFlee::condition(0.2));
    assert!(!action_types::CanFlee::condition(0.5));
    assert!(action_types::CanChase::condition(true));
    assert!(!action_types::CanChase::condition(false));
    assert!(action_types::CanWander::condition());
}

#[test]
fn test_reaction_from_agility() {
    let r10 = action_types::agility_to_reaction(10);
    let r20 = action_types::agility_to_reaction(20);
    assert!(r20 < r10);
    assert!(r10 >= 20.0);
    assert!(r10 <= 100.0);
}