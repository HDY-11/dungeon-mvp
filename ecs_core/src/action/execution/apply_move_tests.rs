//! `super`（`apply_move`）的测试：**移动的唯一写入点**。
//!
//! 这里放着 ISSUES ECS45 的复现断言：同一回合内两个 actor 抢同一格，
//! 只能有一个成功，且**不允许两个实体共格**。

use super::*;
use crate::components::Blocks;
use bevy_ecs::schedule::ScheduleLabel;
use crate::map::{MAP_HEIGHT, MAP_WIDTH, Tile};

/// 造一个全地板的世界 + 占用图（`Blocks` 由测试自己挂，见下）。
fn floor_world() -> (World, Map, OccupancyMap) {
    let mut world = World::new();
    let mut map = Map::new();
    map.tiles = [[Tile::Floor; MAP_WIDTH]; MAP_HEIGHT];
    world.insert_resource(Events::<MoveRequest>::default());
    world.insert_resource(EventLog::new());
    let occupancy = OccupancyMap::new();
    (world, map, occupancy)
}

/// 建一个会占格的 actor（带 `Blocks` + `Position`）。
fn spawn_actor(world: &mut World, pos: (usize, usize)) -> Entity {
    world
        .spawn((Blocks, Position::new(pos.0, pos.1)))
        .id()
}

/// 跑一次 `apply_move_system`（地图与占用图**移入**世界：它们都是资源）。
fn run(world: &mut World, map: Map, occupancy: OccupancyMap) {
    world.insert_resource(map);
    world.insert_resource(occupancy);
    let mut schedule = Schedule::new(TestSchedule);
    schedule.add_systems(apply_move_system);
    world.add_schedule(schedule);
    world.run_schedule(TestSchedule);
}

#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
struct TestSchedule;

/// **ECS45 的复现断言**：同一回合两个 actor 抢同一格 → 只有一个进去。
///
/// 这条测试在重构前**写不出来**（没有 `MoveRequest` 这个单元，判定与写入散在四个
/// 执行器里），所以当时"修没修好"只能靠试玩脚本撞运气。
#[test]
fn two_actors_cannot_enter_the_same_cell_in_one_round() {
    let (mut world, map, mut occupancy) = floor_world();
    let first = spawn_actor(&mut world, (10, 10));
    let second = spawn_actor(&mut world, (10, 12));
    occupancy.set(10, 10, first);
    occupancy.set(10, 12, second);

    // 两个请求都想走到 (10,11)。
    world.send_event(MoveRequest::new(first, 0, 1));
    world.send_event(MoveRequest::new(second, 0, -1));
    run(&mut world, map, occupancy);

    let first_pos = world.get::<Position>(first).unwrap().to_tuple();
    let second_pos = world.get::<Position>(second).unwrap().to_tuple();
    assert_ne!(first_pos, second_pos, "两个实体不得共用同一格");
    assert_eq!(
        first_pos,
        (10, 11),
        "先处理的请求应当成功（顺序由调度固定）"
    );
    assert_eq!(second_pos, (10, 12), "后处理的请求应当被挡住，原地不动");
}

/// 移动成功后**占用图立刻更新**（写穿）：不需要等到轮末重建。
#[test]
fn occupancy_is_updated_immediately_after_a_move() {
    let (mut world, map, mut occupancy) = floor_world();
    let actor = spawn_actor(&mut world, (10, 10));
    occupancy.set(10, 10, actor);

    world.send_event(MoveRequest::new(actor, 1, 0));
    run(&mut world, map, occupancy);

    let occupancy_after = world.resource::<OccupancyMap>();
    assert_eq!(
        occupancy_after.entity_at(11, 10),
        Some(actor),
        "新格必须立刻登记（写穿）"
    );
    assert_eq!(
        occupancy_after.entity_at(10, 10),
        None,
        "旧格必须立刻清空"
    );
}

/// 被拒时**玩家**能看到原因（ISSUES SYN6）；原因来自规则，不是调用点各写一句。
#[test]
fn a_blocked_player_move_reports_a_reason() {
    let (mut world, mut map, mut occupancy) = floor_world();
    map.tiles[10][11] = Tile::Wall;
    let actor = spawn_actor(&mut world, (10, 10));
    world.entity_mut(actor).insert(Player);
    occupancy.set(10, 10, actor);

    world.send_event(MoveRequest::new(actor, 1, 0));
    run(&mut world, map, occupancy);

    let log = world.resource::<EventLog>();
    let last = log.messages.last().expect("必须有拒绝消息");
    assert!(
        last.text.contains("过不去") || last.text.contains("挡着"),
        "拒绝消息必须说原因，实际：{}",
        last.text
    );
    assert_eq!(
        world.get::<Position>(actor).unwrap().to_tuple(),
        (10, 10),
        "被拒时位置不得改变"
    );
}

/// 已经消失的 actor 发来的请求必须被安静丢弃（同一轮里可能被打死）。
#[test]
fn requests_from_despawned_actors_are_dropped() {
    let (mut world, map, occupancy) = floor_world();
    let ghost = world.spawn_empty().id();
    world.send_event(MoveRequest::new(ghost, 1, 0));
    run(&mut world, map, occupancy);
    assert!(!world.resource::<EventLog>().messages.iter().any(|m| m.text.contains("过不去")));
}
