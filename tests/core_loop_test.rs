//! 端到端 headless 集成测试：只用新 `core` 的公共 API 跑完整闭环。
//!
//! 这是 I88 的替代品——旧 `tests/scenario_test.rs` / `tests/throw_test.rs` 针对
//! 已作废的 `dungeon-*` 架构（`dungeon_tui` / `setup_world` 等），已归档到
//! `archive/legacy-tests/`。
//!
//! 与 `core` 内部单元测试的区别：这里不碰任何 `pub(crate)` 细节，也不依赖
//! `#[cfg(test)]` helper，验证的是**应用层真正能用的那组 API**：
//! `new_game` / `apply_player_command` / `player_alive` / `request_quit`。
//!
//! 渲染不在本轮范围：`render-api` 的消费验证属于 REFACTOR §11 Phase G（presentation/tui）。

use bevy_ecs::prelude::{Component, Entity, With, World};
use core::components::{Health, Position};
use core::entity_cls::{Monster, Player, Stairs};
use core::map::{Map, Tile, MAP_HEIGHT, MAP_WIDTH};
use core::world_loop::{apply_player_command, new_game, player_alive, request_quit};
use core::{PlayerCommand, TurnManager};

/// 找一个“相邻、在界内、可走、且没被占用”的方向。
fn find_walkable_step(world: &World, pos: (usize, usize)) -> Option<(isize, isize)> {
    let map = world.resource::<Map>();
    let occupancy = world.resource::<core::OccupancyMap>();
    let dirs: [(isize, isize); 8] = [
        (0, -1),
        (0, 1),
        (-1, 0),
        (1, 0),
        (-1, -1),
        (1, -1),
        (-1, 1),
        (1, 1),
    ];
    dirs.into_iter().find(|&(dx, dy)| {
        let (nx, ny) = Position::new(pos.0, pos.1).offset(dx, dy);
        nx < MAP_WIDTH
            && ny < MAP_HEIGHT
            && map.tiles[ny][nx].walkable()
            && !occupancy.is_occupied(nx, ny)
    })
}

fn player_entity(world: &World) -> Entity {
    let mut query = world
        .try_query::<(Entity, &Player)>()
        .expect("Player 组件已注册")
        ;
    query.iter(world).next().expect("新游戏必须有玩家").0
}

fn player_pos(world: &World) -> (usize, usize) {
    world
        .get::<Position>(player_entity(world))
        .expect("玩家必须有 Position")
        .to_tuple()
}

fn despawn_all<E: Component>(world: &mut World) {
    let entities: Vec<Entity> = {
        let mut query = world.query_filtered::<Entity, With<E>>();
        query.iter(world).collect()
    };
    for entity in entities {
        world.despawn(entity);
    }
}

/// 清掉怪物与楼梯，得到一个只有玩家、地形与楼梯资源的世界（移动测试用）。
fn clear_actors(world: &mut World) {
    despawn_all::<Monster>(world);
    despawn_all::<Stairs>(world);
    core::system::run_settle_systems(world);
}

#[test]
fn new_game_initializes_a_playable_world() {
    let world = new_game(20260915);

    assert!(player_alive(&world), "新游戏必须有存活玩家");

    let (px, py) = player_pos(&world);
    assert!(px < MAP_WIDTH && py < MAP_HEIGHT, "玩家必须在界内");
    assert!(
        world.resource::<Map>().tiles[py][px].walkable(),
        "玩家必须站在可走格上"
    );

    let monsters = {
        let mut query = world
            .try_query_filtered::<Entity, With<Monster>>()
            .expect("Monster 已注册");
        query.iter(&world).count()
    };
    assert!(monsters > 0, "新游戏必须生成怪物");
    assert!(
        !world.resource::<TurnManager>().game_over,
        "新游戏不应当是结束状态"
    );
}

#[test]
fn wait_command_advances_and_keeps_player_consistent() {
    let mut world = new_game(7);
    clear_actors(&mut world);
    let start = player_pos(&world);

    assert!(
        apply_player_command(&mut world, PlayerCommand::Wait),
        "等待命令应当被接受"
    );
    assert_eq!(player_pos(&world), start, "等待不应当移动玩家");
    assert!(player_alive(&world), "等待不应当杀死玩家");
    assert!(!world.resource::<TurnManager>().game_over);
}

#[test]
fn move_command_moves_the_player_one_tile() {
    let mut world = new_game(11);
    clear_actors(&mut world);
    let start = player_pos(&world);
    let (dx, dy) = find_walkable_step(&world, start).expect("出生点周围必须有可走格");
    let expected = (
        start.0.wrapping_add_signed(dx),
        start.1.wrapping_add_signed(dy),
    );

    assert!(
        apply_player_command(&mut world, PlayerCommand::Move { dx, dy }),
        "合法移动必须被接受"
    );
    assert_eq!(player_pos(&world), expected, "玩家应当移动一格");
}

#[test]
fn blocked_move_is_rejected_without_side_effects() {
    let mut world = new_game(12);
    clear_actors(&mut world);

    // 把出生点四周封成墙，构造必定被阻挡的场景（不依赖地形生成的具体形状）。
    let (px, py) = player_pos(&world);
    for (dx, dy) in [(-1isize, 0isize), (1, 0), (0, -1), (0, 1)] {
        let (nx, ny) = Position::new(px, py).offset(dx, dy);
        if nx < MAP_WIDTH && ny < MAP_HEIGHT {
            world.resource_mut::<Map>().tiles[ny][nx] = Tile::Wall;
        }
    }

    assert!(
        !apply_player_command(&mut world, PlayerCommand::Move { dx: 1, dy: 0 }),
        "前方是墙时命令必须被拒绝"
    );
    assert_eq!(player_pos(&world), (px, py), "被拒的移动不得改变位置");
    assert!(player_alive(&world));
}

#[test]
fn player_defeats_an_adjacent_monster() {
    let mut world = new_game(2026);

    // 把玩家四周压成地板（避免被墙挡住），但保留楼梯格不可通行。
    let (px, py) = player_pos(&world);
    for dy in -1isize..=1 {
        for dx in -1isize..=1 {
            let (nx, ny) = Position::new(px, py).offset(dx, dy);
            if nx < MAP_WIDTH && ny < MAP_HEIGHT {
                world.resource_mut::<Map>().tiles[ny][nx] = Tile::Floor;
            }
        }
    }

    // 在玩家右侧放一只弱怪，然后用“向右移动”声明攻击。
    let target = {
        let mut query = world
            .try_query::<(Entity, &Monster, &Position)>()
            .expect("Monster 已注册");
        query
            .iter(&world)
            .find(|(_, _, pos)| pos.x.abs_diff(px) + pos.y.abs_diff(py) > 1)
            .map(|(entity, _, _)| entity)
            .expect("新游戏必须有离玩家一格以上的怪物")
    };
    {
        let mut pos = world.get_mut::<Position>(target).unwrap();
        pos.x = px + 1;
        pos.y = py;
    }
    world.resource_mut::<Map>().tiles[py][px + 1] = Tile::Floor;
    world.entity_mut(target).insert((Health::new(6.0), core::Defense(0.0)));
    // 关键：玩家行动生成读的是占用图，手工搬动实体后必须重建，
    // 否则“走向怪物格”不会被识别为攻击声明（只是失败的移动）。
    core::system::run_settle_systems(&mut world);

    assert!(
        apply_player_command(&mut world, PlayerCommand::Move { dx: 1, dy: 0 }),
        "对相邻怪物应当声明为攻击"
    );

    // 玩家的 AV（敏捷 10）小于这只怪，先手攻击；再等它把怪打死（每次等待推进一轮）。
    let mut killed = world.get_entity(target).is_err();
    for _ in 0..40 {
        if killed {
            break;
        }
        let player_hp = world
            .get::<Health>(player_entity(&world))
            .expect("玩家必须有 Health")
            .current;
        if player_hp <= 0.0 {
            break;
        }
        apply_player_command(&mut world, PlayerCommand::Wait);
        killed = world.get_entity(target).is_err();
    }

    assert!(killed, "相邻怪物应当被打死");
    assert!(player_alive(&world), "玩家不应当在同一过程中死亡");
}

#[test]
fn request_quit_sets_the_flag() {
    let mut world = new_game(1);
    assert!(!world.resource::<TurnManager>().wants_quit);
    request_quit(&mut world);
    assert!(world.resource::<TurnManager>().wants_quit);
}

/// C7：主循环接到 action 实体后，长跑一串命令不得卡死/panic，且世界状态自洽。
///
/// 这是 headless 的「可玩性冒烟」：混合移动/等待，跑到玩家死亡或回合上限为止，
/// 每轮检查位置合法、HP 在界内、玩家空闲时没有残留 action 子实体。
#[test]
fn long_random_walk_keeps_world_consistent() {
    let mut world = new_game(20260915);
    let player = player_entity(&world);

    let directions: [(isize, isize); 8] = [
        (0, -1),
        (0, 1),
        (-1, 0),
        (1, 0),
        (-1, -1),
        (1, -1),
        (-1, 1),
        (1, 1),
    ];
    let mut accepted_commands = 0usize;

    for round in 0..300 {
        if world.resource::<TurnManager>().game_over {
            break;
        }
        // 交替等待与移动：两条玩家路径都被覆盖。
        let accepted = if round % 3 == 0 {
            apply_player_command(&mut world, PlayerCommand::Wait)
        } else {
            let (dx, dy) = directions[round % directions.len()];
            apply_player_command(&mut world, PlayerCommand::Move { dx, dy })
        };
        if accepted {
            accepted_commands += 1;
        }

        // 不变量 1：位置在界内且可走。
        let (px, py) = player_pos(&world);
        assert!(px < MAP_WIDTH && py < MAP_HEIGHT, "玩家越界: ({px},{py})");
        assert!(
            world.resource::<Map>().tiles[py][px].walkable(),
            "玩家站在不可走格上: ({px},{py})"
        );

        // 不变量 2：HP 在界内。
        let health = world.get::<Health>(player).unwrap();
        assert!(
            health.current >= 0.0 && health.current <= health.max,
            "HP 越界: {} / {}",
            health.current,
            health.max
        );

        // 不变量 3：玩家空闲时不得残留 action 子实体。
        if world.get::<core::Idle>(player).is_some() {
            let leftovers = {
                let mut query =
                    world.query_filtered::<Entity, With<bevy_ecs::hierarchy::ChildOf>>();
                query
                    .iter(&world)
                    .filter(|child| {
                        world
                            .get::<bevy_ecs::hierarchy::ChildOf>(*child)
                            .is_some_and(|child_of| child_of.parent() == player)
                    })
                    .count()
            };
            assert_eq!(leftovers, 0, "玩家空闲时不得残留 action 子实体");
        }
    }

    assert!(accepted_commands > 0, "至少应当有一条命令被接受");
    assert!(
        player_alive(&world) || world.resource::<TurnManager>().game_over,
        "玩家要么活着，要么已经 game_over"
    );
}

#[test]
fn game_over_stops_further_commands() {
    let mut world = new_game(7);
    world.resource_mut::<TurnManager>().game_over = true;

    assert!(
        !apply_player_command(&mut world, PlayerCommand::Wait),
        "游戏结束后不得再接受命令"
    );
}

#[test]
fn player_death_ends_the_game() {
    let mut world = new_game(3);

    // 玩家只剩 1 点血，且怪物贴脸；怪物 AV 更小会先手打死玩家。
    let player = player_entity(&world);
    {
        let mut health = world.get_mut::<Health>(player).unwrap();
        health.current = 1.0;
        health.max = 1.0;
    }
    world.entity_mut(player).insert(core::MoveSpeed(0.25));

    let monster = {
        let mut query = world
            .try_query::<(Entity, &Monster)>()
            .expect("Monster 已注册");
        query.iter(&world).next().expect("新游戏必须有怪物").0
    };
    let (px, py) = player_pos(&world);
    {
        let mut pos = world.get_mut::<Position>(monster).unwrap();
        pos.x = px + 1;
        pos.y = py;
    }
    world.resource_mut::<Map>().tiles[py][px + 1] = Tile::Floor;
    // 同上：手工搬动实体后重建占用图，怪物才会真的和玩家相邻并发动攻击。
    // 速度改成**下限** `MIN_SPEED`：玩家的 AV 被拉到最长（800ms 等待 ×4），
    // 怪物一定先手，用最少的轮数钉住「怪物先动手」这一前提。
    world.entity_mut(monster).insert(core::MoveSpeed(core::MIN_SPEED));
    // 同上：手工搬动实体后重建占用图，怪物才会真的和玩家相邻并发动攻击。
    core::system::run_settle_systems(&mut world);

    apply_player_command(&mut world, PlayerCommand::Wait);

    assert!(
        world.resource::<TurnManager>().game_over,
        "玩家被打死后必须进入 game_over"
    );
    assert!(
        !apply_player_command(&mut world, PlayerCommand::Wait),
        "游戏结束后不得再接受命令"
    );
}
