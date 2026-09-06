//! 最小可运行世界循环：初始化、玩家行动、怪物决策、结算。

use crate::action::{advance_until_player_acted, mount_action};
use crate::ai::decide_monster_actions;
use crate::components::Active;
use crate::init::run_initialization;
use crate::input_action::{player_action_generation_system, PlayerActionRequest, PlayerCommand};
use crate::resources::TurnManager;
use crate::system::run_settle_systems;
use bevy_ecs::prelude::*;
use bevy_ecs::system::RunSystemOnce;

/// 创建并初始化一局新游戏。
pub fn new_game(map_seed: u64) -> World {
    let mut world = World::new();
    run_initialization(&mut world, map_seed);
    decide_monster_actions(&mut world);
    world
}

/// 玩家是否正在行动中。
pub fn player_is_busy(world: &World) -> bool {
    let Some(entity) = crate::query::player_entity(world) else {
        return false;
    };
    world.get::<Active>(entity).is_some()
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

    world.insert_resource(PlayerActionRequest::new(command));
    let _ = world.run_system_once(player_action_generation_system);

    if !player_is_busy(world) {
        return false;
    }

    advance_until_player_acted(world);
    decide_monster_actions(world);
    run_settle_systems(world);
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
    crate::query::player_entity(world).is_some()
}
