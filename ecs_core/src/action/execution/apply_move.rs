//! 移动的**唯一写入点**（ISSUES ECS45）。
//!
//! # 为什么这里是唯一的
//!
//! 四条移动路径（`Move` / `Chase` / `Flee` / `Wander`）**只发意图**，不碰 `Position`：
//!
//! ```text
//! execute_*_system  ──MoveRequest{actor, dx, dy}──▶  apply_move_system
//!                                                        │
//!                                          plan_step（纯规则，带原因）
//!                                                        │
//!                              唯一写 Position + 写穿 OccupancyMap
//! ```
//!
//! 这与**攻击链路**同构：`execute_basic_attack_system` 只发 `AttackIntentEvent`，
//! 扣血只有 `apply_damage_system` 一处。移动此前是唯一"执行器直接改状态"的子系统，
//! 于是四条路径各写一遍判定、各写一遍 `Position`、各写一遍"这格被抢了"——
//! 同一件事四份实现，形状还各不相同（`if claim(..)` / `if claim(..) && let ..` /
//! `&& claim(..)` / 压根没有）。这就是为什么"同一回合两个 actor 挤进同一格"
//! 修了两次都没修净：每次都要手工种四处守卫，漏一处就**静默**失效。
//!
//! # 为什么这样就够了（不需要额外的"认领表"）
//!
//! 判定与写入在**同一个循环、同一次调用**里完成：前一个请求写穿了占用图，
//! 后一个请求的 `plan_step` 立刻就能看见。也就是说"检查"和"占位"之间没有窗口，
//! 不需要第二个派生状态来记录"本回合谁被占过"。
//!
//! 代价：`OccupancyMap` 从"纯派生、每轮重建"变成"每轮重建 **+** 移动时写穿"
//! （DESIGN DsnE2 的措辞已随之更新）。

use crate::action::execution::movement::{MovePlan, plan_step};
use crate::components::Position;
use crate::entity_cls::Player;
use crate::map::Map;
use crate::resources::{EventLog, EventMessage, OccupancyMap};
use bevy_ecs::prelude::*;

/// 一次移动意图：某个 actor 想向 `(dx, dy)` 走一步。
///
/// 执行器只描述**意图**，能不能走由 [`apply_move_system`] 判定——所以执行器里
/// 那份判定只用于决定"这个行动算成功还是失败"，**不再决定是否写位置**。
#[derive(Event, Debug, Clone, Copy, PartialEq, Eq)]
pub struct MoveRequest {
    pub actor: Entity,
    pub dx: isize,
    pub dy: isize,
}

impl MoveRequest {
    pub const fn new(actor: Entity, dx: isize, dy: isize) -> Self {
        Self { actor, dx, dy }
    }
}

/// 消费 [`MoveRequest`]：判定 → 写 `Position` → **写穿占用图**。
///
/// 这是全项目**唯一**写 `Position` 的系统（生成期 spawn 除外）。
pub fn apply_move_system(
    mut requests: EventReader<MoveRequest>,
    map: Res<Map>,
    mut occupancy: ResMut<OccupancyMap>,
    mut actors: Query<(&mut Position, Has<Player>)>,
    mut event_log: ResMut<EventLog>,
) {
    for request in requests.read() {
        let Ok((mut position, is_player)) = actors.get_mut(request.actor) else {
            // actor 已在同一轮里消失（例如被打死并 despawn）：请求作废。
            log::debug!("移动请求作废：actor {:?} 已不存在", request.actor);
            continue;
        };
        let from = *position;

        match plan_step(&map, &occupancy, from, request.dx, request.dy) {
            MovePlan::Allowed(next) => {
                // 写穿：先腾出旧格、再占住新格，**在这一步就生效**。
                // 于是同一回合里后续的请求看到的是最新占位，不可能叠格。
                occupancy.clear_at(from.x, from.y);
                occupancy.set(next.x, next.y, request.actor);
                position.x = next.x;
                position.y = next.y;
                log::debug!(
                    "移动: actor={:?} ({},{}) → ({},{})",
                    request.actor,
                    from.x,
                    from.y,
                    next.x,
                    next.y
                );
            }
            MovePlan::Blocked(reason) => {
                // 玩家必须知道"为什么没动"（SYN6）；怪物的拒绝只进调试日志。
                if is_player {
                    event_log.push(EventMessage::system(reason.describe()));
                }
                log::debug!(
                    "移动被拒: actor={:?} 方向=({},{}) 原因={reason:?}",
                    request.actor,
                    request.dx,
                    request.dy
                );
            }
        }
    }
}

#[cfg(test)]
#[path = "apply_move_tests.rs"]
mod tests;
