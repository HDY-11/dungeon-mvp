//! 移动规则与执行。
//!
//! 分两层（Phase C 起沿用这个分层）：
//!
//! 1. **纯规则**：[`can_move_to`] 与 [`moved_position`] 不碰 `World`、不读组件，
//!    只吃「位置 + 方向 + 地图/占用图引用」，因此可单测、可被任意执行器复用；
//! 2. **薄执行器**：[`execute_move`] 是迁移期的 `&mut World` 版本（旧
//!    `decide_monster_actions` 链路在用）；action 实体链路用参数化执行器
//!    `crate::action::entity::execute_move_system`，它只调这两个纯函数。
//!
//! 分层的目的：让「移动规则」成为单一份权威实现，执行器形态（`World` vs `Query`）
//! 不再决定规则住在哪里。

use crate::components::Position;
use crate::map::{MAP_HEIGHT, MAP_WIDTH, Map};
use crate::resources::OccupancyMap;
use bevy_ecs::prelude::*;

/// 判断 `(x, y)` 能否向 `(dx, dy)` 移动。
///
/// 对角移动要求两个正交邻格均可通行且未被占用，禁止 corner-cutting：
///
/// ```text
///     x  y+dy     ← 必须 walkable 且未占用
///        ╲
///         ● 目标
///        ╱
///  x+dx y        ← 必须 walkable 且未占用
/// ```
///
/// 越界（含 `wrapping` 后落到 `usize::MAX` 的负方向）一律返回 `false`。
pub fn can_move_to(
    map: &Map,
    occupancy: &OccupancyMap,
    x: usize,
    y: usize,
    dx: isize,
    dy: isize,
) -> bool {
    let nx = x.wrapping_add_signed(dx);
    let ny = y.wrapping_add_signed(dy);
    if nx >= MAP_WIDTH || ny >= MAP_HEIGHT {
        return false;
    }
    if !map.tiles[ny][nx].walkable() {
        return false;
    }
    if occupancy.is_occupied(nx, ny) {
        return false;
    }

    if dx != 0 && dy != 0 {
        let sx = x.wrapping_add_signed(dx);
        let wy = y.wrapping_add_signed(dy);
        if sx >= MAP_WIDTH || wy >= MAP_HEIGHT {
            return false;
        }
        if !map.tiles[y][sx].walkable() || occupancy.is_occupied(sx, y) {
            return false;
        }
        if !map.tiles[wy][x].walkable() || occupancy.is_occupied(x, wy) {
            return false;
        }
    }
    true
}

/// 合法移动的落点；非法（越界/不可走/被占用/对角穿墙）返回 `None`。
///
/// 纯函数：`can_move_to` 的「通过则给出新坐标」形态，让执行器不必自己重复
/// 一遍越界与偏移计算（此前 `execute_move` 与 action 实体执行器各自算一次，
/// 是同类逻辑双实现）。
pub fn moved_position(
    map: &Map,
    occupancy: &OccupancyMap,
    pos: Position,
    dx: isize,
    dy: isize,
) -> Option<Position> {
    if !can_move_to(map, occupancy, pos.x, pos.y, dx, dy) {
        return None;
    }
    let (nx, ny) = pos.offset(dx, dy);
    Some(Position::new(nx, ny))
}

/// 执行移动。成功返回 `true`。
///
/// 迁移期的 `&mut World` 版本：读 `Position`/`Map`/`OccupancyMap` 后写 `Position`。
/// 规则本身在 [`moved_position`]，这里只负责读写世界。
pub fn execute_move(world: &mut World, entity: Entity, dx: isize, dy: isize) -> bool {
    let Some(pos) = world.get::<Position>(entity).copied() else {
        return false;
    };
    let Some(next) = ({
        let map = world.resource::<Map>();
        let occupancy = world.resource::<OccupancyMap>();
        moved_position(map, occupancy, pos, dx, dy)
    }) else {
        return false;
    };

    if let Some(mut position) = world.get_mut::<Position>(entity) {
        position.x = next.x;
        position.y = next.y;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::Tile;

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

    /// `execute_move`（World 版）只是「读 + 用纯规则 + 写」：成功才改位置。
    #[test]
    fn world_based_move_applies_pure_rule_result() {
        let mut world = crate::test_util::test_world();
        crate::test_util::fill_map(&mut world, Tile::Wall);
        world.resource_mut::<Map>().tiles[10][10] = Tile::Floor;
        world.resource_mut::<Map>().tiles[10][11] = Tile::Floor;

        let actor = world.spawn(Position::new(10, 10)).id();
        assert!(execute_move(&mut world, actor, 1, 0));
        assert_eq!(world.get::<Position>(actor).unwrap().to_tuple(), (11, 10));

        // 撞墙失败：位置不变。
        assert!(!execute_move(&mut world, actor, 0, 1));
        assert_eq!(world.get::<Position>(actor).unwrap().to_tuple(), (11, 10));

        // 没有 Position 的实体：直接 false，不 panic。
        let bare = world.spawn_empty().id();
        assert!(!execute_move(&mut world, bare, 1, 0));
    }
}
