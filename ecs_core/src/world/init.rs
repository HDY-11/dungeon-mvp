//! 世界初始化系统。
//!
//! 初始化顺序：
//! `insert_core_resources -> generate_map -> spawn_player -> spawn_stairs
//!  -> spawn_monsters -> fov/memory/occupancy`。
//!
//! 本轮只做首次初始化；下楼（descend）留待后续。

use crate::balance::{
    PLAYER_ATTACK_SPEED, PLAYER_MOVE_SPEED, exp_to_next_level, max_hp_for, max_mp_for,
};
use crate::components::*;
use crate::entity_cls::*;
use crate::events::{
    ActionFailedEvent, ActionSucceededEvent, AttackEvent, AttackIntentEvent, DeathEvent,
    LevelUpEvent,
};
use crate::map::map_gen::{ensure_connection_between, generate_map_from_seed};
use crate::map::{MAP_HEIGHT, MAP_WIDTH, Map, MapKind, Tile, map_kind_for};
use crate::monster::{MonsterKindId, attach_abilities, monster_template, roll_one_kind};
use crate::resources::*;
use crate::schedule::CoreInitSchedule;
use crate::system::{
    build_core_schedule, fov_system, rebuild_occupancy_system, update_map_memory_system,
    update_visible_memory_system,
};
use bevy_ecs::prelude::*;
use rand::{Rng, RngExt};

/// 初始化配置。`run_initialization` 会先插入该资源。
#[derive(Resource, Debug, Clone, Copy)]
pub struct WorldInitConfig {
    pub map_seed: u64,
}

/// 玩家出生点，由 `spawn_player_system` 写入，供楼梯/怪物系统使用。
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct PlayerSpawn(pub (usize, usize));

/// 楼梯位置，由 `spawn_stairs_system` 写入，供怪物系统排除。
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct StairsPos(pub (usize, usize));

/// 每层地图派生种子。F1 使用 map_seed；深层使用 `map_seed + floor`。
pub fn map_seed_for_floor(map_seed: u64, floor: u32) -> u64 {
    if floor <= 1 {
        map_seed
    } else {
        map_seed.wrapping_add(floor as u64)
    }
}

// ── 资源初始化 ───────────────────────────────────────

/// 插入 core 所需全局资源。直接操作 World，避免 Commands 延迟导致后续系统读不到资源。
pub fn insert_core_resources(world: &mut World, config: WorldInitConfig) {
    world.insert_resource(MapSeed(config.map_seed));
    world.insert_resource(FloorNumber(1));
    world.insert_resource(Map::new());
    world.insert_resource(GameRng::new(config.map_seed.wrapping_add(42)));
    world.insert_resource(MapMemory::new());
    world.insert_resource(VisibleMemory::default());
    world.insert_resource(OccupancyMap::new());
    // 格索引：格子侧效果的派生数据，由 `rebuild_cell_effects_system` 每轮重建
    // （H9；形状照 `OccupancyMap`，但它是效果子系统的私有索引，见 `effect/cell.rs`）。
    world.insert_resource(crate::effect::CellEffects::new());
    world.insert_resource(EventLog::new());
    world.insert_resource(TurnManager::new());
    // AV 时间轴：状态效果按它计时（ISSUES ECS46）。
    world.insert_resource(crate::resources::WorldClock::default());
    world.insert_resource(PlayerSpawn((0, 0)));
    world.insert_resource(StairsPos((0, 0)));
    // 玩家行动请求：action 实体链路的输入口（C2 起）。旧链路的同名资源
    // `action::generation::player::PlayerActionRequest` 在 C8 删除旧链路后消失。
    world.insert_resource(crate::action::entity::PlayerActionRequest::default());

    world.insert_resource(bevy_ecs::event::Events::<AttackIntentEvent>::default());
    world.insert_resource(bevy_ecs::event::Events::<AttackEvent>::default());
    world.insert_resource(bevy_ecs::event::Events::<DeathEvent>::default());
    world.insert_resource(bevy_ecs::event::Events::<LevelUpEvent>::default());
    world.insert_resource(bevy_ecs::event::Events::<ActionSucceededEvent>::default());
    world.insert_resource(bevy_ecs::event::Events::<ActionFailedEvent>::default());

    // 持久 Schedule：只注册一次，之后用 label 重复运行，保留 EventReader 游标等系统状态。
    world.add_schedule(build_init_schedule());
    world.add_schedule(build_core_schedule());
    // 行动链路（C7 起接进主循环）：生成 → 仲裁 → tick → **校验** → 落地 → completion。
    world.add_schedule(crate::action::entity::build_action_poc_schedule());
    // 玩家行动挂载（生成 + 仲裁，不含推进）。
    world.add_schedule(crate::action::entity::build_player_mount_schedule());

    log::info!("core 资源初始化完成: seed={}", config.map_seed);
}
// ── 地图生成系统 ─────────────────────────────────────

/// 读取 `MapSeed` 与 `FloorNumber`，确定性生成当前层地图。
pub fn generate_map_system(mut map: ResMut<Map>, map_seed: Res<MapSeed>, floor: Res<FloorNumber>) {
    let kind = map_kind_for(map_seed.0, floor.0);
    let seed = map_seed_for_floor(map_seed.0, floor.0);
    log::info!("生成地图: seed={seed}, floor={}, kind={kind:?}", floor.0);
    generate_map_from_seed(&mut map, kind, seed);
}

// ── 玩家出生 ─────────────────────────────────────────

/// 玩家初始组件束。
///
/// **注意元组元数上限**：bevy_ecs 0.16 的元组 `Bundle` 只实现到 15 元
/// （`all_tuples!(tuple_impl, 0, 15, B)`），所以基础束必须留在 15 个元素以内；
/// 需要更多组件时不能往元组里加，要先打包成具名 Bundle
/// （见 [`crate::components::Speed`]）或拆成两次 `insert`。
fn player_base_bundle(pos: (usize, usize)) -> impl Bundle {
    let max_hp = max_hp_for(1, 4.0);
    let max_mp = max_mp_for(1, 8.0);
    (
        Player,
        // 占格是一种能力（`Blocks`）：玩家挡住格子。楼梯不挂它，所以仍可被站上去。
        Blocks,
        Position::new(pos.0, pos.1),
        Health::new(max_hp),
        Magic::new(max_mp),
        Level(1),
        Experience::new(0.0, exp_to_next_level(1)),
        Attack(8.0),
        Defense(4.0),
        MagicMastery(8.0),
        Speed {
            move_speed: MoveSpeed(PLAYER_MOVE_SPEED),
            attack_speed: AttackSpeed(PLAYER_ATTACK_SPEED),
        },
        CritRate(0.05),
        CritDamage(0.50),
        EntityName("冒险者".into()),
    )
}

pub fn spawn_player_system(mut commands: Commands, map: Res<Map>, mut spawn: ResMut<PlayerSpawn>) {
    let pos = map.spawn_point();
    spawn.0 = pos;
    log::info!("玩家出生: {pos:?}");

    // 身体 = 容器（DsnE18）：部位槽位里放装备，其中「背包」槽位由**一件物品**（旅者行囊）占着。
    // 行囊自己带一个 `Container`（12 格无过滤），于是"背包也是容器"不是特例。
    let mut body = crate::item::Container::new(&crate::item::BODY);
    let pack = crate::item::spawn_item(&mut commands, crate::item::TRAVELER_PACK_ITEM, 1);
    if let Err(error) = body.insert(
        crate::item::pack_slot_index(),
        pack,
        &[crate::item::ItemTag::Pack],
    ) {
        // 布局表与物品标签不一致是**编码错误**：出生时就要炸出来，而不是玩到一半发现没背包。
        panic!("身体没有能装背包的槽位：{error:?}");
    }

    commands
        .spawn(player_base_bundle(pos))
        .insert(Viewshed::new(10))
        .insert(AttackName("斩击".into()))
        // 身体容器（含行囊）单独 `insert`：`player_base_bundle` 受 15 元组上限约束。
        .insert(body)
        .insert(CanMove)
        .insert(CanWait)
        .insert(CanBasicAttack)
        .insert(Idle);
}

// ── 楼梯 ─────────────────────────────────────────────

fn pick_stair_pos(map: &Map, spawn_pos: (usize, usize), rng: &mut impl Rng) -> (usize, usize) {
    let (spx, spy) = spawn_pos;

    if map.rooms.len() > 1
        && let Some(best) = map.farthest_room_from(spawn_pos)
    {
        return map.nearest_walkable(best.0, best.1);
    }

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

    for r in 15..=MAP_WIDTH.max(MAP_HEIGHT) as isize {
        for dy in -r..=r {
            for dx in -r..=r {
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

    let fx = spx.saturating_add(15).min(MAP_WIDTH - 1);
    map.nearest_walkable(fx, spy)
}

pub fn spawn_stairs_system(
    mut commands: Commands,
    mut map: ResMut<Map>,
    player_spawn: Res<PlayerSpawn>,
    mut stairs_pos: ResMut<StairsPos>,
    map_seed: Res<MapSeed>,
    floor: Res<FloorNumber>,
) {
    let mut rng = crate::resources::derived_rng(
        map_seed.0,
        floor.0,
        crate::resources::DerivePurpose::Stairs,
    );

    let pos = pick_stair_pos(&map, player_spawn.0, &mut rng);
    ensure_connection_between(&mut map, &mut rng, player_spawn.0, pos);
    stairs_pos.0 = pos;
    log::info!("楼梯位置: {pos:?}");

    commands.spawn((
        Stairs,
        Position::new(pos.0, pos.1),
        EntityName("楼梯".into()),
    ));
}

// ── 怪物生成 ─────────────────────────────────────────

/// 噪声密度层 + 元胞扩散生成怪物种群。
fn generate_monster_population(
    map_kind: MapKind,
    tiles: &[[Tile; MAP_WIDTH]; MAP_HEIGHT],
    floor: u32,
    rng: &mut impl Rng,
    exclude: &[(usize, usize)],
) -> Vec<(MonsterKindId, usize, usize)> {
    let threshold = (0.38 - floor as f64 * 0.012).max(0.15);
    let expand_chance = 0.35;
    let min_count = (floor as usize).saturating_mul(2).saturating_add(4);
    let max_count = (floor as usize).saturating_mul(4).saturating_add(8);

    let mut density = [[0.0f64; MAP_WIDTH]; MAP_HEIGHT];
    for y in 0..MAP_HEIGHT {
        for x in 0..MAP_WIDTH {
            if tiles[y][x].walkable() {
                density[y][x] = rng.random_range(0.0..1.0);
            }
        }
    }

    let mut is_monster = [[false; MAP_WIDTH]; MAP_HEIGHT];
    for y in 0..MAP_HEIGHT {
        for x in 0..MAP_WIDTH {
            if density[y][x] > threshold {
                is_monster[y][x] = true;
            }
        }
    }

    for _pass in 0..3 {
        let snapshot = is_monster;
        let mut added = 0usize;
        for y in 0..MAP_HEIGHT {
            for x in 0..MAP_WIDTH {
                if snapshot[y][x] || !tiles[y][x].walkable() {
                    continue;
                }
                let mut has_neighbor = false;
                for dy in [-1isize, 0, 1] {
                    for dx in [-1isize, 0, 1] {
                        if dx == 0 && dy == 0 {
                            continue;
                        }
                        let ny = y.wrapping_add_signed(dy);
                        let nx = x.wrapping_add_signed(dx);
                        if nx < MAP_WIDTH && ny < MAP_HEIGHT && snapshot[ny][nx] {
                            has_neighbor = true;
                        }
                    }
                }
                if has_neighbor && rng.random_range(0.0..1.0) < expand_chance {
                    is_monster[y][x] = true;
                    added += 1;
                }
            }
        }
        if added == 0 {
            break;
        }
    }

    let mut positions: Vec<(usize, usize)> = Vec::new();
    for (y, row) in is_monster.iter().enumerate() {
        for (x, &has) in row.iter().enumerate() {
            if has && !exclude.contains(&(x, y)) {
                positions.push((x, y));
            }
        }
    }

    let max_attempts = min_count.saturating_mul(40);
    let mut attempts = 0;
    while positions.len() < min_count && attempts < max_attempts {
        attempts += 1;
        let x = rng.random_range(3..MAP_WIDTH - 3);
        let y = rng.random_range(3..MAP_HEIGHT - 3);
        if tiles[y][x].walkable() && !positions.contains(&(x, y)) && !exclude.contains(&(x, y)) {
            positions.push((x, y));
        }
    }

    // **按区间取目标值**（ISSUES ECS47）：此前只裁到上限，而密度层产出远多于上限，
    // 于是"6–12 只"实际永远是 12 只——下限从未生效，"稀薄楼层"不可能出现。
    let target = if max_count > min_count {
        min_count + rng.random_range(0..=(max_count - min_count))
    } else {
        min_count
    };
    while positions.len() > target {
        let idx = rng.random_range(0..positions.len());
        positions.swap_remove(idx);
    }

    positions
        .into_iter()
        .map(|(x, y)| (roll_one_kind(map_kind, floor, rng), x, y))
        .collect()
}

fn monster_base_bundle(
    template: &crate::monster::MonsterTemplate,
    pos: (usize, usize),
    floor: u32,
) -> impl Bundle {
    let stats = template.stats(floor);
    (
        Monster,
        Blocks,
        template.kind,
        Position::new(pos.0, pos.1),
        stats.health,
        Level(stats.level),
        stats.experience_reward,
        stats.attack,
        stats.defense,
        stats.magic_mastery,
        Speed {
            move_speed: stats.move_speed,
            attack_speed: stats.attack_speed,
        },
        stats.crit_rate,
        stats.crit_damage,
    )
}

pub fn spawn_monsters_system(
    mut commands: Commands,
    map: Res<Map>,
    player_spawn: Res<PlayerSpawn>,
    stairs_pos: Res<StairsPos>,
    map_seed: Res<MapSeed>,
    floor: Res<FloorNumber>,
) {
    let kind = map_kind_for(map_seed.0, floor.0);
    let mut rng = crate::resources::derived_rng(
        map_seed.0,
        floor.0,
        crate::resources::DerivePurpose::Monsters,
    );
    let exclude = [player_spawn.0, stairs_pos.0];

    let population = generate_monster_population(kind, &map.tiles, floor.0, &mut rng, &exclude);
    log::info!("生成怪物: count={}, kind={kind:?}", population.len());

    for (monster_kind, x, y) in population {
        let template = monster_template(monster_kind);
        let mut cmd = commands.spawn(monster_base_bundle(template, (x, y), floor.0));
        cmd.insert(Viewshed::new(10))
            .insert(EntityName(template.name.into()))
            .insert(AttackName(template.attack_name.into()))
            .insert(LastKnownPlayerPos::default())
            .insert(Idle);
        // 能力来自模板（Phase H6）：这个循环不再列举任何物种名。
        attach_abilities(&mut cmd, template.abilities);

        // 物种身份由 `monster_base_bundle` 里的 `MonsterKindId` 表达，
        // 不再额外插入一层身份 ZST（`Rat`/`Scorpion`/… 已删除，见 entity_cls.rs）。
        let _ = monster_kind;
    }
}

// ── 格子效果内容（C2） ───────────────────────────────

/// 每层泥潭数量。[⃞试调] 内容参数（数值口径属 GAME.md）。
const MUD_PATCHES: usize = 6;

/// 泥潭的移动代价倍率：**处于**这一格时速度减半 → AV 翻倍。[⃞试调]
const MUD_MOVE_COST: f64 = 2.0;

/// 泥潭撒在出生点周围多少格以内。[⃞试调] 见 `spawn_cell_effects_system` 的理由。
const MUD_SPAWN_RADIUS: usize = 8;

/// 地面物品撒在出生点周围多少格以内。[⃞试调]
const ITEM_SPAWN_RADIUS: usize = 10;

/// 在可行走格上洒若干泥潭（格子效果实体，见 `effect/cell.rs`）。
///
/// 为什么是"洒几格"而不是"改地形表"：泥潭**不是一种地形**，它是"这一格发生了另外的
/// 事"（DsnE12 第 2 条）。所以它是效果实体 + 格索引，`TILE_PROPS` 一行都不用动。
///
/// 避开出生点与楼梯，且**同格不重复**——格索引是"一格一个效果实体"，重复登记会让
/// 前一个不可达（维护系统会告警，但内容侧不该制造这种局面）。
pub fn spawn_cell_effects_system(
    mut commands: Commands,
    map: Res<Map>,
    player_spawn: Res<PlayerSpawn>,
    stairs_pos: Res<StairsPos>,
    map_seed: Res<MapSeed>,
    floor: Res<FloorNumber>,
) {
    let mut rng = crate::resources::derived_rng(
        map_seed.0,
        floor.0,
        crate::resources::DerivePurpose::CellEffects,
    );

    let mut taken: Vec<(usize, usize)> = vec![player_spawn.0, stairs_pos.0];
    let mut placed = 0usize;
    let mut attempts = 0usize;
    // 上限 400 次尝试：地图拥挤时宁可少洒，也不要在生成阶段死循环。
    //
    // **撒在出生点附近**（ISSUES ECS47）：全图随机时 6 格 / 4800 可行走格 ≈ 0.1%，
    // 实测 6 局里只踩到 3 回合 —— 内容加了等于没加。半径 8 保证开局几步就能遇到。
    while placed < MUD_PATCHES && attempts < 400 {
        attempts += 1;
        let x = rng.random_range(
            player_spawn.0.0.saturating_sub(MUD_SPAWN_RADIUS)
                ..=(player_spawn.0.0 + MUD_SPAWN_RADIUS).min(MAP_WIDTH - 2),
        );
        let y = rng.random_range(
            player_spawn.0.1.saturating_sub(MUD_SPAWN_RADIUS)
                ..=(player_spawn.0.1 + MUD_SPAWN_RADIUS).min(MAP_HEIGHT - 2),
        );
        if !map.tiles[y][x].walkable() || taken.contains(&(x, y)) {
            continue;
        }
        taken.push((x, y));
        commands.spawn((
            crate::effect::CellEffectAt::new(x, y),
            crate::effect::CellMoveCost::new(MUD_MOVE_COST),
        ));
        placed += 1;
    }
    log::info!("生成泥潭: {placed} 格（尝试 {attempts} 次）");
}

// ── 物品内容（C4 → 物品系统落地） ─────────────────────

/// 本层地面物品的**池子**：装备 + 消耗品。
///
/// 材料（血肉/布/牙/壳…）故意**不在**池子里：它们只能从怪物掉落来。
/// 这样"刷图"与"打怪"各有自己的产出，而不是两个渠道发同一种东西
/// （否则玩家没有理由打怪，而装备系统又会退化成"在地上捡"）。
///
/// 踏水靴不在池子里：GAME.md Gm7 规定"每层 1 件"，所以它由
/// `spawn_ground_items_system` **单独保底放一件**；留在池子里随机抽会出现
/// "这层没有靴子"或"这层两双靴子"，与文档不符（AUDIT-data P2）。
const GROUND_ITEM_POOL: &[crate::item::ItemId] = &[
    crate::item::ItemId(0),  // 锈铁剑
    crate::item::ItemId(1),  // 木盾
    crate::item::ItemId(2),  // 皮甲
    crate::item::ItemId(3),  // 攻击戒指
    crate::item::ItemId(23), // 石锤
    crate::item::ItemId(24), // 匕首
    crate::item::ItemId(25), // 蘑菇
    crate::item::ItemId(32), // 海藻
];

/// 在地上撒本层的地面物品（GAME.md Gm7「地面刷新」 + Gm10「每层物品数」）。
///
/// 口径（AUDIT-data P2 对齐文档）：**合计 4–8 件** =
/// 保底 1 件踏水靴 + 池子里 2–4 件 + 1–3 堆石子。
///
/// 位置由 `(MapSeed, 楼层)` 派生（`DerivePurpose::GroundItems`），所以同一颗种子的第 N 层
/// 永远是同一批东西、同一批位置——存档只存玩家状态，不存地面物品（DsnX17 的确定性口径）。
///
/// 为什么不放在出生点/楼梯/泥潭上：踩上去就会捡，放在这些格子上等于"开局白送"
/// 或者让泥潭变成"靴子的装饰"。
pub fn spawn_ground_items_system(
    mut commands: Commands,
    map: Res<Map>,
    player_spawn: Res<PlayerSpawn>,
    stairs_pos: Res<StairsPos>,
    cell_effects: Query<&crate::effect::CellEffectAt>,
    map_seed: Res<MapSeed>,
    floor: Res<FloorNumber>,
) {
    let mut rng = crate::resources::derived_rng(
        map_seed.0,
        floor.0,
        crate::resources::DerivePurpose::GroundItems,
    );

    let mut taken: Vec<(usize, usize)> = vec![player_spawn.0, stairs_pos.0];
    taken.extend(cell_effects.iter().map(|at| at.to_tuple()));

    // 掷一次"这一层放什么"：踏水靴保底 1 件 + 池子 2–4 件 + 石子 1–3 堆 = 合计 4–8 件。
    // RNG 抽取顺序固定（先池子后石子），所以同种子同层的地面内容仍然确定。
    let mut batch: Vec<(crate::item::ItemId, u32)> = vec![(crate::item::ItemId(40), 1)];
    let pool_count = rng.random_range(2u64..=4);
    for _ in 0..pool_count {
        let index = rng.random_range(0..GROUND_ITEM_POOL.len() as u64) as usize;
        batch.push((GROUND_ITEM_POOL[index], 1));
    }
    let pebbles = rng.random_range(1u64..=3);
    for _ in 0..pebbles {
        batch.push((crate::item::ItemId(18), 1));
    }

    let mut placed = 0usize;
    for (id, count) in batch {
        // 每件物品各找一格：最多试 200 次，地图太挤就放弃这一件（不 panic）。
        let mut spot = None;
        for _ in 0..200 {
            let x = rng.random_range(
                player_spawn.0.0.saturating_sub(ITEM_SPAWN_RADIUS)
                    ..=(player_spawn.0.0 + ITEM_SPAWN_RADIUS).min(MAP_WIDTH - 2),
            );
            let y = rng.random_range(
                player_spawn.0.1.saturating_sub(ITEM_SPAWN_RADIUS)
                    ..=(player_spawn.0.1 + ITEM_SPAWN_RADIUS).min(MAP_HEIGHT - 2),
            );
            if !map.tiles[y][x].walkable() || taken.contains(&(x, y)) {
                continue;
            }
            spot = Some((x, y));
            break;
        }
        let Some((x, y)) = spot else {
            log::warn!("没有找到合适的位置放 {:?}（地图过于拥挤）", id);
            continue;
        };
        taken.push((x, y));
        crate::item::spawn_ground_item(&mut commands, id, count, (x, y));
        placed += 1;
        log::info!("放置地面物品: {} @ ({x},{y})", id.def().name);
    }
    log::info!("地面物品: {placed} 件");
}

// ── 初始化调度 ───────────────────────────────────────

pub fn build_init_schedule() -> Schedule {
    // `core_schedule`（而不是 `Schedule::new`）：构建时强制无歧义，见 `schedule.rs`。
    let mut schedule = crate::schedule::core_schedule(CoreInitSchedule);
    schedule.add_systems(
        (
            generate_map_system,
            spawn_player_system,
            spawn_stairs_system,
            // 泥潭要避开出生点与楼梯，所以排在两者之后、怪物之前。
            spawn_cell_effects_system,
            // 地面物品要避开泥潭，所以排在泥潭之后。
            spawn_ground_items_system,
            spawn_monsters_system,
            fov_system,
            update_map_memory_system,
            update_visible_memory_system,
            rebuild_occupancy_system,
            // 格索引也要在初始化末尾重建一次：否则开局那一帧的格子效果是"未登记"的
            // （`CellEffects` 是派生数据，初始化和每轮结算各维护一次）。
            crate::effect::rebuild_cell_effects_system,
        )
            .chain(),
    );
    schedule
}

/// 运行完整首次初始化。
pub fn run_initialization(world: &mut World, map_seed: u64) {
    insert_core_resources(world, WorldInitConfig { map_seed });
    world.run_schedule(CoreInitSchedule);
}

#[cfg(test)]
#[path = "init_tests.rs"]
mod tests;
