//! `super`（`movement`）的测试：移动规则的纯函数部分。
//!
//! 这些测试**不建 `World`**：规则不碰组件才测得快、测得准（DESIGN ECS28 的红利）。
//! 写入侧（谁真正改 `Position`）在 `apply_move` 的测试里。

use super::*;
use crate::map::Tile;
use bevy_ecs::prelude::*;

/// 全墙 + 指定格挖成地板；返回 `(map, occupancy)`。
fn scene(floors: &[(usize, usize)]) -> (Map, OccupancyMap) {
    let mut map = Map::new();
    for &(x, y) in floors {
        map.tiles[y][x] = Tile::Floor;
    }
    (map, OccupancyMap::new())
}

#[test]
fn moved_position_returns_target_on_legal_move() {
    let (map, occupancy) = scene(&[(10, 10), (11, 10)]);
    let next = moved_position(&map, &occupancy, Position::new(10, 10), 1, 0);
    assert_eq!(next, Some(Position::new(11, 10)));
}

#[test]
fn moved_position_rejects_wall_and_out_of_bounds() {
    let (map, occupancy) = scene(&[(10, 10)]);
    // 目标格是墙。
    assert_eq!(
        moved_position(&map, &occupancy, Position::new(10, 10), 1, 0),
        None
    );
    // 越界（负方向 wrapping 后落到界外）。
    assert_eq!(
        moved_position(&map, &occupancy, Position::new(0, 0), -1, 0),
        None
    );
    assert_eq!(
        moved_position(
            &map,
            &occupancy,
            Position::new(MAP_WIDTH - 1, MAP_HEIGHT - 1),
            1,
            1
        ),
        None
    );
}

#[test]
fn moved_position_rejects_occupied_target() {
    let (map, mut occupancy) = scene(&[(10, 10), (11, 10)]);
    let mut world = World::new();
    let blocker = world.spawn_empty().id();
    occupancy.set(11, 10, blocker);
    assert_eq!(
        moved_position(&map, &occupancy, Position::new(10, 10), 1, 0),
        None
    );
    // 其余三方向同样验证：占用判定与方向无关。
    let (map, mut occupancy) = scene(&[(10, 10), (10, 9), (9, 10), (10, 11)]);
    for (px, py) in [(10, 9), (9, 10), (10, 11)] {
        occupancy.set(px, py, blocker);
    }
    assert_eq!(
        moved_position(&map, &occupancy, Position::new(10, 10), 0, 1),
        None
    );
    assert_eq!(
        moved_position(&map, &occupancy, Position::new(10, 10), 1, 0),
        None
    );
    assert_eq!(
        moved_position(&map, &occupancy, Position::new(10, 10), 0, -1),
        None
    );
}

/// 对角移动禁止 corner-cutting：两个正交邻格任一不可走/被占用，整步作废。
#[test]
fn moved_position_forbids_corner_cutting() {
    // 目标与两个正交邻格都是地板 → 允许。
    let (map, occupancy) = scene(&[(10, 10), (11, 11), (10, 11), (11, 10)]);
    assert_eq!(
        moved_position(&map, &occupancy, Position::new(10, 10), 1, 1),
        Some(Position::new(11, 11))
    );

    // 正交邻格 (11,10) 是墙 → 拒绝。
    let (map, occupancy) = scene(&[(10, 10), (11, 11), (10, 11)]);
    assert_eq!(
        moved_position(&map, &occupancy, Position::new(10, 10), 1, 1),
        None
    );

    // 正交邻格被占用 → 拒绝。
    let (map, mut occupancy) = scene(&[(10, 10), (11, 11), (10, 11), (11, 10)]);
    let mut world = World::new();
    let blocker = world.spawn_empty().id();
    occupancy.set(10, 11, blocker);
    assert_eq!(
        moved_position(&map, &occupancy, Position::new(10, 10), 1, 1),
        None
    );
}

/// **判定必须带原因**（ISSUES SYN6）：四种拒绝各有各的原因，玩家才能区分
/// "撞墙"和"被东西挡住"。改造前这里只能得到 `false`。
#[test]
fn plan_step_reports_why_a_move_is_blocked() {
    // (9,11) 不挖：对角测试会先撞墙，所以对角那条单独用另一个场景。
    let (map, mut occupancy) = scene(&[(10, 10), (11, 10), (10, 11), (11, 11), (12, 10)]);
    let from = Position::new(10, 10);

    // 合法
    assert_eq!(
        plan_step(&map, &occupancy, from, 1, 0),
        MovePlan::Allowed(Position::new(11, 10))
    );

    // 墙：向上是墙（(10,9) 没挖）
    assert_eq!(
        plan_step(&map, &occupancy, from, 0, -1),
        MovePlan::Blocked(BlockReason::Wall)
    );

    // 越界
    assert_eq!(
        plan_step(&map, &occupancy, Position::new(0, 0), -1, 0),
        MovePlan::Blocked(BlockReason::OutOfBounds)
    );

    // 占用
    let mut world = World::new();
    occupancy.set(10, 11, world.spawn_empty().id());
    assert_eq!(
        plan_step(&map, &occupancy, from, 0, 1),
        MovePlan::Blocked(BlockReason::Occupied)
    );
    // 对角：目标 (11,11) 可走，但正交邻格 (10,11) 已被占 → corner-cutting 拒
    // （**不是** `Occupied`：玩家需要知道"那格空着但绕不过去"）。
    assert_eq!(
        plan_step(&map, &occupancy, from, 1, 1),
        MovePlan::Blocked(BlockReason::CornerCutting)
    );
}

/// `can_move_to` / `moved_position` 必须是 `plan_step` 的两个视图，不能各自演化出
/// 第二套规则（那正是 ECS45 之前四份实现的病根）。
#[test]
fn the_three_entry_points_agree() {
    let (mut map, mut occupancy) = scene(&[(10, 10), (11, 10), (10, 11), (11, 11)]);
    // 再加一点不对称：让 (12,10) 成为墙、(11,11) 有占用
    map.tiles[10][12] = Tile::Wall;
    occupancy.set(11, 11, World::new().spawn_empty().id());

    for (x, y) in [(10usize, 10usize), (11, 11), (12, 10)] {
        for dx in -1isize..=1 {
            for dy in -1isize..=1 {
                let from = Position::new(x, y);
                let plan = plan_step(&map, &occupancy, from, dx, dy);
                let allowed = matches!(plan, MovePlan::Allowed(_));
                assert_eq!(
                    can_move_to(&map, &occupancy, x, y, dx, dy),
                    allowed,
                    "can_move_to 与 plan_step 不一致 @({x},{y}) 方向({dx},{dy})"
                );
                assert_eq!(
                    moved_position(&map, &occupancy, from, dx, dy).is_some(),
                    allowed,
                    "moved_position 与 plan_step 不一致 @({x},{y}) 方向({dx},{dy})"
                );
            }
        }
    }
    let _ = &mut map;
}
