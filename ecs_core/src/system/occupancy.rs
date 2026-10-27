//! 占用图重建。
//!
//! `OccupancyMap` 是「某格被哪个实体占着」的派生数据，每轮重建而不是增量维护，
//! 因此不会有增量更新漏改导致的幽灵占用。
//!
//! **谁占格由实体自己声明**（`Blocks`），不是"有 `Position` 就占"：楼梯有位置但可被站
//! 上去，地面物品与格子效果也各有自己的位置组件。以前这里靠一句
//! `if stairs.is_some() { continue }` 打补丁，现在那个特判没了——占格是一种**能力**，
//! 不是"你有没有 `Position`"这个组件集合的副作用。

use crate::components::{Blocks, Position};
use crate::resources::OccupancyMap;
use bevy_ecs::prelude::*;

/// 重建占用图：所有带 [`Blocks`] 的实体各占一格，**每轮重建**。
pub fn rebuild_occupancy_system(
    entities: Query<(Entity, &Position), With<Blocks>>,
    mut occupancy: ResMut<OccupancyMap>,
    mut claims: ResMut<crate::resources::RoundClaims>,
) {
    occupancy.clear();
    // 回合边界：批内预留（ISSUES ECS45）归零。
    claims.clear();
    for (entity, pos) in entities.iter() {
        occupancy.set(pos.x, pos.y, entity);
    }
}
