//! World 初始化与下楼

use dungeon_core::{
    components::*, items::*, resources::*,
    Map, world,
    ActionQueue, InputBuffer, PlayerPreview,
    ChaseIntents, FleeIntents, WanderIntents,
    Reaction, agility_to_reaction,
    CanMove, CanChase, CanFlee, CanWander, CanWait,
};
use crate::loot::{rat_loot, goblin_loot};
use bevy_ecs::prelude::*;
use rand::SeedableRng;

/// 创建并初始化游戏世界
pub fn setup_world() -> World {
    ItemRegistry::load();

    let mut world = World::new();
    let mut rng = rand::rngs::SmallRng::seed_from_u64(42);
    let mut map = Map::new();
    map.generate(&mut rng);

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
        Viewshed { range: 8, visible_tiles: Vec::new() },
        Stats::player(), EntityName("冒险者".into()),
        Inventory::new(36), Equipment::new(), Buffs::new(),
        pc.clone(), AttackName("斩击".into()),
    ));
    cmd.insert(Reaction { time: agility_to_reaction(player_agi) });
    cmd.insert(CanMove::new(100));
    cmd.insert(CanWait::new(0));
    cmd.insert(dungeon_core::Skills { list: pc.skills() });

    let monster_templates: [(char, (u8, u8, u8), &str); 4] = [
        ('r', (255, 0, 0), "老鼠"), ('g', (0, 255, 0), "哥布林"),
        ('r', (255, 128, 128), "老鼠"), ('g', (144, 238, 144), "哥布林"),
    ];
    let spawn_points: Vec<(usize, usize)> = {
        let map_ref = world.resource::<Map>();
        monster_templates.iter().enumerate()
            .filter_map(|(i, _)| map_ref.rooms.get(i + 1).map(|r| r.center())).collect()
    };

    for (i, &(glyph, color, mon_name)) in monster_templates.iter().enumerate() {
        if let Some(&(mx, my)) = spawn_points.get(i) {
            let mon_agi = Stats::monster(glyph, 1).agility;
            let loot = if glyph == 'g' { goblin_loot() } else { rat_loot() };
            let mut cmd = world.spawn((
                Monster, Position { x: mx, y: my }, Renderable { glyph, color },
                Viewshed { range: 8, visible_tiles: Vec::new() },
                Stats::monster(glyph, 1), EntityName(mon_name.into()),
                AttackName(if glyph == 'r' { "撕咬" } else { "重击" }.into()),
                loot,
            ));
            cmd.insert(Reaction { time: agility_to_reaction(mon_agi) });
            cmd.insert(CanChase::new(100));
            cmd.insert(CanFlee::new(200));
            cmd.insert(CanWander::new(50));
            cmd.insert(CanWait::new(0));
        }
    }

    {
        let m = world.resource::<Map>();
        let last = m.rooms.len() - 1;
        let (sx, sy) = m.rooms[last].center();
        world.spawn((Stairs, Position { x: sx, y: sy }, Renderable { glyph: '>', color: (0, 255, 0) }));
    }

    let ground_item_ids = [0, 1, 2, 3];
    for (i, &item_id) in ground_item_ids.iter().enumerate() {
        if let Some(&(ix, iy)) = spawn_points.get(i) {
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
pub fn descend() {
    let mut w = world!(mut);
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

    let mut rng = rand::rngs::SmallRng::seed_from_u64(42 + f as u64);
    let mut map = Map::new(); map.generate(&mut rng);
    w.insert_resource(map); w.insert_resource(MapMemory::new());
    let spawn = { let m = w.resource::<Map>(); m.rooms[0].center() };

    let mut cmd = w.spawn((
        Player, Position { x: spawn.0, y: spawn.1 },
        Renderable { glyph: '@', color: (255, 255, 0) }, MovingDir::default(),
        Viewshed { range: 8, visible_tiles: Vec::new() },
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

    let last_room = { let m = w.resource::<Map>(); let idx = m.rooms.len() - 1; m.rooms[idx].center() };
    w.spawn((Stairs, Position { x: last_room.0, y: last_room.1 },
        Renderable { glyph: '>', color: (0, 255, 0) }));

    let monster_templates: [(char, (u8, u8, u8), &str); 4] = [
        ('r', (255, 0, 0), "老鼠"), ('g', (0, 255, 0), "哥布林"),
        ('r', (255, 128, 128), "老鼠"), ('g', (144, 238, 144), "哥布林"),
    ];
    let spawn_points: Vec<(usize, usize)> = {
        let m = w.resource::<Map>();
        monster_templates.iter().enumerate()
            .filter_map(|(i, _)| m.rooms.get(i + 1).map(|r| r.center())).collect()
    };
    for (i, &(glyph, color, mon_name)) in monster_templates.iter().enumerate() {
        if let Some(&(mx, my)) = spawn_points.get(i) {
            let mon_agi = Stats::monster(glyph, f).agility;
            let loot = if glyph == 'g' { goblin_loot() } else { rat_loot() };
            let mut cmd = w.spawn((
                Monster, Position { x: mx, y: my }, Renderable { glyph, color },
                Viewshed { range: 8, visible_tiles: Vec::new() },
                Stats::monster(glyph, f), EntityName(mon_name.into()),
                AttackName(if glyph == 'r' { "撕咬" } else { "重击" }.into()),
                loot,
            ));
            cmd.insert(Reaction { time: agility_to_reaction(mon_agi) });
            cmd.insert(CanChase::new(100));
            cmd.insert(CanFlee::new(200));
            cmd.insert(CanWander::new(50));
            cmd.insert(CanWait::new(0));
        }
    }

    let ground_item_ids = [0, 1, 2, 3];
    for (i, &item_id) in ground_item_ids.iter().enumerate() {
        if let Some(&(ix, iy)) = spawn_points.get(i) {
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
