//! 测试辅助（`#[cfg(test)]` 专用）。
//!
//! Phase A（REFACTOR.md §11.3）引入：统一搭 World 与 spawn 实体的样板，
//! 避免每个冒烟测试重复 `insert_core_resources` / 组件束。
//!
//! 约定：
//! - [`test_world`] 只插入资源与持久 Schedule，**不生成地图、不 spawn 玩家**；
//!   需要地图的测试自己填 `Map`（见 [`fill_map`] / [`carve_single_floor`]）或调用
//!   [`crate::world_loop::new_game`]。
//! - [`spawn_test_actor`] 只给最小组件束，能力组件不默认插入——测试按需
//!   `world.entity_mut(e).insert(CanWander)`，避免“悄悄授予能力”掩盖断言。
//! - [`world_snapshot`] 给确定性测试用全量快照，比较用 `assert_eq!` 即可。

use crate::components::*;
use crate::entity_cls::{Monster, Player, Stairs};
use crate::map::{MAP_HEIGHT, MAP_WIDTH, Map, Tile};
use crate::monster::MonsterKindId;
use crate::resources::OccupancyMap;
use crate::world::init::{WorldInitConfig, insert_core_resources};
use bevy_ecs::prelude::*;

/// 创建一个已注册 core 资源与 Schedule、但没有地图/玩家的空 World。
pub fn test_world() -> World {
    let mut world = World::new();
    insert_core_resources(&mut world, WorldInitConfig { map_seed: 0 });
    world
}

/// 把整张地图填成 `tile`。用于构造可控的移动/视野场景。
pub fn fill_map(world: &mut World, tile: Tile) {
    world.resource_mut::<Map>().tiles = [[tile; MAP_WIDTH]; MAP_HEIGHT];
}

/// 只把 `(x, y)` 挖成可通行；其余全是墙。
///
/// 移动测试的最小场景：唯一可走格 + 四周阻挡，不需要依赖地形生成。
pub fn carve_single_floor(world: &mut World, x: usize, y: usize) {
    fill_map(world, Tile::Wall);
    world.resource_mut::<Map>().tiles[y][x] = Tile::Floor;
}

/// 玩家实体 id；没有玩家时返回 `None`（不 panic，便于断言）。
pub fn player_entity(world: &World) -> Option<Entity> {
    let mut query = world.try_query::<(Entity, &Player)>()?;
    query.iter(world).next().map(|(entity, _)| entity)
}

/// 楼梯实体位置。地图生成确定性测试用它交叉验证 `StairsPos` 资源。
pub fn stairs_entity_pos(world: &World) -> Option<(usize, usize)> {
    let mut query = world.try_query::<(&Stairs, &Position)>()?;
    query.iter(world).next().map(|(_, pos)| pos.to_tuple())
}

/// 玩家当前位置。
pub fn player_pos(world: &World) -> (usize, usize) {
    let player = player_entity(world).expect("测试世界必须存在玩家实体");
    let pos = world.get::<Position>(player).expect("玩家必须有 Position");
    pos.to_tuple()
}

/// 玩家生命值。
pub fn player_health(world: &World) -> Health {
    let player = player_entity(world).expect("测试世界必须存在玩家实体");
    *world.get::<Health>(player).expect("玩家必须有 Health")
}

/// 在当前地图上找一个“可走且相邻”的方向；找不到返回 `None`。
///
/// 判定与 [`crate::action::execution::movement::can_move_to`] 同源语义
/// （越界 / 不可走 / 被占用都算不可走），但不检查对角 corner-cutting。
pub fn find_walkable_step(world: &World, pos: (usize, usize)) -> Option<(isize, isize)> {
    let map = world.resource::<Map>();
    let occupancy = world.resource::<OccupancyMap>();
    let dirs: [(isize, isize); 8] = [
        (0, -1),
        (0, 1),
        (-1, 0),
        (1, 0),
        (-1, -1),
        (1, -1),
        (-1, 1),
        (1, 1),
    ];
    dirs.into_iter().find(|&(dx, dy)| {
        let (nx, ny) = Position::new(pos.0, pos.1).offset(dx, dy);
        nx < MAP_WIDTH
            && ny < MAP_HEIGHT
            && map.tiles[ny][nx].walkable()
            && !occupancy.is_occupied(nx, ny)
    })
}

/// 在当前地图上找一个“不可走”的方向（越界 / 墙 / 被占用）；找不到返回 `None`。
pub fn find_blocked_step(world: &World, pos: (usize, usize)) -> Option<(isize, isize)> {
    let map = world.resource::<Map>();
    let occupancy = world.resource::<OccupancyMap>();
    let dirs: [(isize, isize); 8] = [
        (0, -1),
        (0, 1),
        (-1, 0),
        (1, 0),
        (-1, -1),
        (1, -1),
        (-1, 1),
        (1, 1),
    ];
    dirs.into_iter().find(|&(dx, dy)| {
        let (nx, ny) = Position::new(pos.0, pos.1).offset(dx, dy);
        nx >= MAP_WIDTH
            || ny >= MAP_HEIGHT
            || !map.tiles[ny][nx].walkable()
            || occupancy.is_occupied(nx, ny)
    })
}

/// 可控最小场景：唯一可走格 `(0, 0)`，玩家站在那里，另有一只怪物留在角落。
///
/// 怪物不是装饰：`insert_core_resources` 只注册资源/调度，组件类型要由实体带上，
/// 玩家行动的推进路径（`decide_monster_actions` 的 `query_filtered`）需要
/// `Monster` / `MonsterKindId` 等类型已注册，否则查询会 panic。
pub fn single_tile_scene() -> (World, Entity) {
    let mut world = test_world();
    carve_single_floor(&mut world, 0, 0);
    let player = spawn_test_player(&mut world, (0, 0));
    spawn_test_monster(
        &mut world,
        MonsterKindId::Rat,
        (MAP_WIDTH - 1, MAP_HEIGHT - 1),
        10.0,
        4.0,
        5.0,
    );
    crate::system::run_settle_systems(&mut world);
    (world, player)
}

/// 最小行动者组件束：位置 / 生命 / 攻防 / 敏捷 / 暴击 / 命名。
///
/// 不插入 `Idle`、不插入任何 `Can*`：行动状态与能力由测试自己给。
pub fn spawn_test_actor(
    world: &mut World,
    pos: (usize, usize),
    hp: f64,
    attack: f64,
    agility: f64,
) -> Entity {
    world
        .spawn((
            Position::new(pos.0, pos.1),
            Health::new(hp),
            Attack(attack),
            Defense(0.0),
            Agility(agility),
            CritRate(0.0),
            CritDamage(0.0),
            EntityName("测试目标".into()),
        ))
        .id()
}

/// 最小玩家组件束：`spawn_test_actor` + `Player` + 升级链路所需的完整数值组件。
///
/// 特意带齐 `Defense` / `MagicMastery`：`apply_exp_system` 升级时会用它们重算
/// HP/MP 上限，缺组件会让测试静默跳过升级。
pub fn spawn_test_player(world: &mut World, pos: (usize, usize)) -> Entity {
    let entity = spawn_test_actor(world, pos, 100.0, 10.0, 10.0);
    world.entity_mut(entity).insert((
        Player,
        Defense(4.0),
        MagicMastery(8.0),
        Magic::new(20.0),
        Level(1),
        Experience::new(0.0, 100.0),
        Idle,
    ));
    entity
}

/// 最小怪物组件束：`spawn_test_actor` + `Monster` + 怪物种类身份。
pub fn spawn_test_monster(
    world: &mut World,
    kind: MonsterKindId,
    pos: (usize, usize),
    hp: f64,
    attack: f64,
    agility: f64,
) -> Entity {
    let entity = spawn_test_actor(world, pos, hp, attack, agility);
    world.entity_mut(entity).insert((Monster, kind, Idle));
    entity
}

/// 连续攻击直到目标消失（最多 4 次尝试）。
///
/// 伤害会被目标防御削减，且暴击率为 0，所以用逐次加码的攻击值兜底，
/// 让“杀死一个怪物”这件事不依赖具体数值表。
pub fn kill_entity(world: &mut World, attacker: Entity, target: Entity) -> bool {
    use crate::events::AttackIntentEvent;
    use crate::system::run_settle_systems;

    const MAX_KILL_ATTEMPTS: usize = 4;

    for attempt in 0..MAX_KILL_ATTEMPTS {
        if world.get_entity(target).is_err() {
            return true;
        }
        let damage = 100.0 * 10f64.powi(attempt as i32);
        if let Some(mut attack) = world.get_mut::<Attack>(attacker) {
            attack.0 = damage;
        }
        world
            .resource_mut::<bevy_ecs::event::Events<AttackIntentEvent>>()
            .send(AttackIntentEvent { attacker, target });
        run_settle_systems(world);
    }
    world.get_entity(target).is_err()
}

/// 单只怪物的指纹：种类 + 位置 + 关键数值。
///
/// 按 `(位置, 种类, 攻击)` 排序，用于确定性比较。
#[derive(Debug, Clone, PartialEq)]
pub struct MonsterSnapshot {
    pub kind: MonsterKindId,
    pub pos: (usize, usize),
    pub hp: (f64, f64),
    pub attack: f64,
    pub agility: f64,
    pub level: u64,
    pub exp_reward: f64,
}

/// 一局世界的确定性指纹。
///
/// 不包含 `GameRng` 状态与 `Schedule`：前者在 `new_game` 路径上不被消耗，
/// 后者不是游戏状态。
#[derive(Debug, Clone, PartialEq)]
pub struct WorldSnapshot {
    pub tiles: Vec<Vec<Tile>>,
    pub rooms: Vec<(usize, usize, usize, usize)>,
    pub player_pos: Option<(usize, usize)>,
    pub player_spawn: (usize, usize),
    pub stairs_pos: (usize, usize),
    pub monsters: Vec<MonsterSnapshot>,
}

/// 采集世界快照（地图、房间、玩家、出生点、楼梯、全部怪物）。
pub fn world_snapshot(world: &World) -> WorldSnapshot {
    use crate::world::init::{PlayerSpawn, StairsPos};

    let map = world.resource::<Map>();
    let tiles = map.tiles.iter().map(|row| row.to_vec()).collect::<Vec<_>>();
    let rooms = map
        .rooms
        .iter()
        .map(|room| (room.x, room.y, room.w, room.h))
        .collect::<Vec<_>>();

    let monsters = collect_monster_snapshots(world);

    WorldSnapshot {
        tiles,
        rooms,
        player_pos: player_entity(world)
            .and_then(|e| world.get::<Position>(e).map(Position::to_tuple)),
        player_spawn: world.resource::<PlayerSpawn>().0,
        stairs_pos: world.resource::<StairsPos>().0,
        monsters,
    }
}

fn collect_monster_snapshots(world: &World) -> Vec<MonsterSnapshot> {
    let mut query = world
        .try_query::<(
            &MonsterKindId,
            &Position,
            &Health,
            &Attack,
            &Agility,
            Option<&Level>,
            Option<&ExperienceReward>,
        )>()
        .expect("怪物组件已在 spawn_test_monster / 初始化中注册");

    let mut monsters: Vec<MonsterSnapshot> = query
        .iter(world)
        .map(
            |(kind, pos, health, attack, agility, level, reward)| MonsterSnapshot {
                kind: *kind,
                pos: pos.to_tuple(),
                hp: (health.current, health.max),
                attack: attack.0,
                agility: agility.0,
                level: level.map(|l| l.0).unwrap_or(0),
                exp_reward: reward.map(|r| r.0).unwrap_or(0.0),
            },
        )
        .collect();
    monsters.sort_by(|a, b| {
        (a.pos, a.kind, a.attack.to_bits()).cmp(&(b.pos, b.kind, b.attack.to_bits()))
    });
    monsters
}
