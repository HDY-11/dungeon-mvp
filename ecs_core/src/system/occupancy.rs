//! 占用图重建。
//!
//! `OccupancyMap` 是「某格被哪个实体占着」的派生数据，每轮重建而不是增量维护，
//! 因此不会有增量更新漏改导致的幽灵占用。

use crate::components::Position;
use crate::entity_cls::Stairs;
use crate::resources::OccupancyMap;
use bevy_ecs::prelude::*;

/// 重建占用图：所有带 `Position` 的实体各占一格，**楼梯除外**。
///
/// 楼梯占格但可以站上去（G31），所以不进占用图；否则玩家会被自己的楼梯挡住。
pub fn rebuild_occupancy_system(
    entities: Query<(Entity, &Position, Option<&Stairs>)>,
    mut occupancy: ResMut<OccupancyMap>,
) {
    occupancy.clear();
    for (entity, pos, stairs) in entities.iter() {
        if stairs.is_some() {
            continue;
        }
        occupancy.set(pos.x, pos.y, entity);
    }
}