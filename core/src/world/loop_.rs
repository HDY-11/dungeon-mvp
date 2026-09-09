//! 最小可运行世界循环：初始化、玩家行动、怪物决策、结算。

use crate::action::mount_action;
use crate::action::execution::run_action_cycle;
use crate::action::generation::ai::decide_monster_actions;
use crate::components::Active;
use crate::world::init::run_initialization;
use crate::action::generation::player::{player_action_generation_system, PlayerActionRequest, PlayerCommand};
use crate::resources::TurnManager;
use crate::system::run_settle_systems;
use bevy_ecs::prelude::*;
use bevy_ecs::system::RunSystemOnce;

/// 创建并初始化一局新游戏。
pub fn new_game(map_seed: u64) -> World {
    log::info!("创建新游戏: seed={map_seed}");
    let mut world = World::new();
    run_initialization(&mut world, map_seed);
    decide_monster_actions(&mut world);
    world
}

/// 玩家是否正在行动中。
pub fn player_is_busy(world: &World) -> bool {
    let Some(entity) = crate::world::query::player_entity(world) else {
        return false;
    };
    world.get::<Active>(entity).is_some()
}

fn advance_until_player_acted(world: &mut World) {
    for _ in 0..10_000 {
        if !player_is_busy(world) || world.resource::<TurnManager>().game_over {
            break;
        }
        // 每轮先为 Idle/Failure 的怪物生成新行动，再推进 AV、执行 Ready 行动并结算。
        // 这样 AV 更小的快怪可以在玩家行动期间执行多次。
        decide_monster_actions(world);
        run_action_cycle(world);
        run_settle_systems(world);
    }
}

/// 将玩家命令写入请求并推进世界。
///
/// 返回 `true` 表示命令被接受并发生了世界推进。
pub fn apply_player_command(world: &mut World, command: PlayerCommand) -> bool {
    if world.resource::<TurnManager>().game_over {
        return false;
    }
    if player_is_busy(world) {
        return false;
    }

    log::debug!("玩家命令: {command:?}");
    world.insert_resource(PlayerActionRequest::new(command));
    let _ = world.run_system_once(player_action_generation_system);

    if !player_is_busy(world) {
        return false;
    }

    advance_until_player_acted(world);
    // 为下一轮准备怪物行动；本轮结算已在循环内完成。
    decide_monster_actions(world);
    true
}

/// 请求退出。
pub fn request_quit(world: &mut World) {
    world.resource_mut::<TurnManager>().wants_quit = true;
}

/// 手动为玩家挂载一个行动（调试/测试入口）。
pub fn mount_player_action(world: &mut World, entity: Entity, action: crate::action::ActionKind, av: f64) {
    mount_action(world, entity, action, av);
}

/// 当前是否有玩家实体存活。
pub fn player_alive(world: &World) -> bool {
    crate::world::query::player_entity(world).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::generation::player::PlayerCommand;

    #[test]
    fn apply_wait_command_advances_without_panic() {
        let mut world = new_game(7);
        assert!(apply_player_command(&mut world, PlayerCommand::Wait));
    }
}
