//! 移动规则（纯函数）。
//!
//! # 分层（Validate 阶段落地之后）
//!
//! | 层 | 内容 | 在哪 |
//! |---|---|---|
//! | **纯规则** | [`plan_step`] / [`can_move_to`] / [`moved_position`]：不碰 `World`，只吃「位置 + 方向 + 地图/占用图引用」 | 本模块 |
//! | **判定 + 预留** | 校验器调 [`plan_step`]，合法就立刻占住目标格 | `action::validation` |
//! | **落地** | 把定案写进 `Position` 并腾出旧格 | `action::entity::settle_action_system` |
//!
//! # 为什么规则要返回"原因"而不是 `bool`
//!
//! 因为拒绝原因要能给玩家看（ISSUES SYN6：撞墙、被挡、穿角三种情况玩家必须能区分），
//! 而且**只能有一个地方产生这个原因**。`can_move_to` 只是 [`plan_step`] 的
//! `bool` 视图，保留它是因为既有调用点与测试都在用它。
//!
//! # 为什么"判定"只能有一处
//!
//! 曾经四条移动路径（`Move` / `Chase` / `Flee` / `Wander`）各自判定、各自写
//! `Position`、各自处理"这一格被抢了"。同一件事四份实现，形状还各不相同——
//! 于是"同一回合两个 actor 挤进同一格"（ISSUES ECS45）修了两次都没修净：
//! 每次都要手工种四处守卫，而漏一处就静默失效。
//!
//! 现在判定统一发生在 `ActionPhase::Validate`（串行阶段，所以"占住"是可靠的），
//! 落地只剩一个系统。本模块因此不含任何系统：它只是"这一步能不能走"的**唯一答案**。

use crate::components::Position;
use crate::map::{MAP_HEIGHT, MAP_WIDTH, Map};
use crate::resources::OccupancyMap;

/// 移动被拒绝的原因。
///
/// **它是玩家可见的**（走 `EventLog`）：回合制游戏里"为什么这一下没动"必须能回答，
/// 否则玩家只能猜是按键丢了还是卡住了（ISSUES SYN6）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockReason {
    /// 越界（含负方向 `wrapping` 之后落到界外）。
    OutOfBounds,
    /// 目标格不可走（墙、深水、障碍）。
    Wall,
    /// 目标格被别人占着。
    Occupied,
    /// 对角移动的两个正交邻格有不可走/被占的（禁止 corner-cutting）。
    CornerCutting,
}

impl BlockReason {
    /// 给玩家看的一句话。
    pub fn describe(self) -> &'static str {
        match self {
            BlockReason::OutOfBounds => "那边是墙外，走不过去",
            BlockReason::Wall => "前面过不去",
            BlockReason::Occupied => "那里有东西挡着",
            BlockReason::CornerCutting => "贴墙角斜着走不过去",
        }
    }
}

/// 一次移动的判定结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MovePlan {
    /// 可以走，落到这一格。
    Allowed(Position),
    /// 走不了，附带原因。
    Blocked(BlockReason),
}

/// **移动规则的唯一实现**：从 `from` 向 `(dx, dy)` 走一步会怎样。
///
/// 判定顺序（顺序即可读性：越界 → 墙 → 占用 → 对角穿墙）：
///
/// ```text
///     x  y+dy     ← 必须 walkable 且未占用
///        ╲
///         ● 目标
///        ╱
///  x+dx y        ← 必须 walkable 且未占用
/// ```
///
/// 纯函数：不碰 `World`、不读组件、不写状态，因此可以逐条单测（本模块的 tests）。
pub fn plan_step(
    map: &Map,
    occupancy: &OccupancyMap,
    from: Position,
    dx: isize,
    dy: isize,
) -> MovePlan {
    let (x, y) = (from.x, from.y);
    let nx = x.wrapping_add_signed(dx);
    let ny = y.wrapping_add_signed(dy);
    if nx >= MAP_WIDTH || ny >= MAP_HEIGHT {
        return MovePlan::Blocked(BlockReason::OutOfBounds);
    }
    if !map.tiles[ny][nx].walkable() {
        return MovePlan::Blocked(BlockReason::Wall);
    }
    if occupancy.is_occupied(nx, ny) {
        return MovePlan::Blocked(BlockReason::Occupied);
    }

    if dx != 0 && dy != 0 {
        let sx = x.wrapping_add_signed(dx);
        let wy = y.wrapping_add_signed(dy);
        if sx >= MAP_WIDTH || wy >= MAP_HEIGHT {
            return MovePlan::Blocked(BlockReason::OutOfBounds);
        }
        if !map.tiles[y][sx].walkable() || occupancy.is_occupied(sx, y) {
            return MovePlan::Blocked(BlockReason::CornerCutting);
        }
        if !map.tiles[wy][x].walkable() || occupancy.is_occupied(x, wy) {
            return MovePlan::Blocked(BlockReason::CornerCutting);
        }
    }
    MovePlan::Allowed(Position::new(nx, ny))
}

/// 判断 `(x, y)` 能否向 `(dx, dy)` 移动。
///
/// [`plan_step`] 的 `bool` 视图；保留它是因为既有调用点与测试都在用这个形状。
pub fn can_move_to(
    map: &Map,
    occupancy: &OccupancyMap,
    x: usize,
    y: usize,
    dx: isize,
    dy: isize,
) -> bool {
    matches!(
        plan_step(map, occupancy, Position::new(x, y), dx, dy),
        MovePlan::Allowed(_)
    )
}

/// 合法移动的落点；非法（越界/不可走/被占用/对角穿墙）返回 `None`。
///
/// [`plan_step`] 的 `Option` 视图：让调用方不必自己重复一遍越界与偏移计算。
pub fn moved_position(
    map: &Map,
    occupancy: &OccupancyMap,
    pos: Position,
    dx: isize,
    dy: isize,
) -> Option<Position> {
    match plan_step(map, occupancy, pos, dx, dy) {
        MovePlan::Allowed(next) => Some(next),
        MovePlan::Blocked(_) => None,
    }
}

#[cfg(test)]
#[path = "movement_tests.rs"]
mod tests;
