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
    use crate::components::{Failure, Position};
    use crate::test_util::{
        find_blocked_step, find_walkable_step, player_entity, player_pos, stairs_entity_pos,
        world_snapshot,
    };

    #[test]
    fn apply_wait_command_advances_without_panic() {
        let mut world = new_game(7);
        assert!(apply_player_command(&mut world, PlayerCommand::Wait));
    }

    /// A1：同一 `(seed, floor)` 必须生成完全相同的世界（地图 / 房间 / 出生点 / 楼梯 / 怪物）。
    ///
    /// 这是 REFACTOR §10.7「地图确定性」的回归闸门：改动地图生成算法会在这里暴露。
    #[test]
    fn map_generation_is_deterministic() {
        let first = new_game(20260915);
        let second = new_game(20260915);
        let first_snapshot = world_snapshot(&first);
        let second_snapshot = world_snapshot(&second);

        assert_eq!(
            first_snapshot.tiles, second_snapshot.tiles,
            "同一 seed 的地图 tiles 必须逐一相等"
        );
        assert_eq!(
            first_snapshot.rooms, second_snapshot.rooms,
            "房间列表必须相等"
        );
        assert_eq!(
            first_snapshot.player_spawn, second_snapshot.player_spawn,
            "玩家出生点必须相等"
        );
        assert_eq!(
            first_snapshot.player_pos, second_snapshot.player_pos,
            "玩家初始位置必须相等"
        );
        assert_eq!(
            first_snapshot.stairs_pos, second_snapshot.stairs_pos,
            "楼梯位置必须相等"
        );
        assert_eq!(
            first_snapshot.monsters, second_snapshot.monsters,
            "怪物种类/位置/数值必须相等"
        );
        assert_eq!(
            stairs_entity_pos(&first),
            Some(first_snapshot.stairs_pos),
            "楼梯实体位置必须与 StairsPos 资源一致"
        );
        assert!(
            !first_snapshot.monsters.is_empty(),
            "确定性测试至少要有一只怪物作为对照"
        );
        assert_eq!(first_snapshot, second_snapshot);
    }

    /// A1 补充：不同 seed 至少有一项不同（防止“全都相等”是因为生成器完全没跑）。
    #[test]
    fn different_seed_changes_the_world() {
        let first = world_snapshot(&new_game(1));
        let second = world_snapshot(&new_game(2));
        assert!(
            first.tiles != second.tiles || first.monsters != second.monsters,
            "不同 seed 应当产生不同地图或不同怪物分布"
        );
    }

    /// A2：合法移动改变 `Position`，且落点就是目标格。
    #[test]
    fn player_move_into_free_tile() {
        let mut world = new_game(11);

        // 清掉怪物与楼梯，确保目标格没有被占用（G31 后楼梯不可通行）。
        let monsters: Vec<Entity> = {
            let mut query = world.query_filtered::<Entity, With<crate::entity_cls::Monster>>();
            query.iter(&world).collect()
        };
        for monster in monsters {
            world.despawn(monster);
        }
        let stairs: Vec<Entity> = {
            let mut query = world.query_filtered::<Entity, With<crate::entity_cls::Stairs>>();
            query.iter(&world).collect()
        };
        for stairs_entity in stairs {
            world.despawn(stairs_entity);
        }
        crate::system::run_settle_systems(&mut world);

        let start = player_pos(&world);
        let Some((dx, dy)) = find_walkable_step(&world, start) else {
            panic!("seed=11 出生点周围必须有可走格: {start:?}");
        };
        let expected = start.0.wrapping_add_signed(dx);
        let expected = (expected, start.1.wrapping_add_signed(dy));

        assert!(
            apply_player_command(&mut world, PlayerCommand::Move { dx, dy }),
            "合法移动必须被接受"
        );
        assert_eq!(
            player_pos(&world),
            expected,
            "玩家应当移动到 {expected:?}（起始 {start:?}，方向 {dx},{dy}）"
        );
    }

    /// A2：撞墙时命令不被接受，位置保持不变。
    #[test]
    fn player_move_blocked_by_wall() {
        let mut world = new_game(12);
        crate::test_util::player_entity(&world).expect("新游戏必须有玩家");
        let start = player_pos(&world);
        let Some((dx, dy)) = find_blocked_step(&world, start) else {
            panic!("seed=12 出生点周围必须有阻挡方向: {start:?}");
        };

        assert!(
            !apply_player_command(&mut world, PlayerCommand::Move { dx, dy }),
            "被墙/占用/越界挡住的方向不得被接受"
        );
        assert_eq!(player_pos(&world), start, "被挡住的移动不得改变位置");
    }

    /// A2：越界方向返回 `false`，位置不变；顺带验证唯一地板 + 四周围墙的最小场景。
    #[test]
    fn player_move_out_of_bounds_is_rejected() {
        let (mut world, player) = crate::test_util::single_tile_scene();

        assert!(
            !apply_player_command(&mut world, PlayerCommand::Move { dx: -1, dy: 0 }),
            "向负方向移动越界，必须返回 false"
        );
        assert_eq!(world.get::<Position>(player).unwrap().to_tuple(), (0, 0));

        // 四周只有 (0,0) 是地板：向右移动同样被拒绝。
        assert!(
            !apply_player_command(&mut world, PlayerCommand::Move { dx: 1, dy: 0 }),
            "相邻格是墙，必须返回 false"
        );
        assert_eq!(world.get::<Position>(player).unwrap().to_tuple(), (0, 0));

        // 被拒命令不能让玩家卡在 Active/Failure。
        assert!(world.get::<crate::components::Idle>(player).is_some());

        // 全墙地图时玩家仍能等待（Wait 不依赖地形）。
        assert!(apply_player_command(&mut world, PlayerCommand::Wait));
    }

    /// A2 补充：被拒的移动不能让玩家卡在 `Active`（否则输入永久失效）。
    #[test]
    fn rejected_move_leaves_player_idle() {
        let mut world = new_game(13);
        let player = crate::test_util::player_entity(&world).expect("新游戏必须有玩家");
        let start = player_pos(&world);
        let Some((dx, dy)) = find_blocked_step(&world, start) else {
            panic!("seed=13 出生点周围必须有阻挡方向");
        };

        assert!(!apply_player_command(
            &mut world,
            PlayerCommand::Move { dx, dy }
        ));
        assert!(!player_is_busy(&world), "被拒命令不应让玩家保持忙碌");
        assert!(world.get::<crate::components::Idle>(player).is_some());
        assert!(world.get::<Failure>(player).is_none());
        assert_eq!(player_pos(&world), start);
        assert_eq!(player_entity(&world), Some(player));
    }
}
