//! 怪物 AI 行动生成：视野/生命值条件与优先级决策。
//!
//! 具体执行系统在 `action::execution`。

use crate::action::{mount_action, ActionKind};
use crate::balance::{
    action_av, CHASE_DURATION, FLEE_DURATION, FLEE_HP_RATIO, FLEE_HP_RATIO_EXIT, WANDER_DURATION,
    WAIT_DURATION,
};
use crate::components::*;
use crate::entity_cls::Monster;
use bevy_ecs::prelude::*;
use bevy_ecs::query::Or;

pub fn player_visible_to(world: &World, entity: Entity) -> bool {
    let Some(pp) = crate::world::query::player_pos(world) else {
        return false;
    };
    world
        .get::<Viewshed>(entity)
        .map(|v| v.can_see(pp))
        .unwrap_or(false)
}

/// 追击保活条件：仍能看到玩家，或仍有最后已知位置。
pub fn chase_condition(world: &World, entity: Entity) -> bool {
    player_visible_to(world, entity)
        || world
            .get::<LastKnownPlayerPos>(entity)
            .map(|l| l.0.is_some())
            .unwrap_or(false)
}

/// 逃跑保活条件（滞回退出阈值）。
pub fn flee_condition(world: &World, entity: Entity) -> bool {
    world
        .get::<Health>(entity)
        .map(|h| h.ratio() < FLEE_HP_RATIO_EXIT)
        .unwrap_or(false)
}

/// 逃跑决策条件（进入阈值）。
pub fn wants_to_flee(world: &World, entity: Entity) -> bool {
    world
        .get::<Health>(entity)
        .map(|h| h.ratio() < FLEE_HP_RATIO)
        .unwrap_or(false)
}

/// 为所有空闲/失败且有能力的怪物挂载下一轮行动。
pub fn decide_monster_actions(world: &mut World) {
    let candidates: Vec<Entity> = {
        let mut query = world.query_filtered::<
            Entity,
            (
                With<Monster>,
                Or<(With<Idle>, With<Failure>)>,
                Or<(
                    With<CanChase>,
                    With<CanFlee>,
                    With<CanWander>,
                    With<CanWait>,
                )>,
                Without<Active>,
            ),
        >();
        query.iter(world).collect()
    };

    for entity in candidates {
        if let Some((action, av)) = choose_action(world, entity) {
            mount_action(world, entity, action, av);
        }
    }
}

fn choose_action(world: &World, entity: Entity) -> Option<(ActionKind, f64)> {
    let agility = world.get::<Agility>(entity).map(|a| a.0).unwrap_or(0.0);

    let selected = if world.get::<CanFlee>(entity).is_some() && wants_to_flee(world, entity) {
        Some((ActionKind::Flee, action_av(FLEE_DURATION, agility)))
    } else if world.get::<CanChase>(entity).is_some() && chase_condition(world, entity) {
        Some((ActionKind::Chase, action_av(CHASE_DURATION, agility)))
    } else if world.get::<CanWander>(entity).is_some() {
        Some((ActionKind::Wander, action_av(WANDER_DURATION, agility)))
    } else if world.get::<CanWait>(entity).is_some() {
        Some((ActionKind::Wait, action_av(WAIT_DURATION, agility)))
    } else {
        None
    };

    match selected {
        Some((action, _)) => log::debug!("AI 决策: entity={entity:?}, action={action:?}"),
        None => log::debug!("AI 决策: entity={entity:?}, action=None"),
    }
    selected
}