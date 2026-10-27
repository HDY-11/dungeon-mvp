//! 端到端 headless 集成测试：只用新 `ecs_core` 的公共 API 跑完整闭环。
//!
//! 这是 I88 的替代品——旧 `tests/scenario_test.rs` / `tests/throw_test.rs` 针对
//! 已作废的 `dungeon-*` 架构（`dungeon_tui` / `setup_world` 等），已归档到
//! `archive/legacy-tests/`。
//!
//! 与 `ecs_core` 内部单元测试的区别：这里不碰任何 `pub(crate)` 细节，也不依赖
//! `#[cfg(test)]` helper，验证的是**应用层真正能用的那组 API**：
//! `new_game` / `apply_player_command` / `player_alive` / `request_quit`。
//!
//! 渲染不在本轮范围：`render-api` 的消费验证属于 REFACTOR §11 Phase G（presentation/tui）。

use bevy_ecs::prelude::{Component, Entity, With, Without, World};
use ecs_core::components::{ActionTimer, Active, Health, Idle, Position};
use ecs_core::effect::{CellEffectAt, CellEffects};
use ecs_core::entity_cls::{Monster, Player, Stairs};
use ecs_core::map::{Map, Tile, MAP_HEIGHT, MAP_WIDTH};
use ecs_core::world_loop::{apply_player_command, new_game, player_alive, request_quit};
use ecs_core::{PlayerActionRequest, PlayerCommand, TurnManager};

/// 找一个“相邻、在界内、可走、且没被占用”的方向。
fn find_walkable_step(world: &World, pos: (usize, usize)) -> Option<(isize, isize)> {
    let map = world.resource::<Map>();
    let occupancy = world.resource::<ecs_core::OccupancyMap>();
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
    ecs_core::system::run_settle_systems(world);
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
    world.entity_mut(target).insert((Health::new(6.0), ecs_core::Defense(0.0)));
    // 关键：玩家行动生成读的是占用图，手工搬动实体后必须重建，
    // 否则“走向怪物格”不会被识别为攻击声明（只是失败的移动）。
    ecs_core::system::run_settle_systems(&mut world);

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
        if world.get::<ecs_core::Idle>(player).is_some() {
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
    world.entity_mut(player).insert(ecs_core::MoveSpeed(0.25));

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
    world.entity_mut(monster).insert(ecs_core::MoveSpeed(ecs_core::MIN_SPEED));
    // 同上：手工搬动实体后重建占用图，怪物才会真的和玩家相邻并发动攻击。
    ecs_core::system::run_settle_systems(&mut world);

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

// ── C2 内容：泥潭格在真实世界里真的让玩家变慢 ──────────

/// 把玩家挪到 `pos`，量一次移动行动的 AV（不推进世界，只跑挂载调度）。
fn move_av_at(world: &mut World, pos: (usize, usize)) -> f64 {
    let player = player_entity(world);

    // 清掉上一次的行动，让 `Without<Active>` 通过。
    let stale: Vec<Entity> = {
        let mut query =
            world.query_filtered::<Entity, With<ecs_core::action::entity::ActiveAction>>();
        query.iter(world).collect()
    };
    for action in stale {
        world.despawn(action);
    }
    world.entity_mut(player).remove::<Active>().insert(Idle);

    // 挪人 + 重建占用图（移动判定会读占用）。
    {
        let mut position = world.get_mut::<Position>(player).expect("玩家有位置");
        position.x = pos.0;
        position.y = pos.1;
    }
    ecs_core::system::run_settle_systems(world);

    let step = find_walkable_step(world, player_pos(world)).expect("周围必须有可走方向");
    world.resource_mut::<PlayerActionRequest>().command = Some(PlayerCommand::Move {
        dx: step.0,
        dy: step.1,
    });
    world.run_schedule(ecs_core::PlayerMountSchedule);

    let avs: Vec<f64> = {
        let mut query = world.query::<&ActionTimer>();
        query.iter(world).map(|timer| timer.remaining_av).collect()
    };
    assert_eq!(avs.len(), 1, "一次挂载必须恰好产出一个行动（位置 {pos:?}）");
    avs[0]
}

/// **C2 的协同验收**：初始化洒下的泥潭，必须能在真实世界里让玩家变慢一倍。
///
/// 这条串起了四段彼此独立的代码：`spawn_cell_effects_system`（内容）→
/// `CellEffects`（每轮重建的索引）→ `EffectContext::collect`（收桶）→
/// `SpeedRule::action_av_ignoring`（规则）。任何一段接错，AV 都不会是两倍。
#[test]
fn mud_doubles_the_player_move_av_in_the_real_world() {
    let mut world = new_game(2026);
    // 清场：泥潭测试不关心怪物会不会走进那一格。
    clear_actors(&mut world);
    ecs_core::system::run_settle_systems(&mut world);

    // 选一格泥潭、一格普通地板（都要有可走邻格）。
    let mud: Vec<(usize, usize)> = {
        let mut query = world.query::<&CellEffectAt>();
        query.iter(&world).map(|at| at.to_tuple()).collect()
    };
    let mud_target = mud
        .into_iter()
        .find(|pos| find_walkable_step(&world, *pos).is_some())
        .expect("至少要有一格带可走邻格的泥潭");

    let plain_target = {
        let map = world.resource::<Map>();
        let mut found = None;
        for (y, row) in map.tiles.iter().enumerate() {
            for (x, tile) in row.iter().enumerate() {
                if tile.walkable()
                    && world.resource::<CellEffects>().entity_at(x, y).is_none()
                    && find_walkable_step(&world, (x, y)).is_some()
                {
                    found = Some((x, y));
                    break;
                }
            }
            if found.is_some() {
                break;
            }
        }
        found.expect("地图上必须有普通地板")
    };

    let plain_av = move_av_at(&mut world, plain_target);
    let mud_av = move_av_at(&mut world, mud_target);

    assert!(
        (mud_av - plain_av * 2.0).abs() < 1e-9,
        "泥潭必须让移动 AV 恰好翻倍：平地 {plain_av} / 泥潭 {mud_av}"
    );
}

// ── 主循环：占用图必须与 Position 一致 ────────────────

/// **回归测试**：`apply_player_command` 返回之后，占用图必须与所有实体的 `Position` 一致。
///
/// 这条是被自动试玩抓出来的（`examples/playthrough.rs`）：`apply_player_command` 末尾原本
/// 还会再跑一次 `ActionPocSchedule`（注释说是"为下一轮准备"），但那一次会真的 **Tick + 执行**
/// ——怪物因此移动，而 `rebuild_occupancy_system` 在结算调度里，**不会跟着跑**。
/// 于是每条玩家命令之后占用图都陈旧一帧，后果是：
///
/// - 玩家撞不进、也打不到某个怪物（占用图说那格是空的 → 攻击退化成移动 → 又因对角穿墙被拒）
///   → 实测被卡死：400 条命令拒绝 395 条、HP 一直满、0 击杀；
/// - 直线方向更糟：玩家会直接**走进**怪物那一格（`execute_move` 只查占用图）。
///
/// 修法是删掉那次尾随调度（下一轮开始时本来就会生成+仲裁）。
#[test]
fn occupancy_matches_positions_after_each_player_command() {
    let mut world = new_game(2026);

    for step in 0..24 {
        // 先朝一个可走方向走；不行就换方向；再不行就等待。
        let pos = player_pos(&world);
        let mut moved = false;
        let dirs = [(1, 0), (0, 1), (-1, 0), (0, -1), (1, 1), (-1, -1)];
        for k in 0..dirs.len() {
            let (dx, dy) = dirs[(k + step) % dirs.len()];
            if apply_player_command(&mut world, PlayerCommand::Move { dx, dy }) {
                moved = true;
                break;
            }
        }
        if !moved {
            apply_player_command(&mut world, PlayerCommand::Wait);
        }
        if world.resource::<TurnManager>().game_over {
            break;
        }
        let _ = pos;

        // 一致性：每个带 Position 的实体（楼梯除外）都必须出现在占用图里。
        let rows: Vec<(Entity, (usize, usize))> = {
            let mut query = world.query_filtered::<(Entity, &Position), Without<Stairs>>();
            query.iter(&world).map(|(e, p)| (e, p.to_tuple())).collect()
        };
        for (entity, pos) in rows {
            let registered = world
                .resource::<ecs_core::OccupancyMap>()
                .entity_at(pos.0, pos.1);
            assert!(
                registered.is_some(),
                "第 {step} 步：{pos:?} 上的实体 {entity:?} 不在占用图里（占用图陈旧）"
            );
        }
    }
}

// ── C3 内容：中毒的结算顺序 ──────────────────────────

/// **C3 的协同验收**：中毒伤害必须在**同一轮**被死亡判定收口。
///
/// 这条钉的是 `build_core_schedule` 里的顺序：`poison_tick_system` 必须在
/// `check_death_system` **之前**。若有人把 tick 挪到死亡判定之后，玩家会看到血量归零
/// 却没死——而且没有任何测试会报错，除了这一条。
#[test]
fn poison_damage_kills_in_the_same_round() {
    let mut world = new_game(2026);
    clear_actors(&mut world);
    let player = player_entity(&world);

    // 一剂必死的毒：1 回合、伤害远超血量上限。
    world.spawn((
        ecs_core::EffectOf(player),
        ecs_core::Poison {
            damage_per_round: 10_000.0,
            rounds_left: 1,
        },
    ));

    ecs_core::system::run_settle_systems(&mut world);

    assert!(
        world.resource::<TurnManager>().game_over,
        "中毒伤害必须在同一轮触发死亡（tick 必须在 check_death 之前）"
    );
}

// ── C4 内容：捡到装备后泥潭对它无效 ──────────────────

/// **C4 的协同验收**：捡起踏水靴之后，泥潭不再让玩家变慢。
///
/// 这条串起三段彼此独立的机制：地面物品（`GroundItemAt`）→ 拾取与装备关系
/// （`EquippedBy`）→ 装备授予的效果实例化成 `EffectOf` 子实体 → 求值层丢掉
/// `Terrain` 桶。任何一段没接上，AV 都会仍然是两倍。
#[test]
fn picking_up_the_boots_cancels_the_mud_penalty() {
    let mut world = new_game(2026);
    clear_actors(&mut world);
    ecs_core::system::run_settle_systems(&mut world);

    let mud_target = {
        let cells: Vec<(usize, usize)> = {
            let mut query = world.query::<&CellEffectAt>();
            query.iter(&world).map(|at| at.to_tuple()).collect()
        };
        cells
            .into_iter()
            .find(|pos| find_walkable_step(&world, *pos).is_some())
            .expect("需要一格带可走邻格的泥潭")
    };

    // 没靴子：泥潭让 AV 翻倍。
    let plain_av = {
        let map = world.resource::<Map>();
        let mut found = None;
        for (y, row) in map.tiles.iter().enumerate() {
            for (x, tile) in row.iter().enumerate() {
                if tile.walkable()
                    && world.resource::<CellEffects>().entity_at(x, y).is_none()
                    && find_walkable_step(&world, (x, y)).is_some()
                {
                    found = Some((x, y));
                    break;
                }
            }
            if found.is_some() {
                break;
            }
        }
        move_av_at(&mut world, found.expect("地图上必须有普通地板"))
    };
    let mud_av_before = move_av_at(&mut world, mud_target);
    assert!(
        (mud_av_before - plain_av * 2.0).abs() < 1e-9,
        "前提：没靴子时泥潭必须让 AV 翻倍"
    );

    // 把靴子挪到玩家脚下，跑一轮结算 → 拾取并装备。
    let player = player_entity(&world);
    let item = {
        let mut query = world.query_filtered::<Entity, With<ecs_core::GroundItem>>();
        query.iter(&world).next().expect("开局必须放了一件地面物品")
    };
    let player_pos = player_pos(&world);
    world
        .entity_mut(item)
        .insert(ecs_core::GroundItemAt::new(player_pos.0, player_pos.1));
    ecs_core::system::run_settle_systems(&mut world);

    assert!(
        world.get::<ecs_core::EquippedItems>(player).is_some(),
        "踩上去之后必须真的装备了"
    );

    // 有靴子：同一格泥潭不再减速。
    let mud_av_after = move_av_at(&mut world, mud_target);
    assert!(
        (mud_av_after - plain_av).abs() < 1e-9,
        "捡到踏水靴后泥潭必须失效：平地 {plain_av} / 泥潭 {mud_av_after}"
    );
}
