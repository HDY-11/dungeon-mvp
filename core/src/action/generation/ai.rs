//! 行动条件判定（纯查询辅助）。
//!
//! C8 删除了旧 AI 决策（`decide_monster_actions` / `choose_action`）与
//! actor 上的行动挂载；这里只保留**被 action 实体链路复用的条件函数**。

use crate::balance::{FLEE_HP_RATIO, FLEE_HP_RATIO_EXIT};
use crate::components::{Health, LastKnownPlayerPos, Viewshed};
use crate::world::query::player_pos;
use bevy_ecs::prelude::*;

/// 追击保活条件：仍能看到玩家，或仍有最后已知位置。
pub fn chase_condition(world: &World, entity: Entity) -> bool {
    player_visible_to(world, entity)
        || world
            .get::<LastKnownPlayerPos>(entity)
            .map(|last_known| last_known.0.is_some())
            .unwrap_or(false)
}

/// 逃跑保活条件（滞回退出阈值）。
pub fn flee_condition(world: &World, entity: Entity) -> bool {
    world
        .get::<Health>(entity)
        .map(|health| health.ratio() < FLEE_HP_RATIO_EXIT)
        .unwrap_or(false)
}

/// 逃跑决策条件（进入阈值）。
pub fn wants_to_flee(world: &World, entity: Entity) -> bool {
    world
        .get::<Health>(entity)
        .map(|health| health.ratio() < FLEE_HP_RATIO)
        .unwrap_or(false)
}

/// 实体视野里是否包含玩家。
pub fn player_visible_to(world: &World, entity: Entity) -> bool {
    let Some(player_position) = player_pos(world) else {
        return false;
    };
    world
        .get::<Viewshed>(entity)
        .map(|viewshed| viewshed.can_see(player_position))
        .unwrap_or(false)
}
