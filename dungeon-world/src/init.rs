//! World 初始化与下楼

use dungeon_core::{
    components::*, items::*, resources::*,
    Map,
    ActionQueue, InputBuffer, PlayerPreview,
    ChaseIntents, FleeIntents, WanderIntents,
    Reaction, agility_to_reaction,
    CanMove, CanChase, CanFlee, CanWander, CanWait,
};
use bevy_ecs::prelude::*;
use rand::{RngExt, SeedableRng};

/// 创建并初始化游戏世界
pub fn setup_world() -> World {
    ItemRegistry::load();

    let mut world = World::new();
    let map_seed: u64 = rand::random();
    let mut rng = rand::rngs::SmallRng::seed_from_u64(map_seed);
    let mut map = Map::new();
    map.generate(&mut rng);

    world.insert_resource(MapSeed(map_seed));
    world.insert_resource(MapMemory::new());
    world.insert_resource(OccupancyMap::new());
    world.insert_resource(PendingExp::default());
    world.insert_resource(EventLog::new());
    world.insert_resource(GameRng { rng: rand::rngs::SmallRng::seed_from_u64(0) });
    world.insert_resource(TurnManager::new());
    world.insert_resource(FloorNumber(1));
    world.insert_resource(PendingLevelUp::default());
    world.insert_resource(VisibleMemory::default());
    world.insert_resource(ActionQueue::default());
    world.insert_resource(InputBuffer::default());
    world.insert_resource(PlayerPreview::default());
    world.insert_resource(ChaseIntents::default());
    world.insert_resource(FleeIntents::default());
    world.insert_resource(WanderIntents::default());

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
    cmd.insert(Reaction { time: agility_to_reaction(player_agi) });
    cmd.insert(CanMove::new(100));
    cmd.insert(CanWait::new(0));
    cmd.insert(dungeon_core::Skills { list: pc.skills() });

    // ── 概率生成怪物 ────────────────────────────
    let (room_centers, spawn_base) = {
        let m = world.resource::<Map>();
        let centers: Vec<(usize, usize)> = m.rooms.iter().skip(1).map(|r| r.center()).collect();
        let (sx, sy) = m.rooms[0].center();
        (centers, (sx, sy))
    };
    let kinds = dungeon_core::monster_def::roll_monster_kinds(12, 1, &mut rng);
    // 先在房间中心放，不够则找随机可行走格
    for (i, &kind) in kinds.iter().enumerate() {
        let (mx, my) = if let Some(&c) = room_centers.get(i) { c } else {
            let map_tiles = world.resource::<Map>().tiles;
            let mut pos = None;
            for _ in 0..200 {
                let tx = rng.random_range(3..dungeon_core::MAP_WIDTH - 3);
                let ty = rng.random_range(3..dungeon_core::MAP_HEIGHT - 3);
                if map_tiles[ty][tx].walkable() && (tx.abs_diff(spawn_base.0) + ty.abs_diff(spawn_base.1)) > 6 {
                    pos = Some((tx, ty)); break;
                }
            }
            pos.unwrap_or((10 + i * 3, 10 + i * 3))
        };
            let glyph = dungeon_core::monster_def::monster_glyph(kind);
            let color = dungeon_core::monster_def::monster_color(kind);
            let mon_agi = dungeon_core::monster_def::monster_stats(kind, 1).agility;
            let loot = dungeon_core::monster_def::monster_loot(kind);
            let attk = dungeon_core::monster_def::monster_attack_name(kind);
            let name = dungeon_core::monster_def::monster_name(kind);
            let mut cmd = world.spawn((
                Monster, Position { x: mx, y: my }, Renderable { glyph, color },
                Viewshed { range: 10, visible_tiles: Vec::new() },
                dungeon_core::monster_def::monster_stats(kind, 1), EntityName(name.into()),
                AttackName(attk.into()), loot,
            ));
            cmd.insert(Reaction { time: agility_to_reaction(mon_agi) });
            cmd.insert(CanChase::new(100));
            cmd.insert(CanFlee::new(200));
            cmd.insert(CanWander::new(50));
            cmd.insert(CanWait::new(0));
        }

    {
        let m = world.resource::<Map>();
        let (spx, spy) = m.rooms[0].center();
        let (sx, sy) = m.rooms.iter()
            .map(|r| (r.center(), r.center().0.abs_diff(spx) + r.center().1.abs_diff(spy)))
            .max_by_key(|(_, d)| *d)
            .map(|(p, _)| p)
            .unwrap_or(m.rooms[0].center());
        world.spawn((Stairs, Position { x: sx, y: sy }, Renderable { glyph: '>', color: (0, 255, 0) }));
    }

    // ── 地面物品（独立于怪物数量，使用房间中心）──
    let ground_item_ids = [0, 1, 2, 3, 0, 1, 3, 2];
    for (i, &item_id) in ground_item_ids.iter().enumerate() {
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

/// 下楼：生成新楼层
pub fn descend(world: &mut World) {
    let w = world;
    let mut floor = w.resource_mut::<FloorNumber>();
    floor.0 += 1; let f = floor.0;

    let player_data = {
        let mut q = w.query::<(Entity, &Stats, &Position, &Inventory, &Equipment, &dungeon_core::Skills, &PlayerClass, &AttackName)>();
        let (e, s, p, inv, eq, sk, cls, atk) = q.iter(&mut *w).next().unwrap();
        (e, s.clone(), *p, inv.stacks.clone(), inv.capacity,
         dungeon_core::Equipment { weapon: eq.weapon.clone(), armor: eq.armor.clone(), ring: eq.ring.clone() },
         sk.list.clone(), Buffs::new(), cls.clone(), atk.0.clone())
    };

    let to_despawn: Vec<Entity> = { let mut q = w.query::<(Entity,)>();
        q.iter(&mut *w).map(|(e,)| e).collect() };
    for e in to_despawn { let _ = w.despawn(e); }

    let base_seed = w.resource::<MapSeed>().0;
    let mut rng = rand::rngs::SmallRng::seed_from_u64(base_seed.wrapping_add(f as u64));
    let mut map = Map::new(); map.generate(&mut rng);
    w.insert_resource(map); w.insert_resource(MapMemory::new());
    let spawn = { let m = w.resource::<Map>(); m.rooms[0].center() };

    let mut cmd = w.spawn((
        Player, Position { x: spawn.0, y: spawn.1 },
        Renderable { glyph: '@', color: (255, 255, 0) }, MovingDir::default(),
        Viewshed { range: 10, visible_tiles: Vec::new() },
        player_data.1.clone(), EntityName("冒险者".into()),
        Inventory { stacks: player_data.3, capacity: player_data.4 },
    ));
    cmd.insert(player_data.5);  // Equipment
    cmd.insert(dungeon_core::Skills { list: player_data.6 });
    cmd.insert(player_data.7);  // Buffs
    cmd.insert(player_data.8.clone());
    cmd.insert(AttackName(player_data.9.clone()));
    cmd.insert(Reaction { time: agility_to_reaction(player_data.1.agility) });
    cmd.insert(CanMove::new(100));
    cmd.insert(CanWait::new(0));

    let stairs_pos = {
        let m = w.resource::<Map>();
        let (spx, spy) = m.rooms[0].center();
        m.rooms.iter()
            .map(|r| (r.center(), r.center().0.abs_diff(spx) + r.center().1.abs_diff(spy)))
            .max_by_key(|(_, d)| *d)
            .map(|(p, _)| p)
            .unwrap_or(m.rooms[0].center())
    };
    w.spawn((Stairs, Position { x: stairs_pos.0, y: stairs_pos.1 },
        Renderable { glyph: '>', color: (0, 255, 0) }));

    // ── 概率生成怪物（楼层 f）──────────────────
    let (room_centers, spawn_base, map) = {
        let m = w.resource::<Map>();
        let centers: Vec<(usize, usize)> = m.rooms.iter().skip(1).map(|r| r.center()).collect();
        let (sx, sy) = m.rooms[0].center();
        let map = m.tiles;
        (centers, (sx, sy), map)
    };
    let kinds = dungeon_core::monster_def::roll_monster_kinds(12, f, &mut rng);
    for (i, &kind) in kinds.iter().enumerate() {
        let (mx, my) = if let Some(&c) = room_centers.get(i) { c } else {
            let mut pos = None;
            for _ in 0..200 {
                let tx = rng.random_range(3..dungeon_core::MAP_WIDTH - 3);
                let ty = rng.random_range(3..dungeon_core::MAP_HEIGHT - 3);
                if map[ty][tx].walkable() && (tx.abs_diff(spawn_base.0) + ty.abs_diff(spawn_base.1)) > 6 {
                    pos = Some((tx, ty)); break;
                }
            }
            pos.unwrap_or((10 + i * 3, 10 + i * 3))
        };
            let glyph = dungeon_core::monster_def::monster_glyph(kind);
            let color = dungeon_core::monster_def::monster_color(kind);
            let mon_agi = dungeon_core::monster_def::monster_stats(kind, f).agility;
            let loot = dungeon_core::monster_def::monster_loot(kind);
            let attk = dungeon_core::monster_def::monster_attack_name(kind);
            let name = dungeon_core::monster_def::monster_name(kind);
            let mut cmd = w.spawn((
                Monster, Position { x: mx, y: my }, Renderable { glyph, color },
                Viewshed { range: 10, visible_tiles: Vec::new() },
                dungeon_core::monster_def::monster_stats(kind, f), EntityName(name.into()),
                AttackName(attk.into()), loot,
            ));
            cmd.insert(Reaction { time: agility_to_reaction(mon_agi) });
            cmd.insert(CanChase::new(100));
            cmd.insert(CanFlee::new(200));
            cmd.insert(CanWander::new(50));
            cmd.insert(CanWait::new(0));
        }

    let ground_item_ids = [0, 1, 2, 3, 0, 1, 3, 2];
    for (i, &item_id) in ground_item_ids.iter().enumerate() {
        if let Some(&(ix, iy)) = room_centers.get(i) {
            let def = ItemRegistry::global().get(item_id).unwrap();
            w.spawn((
                ItemPickup { stack: ItemStack::new(item_id, 1) },
                Position { x: ix + 1, y: iy },
                Renderable { glyph: def.glyph, color: def.color },
            ));
        }
    }
    w.resource_mut::<EventLog>().push(format!("=== 第 {} 层 ===", f));
}
