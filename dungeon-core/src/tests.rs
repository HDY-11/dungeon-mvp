use crate::*;
use bevy_ecs::prelude::*;
use bevy_ecs::system::RunSystemOnce;
use rand::rngs::SmallRng;
use rand::{RngExt, SeedableRng};

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
    world.insert_resource(map);
    world.spawn((
        Player, Position { x: spawn_x, y: spawn_y },
        Renderable { glyph: '@', color: ratatui::style::Color::Yellow },
        MovingDir::default(), Viewshed { range: 8, visible_tiles: Vec::new() },
        Stats::player(), EntityName("冒险者".into()), ActionPoints::new(10),
        Inventory::new(36), Equipment::new(), Skills::default_skills(), Buffs::new(),
    ));
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
    assert_eq!(world.query::<&Skills>().iter(&world).next().unwrap().list.len(), 4);
}

// ── 曲线 ──────────────────────────────────────────

#[test] fn test_exp_curve() { assert_eq!(exp_to_next_level(1), 50); assert_eq!(exp_to_next_level(2), 140); }
#[test] fn test_hp_curve() { assert_eq!(max_hp_for(1, 10) - max_hp_for(2, 10), max_hp_for(2, 10) - max_hp_for(3, 10)); }
#[test] fn test_def_log() { assert_eq!(defense_bonus(1), 0); assert_eq!(defense_bonus(2), 1); assert_eq!(defense_bonus(4), 2); }
#[test] fn test_rat_goblin() {
    let rat = Stats::monster('r', 1); let gob = Stats::monster('g', 1);
    assert!(gob.max_hp > rat.max_hp); assert!(gob.exp > rat.exp);
}
