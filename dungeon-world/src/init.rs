//! World 初始化与下楼

use dungeon_core::{
    components::*, items::*, resources::*,
    Map, MAP_WIDTH, MAP_HEIGHT,
};
use dungeon_action::{
    ActionQueue, InputBuffer, PlayerPreview,
    ChaseIntents, FleeIntents, WanderIntents,
    Reaction, agility_to_reaction,
    CanMove, CanChase, CanFlee, CanWander, CanWait,
};
use bevy_ecs::prelude::*;
use rand::{Rng, SeedableRng};

// ══════════════════════════════════════════════════════
// 共享辅助函数（setup_world 与 descend 共用）
// ══════════════════════════════════════════════════════

/// 在 walkable 格上放置怪物种群，避开 exclude 坐标
fn spawn_monsters(world: &mut World, floor: u32, rng: &mut impl Rng, exclude: &[(usize, usize)]) {
    let tiles = world.resource::<Map>().tiles;
    let population = crate::population::generate_monster_population(&tiles, floor, rng, exclude);
    for &(kind, mx, my) in &population {
        let glyph = dungeon_core::monster_def::monster_glyph(kind);
        let color = dungeon_core::monster_def::monster_color(kind);
        let mon_agi = dungeon_core::monster_def::monster_stats(kind, floor).agility;
        let loot = dungeon_core::monster_def::monster_loot(kind);
        let attk = dungeon_core::monster_def::monster_attack_name(kind);
        let name = dungeon_core::monster_def::monster_name(kind);
        let mut cmd = world.spawn((
            Monster, Position { x: mx, y: my }, Renderable { glyph, color },
            Viewshed { range: 10, visible_tiles: Vec::new() },
            dungeon_core::monster_def::monster_stats(kind, floor), EntityName(name.into()),
            AttackName(attk.into()), loot,
        ));
        let entity = cmd.id();
        cmd.insert(Reaction { time: agility_to_reaction(mon_agi) });
        cmd.insert(LastKnownPlayerPos::default());
        cmd.insert(CanChase::new(100));
        cmd.insert(CanFlee::new(200));
        cmd.insert(CanWander::new(50));
        cmd.insert(CanWait::new(0));
        // I35: 将独特色写入 Renderable.color，持久化后跨存档/下楼一致
        if let Some(mut rend) = world.get_mut::<Renderable>(entity) {
            rend.color = dungeon_core::color::entity_color(entity.to_bits(), 0);
        }
    }
}

/// 在地图中放置地面物品。
/// 优先使用非出生房间的中心，若仅有 1 个房间则退回到出生房间内偏移放置。
fn place_ground_items(world: &mut World, item_ids: &[usize], exclude: &[(usize, usize)]) {
    use rand::RngExt;
    let room_centers: Vec<(usize, usize)> = {
        let rooms = &world.resource::<Map>().rooms;
        if rooms.len() > 1 {
            rooms.iter().skip(1).map(|r| r.center()).collect()
        } else {
            // I16: 单房间时从房间内随机找偏离中心的 walkable 格
            let r = &rooms[0];
            let mut alt = Vec::new();
            let mut rng2 = rand::rngs::SmallRng::seed_from_u64(42);
            for _ in 0..20 {
                let ox = rng2.random_range(2..r.w.saturating_sub(2));
                let oy = rng2.random_range(2..r.h.saturating_sub(2));
                let px = r.x + ox;
                let py = r.y + oy;
                if px < MAP_WIDTH && py < MAP_HEIGHT
                    && world.resource::<Map>().tiles[py][px].walkable()
                    && !exclude.contains(&(px, py))
                {
                    alt.push((px, py));
                }
            }
            alt
        }
    };

    let item_count = room_centers.len().min(item_ids.len());
    for (i, &item_id) in item_ids[..item_count].iter().enumerate() {
        if let Some(&(ix, iy)) = room_centers.get(i) {
            let def = ItemRegistry::global().get(item_id).expect("item_id exists in registry");
            world.spawn((
                ItemPickup { stack: ItemStack::new(item_id, 1) },
                Position { x: ix + 1, y: iy },
                Renderable { glyph: def.glyph, color: def.color },
            ));
        }
    }
}

/// 选择楼梯位置：尽量远离 spawn_pos，至少 15 格。
/// 优先选最远房间。仅 1 个房间时用醉汉游走 60 步找 ≥15 格外的位置（G9）。
fn pick_stair_pos(map: &Map, spawn_pos: (usize, usize), rng: &mut impl Rng) -> (usize, usize) {
    use rand::RngExt;
    let (spx, spy) = spawn_pos;

    // 仅当有多个房间时才用最远房间；单房间时 farthest_room_from 返回 spawn 本身
    if map.rooms.len() > 1 {
        if let Some(best) = map.farthest_room_from(spawn_pos) {
            return best;
        }
    }

    // G9: 单房间 → 醉汉游走 60 步
    let (mut cx, mut cy) = (spx as isize, spy as isize);
    for _ in 0..60 {
        let dx = rng.random_range(-1i32..2) as isize;
        let dy = rng.random_range(-1i32..2) as isize;
        if dx == 0 && dy == 0 { continue; }
        cx = (cx + dx).clamp(0, MAP_WIDTH as isize - 1);
        cy = (cy + dy).clamp(0, MAP_HEIGHT as isize - 1);
        if (cx as usize).abs_diff(spx) + (cy as usize).abs_diff(spy) >= 15
            && map.tiles[cy as usize][cx as usize].walkable()
        {
            return (cx as usize, cy as usize);
        }
    }
    // 兜底：从 spawn 向外螺旋搜索最近的 walkable 格（至少 15 格）
    for r in 15..=40 {
        for dy in -(r as isize)..=r as isize {
            for dx in -(r as isize)..=r as isize {
                if dx == 0 && dy == 0 { continue; }
                let nx = spx.wrapping_add_signed(dx);
                let ny = spy.wrapping_add_signed(dy);
                if nx < MAP_WIDTH && ny < MAP_HEIGHT
                    && map.tiles[ny][nx].walkable()
                {
                    return (nx, ny);
                }
            }
        }
    }
    (spx + 15, spy)
}

// ══════════════════════════════════════════════════════
// 公共 API
// ══════════════════════════════════════════════════════

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
    world.insert_resource(GameRng::new(map_seed.wrapping_add(42)));
    world.insert_resource(TurnManager::new());
    world.insert_resource(FloorNumber(1));
    world.insert_resource(VisibleMemory::default());
    world.insert_resource(LookCursor { active: false, x: 0, y: 0 });
    world.insert_resource(ActionQueue::default());
    world.insert_resource(InputBuffer::default());
    world.insert_resource(PlayerPreview::default());
    world.insert_resource(ChaseIntents::default());
    world.insert_resource(FleeIntents::default());
    world.insert_resource(WanderIntents::default());

    let (spawn_x, spawn_y) = map.spawn_point();
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
    cmd.insert(ActiveBuffs::new());

    // ── 楼梯放置（避开出生点，G9） ──
    let stairs_pos = {
        let m = world.resource::<Map>();
        pick_stair_pos(m, (spawn_x, spawn_y), &mut rng)
    };
    world.spawn((Stairs, Position { x: stairs_pos.0, y: stairs_pos.1 },
        Renderable { glyph: '>', color: (0, 255, 0) }));
    {
        let mut map = world.resource_mut::<Map>();
        dungeon_core::map_gen::ensure_connection_between(&mut map, &mut rng, (spawn_x, spawn_y), (stairs_pos.0, stairs_pos.1));
    }

    // ── 怪物生成（排除楼梯和出生点，G10） ──
    spawn_monsters(&mut world, 1, &mut rng, &[(spawn_x, spawn_y), (stairs_pos.0, stairs_pos.1)]);

    // ── 地面物品 ──
    let ground_item_ids = [0, 1, 2, 3, 0, 1, 3, 2];
    place_ground_items(&mut world, &ground_item_ids, &[(spawn_x, spawn_y), (stairs_pos.0, stairs_pos.1)]);

    world
}

/// 下楼：生成新楼层
pub fn descend(world: &mut World) {
    let w = world;
    let mut floor = w.resource_mut::<FloorNumber>();
    floor.0 += 1; let f = floor.0;

    let player_data = {
        let mut q = w.query::<(Entity, &Stats, &Inventory, &Equipment, &PlayerClass, &AttackName)>();
        let (e, s, inv, eq, cls, atk) = q.iter(&*w).next().expect("Player exists for descend");
        (e, s.clone(), inv.stacks.clone(), inv.capacity,
         dungeon_core::Equipment { weapon: eq.weapon.clone(), armor: eq.armor.clone(), ring: eq.ring.clone() },
         Buffs::new(), cls.clone(), atk.0.clone())
    };

    let to_despawn: Vec<Entity> = { let mut q = w.query::<(Entity,)>();
        q.iter(&*w).map(|(e,)| e).collect() };
    for e in to_despawn { let _ = w.despawn(e); }

    let base_seed = w.resource::<MapSeed>().0;
    let mut rng = rand::rngs::SmallRng::seed_from_u64(base_seed.wrapping_add(f as u64));
    let mut map = Map::new(); map.generate(&mut rng);
    w.insert_resource(map); w.insert_resource(MapMemory::new());

    // ── 重建玩家 ──
    let spawn = { w.resource::<Map>().spawn_point() };
    let mut cmd = w.spawn((
        Player, Position { x: spawn.0, y: spawn.1 },
        Renderable { glyph: '@', color: (255, 255, 0) }, MovingDir::default(),
        Viewshed { range: 10, visible_tiles: Vec::new() },
        player_data.1.clone(), EntityName("冒险者".into()),
        Inventory { stacks: player_data.2, capacity: player_data.3 },
    ));
    cmd.insert(player_data.4);  // Equipment
    cmd.insert(dungeon_core::Skills { list: player_data.6.skills() });
    cmd.insert(player_data.5);  // Buffs
    cmd.insert(player_data.6.clone());  // PlayerClass
    cmd.insert(AttackName(player_data.7.clone()));
    cmd.insert(ActiveBuffs::new());
    cmd.insert(Reaction { time: agility_to_reaction(player_data.1.agility) });
    cmd.insert(CanMove::new(100));
    cmd.insert(CanWait::new(0));

    // ── 楼梯放置（避开出生点，G9） ──
    let stairs_pos = {
        let m = w.resource::<Map>();
        pick_stair_pos(m, spawn, &mut rng)
    };
    w.spawn((Stairs, Position { x: stairs_pos.0, y: stairs_pos.1 },
        Renderable { glyph: '>', color: (0, 255, 0) }));
    {
        let mut map = w.resource_mut::<Map>();
        dungeon_core::map_gen::ensure_connection_between(&mut map, &mut rng, spawn, (stairs_pos.0, stairs_pos.1));
    }

    // ── 怪物生成（排除楼梯和出生点，G10） ──
    spawn_monsters(w, f, &mut rng, &[spawn, (stairs_pos.0, stairs_pos.1)]);

    // ── 地面物品 ──
    let ground_item_ids = [0, 1, 2, 3, 0, 1, 3, 2];
    place_ground_items(w, &ground_item_ids, &[spawn, (stairs_pos.0, stairs_pos.1)]);

    w.resource_mut::<EventLog>().push(format!("=== 第 {} 层 ===", f));
}
