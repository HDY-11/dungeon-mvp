use crate::{
    ai::MonsterBrain,
    components::*,
    items::*,
    resources::*,
    make_items, MAP_HEIGHT, MAP_WIDTH, Tile, Map, Room,
};
use bevy_ecs::prelude::*;
use ratatui::style::Color;
use rand::{RngExt, SeedableRng};

// ── 升级曲线 ────────────────────────────────────────

pub fn exp_to_next_level(level: u32) -> u64 { 20 * (level as u64).pow(2) + 30 * (level as u64) }
pub fn max_hp_for(level: u32, vitality: u32) -> i32 { 20 + level as i32 * 5 + vitality as i32 * 2 }
pub fn max_mp_for(level: u32, intelligence: u32) -> i32 { 5 + level as i32 * 3 + intelligence as i32 }
pub fn defense_bonus(level: u32) -> u32 { (level as f64).log2().floor() as u32 }

// ── 装备加成 ────────────────────────────────────────

pub fn equipment_bonus(inv: &Inventory, equip: &Equipment) -> StatBonus {
    let mut total = StatBonus::default();
    for &opt in [&equip.weapon, &equip.armor, &equip.ring] {
        if let Some(idx) = opt {
            if let Some(item) = inv.items.get(idx) {
                let b = &item.bonus;
                total.strength += b.strength; total.dexterity += b.dexterity;
                total.intelligence += b.intelligence; total.vitality += b.vitality;
                total.hp += b.hp; total.attack += b.attack; total.defense += b.defense;
            }
        }
    }
    total
}

pub fn effective_attack(stats: &Stats, inv: &Inventory, equip: &Equipment, buffs: Option<&Buffs>) -> u32 {
    let bonus = equipment_bonus(inv, equip);
    let mut atk = (stats.attack() as i32) + bonus.attack + bonus.strength;
    if let Some(b) = buffs { atk += b.berserk_atk; }
    atk.max(1) as u32
}

pub fn effective_defense(stats: &Stats, inv: &Inventory, equip: &Equipment, buffs: Option<&Buffs>) -> u32 {
    let bonus = equipment_bonus(inv, equip);
    let mut def = (stats.defense() as i32) + bonus.defense + bonus.vitality;
    if let Some(b) = buffs { def += b.shield_def; }
    def.max(0) as u32
}

// ── FOV 工具 ────────────────────────────────────────

fn has_line_of_sight(x0: usize, y0: usize, x1: usize, y1: usize, map: &Map) -> bool {
    let mut cx = x0 as isize; let mut cy = y0 as isize;
    let ex = x1 as isize; let ey = y1 as isize;
    let dx = (ex - cx).abs(); let dy = -(ey - cy).abs();
    let sx = if cx < ex { 1 } else { -1 }; let sy = if cy < ey { 1 } else { -1 };
    let mut err = dx + dy;
    loop {
        if (cx, cy) == (ex, ey) { return true; }
        if (cx, cy) != (x0 as isize, y0 as isize) {
            let ux = cx as usize; let uy = cy as usize;
            if ux >= MAP_WIDTH || uy >= MAP_HEIGHT { return false; }
            if map.tiles[uy][ux] == Tile::Wall { return false; }
        }
        let e2 = 2 * err;
        if e2 >= dy { err += dy; cx += sx; }
        if e2 <= dx { err += dx; cy += sy; }
    }
}

pub fn calculate_visible_tiles(x: usize, y: usize, range: usize, map: &Map) -> Vec<(usize, usize)> {
    let mut visible = Vec::new();
    let range_sq = (range as isize) * (range as isize);
    for dy in -(range as isize)..=range as isize {
        for dx in -(range as isize)..=range as isize {
            let tx = x.wrapping_add_signed(dx); let ty = y.wrapping_add_signed(dy);
            if tx >= MAP_WIDTH || ty >= MAP_HEIGHT { continue; }
            if dx * dx + dy * dy > range_sq { continue; }
            if (tx, ty) == (x, y) { visible.push((tx, ty)); continue; }
            if has_line_of_sight(x, y, tx, ty, map) { visible.push((tx, ty)); }
        }
    }
    visible
}

// ── World 操作 ──────────────────────────────────────

pub fn update_map_memory(world: &mut World) {
    let visible: Vec<(usize, usize)> = {
        let mut q = world.query::<(&Player, &Viewshed)>();
        q.iter(world).next().map(|(_, v)| v.visible_tiles.clone()).unwrap_or_default()
    };
    let mut memory = world.resource_mut::<MapMemory>();
    for &(x, y) in &visible { memory.explored[y][x] = true; }
}

pub fn rebuild_occupancy(world: &mut World) {
    let positions: Vec<(Entity, usize, usize)> = {
        let mut q = world.query::<(Entity, &Position)>();
        q.iter(world).map(|(e, p)| (e, p.x, p.y)).collect()
    };
    let mut occupancy = world.resource_mut::<OccupancyMap>();
    occupancy.clear();
    for (entity, x, y) in positions { occupancy.set(x, y, entity); }
}

pub fn set_player_dir(world: &mut World, dx: isize, dy: isize) {
    let mut query = world.query::<&mut MovingDir>();
    for mut dir in query.iter_mut(world) { dir.dx = dx; dir.dy = dy; }
}

pub fn collect_renderables(world: &mut World) -> Vec<(usize, usize, char, Color)> {
    let mut query = world.query::<(&Position, &Renderable)>();
    query.iter(world).map(|(pos, rend)| (pos.x, pos.y, rend.glyph, rend.color)).collect()
}

// ── setup_world ─────────────────────────────────────

pub fn setup_world() -> World {
    let mut world = World::new();
    let mut rng = rand::rngs::SmallRng::seed_from_u64(42);
    let mut map = Map::new();
    map.generate(&mut rng);

    world.insert_resource(MapMemory::new());
    world.insert_resource(OccupancyMap::new());
    world.insert_resource(PendingExp::default());
    world.insert_resource(PendingPickup::default());
    world.insert_resource(PendingSkill::default());
    world.insert_resource(EventLog::new());
    world.insert_resource(GameRng { rng: rand::rngs::SmallRng::seed_from_u64(0) });
    world.insert_resource(TurnManager::new());
    world.insert_resource(FloorNumber(1));
    world.insert_resource(PendingLevelUp::default());

    let (spawn_x, spawn_y) = map.rooms[0].center();
    world.insert_resource(map);
    let player_dex = 10;

    world.spawn((
        Player, Position { x: spawn_x, y: spawn_y },
        Renderable { glyph: '@', color: Color::Yellow }, MovingDir::default(),
        Viewshed { range: 8, visible_tiles: Vec::new() },
        Stats::player(), EntityName("冒险者".into()), ActionPoints::new(player_dex),
        Inventory::new(36), Equipment::new(), Skills::default_skills(), Buffs::new(),
    ));

    let monster_templates: [(char, Color, &str); 4] = [
        ('r', Color::Red, "老鼠"), ('g', Color::Green, "哥布林"),
        ('r', Color::LightRed, "老鼠"), ('g', Color::LightGreen, "哥布林"),
    ];
    let spawn_points: Vec<(usize, usize)> = {
        let map_ref = world.resource::<Map>();
        monster_templates.iter().enumerate()
            .filter_map(|(i, _)| map_ref.rooms.get(i + 1).map(|r| r.center())).collect()
    };

    for (i, &(glyph, color, mon_name)) in monster_templates.iter().enumerate() {
        if let Some(&(mx, my)) = spawn_points.get(i) {
            world.spawn((
                Monster, MonsterBrain::creature(),
                Position { x: mx, y: my }, Renderable { glyph, color },
                Viewshed { range: 5, visible_tiles: Vec::new() },
                Stats::monster(glyph, 1), EntityName(mon_name.into()),
                ActionPoints::new(Stats::monster(glyph, 1).dexterity),
                FleeLogState::default(),
            ));
        }
    }

    // 楼梯
    { let m = world.resource::<Map>(); let last = m.rooms.len() - 1;
        let (sx, sy) = m.rooms[last].center();
        world.spawn((Stairs, Position { x: sx, y: sy }, Renderable { glyph: '>', color: Color::Green })); }

    // 物品
    let items = make_items();
    for (i, item) in items.iter().enumerate() {
        if let Some(&(ix, iy)) = spawn_points.get(i) {
            world.spawn((ItemPickup { item: item.clone() }, Position { x: ix + 1, y: iy },
                Renderable { glyph: item.glyph, color: item.color })); }
    }
    world
}

// ── descend ─────────────────────────────────────────

pub fn descend(world: &mut World) {
    let mut floor = world.resource_mut::<FloorNumber>();
    floor.0 += 1; let f = floor.0;

    let player_data = {
        let mut q = world.query::<(Entity, &Stats, &Position, &ActionPoints, &Inventory, &Equipment, &Skills, &Buffs)>();
        let (e, s, p, ap, inv, eq, sk, bu) = q.iter(world).next().unwrap();
        (e, s.clone(), *p, ap.points, ap.speed, inv.items.clone(), inv.capacity,
         Equipment { weapon: eq.weapon, armor: eq.armor, ring: eq.ring },
         sk.list.clone(), Buffs::new())
    };

    let to_despawn: Vec<Entity> = { let mut q = world.query::<(Entity,)>();
        q.iter(world).map(|(e,)| e).collect() };
    for e in to_despawn { let _ = world.despawn(e); }

    let mut rng = rand::rngs::SmallRng::seed_from_u64(42 + f as u64);
    let mut map = Map::new(); map.generate(&mut rng);
    world.insert_resource(map); world.insert_resource(MapMemory::new());
    let spawn = { let m = world.resource::<Map>(); m.rooms[0].center() };

    let e = { let mut cmd = world.spawn((
        Player, Position { x: spawn.0, y: spawn.1 },
        Renderable { glyph: '@', color: Color::Yellow }, MovingDir::default(),
        Viewshed { range: 8, visible_tiles: Vec::new() },
        player_data.1.clone(), EntityName("冒险者".into()),
    ));
        cmd.insert(ActionPoints { points: 0.0, speed: player_data.4 });
        cmd.insert(Inventory { items: player_data.5, capacity: player_data.6 });
        cmd.insert(player_data.7); cmd.insert(Skills { list: player_data.8 });
        cmd.insert(player_data.9.clone()); cmd.id() };

    let last_room = { let m = world.resource::<Map>(); let idx = m.rooms.len() - 1; m.rooms[idx].center() };
    world.spawn((Stairs, Position { x: last_room.0, y: last_room.1 },
        Renderable { glyph: '>', color: Color::Green }));

    let monster_templates: [(char, Color, &str); 4] = [
        ('r', Color::Red, "老鼠"), ('g', Color::Green, "哥布林"),
        ('r', Color::LightRed, "老鼠"), ('g', Color::LightGreen, "哥布林"),
    ];
    let spawn_points: Vec<(usize, usize)> = {
        let m = world.resource::<Map>();
        monster_templates.iter().enumerate()
            .filter_map(|(i, _)| m.rooms.get(i + 1).map(|r| r.center())).collect()
    };
    for (i, &(glyph, color, mon_name)) in monster_templates.iter().enumerate() {
        if let Some(&(mx, my)) = spawn_points.get(i) {
            world.spawn((Monster, MonsterBrain::creature(),
                Position { x: mx, y: my }, Renderable { glyph, color },
                Viewshed { range: 5, visible_tiles: Vec::new() },
                Stats::monster(glyph, f), EntityName(mon_name.into()),
                ActionPoints::new(Stats::monster(glyph, f).dexterity),
                FleeLogState::default())); }
    }

    let items = make_items();
    for (i, item) in items.iter().enumerate() {
        if let Some(&(ix, iy)) = spawn_points.get(i) {
            world.spawn((ItemPickup { item: item.clone() }, Position { x: ix + 1, y: iy },
                Renderable { glyph: item.glyph, color: item.color })); }
    }
    world.resource_mut::<EventLog>().push(format!("=== 第 {} 层 ===", f));
}
