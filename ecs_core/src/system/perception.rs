//! 视野、地图记忆、可见记忆。
//!
//! 三者是一条链：`fov_system` 按真实位置重算每个实体的 `Viewshed`，
//! 再由玩家视野更新「永久记忆」（哪些格探索过）与「可见记忆」（上一帧看见谁）。
//!
//! **顺序要紧**：`fov_system` 必须先跑——它按实体的真实 `Position` 重算视野，
//! 先设 `Viewshed` 再跑它就会被覆盖（Phase C 写测试时踩过）。

use crate::components::{Position, Viewshed};
use crate::entity_cls::Player;
use crate::map::Map;
use crate::resources::{MapMemory, VisibleMemory};
use crate::spatial::fov::calculate_visible_tiles;
use bevy_ecs::prelude::*;
use std::collections::HashSet;

/// 按实体当前位置重算视野。
pub fn fov_system(mut query: Query<(&Position, &mut Viewshed)>, map: Res<Map>) {
    for (pos, mut viewshed) in query.iter_mut() {
        viewshed.visible_tiles = calculate_visible_tiles(pos.x, pos.y, viewshed.range, &map);
    }
}

/// 把玩家当前可见的格子标为已探索（永久记忆，不会随视野变化回退）。
pub fn update_map_memory_system(players: Query<(&Player, &Viewshed)>, mut memory: ResMut<MapMemory>) {
    for (_, viewshed) in players.iter() {
        for &(x, y) in &viewshed.visible_tiles {
            memory.explored[y][x] = true;
        }
    }
}

/// 记录「玩家这一帧能看见哪些非玩家实体」，并清掉已死亡实体的记录。
pub fn update_visible_memory_system(
    players: Query<(&Player, &Viewshed)>,
    entities: Query<(Entity, &Position, Option<&Player>)>,
    alive: Query<(Entity,)>,
    mut memory: ResMut<VisibleMemory>,
) {
    let visible: HashSet<(usize, usize)> = players
        .iter()
        .next()
        .map(|(_, v)| v.visible_tiles.iter().copied().collect())
        .unwrap_or_default();

    let seen: Vec<(Entity, (usize, usize))> = entities
        .iter()
        .filter(|(_, pos, player)| player.is_none() && visible.contains(&(pos.x, pos.y)))
        .map(|(entity, pos, _)| (entity, (pos.x, pos.y)))
        .collect();

    memory.entries.clear();
    for (entity, pos) in seen {
        memory.entries.insert(entity, pos);
    }

    // 已消失的实体不能留在记忆里（否则渲染层会去查一个不存在的实体）。
    let alive: HashSet<Entity> = alive.iter().map(|(entity,)| entity).collect();
    memory.entries.retain(|e, _| alive.contains(e));
}