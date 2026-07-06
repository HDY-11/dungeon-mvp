//! 玩家 tap-tap 行动处理

use dungeon_core::{
    action_types::*, components::*,
    Map, Tile, OccupancyMap, MAP_WIDTH, MAP_HEIGHT, Monster,
};
use bevy_ecs::prelude::*;

/// tap-tap 核心：返回 true 表示确认入队
pub fn handle_timed_action(world: &mut World, entity: Entity, kind: ActionKindV3, av: f32) -> bool {
    let is_confirm = {
        let preview = world.resource::<PlayerPreview>();
        match (&preview.kind, &kind) {
            (Some(ActionKindV3::Move { dx: pd, dy: pd2 }), ActionKindV3::Move { dx, dy })
                if pd == dx && pd2 == dy => true,
            (Some(ActionKindV3::Wait), ActionKindV3::Wait) => true,
            (Some(ActionKindV3::Skill(a)), ActionKindV3::Skill(b)) if a == b => true,
            (Some(ActionKindV3::Attack { .. }), ActionKindV3::Attack { .. }) => true,
            _ => false,
        }
    };

    if is_confirm {
        world.resource_mut::<ActionQueue>().enqueue_if_absent(entity, kind, av);
        world.resource_mut::<PlayerPreview>().kind = None;
        true
    } else {
        world.resource_mut::<PlayerPreview>().kind = Some(kind);
        false
    }
}

/// 方向键 tap-tap：返回 true 表示确认了行动
pub fn handle_player_direction(world: &mut World, dx: isize, dy: isize) -> bool {
    let Some(entity) = dungeon_core::ops::player_entity(world) else { return false };

    let kind = {
        let Some(pos) = world.get::<Position>(entity) else { return false };
        let nx = pos.x.wrapping_add_signed(dx);
        let ny = pos.y.wrapping_add_signed(dy);
        if nx >= MAP_WIDTH || ny >= MAP_HEIGHT { return false; }
        let tile = world.resource::<Map>().tiles[ny][nx];
        let has_enemy = world.resource::<OccupancyMap>().cells[ny][nx]
            .and_then(|e| if world.get::<Monster>(e).is_some() { Some(e) } else { None });
        if tile != Tile::Floor && has_enemy.is_none() { return false; }
        if let Some(target) = has_enemy {
            ActionKindV3::Attack { target }
        } else {
            ActionKindV3::Move { dx, dy }
        }
    };

    let reaction_time = world.get::<Reaction>(entity).map(|r| r.time).unwrap_or(50.0);
    let agility = world.get::<Stats>(entity).map(|s| s.agility).unwrap_or(10);
    let duration = world.get::<CanMove>(entity).map(|m| m.duration * agility_speed_factor(agility)).unwrap_or(300.0);
    handle_timed_action(world, entity, kind, reaction_time + duration)
}

/// 处理等待键
pub fn handle_wait(world: &mut World) -> bool {
    if let Some(e) = dungeon_core::ops::player_entity(world) {
        let reaction_time = world.get::<Reaction>(e).map(|r| r.time).unwrap_or(50.0);
        let agility = world.get::<Stats>(e).map(|s| s.agility).unwrap_or(10);
        let duration = world.get::<CanWait>(e).map(|w| w.duration * agility_speed_factor(agility)).unwrap_or(800.0);
        handle_timed_action(world, e, ActionKindV3::Wait, reaction_time + duration)
    } else {
        false
    }
}

/// 处理技能键（idx: 技能索引 0..3）
pub fn handle_skill(world: &mut World, idx: usize) -> bool {
    if let Some(e) = dungeon_core::ops::player_entity(world) {
        let reaction_time = world.get::<Reaction>(e).map(|r| r.time).unwrap_or(50.0);
        let agility = world.get::<Stats>(e).map(|s| s.agility).unwrap_or(10);
        handle_timed_action(world, e, ActionKindV3::Skill(idx), reaction_time + 600.0 * agility_speed_factor(agility))
    } else {
        false
    }
}
