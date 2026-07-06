use bevy_ecs::prelude::*;
use rand::SeedableRng;
use crate::{
    components::*, items::*, resources::*,
    MAP_HEIGHT, MAP_WIDTH, Tile, Map,
};

use crate::world;

pub fn exp_to_next_level(level: u32) -> u64 {
    (25.0 * (level as f64).powf(1.5) + 10.0 * level as f64) as u64
}
pub fn max_hp_for(level: u32, defense: u32) -> i32 { 20 + level as i32 * 5 + defense as i32 * 2 }
pub fn max_mp_for(level: u32, mastery: u32) -> i32 { 5 + level as i32 * 3 + mastery as i32 }


// ── effective_attack / effective_defense ────────────
// (equipment_bonus 已在 items.rs 中定义)

pub fn effective_attack(stats: &Stats, inv: &Inventory, equip: &Equipment, buffs: Option<&Buffs>) -> u32 {
    let bonus = equipment_bonus(inv, equip);
    let mut atk = (stats.attack as i32) + bonus.attack;
    if let Some(b) = buffs { atk += b.berserk_atk; }
    atk.max(1) as u32
}

pub fn effective_defense(stats: &Stats, inv: &Inventory, equip: &Equipment, buffs: Option<&Buffs>) -> u32 {
    let bonus = equipment_bonus(inv, equip);
    let mut def = (stats.defense as i32) + bonus.defense;
    if let Some(b) = buffs { def += b.shield_def; }
    def.max(0) as u32
}

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
        map.tiles[pos.1 as usize][pos.0 as usize] == Tile::Wall
    };

    let mut mark_visible = |pos: (isize, isize)| {
        let dx = pos.0 - origin.0;
        let dy = pos.1 - origin.1;
        if dx * dx + dy * dy <= r2 {
            visible.push((pos.0 as usize, pos.1 as usize));
        }
    };

    compute_fov(origin, &mut is_blocking, &mut mark_visible);
    visible
}

// ── World 操作 ──────────────────────────────────────

pub fn update_map_memory() {
    let visible: Vec<(usize, usize)> = {
        let mut w = world!(mut);
        let mut q = w.query::<(&Player, &Viewshed)>();
        q.iter(&mut *w).next().map(|(_, v)| v.visible_tiles.clone()).unwrap_or_default()
    };
    let mut w = world!(mut);
    let mut memory = w.resource_mut::<MapMemory>();
    for &(x, y) in &visible { memory.explored[y][x] = true; }
}

/// 更新可见实体记忆（记录视野内的实体，用于视野外灰色显示）
pub fn update_visible_memory() {
    let player_visible: std::collections::HashSet<(usize, usize)>;
    let entities: Vec<(Entity, usize, usize, char, (u8, u8, u8))>;
    {
        let mut w = world!(mut);
        player_visible = {
            let mut q = w.query::<(&Player, &Viewshed)>();
            q.iter(&mut *w).next()
                .map(|(_, v)| v.visible_tiles.iter().copied().collect())
                .unwrap_or_default()
        };
        entities = {
            let mut q = w.query::<(Entity, &Position, &Renderable)>();
            q.iter(&mut *w)
                .filter(|(_, pos, _)| player_visible.contains(&(pos.x, pos.y)))
                .map(|(e, pos, rend)| (e, pos.x, pos.y, rend.glyph, rend.color))
                .collect()
        };
    }
    let mut w = world!(mut);
    // 移除已不存在的实体（死亡/消失）
    let alive: std::collections::HashSet<Entity> = {
        let mut q = w.query::<(Entity,)>();
        q.iter(&mut *w).map(|(e,)| e).collect()
    };
    let mut memory = w.resource_mut::<VisibleMemory>();
    for (entity, x, y, glyph, color) in entities {
        memory.entries.insert(entity, (x, y, glyph, color));
    }
    memory.entries.retain(|&e, _| alive.contains(&e));
}

pub fn rebuild_occupancy() {
    let mut w = world!(mut);
    // 收集不可通行的实体（排除 ItemPickup 和 Stairs）
    let positions: Vec<(Entity, usize, usize)> = {
        let mut q = w.query::<(Entity, &Position, Option<&ItemPickup>, Option<&Stairs>)>();
        q.iter(&mut *w)
            .filter(|(_, _, pickup, stairs)| pickup.is_none() && stairs.is_none())
            .map(|(e, p, _, _)| (e, p.x, p.y)).collect()
    };
    let mut occupancy = w.resource_mut::<OccupancyMap>();
    occupancy.clear();
    for (entity, x, y) in positions { occupancy.set(x, y, entity); }
}

pub fn set_player_dir(dx: isize, dy: isize) {
    let mut w = world!(mut);
    let mut query = w.query::<&mut MovingDir>();
    for mut dir in query.iter_mut(&mut *w) { dir.dx = dx; dir.dy = dy; }
}

pub fn collect_renderables() -> Vec<(usize, usize, char, RgbColor)> {
    let w = world!();
    let mut query = w.try_query::<(&Position, &Renderable)>().unwrap();
    let mut v: Vec<(usize, usize, char, RgbColor)> = query.iter(&w)
        .map(|(pos, rend)| (pos.x, pos.y, rend.glyph, rend.color)).collect();
    // 玩家 (@) 最后渲染，确保显示在最上层
    v.sort_by_key(|(_, _, glyph, _)| if *glyph == '@' { 1 } else { 0 });
    v
}

// ── 工具函数：给怪物添加 LootTable ─────────────────

pub(crate) fn rat_loot() -> LootTable {
    LootTable {
        entries: vec![
            LootEntry { item_id: 10, chance: 1.0, min_count: 1, max_count: 2 }, // 生物血肉
        ],
    }
}

pub(crate) fn goblin_loot() -> LootTable {
    LootTable {
        entries: vec![
            LootEntry { item_id: 10, chance: 1.0, min_count: 1, max_count: 3 }, // 生物血肉
            LootEntry { item_id: 11, chance: 0.6, min_count: 1, max_count: 1 }, // 破布
            LootEntry { item_id: 12, chance: 0.4, min_count: 1, max_count: 1 }, // 坚硬木棍
            LootEntry { item_id: 13, chance: 0.3, min_count: 1, max_count: 1 }, // 染血兽牙
        ],
    }
}

// ── setup_world ─────────────────────────────────────

pub fn setup_world() -> World {
    ItemRegistry::load(); // 初始化全局物品注册表

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
    world.insert_resource(crate::action::ActionQueue::default());
    world.insert_resource(crate::action::InputBuffer::default());
    world.insert_resource(crate::action::PlayerPreview::default());

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
    cmd.insert(crate::action::Reaction { time: crate::action::agility_to_reaction(player_agi) });
    cmd.insert(crate::action::CanMove::new(100));
    cmd.insert(crate::action::CanWait::new(0));
    cmd.insert(Skills { list: pc.skills() });

    let monster_templates: [(char, RgbColor, &str); 4] = [
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
            cmd.insert(crate::action::Reaction { time: crate::action::agility_to_reaction(mon_agi) });
            cmd.insert(crate::action::CanChase::new(100));
            cmd.insert(crate::action::CanFlee::new(200));
            cmd.insert(crate::action::CanWander::new(50));
            cmd.insert(crate::action::CanWait::new(0));
        }
    }

    {
        let m = world.resource::<Map>();
        let last = m.rooms.len() - 1;
        let (sx, sy) = m.rooms[last].center();
        world.spawn((Stairs, Position { x: sx, y: sy }, Renderable { glyph: '>', color: (0, 255, 0) }));
    }

    // 地面物品（使用 ItemStack + ItemRegistry）
    let ground_item_ids = [0, 1, 2, 3]; // 锈铁剑, 木盾, 皮甲, 攻击戒指
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

// ── descend ─────────────────────────────────────────

pub fn descend() {
    let mut w = world!(mut);
    let mut floor = w.resource_mut::<FloorNumber>();
    floor.0 += 1; let f = floor.0;

    let player_data = {
        let mut q = w.query::<(Entity, &Stats, &Position, &Inventory, &Equipment, &Skills, &PlayerClass, &AttackName)>();
        let (e, s, p, inv, eq, sk, cls, atk) = q.iter(&mut *w).next().unwrap();
        (e, s.clone(), *p, inv.stacks.clone(), inv.capacity,
         Equipment { weapon: eq.weapon.clone(), armor: eq.armor.clone(), ring: eq.ring.clone() },
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
    cmd.insert(Skills { list: player_data.6 });
    cmd.insert(player_data.7);  // Buffs
    cmd.insert(player_data.8.clone());
    cmd.insert(AttackName(player_data.9.clone()));
    cmd.insert(crate::action::Reaction { time: crate::action::agility_to_reaction(player_data.1.agility) });
    cmd.insert(crate::action::CanMove::new(100));
    cmd.insert(crate::action::CanWait::new(0));

    let last_room = { let m = w.resource::<Map>(); let idx = m.rooms.len() - 1; m.rooms[idx].center() };
    w.spawn((Stairs, Position { x: last_room.0, y: last_room.1 },
        Renderable { glyph: '>', color: (0, 255, 0) }));

    let monster_templates: [(char, RgbColor, &str); 4] = [
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
            cmd.insert(crate::action::Reaction { time: crate::action::agility_to_reaction(mon_agi) });
            cmd.insert(crate::action::CanChase::new(100));
            cmd.insert(crate::action::CanFlee::new(200));
            cmd.insert(crate::action::CanWander::new(50));
            cmd.insert(crate::action::CanWait::new(0));
        }
    }

    // 地面物品（使用 ItemStack + ItemRegistry）
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

// ── 工具函数 ────────────────────────────────────────

/// 获取玩家实体
pub fn player_entity() -> Option<Entity> {
    let w = world!();
    let mut q = w.try_query::<(Entity, &Player)>().unwrap();
    q.iter(&w).next().map(|(e, _)| e)
}

/// 判断玩家是否站在楼梯上
pub fn on_stairs() -> bool {
    let w = world!();
    let pp = *w.try_query::<&Position>().unwrap().iter(&w).next().unwrap_or(&Position { x: 0, y: 0 });
    let mut q2 = w.try_query::<(&Stairs, &Position)>().unwrap();
    q2.iter(&w).any(|(_, sp)| sp.x == pp.x && sp.y == pp.y)
}

/// 拾取玩家所在格的全部地面物品
pub fn pickup_ground() {
    let (ppx, ppy) = {
        let w = world!();
        let mut q = w.try_query::<(&Player, &Position)>().unwrap();
        q.iter(&w).next().map(|(_, p)| (p.x, p.y)).unwrap_or((0, 0))
    };
    let ground: Vec<(Entity, ItemStack)> = {
        let w = world!();
        let mut q = w.try_query::<(Entity, &ItemPickup, &Position)>().unwrap();
        q.iter(&w)
            .filter(|(_, _, pos)| pos.x == ppx && pos.y == ppy)
            .map(|(e, p, _)| (e, p.stack.clone()))
            .collect()
    };
    if ground.is_empty() { return; }
    let mut logs = Vec::new();
    let mut despawn = Vec::new();
    for (entity, stack) in &ground {
        let mut w = world!(mut);
        let mut q = w.query::<(&mut Inventory,)>();
        if let Some((mut inv,)) = q.iter_mut(&mut *w).next() {
            let leftover = inv.add(stack.item_id, stack.count);
            let picked = stack.count - leftover;
            if picked > 0 { logs.push(format!("拾取了{}x{}", stack.name(), picked)); }
            despawn.push(*entity);
        }
    }
    for e in despawn { let mut w = world!(mut); w.entity_mut(e).despawn(); }
    for msg in logs { let mut w = world!(mut); w.resource_mut::<EventLog>().push(msg); }
}
