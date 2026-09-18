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
    LevelUpEvent, ThreatEvent,
};
use crate::map::map_gen::{ensure_connection_between, generate_map_from_seed};
use crate::map::{MAP_HEIGHT, MAP_WIDTH, Map, MapKind, Tile, map_kind_for};
use crate::monster::{MonsterKindId, monster_template, roll_one_kind};
use crate::resources::*;
use crate::schedule::CoreInitSchedule;
use crate::system::{
    build_core_schedule, fov_system, rebuild_occupancy_system, update_map_memory_system,
    update_visible_memory_system,
};
use bevy_ecs::prelude::*;
use rand::{Rng, RngExt, SeedableRng};

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
    world.insert_resource(PendingExp::default());
    world.insert_resource(EventLog::new());
    world.insert_resource(TurnManager::new());
    world.insert_resource(ThreatTable::default());
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
    world.insert_resource(bevy_ecs::event::Events::<ThreatEvent>::default());

    // 持久 Schedule：只注册一次，之后用 label 重复运行，保留 EventReader 游标等系统状态。
    world.add_schedule(build_init_schedule());
    world.add_schedule(build_core_schedule());
    // 行动链路（C7 起接进主循环）：生成 → 仲裁 → tick → 执行 → completion。
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
        EntityClass::Actor,
        CreatureKind::Humanoid,
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

    commands
        .spawn(player_base_bundle(pos))
        .insert(Viewshed::new(10))
        .insert(AttackName("斩击".into()))
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
    let seed = map_seed_for_floor(map_seed.0, floor.0).wrapping_add(777);
    let mut rng = rand::rngs::SmallRng::seed_from_u64(seed);

    let pos = pick_stair_pos(&map, player_spawn.0, &mut rng);
    ensure_connection_between(&mut map, &mut rng, player_spawn.0, pos);
    stairs_pos.0 = pos;
    log::info!("楼梯位置: {pos:?}");

    commands.spawn((
        Stairs,
        EntityClass::Field,
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

    while positions.len() > max_count {
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
        EntityClass::Actor,
        template.creature_kind,
        template.kind,
        Position::new(pos.0, pos.1),
        stats.health,
        stats.magic,
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
    let seed = map_seed_for_floor(map_seed.0, floor.0).wrapping_add(999);
    let mut rng = rand::rngs::SmallRng::seed_from_u64(seed);
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
            .insert(CanChase)
            .insert(CanFlee)
            .insert(CanWander)
            .insert(CanWait)
            .insert(Idle);

        match monster_kind {
            MonsterKindId::Rat => {
                cmd.insert(Rat);
            }
            MonsterKindId::Scorpion => {
                cmd.insert(Scorpion);
            }
            MonsterKindId::Goblin => {
                cmd.insert(Goblin);
            }
            MonsterKindId::Sporeling => {
                cmd.insert(Sporeling);
            }
            MonsterKindId::MushroomGolem => {
                cmd.insert(MushroomGolem);
            }
            MonsterKindId::CaveFish => {
                cmd.insert(CaveFish);
            }
            MonsterKindId::CaveCrab => {
                cmd.insert(CaveCrab);
            }
            MonsterKindId::DeepEel => {
                cmd.insert(DeepEel);
            }
        }
    }
}

// ── 初始化调度 ───────────────────────────────────────

pub fn build_init_schedule() -> Schedule {
    let mut schedule = Schedule::new(CoreInitSchedule);
    schedule.add_systems(
        (
            generate_map_system,
            spawn_player_system,
            spawn_stairs_system,
            spawn_monsters_system,
            fov_system,
            update_map_memory_system,
            update_visible_memory_system,
            rebuild_occupancy_system,
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
