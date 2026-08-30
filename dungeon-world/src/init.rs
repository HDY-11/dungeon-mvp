//! World 初始化与下楼

use bevy_ecs::prelude::*;
use dungeon_action::{
    ActionQueue, CanChase, CanFlee, CanMove, CanWait, CanWander, ChaseIntents, FleeIntents,
    PlayerPreview, WanderIntents,
};
use dungeon_core::OptionLogExt;
use dungeon_core::{MAP_HEIGHT, MAP_WIDTH, Map, components::*, items::*, resources::*};
use rand::{Rng, SeedableRng};

// ══════════════════════════════════════════════════════
// 共享辅助函数（setup_world 与 descend 共用）
// ══════════════════════════════════════════════════════

/// 在 walkable 格上放置怪物种群，避开 exclude 坐标
fn spawn_monsters(
    world: &mut World,
    kind: dungeon_core::MapKind,
    floor: u32,
    rng: &mut impl Rng,
    exclude: &[(usize, usize)],
) {
    let tiles = world.resource::<Map>().tiles;
    let population =
        crate::population::generate_monster_population(kind, &tiles, floor, rng, exclude);
    for &(kind, mx, my) in &population {
        let glyph = dungeon_core::monster_def::monster_glyph(kind);
        let color = dungeon_core::monster_def::monster_color(kind);
        let loot = dungeon_core::monster_def::monster_loot(kind);
        let attk = dungeon_core::monster_def::monster_attack_name(kind);
        let name = dungeon_core::monster_def::monster_name(kind);
        let mut cmd = world.spawn((
            Monster,
            Position { x: mx, y: my },
            Renderable { glyph, color },
            Viewshed {
                range: 10,
                visible_tiles: Vec::new(),
            },
            dungeon_core::monster_def::monster_stats(kind, floor),
            EntityName(name.into()),
            AttackName(attk.into()),
            loot,
            kind,
        ));
        let entity = cmd.id();
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

/// 随机生成每层地面物品组合（G12：从基础装备池抽取 4-8 件）
fn roll_ground_item_ids(rng: &mut impl Rng) -> Vec<usize> {
    use rand::RngExt;
    // I67: 石锤/匕首加入地面池（低权重），提供攻速选择
    let pool = [
        dungeon_core::ITEM_RUSTY_SWORD,
        dungeon_core::ITEM_WOOD_SHIELD,
        dungeon_core::ITEM_LEATHER_ARMOR,
        dungeon_core::ITEM_ATTACK_RING,
        dungeon_core::ITEM_STONE_HAMMER,
        dungeon_core::ITEM_DAGGER,
    ];
    let count = rng.random_range(4u32..=8) as usize;
    (0..count)
        .map(|_| pool[rng.random_range(0..pool.len())])
        .collect()
}

/// 在地图中放置地面物品。
/// 优先使用非出生房间的中心，若仅有 1 个房间则退回到出生房间内偏移放置（D18: 使用传入 rng，不再硬编码种子）。
fn place_ground_items(
    world: &mut World,
    item_ids: &[usize],
    exclude: &[(usize, usize)],
    rng: &mut impl Rng,
) {
    use rand::RngExt;
    let room_centers: Vec<(usize, usize)> = {
        let map = world.resource::<Map>();
        if map.rooms.len() > 1 {
            // G22: 房间中心可能被钟乳石/水覆盖，兜底到最近可行走格
            map.rooms
                .iter()
                .skip(1)
                .map(|r| {
                    let c = r.center();
                    map.nearest_walkable(c.0, c.1)
                })
                .collect()
        } else {
            // I16: 单房间时从房间内随机找偏离中心的 walkable 格
            let r = &map.rooms[0];
            if r.w <= 4 || r.h <= 4 {
                // I83: 房间 bounding box ≤4 时采样区间为空会 panic——兜底到中心最近可行走格
                let c = r.center();
                vec![map.nearest_walkable(c.0, c.1)]
            } else {
                let mut alt = Vec::new();
                for _ in 0..20 {
                    let ox = rng.random_range(2..r.w.saturating_sub(2));
                    let oy = rng.random_range(2..r.h.saturating_sub(2));
                    let px = r.x + ox;
                    let py = r.y + oy;
                    if px < MAP_WIDTH
                        && py < MAP_HEIGHT
                        && map.tiles[py][px].walkable()
                        && !exclude.contains(&(px, py))
                    {
                        alt.push((px, py));
                    }
                }
                alt
            }
        }
    };

    let item_count = room_centers.len().min(item_ids.len());
    for (i, &item_id) in item_ids[..item_count].iter().enumerate() {
        if let Some(&(ix, iy)) = room_centers.get(i) {
            let def = ItemRegistry::global()
                .get(item_id)
                .expect_log("item_id exists in registry");
            // G22: 落点（中心 +1 偏移）也兜底到最近可行走格
            let (px, py) = {
                let map = world.resource::<Map>();
                map.nearest_walkable(ix + 1, iy)
            };
            world.spawn((
                ItemPickup {
                    stack: ItemStack::new(item_id, 1),
                },
                Position { x: px, y: py },
                Renderable {
                    glyph: def.glyph,
                    color: def.color,
                },
            ));
        }
    }
}

/// 在地图可行走格上随机放置技能卷轴，每层 1-3 张，深层额外增加
fn place_skill_scrolls(
    world: &mut World,
    floor: u32,
    rng: &mut impl Rng,
    exclude: &[(usize, usize)],
) {
    use rand::RngExt;
    let count = rng.random_range(1u32..=3) + floor.saturating_sub(1) / 5;
    let scroll_ids = [
        dungeon_core::ITEM_SCROLL_HEAL as u32,
        dungeon_core::ITEM_SCROLL_SHIELD as u32,
        dungeon_core::ITEM_SCROLL_BERSERK as u32,
    ];
    for _ in 0..count {
        let idx = rng.random_range(0usize..3);
        let item_id = scroll_ids[idx] as usize;
        for _attempt in 0..30 {
            let x = rng.random_range(3..dungeon_core::MAP_WIDTH as u16 - 3) as usize;
            let y = rng.random_range(3..dungeon_core::MAP_HEIGHT as u16 - 3) as usize;
            if world.resource::<dungeon_core::Map>().tiles[y][x].walkable()
                && !exclude.contains(&(x, y))
            {
                let def = dungeon_core::ItemRegistry::global()
                    .get(item_id)
                    .expect_log("scroll exists");
                world.spawn((
                    dungeon_core::ItemPickup {
                        stack: dungeon_core::ItemStack::new(item_id, 1),
                    },
                    dungeon_core::Position { x, y },
                    dungeon_core::Renderable {
                        glyph: def.glyph,
                        color: def.color,
                    },
                ));
                break;
            }
        }
    }
}

/// 在地图可行走格上散布石子堆，每层 1-3 堆。
fn scatter_stones(world: &mut World, rng: &mut impl Rng, exclude: &[(usize, usize)]) {
    use rand::RngExt;
    let count = rng.random_range(1u32..=3);
    for _ in 0..count {
        for _attempt in 0..30 {
            let x = rng.random_range(3..dungeon_core::MAP_WIDTH as u16 - 3) as usize;
            let y = rng.random_range(3..dungeon_core::MAP_HEIGHT as u16 - 3) as usize;
            if world.resource::<dungeon_core::Map>().tiles[y][x].walkable()
                && !exclude.contains(&(x, y))
            {
                world.spawn((
                    dungeon_core::ItemPickup {
                        stack: dungeon_core::ItemStack::new(dungeon_core::ITEM_STONE, 1),
                    },
                    dungeon_core::Position { x, y },
                    dungeon_core::Renderable {
                        glyph: '·',
                        color: (160, 140, 120),
                    },
                ));
                break;
            }
        }
    }
}

/// 选择楼梯位置：尽量远离 spawn_pos，至少 15 格。
/// 优先选最远房间。仅 1 个房间时用醉汉游走 60 步找 ≥15 格外的位置（G9）。
/// G22: 落点兜底到最近可行走格（房间中心可能被钟乳石/水覆盖）。
pub(crate) fn pick_stair_pos(
    map: &Map,
    spawn_pos: (usize, usize),
    rng: &mut impl Rng,
) -> (usize, usize) {
    use rand::RngExt;
    let (spx, spy) = spawn_pos;

    // 仅当有多个房间时才用最远房间；单房间时 farthest_room_from 返回 spawn 本身
    if map.rooms.len() > 1
        && let Some(best) = map.farthest_room_from(spawn_pos)
    {
        // G22: 房间中心可能被水覆盖，兜底到最近可行走格
        return map.nearest_walkable(best.0, best.1);
    }

    // G9: 单房间 → 醉汉游走 60 步
    let (mut cx, mut cy) = (spx as isize, spy as isize);
    for _ in 0..60 {
        let dx = rng.random_range(-1i32..2) as isize;
        let dy = rng.random_range(-1i32..2) as isize;
        if dx == 0 && dy == 0 {
            continue;
        }
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
                if dx == 0 && dy == 0 {
                    continue;
                }
                let nx = spx.wrapping_add_signed(dx);
                let ny = spy.wrapping_add_signed(dy);
                if nx < MAP_WIDTH && ny < MAP_HEIGHT && map.tiles[ny][nx].walkable() {
                    return (nx, ny);
                }
            }
        }
    }
    // I84: 兜底坐标钳制 + walkable 校验——spx+15 可能越界（spawn 靠近右边界时），
    // 越界坐标传给 ensure_connection_between 会索引越界 panic
    let fx = spx.saturating_add(15).min(MAP_WIDTH - 1);
    map.nearest_walkable(fx, spy)
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
    // Dsn24: F1 固定标准洞穴（新手层）
    map.generate(dungeon_core::MapKind::Cavern, &mut rng);

    world.insert_resource(MapSeed(map_seed));
    world.insert_resource(MapMemory::new());
    world.insert_resource(OccupancyMap::new());
    world.insert_resource(PendingExp::default());
    world.insert_resource(EventLog::new());
    world.insert_resource(GameRng::new(map_seed.wrapping_add(42)));
    world.insert_resource(TurnManager::new());
    world.insert_resource(FloorNumber(1));
    world.insert_resource(VisibleMemory::default());
    world.insert_resource(LookCursor {
        active: false,
        x: 0,
        y: 0,
    });
    world.insert_resource(ActionQueue::default());
    world.insert_resource(PlayerPreview::default());
    world.insert_resource(ChaseIntents::default());
    world.insert_resource(FleeIntents::default());
    world.insert_resource(WanderIntents::default());
    world.insert_resource(dungeon_core::ThrowPreview::default());

    let (spawn_x, spawn_y) = map.spawn_point();
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
    cmd.insert(dungeon_core::Skills { list: pc.skills() });
    cmd.insert(ActiveBuffs::new());

    // ── 楼梯放置（避开出生点，G9） ──
    let stairs_pos = {
        let m = world.resource::<Map>();
        pick_stair_pos(m, (spawn_x, spawn_y), &mut rng)
    };
    world.spawn((
        Stairs,
        Position {
            x: stairs_pos.0,
            y: stairs_pos.1,
        },
        Renderable {
            glyph: '>',
            color: (0, 255, 0),
        },
    ));
    {
        let mut map = world.resource_mut::<Map>();
        dungeon_core::map_gen::ensure_connection_between(
            &mut map,
            &mut rng,
            (spawn_x, spawn_y),
            (stairs_pos.0, stairs_pos.1),
        );
    }

    // ── 怪物生成（排除楼梯和出生点，G10） ──
    spawn_monsters(
        &mut world,
        dungeon_core::MapKind::Cavern,
        1,
        &mut rng,
        &[(spawn_x, spawn_y), (stairs_pos.0, stairs_pos.1)],
    );

    // ── 地面物品（G12: 每层随机组合） ──
    let ground_item_ids = roll_ground_item_ids(&mut rng);
    place_ground_items(
        &mut world,
        &ground_item_ids,
        &[(spawn_x, spawn_y), (stairs_pos.0, stairs_pos.1)],
        &mut rng,
    );

    // ── 技能卷轴 ──
    place_skill_scrolls(
        &mut world,
        1,
        &mut rng,
        &[(spawn_x, spawn_y), (stairs_pos.0, stairs_pos.1)],
    );

    // ── 石子散布 ──
    scatter_stones(
        &mut world,
        &mut rng,
        &[(spawn_x, spawn_y), (stairs_pos.0, stairs_pos.1)],
    );

    world
}

/// 下楼：生成新楼层
pub fn descend(world: &mut World) {
    let w = world;
    let mut floor = w.resource_mut::<FloorNumber>();
    floor.0 += 1;
    let f = floor.0;

    let (
        player_stats,
        player_inv_stacks,
        player_inv_cap,
        player_equip,
        player_class,
        player_atk_name,
        player_active_buffs_vec,
        player_skills,
    ) = {
        // A25: 查询显式包含 &Player 组件（L31 最具体约束）
        let mut q = w.query::<(
            &Player,
            &Stats,
            &Inventory,
            &Equipment,
            &PlayerClass,
            &AttackName,
            &ActiveBuffs,
            &Skills,
        )>();
        let (_, s, inv, eq, cls, atk, ab, sk) =
            q.iter(&*w).next().expect_log("Player exists for descend");
        (
            s.clone(),
            inv.stacks.clone(),
            inv.capacity,
            dungeon_core::Equipment {
                main_hand: eq.main_hand.clone(),
                off_hand: eq.off_hand.clone(),
                armor: eq.armor.clone(),
                ring: eq.ring.clone(),
            },
            cls.clone(),
            atk.0.clone(),
            ab.0.clone(),
            sk.clone(),
        )
    };

    let to_despawn: Vec<Entity> = {
        let mut q = w.query::<(Entity,)>();
        q.iter(&*w).map(|(e,)| e).collect()
    };
    for e in to_despawn {
        let _ = w.despawn(e);
    }

    // G34: 实体已全部 despawn，同步清空行动队列与意图缓冲区，避免残留失效 Entity 条目
    w.resource_mut::<dungeon_action::ActionQueue>()
        .entries
        .clear();
    w.resource_mut::<dungeon_action::ChaseIntents>().0.clear();
    w.resource_mut::<dungeon_action::FleeIntents>().0.clear();
    w.resource_mut::<dungeon_action::WanderIntents>().0.clear();

    let base_seed = w.resource::<MapSeed>().0;
    let mut rng = rand::rngs::SmallRng::seed_from_u64(base_seed.wrapping_add(f as u64));
    let mut map = Map::new();
    // Dsn24: 下一层类型由 (seed, floor) 确定性派生（F1 固定 Cavern）
    let kind = dungeon_core::map_kind_for(base_seed, f);
    map.generate(kind, &mut rng);
    w.insert_resource(map);
    w.insert_resource(MapMemory::new());
    w.insert_resource(GameRng::new(
        base_seed.wrapping_add(f as u64).wrapping_add(42),
    )); // I44: 下楼时重置 RNG 种子

    // ── 重建玩家 ──
    let spawn = { w.resource::<Map>().spawn_point() };
    let mut cmd = w.spawn((
        Player,
        Position {
            x: spawn.0,
            y: spawn.1,
        },
        Renderable {
            glyph: '@',
            color: (255, 255, 0),
        },
        Viewshed {
            range: 10,
            visible_tiles: Vec::new(),
        },
        player_stats.clone(),
        EntityName("冒险者".into()),
        Inventory {
            stacks: player_inv_stacks,
            capacity: player_inv_cap,
        },
    ));
    cmd.insert(player_equip); // Equipment
    cmd.insert(player_skills);
    cmd.insert(player_class.clone()); // PlayerClass
    cmd.insert(AttackName(player_atk_name));
    cmd.insert(ActiveBuffs(player_active_buffs_vec)); // D9: 保存并恢复 ActiveBuffs
    cmd.insert(CanMove::new(100));
    cmd.insert(CanWait::new(0));

    // ── 楼梯放置（避开出生点，G9） ──
    let stairs_pos = {
        let m = w.resource::<Map>();
        pick_stair_pos(m, spawn, &mut rng)
    };
    w.spawn((
        Stairs,
        Position {
            x: stairs_pos.0,
            y: stairs_pos.1,
        },
        Renderable {
            glyph: '>',
            color: (0, 255, 0),
        },
    ));
    {
        let mut map = w.resource_mut::<Map>();
        dungeon_core::map_gen::ensure_connection_between(
            &mut map,
            &mut rng,
            spawn,
            (stairs_pos.0, stairs_pos.1),
        );
    }

    // ── 怪物生成（排除楼梯和出生点，G10） ──
    spawn_monsters(w, kind, f, &mut rng, &[spawn, (stairs_pos.0, stairs_pos.1)]);

    // ── 地面物品（G12: 每层随机组合） ──
    let ground_item_ids = roll_ground_item_ids(&mut rng);
    place_ground_items(
        w,
        &ground_item_ids,
        &[spawn, (stairs_pos.0, stairs_pos.1)],
        &mut rng,
    );

    // ── 技能卷轴 ──
    place_skill_scrolls(w, f, &mut rng, &[spawn, (stairs_pos.0, stairs_pos.1)]);

    // ── 石子散布 ──
    scatter_stones(w, &mut rng, &[spawn, (stairs_pos.0, stairs_pos.1)]);

    w.resource_mut::<EventLog>()
        .push(dungeon_core::EventMessage::system(format!(
            "=== 第 {} 层 ===",
            f
        )));
}
