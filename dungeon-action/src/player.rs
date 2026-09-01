//! 玩家 tap-tap 行动处理

use crate::types::*;
use bevy_ecs::prelude::*;
use dungeon_core::{Equipment, MAP_HEIGHT, MAP_WIDTH, Map, Monster, OccupancyMap, components::*};

/// tap-tap 核心：返回 true 表示确认入队
pub fn handle_timed_action(world: &mut World, entity: Entity, kind: ActionKindV3, av: f32) -> bool {
    let is_confirm = {
        let preview = world.resource::<PlayerPreview>();
        match (&preview.kind, &kind) {
            (Some(ActionKindV3::Move { dx: pd, dy: pd2 }), ActionKindV3::Move { dx, dy })
                if pd == dx && pd2 == dy =>
            {
                true
            }
            (Some(ActionKindV3::Wait), ActionKindV3::Wait) => true,
            (Some(ActionKindV3::Skill(a)), ActionKindV3::Skill(b)) if a == b => true,
            _ => false,
        }
    };

    if is_confirm {
        world
            .resource_mut::<ActionQueue>()
            .enqueue_or_replace(entity, kind, av);
        world.resource_mut::<PlayerPreview>().kind = None;
        true
    } else {
        world.resource_mut::<PlayerPreview>().kind = Some(kind);
        false
    }
}

/// 方向键 tap-tap：返回 true 表示确认了行动
pub fn handle_player_direction(world: &mut World, dx: isize, dy: isize) -> bool {
    let Some(entity) = dungeon_core::ops::player_entity(world) else {
        return false;
    };

    let kind = {
        let Some(pos) = world.get::<Position>(entity) else {
            return false;
        };
        let nx = pos.x.wrapping_add_signed(dx);
        let ny = pos.y.wrapping_add_signed(dy);
        if nx >= MAP_WIDTH || ny >= MAP_HEIGHT {
            return false;
        }
        let tile = world.resource::<Map>().tiles[ny][nx];
        let has_enemy = world.resource::<OccupancyMap>().cells[ny][nx].and_then(|e| {
            if world.get::<Monster>(e).is_some() {
                Some(e)
            } else {
                None
            }
        });
        if !tile.walkable() && has_enemy.is_none() {
            return false;
        }
        if let Some(target) = has_enemy {
            ActionKindV3::Attack { target }
        } else {
            // G31: 入队前预检对角穿墙（与 check_condition 的执行前保活同规则）
            if !crate::execute::can_move_to(
                world.resource::<Map>(),
                world.resource::<OccupancyMap>(),
                pos.x,
                pos.y,
                dx,
                dy,
            ) {
                return false;
            }
            ActionKindV3::Move { dx, dy }
        }
    };

    let agility = world.get::<Stats>(entity).map(|s| s.agility).unwrap_or(10);
    let reaction_time = agility_to_reaction(agility);
    // I67: 移动/攻击耗时由主手武器 speed 决定（无武器 300ms 基准），
    // 重武器慢速高攻、轻武器快速低攻——换武器产生攻速决策。
    let weapon_speed = world
        .get::<Equipment>(entity)
        .and_then(|eq| eq.main_hand.as_ref())
        .and_then(|s| s.def().and_then(|d| d.speed))
        .unwrap_or(300) as f32;
    let duration = weapon_speed * agility_speed_factor(agility);
    let av = reaction_time + duration;

    // I62: Attack 按目标确认（与 Move/Wait/Skill 的"同键确认"语义一致）：
    // 预览攻击 A 时按向攻击 B 的方向键 → 只更新预览，不执行
    if let ActionKindV3::Attack { target } = kind {
        let is_confirm = matches!(
            world.resource::<PlayerPreview>().kind,
            Some(ActionKindV3::Attack { target: pt }) if pt == target
        );
        if is_confirm {
            world
                .resource_mut::<ActionQueue>()
                .enqueue_or_replace(entity, kind, av);
            world.resource_mut::<PlayerPreview>().kind = None;
            true
        } else {
            world.resource_mut::<PlayerPreview>().kind = Some(kind);
            false
        }
    } else {
        handle_timed_action(world, entity, kind, av)
    }
}

/// 处理等待键
pub fn handle_wait(world: &mut World) -> bool {
    if let Some(e) = dungeon_core::ops::player_entity(world) {
        let agility = world.get::<Stats>(e).map(|s| s.agility).unwrap_or(10);
        let reaction_time = agility_to_reaction(agility);
        let duration = world
            .get::<CanWait>(e)
            .map(|w| w.duration * agility_speed_factor(agility))
            .unwrap_or(800.0);
        handle_timed_action(world, e, ActionKindV3::Wait, reaction_time + duration)
    } else {
        false
    }
}

/// 处理技能键（idx: 按键索引，0→技能1键，1→技能2键…）
pub fn handle_skill(world: &mut World, idx: usize) -> bool {
    if let Some(e) = dungeon_core::ops::player_entity(world) {
        // 按键索引 → 对应快捷键字符（0→'1', 1→'2', 2→'3', 3→'4'）
        let key_char = char::from_digit(idx as u32 + 1, 10).unwrap_or('1');
        // 在已学技能列表中按快捷键查找实际索引
        let real_idx = world
            .get::<dungeon_core::Skills>(e)
            .and_then(|s| s.index_of_key(key_char));
        let Some(real_idx) = real_idx else {
            world
                .resource_mut::<dungeon_core::EventLog>()
                .push(dungeon_core::EventMessage::skill(format!(
                    "技能 {} 未学习",
                    key_char
                )));
            return false;
        };
        let agility = world.get::<Stats>(e).map(|s| s.agility).unwrap_or(10);
        let reaction_time = agility_to_reaction(agility);
        handle_timed_action(
            world,
            e,
            ActionKindV3::Skill(real_idx),
            reaction_time + 600.0 * agility_speed_factor(agility),
        )
    } else {
        false
    }
}
