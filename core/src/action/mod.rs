//! 行动模型：行动类型、挂载与状态轮转。
//!
//! 具体执行系统位于 `execution`；生成系统位于 `generation`。

use crate::components::*;
use bevy_ecs::prelude::*;

pub mod execution;
pub mod generation;

pub use execution::*;
pub use generation::*;

/// 可挂载的具体行动。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionKind {
    Wait,
    Move { dx: isize, dy: isize },
    BasicAttack { target: Entity },
    Chase,
    Flee,
    Wander,
}

/// 挂载行动：替换旧行动，写入 `Active + ActionTimer + 具体行动组件`。
pub fn mount_action(world: &mut World, entity: Entity, action: ActionKind, av: f64) {
    clear_concrete_actions(world, entity);

    let mut entity_mut = world.entity_mut(entity);
    entity_mut.remove::<Idle>();
    entity_mut.remove::<Failure>();
    entity_mut.remove::<Active>();
    entity_mut.remove::<ActionTimer>();
    entity_mut.insert(Active);
    entity_mut.insert(ActionTimer { remaining_av: av.max(0.0) });

    match action {
        ActionKind::Wait => {
            entity_mut.insert(Wait);
        }
        ActionKind::Move { dx, dy } => {
            entity_mut.insert(Move { dx, dy });
        }
        ActionKind::BasicAttack { target } => {
            entity_mut.insert(BasicAttack { target });
        }
        ActionKind::Chase => {
            entity_mut.insert(Chase);
        }
        ActionKind::Flee => {
            entity_mut.insert(Flee);
        }
        ActionKind::Wander => {
            entity_mut.insert(Wander);
        }
    }

    log::debug!("挂载行动: entity={entity:?}, action={action:?}, av={av:.2}");
}

pub fn finish_action_success(world: &mut World, entity: Entity) {
    clear_action_state(world, entity);
    world.entity_mut(entity).insert(Idle);
    log::debug!("行动成功: {entity:?}");
}

pub fn finish_action_failure(world: &mut World, entity: Entity) {
    clear_action_state(world, entity);
    world.entity_mut(entity).insert(Failure);
    log::debug!("行动失败: {entity:?}");
}

/// 挂载新行动前清理旧具体行动组件。
/// 执行期清理由各执行系统负责，不由本函数处理。
fn clear_concrete_actions(world: &mut World, entity: Entity) {
    let mut entity_mut = world.entity_mut(entity);
    entity_mut.remove::<Wait>();
    entity_mut.remove::<Move>();
    entity_mut.remove::<BasicAttack>();
    entity_mut.remove::<Chase>();
    entity_mut.remove::<Flee>();
    entity_mut.remove::<Wander>();
}

fn clear_action_state(world: &mut World, entity: Entity) {
    let mut entity_mut = world.entity_mut(entity);
    entity_mut.remove::<Active>();
    entity_mut.remove::<ActionTimer>();
}