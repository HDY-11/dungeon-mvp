use bevy_ecs::prelude::*;
use rand::SeedableRng;
use crate::{
    components::*, items::*, resources::*,
    MAP_HEIGHT, MAP_WIDTH, Map,
};

// exp_to_next_level, max_hp_for, max_mp_for, effective_attack, effective_defense
// 移至 ops.rs（通过 lib.rs pub use ops::* 导出）

// ── FOV ─────────────────────────────────────────────

pub fn calculate_visible_tiles(x: usize, y: usize, range: usize, map: &Map) -> Vec<(usize, usize)> {
    use symmetric_shadowcasting::compute_fov;
    let r2 = (range * range) as isize;
    let mut visible = Vec::new();
    let origin = (x as isize, y as isize);

    let mut is_blocking = |pos: (isize, isize)| {
        if pos.0 < 0 || pos.0 >= MAP_WIDTH as isize || pos.1 < 0 || pos.1 >= MAP_HEIGHT as isize {
            return true;
        }
        map.tiles[pos.1 as usize][pos.0 as usize].blocks_vision()
    };

    let mut mark_visible = |pos: (isize, isize)| {
        if pos.0 < 0 || pos.0 >= MAP_WIDTH as isize || pos.1 < 0 || pos.1 >= MAP_HEIGHT as isize {
            return;
        }
        let dx = pos.0 - origin.0;
        let dy = pos.1 - origin.1;
        if dx * dx + dy * dy <= r2 {
            visible.push((pos.0 as usize, pos.1 as usize));
        }
    };

    compute_fov(origin, &mut is_blocking, &mut mark_visible);
    visible
}

// (update_map_memory, update_visible_memory, rebuild_occupancy,
//  set_player_dir, collect_renderables 已移至 ops.rs)

// ── setup_world ─────────────────────────────────────

pub fn setup_world() -> World {
    ItemRegistry::load(); // 初始化全局物品注册表

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

    // ── 噪声+元胞生成怪物 ──────────────────────
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

    // 地面物品（使用房间中心位置）
    let room_centers: Vec<(usize, usize)> = world.resource::<Map>().rooms.iter().skip(1).map(|r| r.center()).collect();
    let ground_item_ids = [0, 1, 2, 3, 0, 1, 3, 2]; // 锈铁剑, 木盾, 皮甲, 攻击戒指 ×2
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

// descend 已移至 dungeon-world/init.rs
