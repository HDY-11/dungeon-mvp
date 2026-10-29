//! 常用实体查询辅助。

use crate::components::Position;
use crate::entity_cls::{Player, Stairs};
use bevy_ecs::prelude::*;

pub fn player_entity(world: &World) -> Option<Entity> {
    let mut q = world.try_query::<(Entity, &Player)>()?;
    q.iter(world).next().map(|(e, _)| e)
}

pub fn player_pos(world: &World) -> Option<(usize, usize)> {
    let mut q = world.try_query::<(&Player, &Position)>()?;
    q.iter(world).next().map(|(_, p)| (p.x, p.y))
}

pub fn on_stairs(world: &World) -> bool {
    let Some(pp) = player_pos(world) else {
        return false;
    };
    let Some(mut q) = world.try_query::<(&Stairs, &Position)>() else {
        return false;
    };
    q.iter(world).any(|(_, sp)| sp.x == pp.0 && sp.y == pp.1)
}
// 这里曾有 `update_visible_memory(&mut World)` 与 `rebuild_occupancy(&mut World)`
// 两个"World 版"辅助函数——它们是 Phase B 迁移期的产物，后来被
// `system::perception::update_visible_memory_system` 与
// `system::occupancy::rebuild_occupancy_system` 取代，但一直没删：
//
// - **没有调用者**（死代码），
// - 而且 `rebuild_occupancy` 用的是**旧的占格规则**（`Option<&Stairs>` 过滤），
//   在"占格改成 `Blocks` 能力"之后它的语义已经错了——一旦有人顺手调用，就会
//   得到与调度里那份不同的占用图，而且**没有任何报错**。
//
// 这是"第二入口"的典型：同一个职责两个实现，其中一个还是过期的。
// 已删除；占用图只能由 `system::occupancy::rebuild_occupancy_system` 与
// `action::execution::apply_move_system`（写穿）维护。
