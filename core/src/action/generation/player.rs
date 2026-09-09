//! 玩家输入 → 行动生成的系统桥。
//!
//! 应用层只负责把按键翻译成 `PlayerCommand` 写入 `PlayerActionRequest`；
//! 本系统负责验证并挂载玩家行动。

use crate::action::ActionKind;
use crate::balance::{action_av, UNARMED_ATTACK_DURATION, WAIT_DURATION};
use crate::components::*;
use crate::entity_cls::{Monster, Player};
use crate::map::{Map, MAP_HEIGHT, MAP_WIDTH};
use crate::action::execution::movement::can_move_to;
use crate::resources::OccupancyMap;
use bevy_ecs::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerCommand {
    Move { dx: isize, dy: isize },
    Wait,
}

#[derive(Resource, Default)]
pub struct PlayerActionRequest {
    pub command: Option<PlayerCommand>,
}

impl PlayerActionRequest {
    pub fn new(command: PlayerCommand) -> Self {
        Self {
            command: Some(command),
        }
    }
}

/// 消费 `PlayerActionRequest`，为玩家挂载行动。
pub fn player_action_generation_system(
    mut commands: Commands,
    mut request: ResMut<PlayerActionRequest>,
    players: Query<(Entity, &Position, &Agility, Option<&Active>), With<Player>>,
    monsters: Query<Entity, With<Monster>>,
    map: Res<Map>,
    occupancy: Res<OccupancyMap>,
) {
    let Some(command) = request.command.take() else {
        return;
    };
    log::debug!("处理玩家行动请求: {command:?}");

    let Ok((player, pos, agility, active)) = players.single() else {
        log::warn!("玩家实体不存在或查询失败，忽略行动请求");
        return;
    };

    if active.is_some() {
        log::debug!("玩家已有 Active 行动，忽略请求: {command:?}");
        return;
    }

    let action = match command {
        PlayerCommand::Wait => Some(ActionKind::Wait),
        PlayerCommand::Move { dx, dy } => {
            let (nx, ny) = pos.offset(dx, dy);
            if nx >= MAP_WIDTH || ny >= MAP_HEIGHT {
                None
            } else if let Some(occupant) = occupancy.entity_at(nx, ny) {
                if monsters.get(occupant).is_ok() {
                    Some(ActionKind::BasicAttack { target: occupant })
                } else {
                    None
                }
            } else if can_move_to(&map, &occupancy, pos.x, pos.y, dx, dy) {
                Some(ActionKind::Move { dx, dy })
            } else {
                None
            }
        }
    };

    let Some(action) = action else {
        log::debug!("行动请求无效，未挂载: {command:?}");
        return;
    };

    let duration = match action {
        ActionKind::Wait => WAIT_DURATION,
        _ => UNARMED_ATTACK_DURATION,
    };
    let av = action_av(duration, agility.0);

    mount_player_action(&mut commands, player, action, av);
}

fn mount_player_action(
    commands: &mut Commands,
    entity: Entity,
    action: ActionKind,
    av: f64,
) {
    let mut cmd = commands.entity(entity);
    cmd.remove::<Idle>()
        .remove::<Failure>()
        .remove::<Active>()
        .remove::<ActionTimer>()
        .remove::<Ready>()
        .remove::<Wait>()
        .remove::<Move>()
        .remove::<BasicAttack>()
        .remove::<Chase>()
        .remove::<Flee>()
        .remove::<Wander>();

    cmd.insert(Active);
    cmd.insert(ActionTimer { remaining_av: av.max(0.0) });

    match action {
        ActionKind::Wait => {
            cmd.insert(Wait);
        }
        ActionKind::Move { dx, dy } => {
            cmd.insert(Move { dx, dy });
        }
        ActionKind::BasicAttack { target } => {
            cmd.insert(BasicAttack { target });
        }
        ActionKind::Chase | ActionKind::Flee | ActionKind::Wander => {}
    }
}
