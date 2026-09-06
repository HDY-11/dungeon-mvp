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

/// 更新视野内实体的可见记忆；由系统或世界推进逻辑调用。
pub fn update_visible_memory(world: &mut World) {
    use crate::resources::VisibleMemory;
    use std::collections::HashSet;

    let visible: HashSet<(usize, usize)> = {
        let mut q = world.try_query::<(&Player, &crate::components::Viewshed)>();
        q.as_mut()
            .and_then(|q| q.iter(world).next())
            .map(|(_, v)| v.visible_tiles.iter().copied().collect())
            .unwrap_or_default()
    };

    let entities: Vec<(Entity, (usize, usize))> = {
        let mut q = world.try_query::<(Entity, &Position, Option<&Player>)>();
        let Some(q) = q.as_mut() else {
            return;
        };
        q.iter(world)
            .filter(|(_, pos, player)| player.is_none() && visible.contains(&(pos.x, pos.y)))
            .map(|(e, pos, _)| (e, (pos.x, pos.y)))
            .collect()
    };

    let alive: HashSet<Entity> = {
        let mut q = world.try_query::<(Entity,)>();
        q.as_mut()
            .map(|q| q.iter(world).map(|(e,)| e).collect())
            .unwrap_or_default()
    };

    let mut memory = world.resource_mut::<VisibleMemory>();
    for (entity, pos) in entities {
        memory.entries.insert(entity, pos);
    }
    memory.entries.retain(|e, _| alive.contains(e));
}

/// 全量重建碰撞占用图。
pub fn rebuild_occupancy(world: &mut World) {
    use crate::entity_cls::Stairs;
    use crate::resources::OccupancyMap;

    let positions: Vec<(Entity, usize, usize)> = {
        let Some(mut q) = world.try_query::<(Entity, &Position, Option<&Stairs>)>() else {
            return;
        };
        q.iter(world)
            .filter(|(_, _, stairs)| stairs.is_none())
            .map(|(e, p, _)| (e, p.x, p.y))
            .collect()
    };

    let mut occupancy = world.resource_mut::<OccupancyMap>();
    occupancy.clear();
    for (entity, x, y) in positions {
        occupancy.set(x, y, entity);
    }
}
