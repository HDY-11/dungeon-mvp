//! **下楼**：换一层 = 保留玩家，重建其余一切。
//!
//! # 为什么不是"再建一整个世界"
//!
//! 玩家的状态（等级/经验/生命/背包/装备）跨层延续，而地图/怪物/地面物品/泥潭/楼梯
//! 每层重来。所以下楼既不是 `new_game`（那会把玩家也重置），也不是"什么都不做"。
//!
//! 实现上用**既有的初始化系统**重建层内容，而不是再写一套生成逻辑：
//!
//! ```text
//! 清场（怪物/地面物品/楼梯/格子效果）→ 楼层 +1 → generate_map
//!     → spawn_stairs → spawn_cell_effects → spawn_ground_items → spawn_monsters
//!     → 玩家搬到新出生点 + 视野记忆重置 + 占用图/格索引重建
//! ```
//!
//! 顺序与 `build_init_schedule` 一致（避开出生点、避开楼梯、避开泥潭这些约束都在
//! 那几个系统自己的实现里），只是**跳过 `spawn_player_system`**——玩家已经在了。
//!
//! # 确定性
//!
//! 每层的内容种子由 `(MapSeed, floor)` 派生（`map_seed_for_floor` 与
//! `resources::derived_rng` 的 `DerivePurpose`），所以"第几层"完全决定"那一层长什么样"：
//! 同一颗种子下，第 3 层永远一样，与你在前两层做过什么无关。这也是存档只需要
//! `(seed, floor, 玩家状态)` 的原因。
//!
//! # 状态效果的跨层取舍
//!
//! 中毒这类持续效果挂在玩家名下（`EffectOf` 关系，随玩家存活），所以**跨层保留**。
//! 这是有意的：换层不该变成"解毒"。要改成清空，得先回答"换层算不算经过时间"
//! （AV 时间轴要不要跳），那是另一条设计决策。

use crate::components::*;
use crate::entity_cls::{Monster, Stairs};
use crate::map::Map;
use crate::events::{ActionFailedEvent, ActionSucceededEvent, AttackEvent, AttackIntentEvent, DeathEvent, LevelUpEvent};
use crate::resources::{EventLog, EventMessage, FloorNumber, MapMemory, TurnManager, VisibleMemory};
use crate::world::init::{
    PlayerSpawn, generate_map_system, spawn_cell_effects_system, spawn_ground_items_system,
    spawn_monsters_system, spawn_stairs_system,
};
use bevy_ecs::prelude::*;
use bevy_ecs::system::RunSystemOnce;

/// 玩家是否站在楼梯上。
pub fn player_on_stairs(world: &mut World) -> bool {
    let Some(player) = crate::world::query::player_entity(world) else {
        return false;
    };
    let Some(position) = world.get::<Position>(player).map(Position::to_tuple) else {
        return false;
    };
    let mut query = world.query_filtered::<&Position, With<Stairs>>();
    query
        .iter(world)
        .any(|stairs| stairs.to_tuple() == position)
}

/// 下楼。返回 `true` 表示真的换了一层。
///
/// 三个前置条件（不满足就**什么也不做**并给玩家一句话）：
/// 玩家存在且活着、玩家不在行动中、玩家站在楼梯上。
pub fn descend(world: &mut World) -> bool {
    if world.resource::<TurnManager>().game_over {
        return false;
    }
    if crate::world_loop::player_is_busy(world) {
        return false;
    }
    let Some(player) = crate::world::query::player_entity(world) else {
        return false;
    };
    if !player_on_stairs(world) {
        world
            .resource_mut::<EventLog>()
            .push(EventMessage::system("这里没有楼梯，下不去"));
        return false;
    }

    // ① 清掉本层的内容（玩家与它名下的装备/效果不动）。
    despawn_floor_contents(world);

    // ② 楼层 +1：之后所有生成系统都会按新楼层派生种子。
    let floor = {
        let mut floor = world.resource_mut::<FloorNumber>();
        floor.0 = floor.0.saturating_add(1);
        floor.0
    };
    log::info!("下楼: 进入第 {floor} 层");

    // ③ 重建层内容（跳过 `spawn_player_system`：玩家已经在了）。
    let _ = world.run_system_once(generate_map_system);

    // 出生点由新地图决定；先写进资源，后面的"避开出生点"约束才看得见。
    let spawn = world.resource::<Map>().spawn_point();
    world.resource_mut::<PlayerSpawn>().0 = spawn;

    let _ = world.run_system_once(spawn_stairs_system);
    let _ = world.run_system_once(spawn_cell_effects_system);
    let _ = world.run_system_once(spawn_ground_items_system);
    let _ = world.run_system_once(spawn_monsters_system);

    // ④ 玩家落位 + 行动状态归零 + 视野记忆重置。
    if let Some(mut position) = world.get_mut::<Position>(player) {
        position.x = spawn.0;
        position.y = spawn.1;
    }
    world
        .entity_mut(player)
        .remove::<Active>()
        .remove::<Failure>()
        .remove::<Ready>()
        .remove::<ActionTimer>()
        .insert(Idle);
    // 行动实体（正常情况下不存在：上楼前已确认不忙碌）一并清掉，避免残留子实体。
    despawn_player_actions(world, player);

    world.insert_resource(MapMemory::new());
    world.insert_resource(VisibleMemory::default());
    clear_events(world);

    // ⑤ 派生数据重建：视野 / 记忆 / 占用图 / 格索引。
    let _ = world.run_system_once(crate::system::fov_system);
    let _ = world.run_system_once(crate::system::update_map_memory_system);
    let _ = world.run_system_once(crate::system::update_visible_memory_system);
    let _ = world.run_system_once(crate::system::rebuild_occupancy_system);
    let _ = world.run_system_once(crate::effect::rebuild_cell_effects_system);

    world
        .resource_mut::<EventLog>()
        .push(EventMessage::system(format!("你走下楼梯，来到第 {floor} 层")));
    true
}

/// 清掉本层的怪物、地面物品、楼梯与格子效果。
///
/// 用"按组件白名单逐个查删"而不是 `world.clear_entities()`：玩家、它名下的装备与效果
/// 必须活着——这正是"保留玩家"的实现方式。
fn despawn_floor_contents(world: &mut World) {
    let mut doomed: Vec<Entity> = Vec::new();
    {
        let mut query = world.query_filtered::<Entity, With<Monster>>();
        doomed.extend(query.iter(world));
    }
    {
        let mut query = world.query_filtered::<Entity, With<crate::item::GroundItemAt>>();
        doomed.extend(query.iter(world));
    }
    {
        let mut query = world.query_filtered::<Entity, With<Stairs>>();
        doomed.extend(query.iter(world));
    }
    {
        let mut query = world.query_filtered::<Entity, With<crate::effect::CellEffectAt>>();
        doomed.extend(query.iter(world));
    }
    for entity in doomed {
        // 已经不在的实体（例如随关系级联消失）跳过，不 panic。
        if world.get_entity(entity).is_ok() {
            world.despawn(entity);
        }
    }
}

/// 清掉玩家名下的行动实体（`ActionOf(player)` 的那些）。
fn despawn_player_actions(world: &mut World, player: Entity) {
    let mut doomed: Vec<Entity> = Vec::new();
    {
        let mut query = world.query::<(Entity, &crate::action::ActionOf)>();
        for (entity, of) in query.iter(world) {
            if of.parent() == player {
                doomed.push(entity);
            }
        }
    }
    for entity in doomed {
        if world.get_entity(entity).is_ok() {
            world.despawn(entity);
        }
    }
}

/// 清空六个事件缓冲：跨层的旧事件（上一层的伤害/死亡/终态）不该在新层继续被读。
fn clear_events(world: &mut World) {
    world.resource_mut::<Events<AttackIntentEvent>>().clear();
    world.resource_mut::<Events<AttackEvent>>().clear();
    world.resource_mut::<Events<DeathEvent>>().clear();
    world.resource_mut::<Events<LevelUpEvent>>().clear();
    world.resource_mut::<Events<ActionSucceededEvent>>().clear();
    world.resource_mut::<Events<ActionFailedEvent>>().clear();
}

#[cfg(test)]
#[path = "floor_tests.rs"]
mod tests;
