//! Throw action execution and confirmation.

use crate::types::*;
use bevy_ecs::prelude::*;
use dungeon_core::{
    FloorNumber, Map, Monster, Position, Stats, THROW_RANGE, THROW_DURATION, components::*,
    items::*, ops, resources::*,
};

use super::combat::{calc_player_crit, handle_kill};

fn validate_throw(
    world: &World,
    attacker: Entity,
    tx: usize,
    ty: usize,
) -> Result<(), &'static str> {
    let has_throwable = world
        .get::<Equipment>(attacker)
        .and_then(|eq| eq.off_hand.as_ref())
        .map(|s| dungeon_core::is_throwable(s.item_id))
        .unwrap_or(false);
    if !has_throwable {
        return Err("没有可投掷的物品");
    }
    let Some(pos) = world.get::<Position>(attacker).map(|p| (p.x, p.y)) else {
        return Err("无法定位投掷者");
    };
    if ops::chebyshev(pos, (tx, ty)) > THROW_RANGE {
        return Err("目标超出射程");
    }
    let los_clear = {
        let map = world.resource::<Map>();
        ops::los_clear(map, pos, (tx, ty))
    };
    if !los_clear {
        return Err("视线受阻，无法投掷");
    }
    Ok(())
}

pub fn confirm_throw(world: &mut World) -> bool {
    let valid = world.resource::<ThrowPreview>().valid_target;
    if !valid {
        world
            .resource_mut::<EventLog>()
            .push(dungeon_core::EventMessage::system("目标超出射程或视线受阻"));
        return false;
    }
    let (tx, ty) = {
        let tp = world.resource::<ThrowPreview>();
        tp.cursor
    };
    world.resource_mut::<ThrowPreview>().active = false;
    world.resource_mut::<LookCursor>().active = false;
    world.resource_mut::<PlayerPreview>().kind = None;
    world.resource_mut::<PageStack>().pop();
    let Some(player) = dungeon_core::ops::player_entity(world) else {
        return false;
    };
    let agility = world.get::<Stats>(player).map(|s| s.agility).unwrap_or(10);
    let av = agility_to_reaction(agility) + THROW_DURATION * agility_speed_factor(agility);
    world.resource_mut::<ActionQueue>().enqueue_or_replace(
        player,
        ActionKindV3::Throw { tx, ty },
        av,
    );
    true
}

pub(crate) fn execute_throw(world: &mut World, attacker: Entity, tx: usize, ty: usize) {
    if let Err(reason) = validate_throw(world, attacker, tx, ty) {
        world
            .resource_mut::<EventLog>()
            .push(dungeon_core::EventMessage::system(reason.to_string()));
        return;
    }

    let (extra, crit_roll) = {
        let mut rng = world.resource_mut::<GameRng>();
        (rng.random_range(0u8, 2u8) as u32, rng.random_f32())
    };

    let target = {
        let Some(mut q) = world.try_query::<(Entity, &Position, &Monster)>() else {
            world
                .resource_mut::<EventLog>()
                .push(dungeon_core::EventMessage::system(
                    "投掷内部错误：无法查询实体".to_string(),
                ));
            return;
        };
        q.iter(world)
            .find(|(_, pos, _)| pos.x == tx && pos.y == ty)
            .map(|(e, _, _)| e)
    };

    let floor = world.resource::<FloorNumber>().0;
    let base_dmg = 3 + floor / 2;
    let (is_crit, crit_mult) = calc_player_crit(world, crit_roll);

    if let Some(target_entity) = target {
        let target_def = world
            .get::<Stats>(target_entity)
            .map(|s| s.defense as i32)
            .unwrap_or(0);
        let raw_dmg = ((base_dmg as i32 + extra as i32 - target_def).max(1)) as u32;
        let final_dmg = (raw_dmg as f32 * crit_mult).round() as i32;
        let target_name = world
            .get::<EntityName>(target_entity)
            .map(|n| n.0.clone())
            .unwrap_or("怪物".into());

        if let Some(mut s) = world.get_mut::<Stats>(target_entity) {
            s.hp -= final_dmg;
        }

        let dead = world
            .get::<Stats>(target_entity)
            .map(|s| s.hp <= 0)
            .unwrap_or(false);
        if dead {
            handle_kill(world, target_entity, &target_name);
        } else {
            world
                .resource_mut::<EventLog>()
                .push(dungeon_core::EventMessage::combat(format!(
                    "石头命中了{}，{}，造成{}点伤害",
                    target_name,
                    if is_crit { "暴击" } else { "" },
                    final_dmg
                )));
        }
    } else {
        world
            .resource_mut::<EventLog>()
            .push(dungeon_core::EventMessage::combat(
                "石头落在地上".to_string(),
            ));
    }

    ops::consume_off_hand(world, attacker);
}
