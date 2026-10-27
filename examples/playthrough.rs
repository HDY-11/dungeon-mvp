//! 自动试玩：让脚本自己打一局，把日志与关键指标打出来，用来做人工不便做的观察。
//!
//! 用法：
//!
//! ```text
//! cargo run --example playthrough                       # 默认 6 个种子 × 400 回合，摘要
//! cargo run --example playthrough -- --trace            # 逐回合日志（很长）
//! cargo run --example playthrough -- --seeds 7,8 --turns 200 --trace
//! ```
//!
//! 期望它做的不是"跑通不 panic"（那是测试的事），而是**把一局里发生的事摊开**：
//! 谁死了、伤害从哪来、日志被什么占满、泥潭/毒/靴子有没有真的被遇到。
//!
//! 判定口径：这里**不 panic**——发现异常就打一行 `!!`，让整局继续跑完，
//! 这样一次运行能看到全部问题，而不是被第一个问题打断。

use std::collections::BTreeMap;

use bevy_ecs::prelude::*;
use ecs_core::components::{ActionTimer, Active, Health, Idle, Position, Ready};
use ecs_core::entity_cls::{Monster, Player};
use ecs_core::map::Map;
use ecs_core::world_loop::{apply_player_command, new_game};
use ecs_core::{
    ActionOf, ActiveAction, CellEffectAt, CellMoveCost, GroundItem, GroundItemAt, MonsterKindId,
    Poison, PlayerCommand, TurnManager,
};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut seeds: Vec<u64> = vec![2026, 1, 2, 3, 7, 11];
    let mut turns = 400usize;
    let mut trace = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--seeds" => {
                i += 1;
                seeds = args[i]
                    .split(',')
                    .filter_map(|s| s.trim().parse().ok())
                    .collect();
            }
            "--turns" => {
                i += 1;
                turns = args[i].parse().unwrap_or(400);
            }
            "--trace" => trace = true,
            other => eprintln!("未知参数 {other}"),
        }
        i += 1;
    }

    for seed in seeds {
        play_one(seed, turns, trace);
    }
}

/// 一局的可观测结果。
#[derive(Default, Debug)]
struct Report {
    commands: usize,
    rejected: usize,
    accepted: usize,
    kills: usize,
    damage_taken: f64,
    died_turn: Option<usize>,
    poison_turns: usize,
    poison_ticks: usize,
    mud_turns: usize,
    water_turns: usize,
    boots_turn: Option<usize>,
    logged_lines: usize,
    anomalies: Vec<String>,
}

fn play_one(seed: u64, max_turns: usize, trace: bool) {
    let mut world = new_game(seed);
    let mut report = Report::default();

    let player = player_entity(&mut world);
    let monsters_at_start = count::<Monster>(&mut world);
    let start_hp = health_of(&world, player);
    let mud_cells = {
        let mut q = world.query::<&CellEffectAt>();
        query_count(&mut q, &world)
    };
    let boots_start = count::<GroundItem>(&mut world);

    let stair = world
        .query_filtered::<&Position, With<ecs_core::Stairs>>()
        .iter(&world)
        .next()
        .map(Position::to_tuple);

    println!("=== seed {seed} ===");
    println!(
        "开局：楼层 {} / 地图 {} / 怪物 {} 只 / 泥潭 {mud_cells} 格 / 地面物品 {boots_start} 件 / 楼梯 {:?}",
        world.resource::<ecs_core::FloorNumber>().0,
        format!("{:?}", ecs_core::map_kind_for(seed, 1)),
        monsters_at_start,
        stair,
    );
    println!(
        "玩家：HP {:.0}/{:.0} / 攻 {:.0} / 防 {:.0} / 移速 {:.2}",
        start_hp.0,
        start_hp.1,
        stat(&world, player, "attack"),
        stat(&world, player, "defense"),
        stat(&world, player, "speed"),
    );

    // t0 一致性快照：占用图登记数 vs 应该登记的实体数。
    {
        let positioned = {
            let mut q = world.query::<(&Position, Option<&ecs_core::Stairs>)>();
            q.iter(&world).filter(|(_, s)| s.is_none()).count()
        };
        let registered = world
            .resource::<ecs_core::OccupancyMap>()
            .cells
            .iter()
            .flatten()
            .filter(|c| c.is_some())
            .count();
        println!("开局占用图：应登记 {positioned} 个实体，实际登记 {registered} 个");
    }

    let mut last_log = 0usize;
    let mut last_kinds = kinds(&mut world);
    let mut hp_before = start_hp.0;

    for turn in 0..max_turns {
        if world.resource::<TurnManager>().game_over {
            report.died_turn = Some(turn);
            break;
        }

        // ── 玩家脚下的格子 / 状态 ──────────────────────
        let pos = player_pos(&mut world);
        let on_mud = world
            .resource::<ecs_core::CellEffects>()
            .entity_at(pos.0, pos.1)
            .is_some();
        if on_mud {
            report.mud_turns += 1;
        }
        if world.resource::<Map>().tiles[pos.1][pos.0] == ecs_core::Tile::ShallowWater {
            report.water_turns += 1;
        }
        let poisoned = is_poisoned(&world, player);
        if poisoned {
            report.poison_turns += 1;
        }

        // ── 决策 ────────────────────────────────────────
        let (command, why) = choose_command(&world, player, pos, turn);
        let accepted = apply_player_command(&mut world, command);
        report.commands += 1;
        if accepted {
            report.accepted += 1;
        } else {
            report.rejected += 1;
            // 前三次拒绝打印诊断：世界到底卡在什么状态。
            if report.rejected <= 3 {
                let extra = match command {
                    PlayerCommand::Move { dx, dy } => {
                        let p = Position::new(pos.0, pos.1).offset(dx, dy);
                        let tile = world.resource::<Map>().tiles[p.1][p.0];
                        let occupant = world.resource::<ecs_core::OccupancyMap>().entity_at(p.0, p.1);
                        let is_monster = occupant.map(|e| world.get::<Monster>(e).is_some());
                        let legal = can_step(&world, pos, dx, dy);
                        let neigh = nearest_monster(&world, pos);
                        format!("目标格({},{}) tile={tile:?} 占用={occupant:?} 是怪物={is_monster:?} 我们判合法={legal} 最近怪={neigh:?}", p.0, p.1)
                    }
                    _ => String::from("(非移动)"),
                };
                println!(
                    "  [t{turn}] 拒绝 {command:?} (来自 {why}) 玩家@{pos:?} ← {} | {extra}",
                    describe_player(&mut world, player)
                );
            }
        }

        // ── 日志（新行） ────────────────────────────────
        {
            let log = world.resource::<ecs_core::EventLog>();
            let total = log.messages.len();
            // 环形缓冲会覆盖，所以只在"总数没到上限"时按索引取新行；
            // 到上限后只能全量比较尾部。
            let new_lines: Vec<String> = if total > last_log {
                log.messages[last_log..].iter().map(|m| m.text.clone()).collect()
            } else if total == 50 {
                // 已满：无法区分新旧，直接给出尾部一行作为样本。
                log.messages.last().map(|m| m.text.clone()).into_iter().collect()
            } else {
                Vec::new()
            };
            last_log = total;
            report.logged_lines += new_lines.len();
            for line in &new_lines {
                if line.contains("中毒") && line.contains("伤害") {
                    report.poison_ticks += 1;
                }
                if trace {
                    println!("  [t{turn}] {line}");
                }
            }
            if trace && accepted {
                println!("  [t{turn}] > {command:?}  HP {:.0}", health_of(&world, player).0);
            }
        }

        // ── 状态变化 ────────────────────────────────────
        let hp_now = health_of(&world, player).0;
        if hp_now < hp_before {
            report.damage_taken += hp_before - hp_now;
        }
        hp_before = hp_now;

        let kinds_now = kinds(&mut world);
        let before: usize = last_kinds.values().sum();
        let after: usize = kinds_now.values().sum();
        if after < before {
            report.kills += before - after;
        }
        last_kinds = kinds_now;

        if report.boots_turn.is_none() && count::<GroundItem>(&mut world) < boots_start {
            report.boots_turn = Some(turn);
        }

        // ── 不变量检查（不 panic，只记录） ──────────────
        check_invariants(&mut report, &mut world, player, turn, pos);

        for m in occupancy_mismatches(&mut world) {
            let msg = format!("t{turn} 占用图不一致：{m}");
            if !report.anomalies.contains(&msg) && report.anomalies.len() < 6 {
                report.anomalies.push(msg);
            }
        }
    }

    // ── 摘要 ────────────────────────────────────────────
    let end_hp = health_of(&world, player);
    println!(
        "结束：回合 {} / 命令 {}(接受 {} 拒绝 {}) / 击杀 {} / 受伤害 {:.0} / HP {:.0}/{:.0} / 结束原因 {}",
        report.commands,
        report.commands,
        report.accepted,
        report.rejected,
        report.kills,
        report.damage_taken,
        end_hp.0,
        end_hp.1,
        if world.resource::<TurnManager>().game_over {
            format!("死亡（第 {:?} 回合）", report.died_turn)
        } else {
            "回合用尽".to_string()
        }
    );
    println!(
        "覆盖：泥潭 {} 回合 / 浅水 {} 回合 / 中毒 {} 回合（{} 次 tick）/ 捡到靴子 {:?} / 日志行 {}",
        report.mud_turns, report.water_turns, report.poison_turns, report.poison_ticks,
        report.boots_turn, report.logged_lines,
    );
    let dist = kinds(&mut world);
    println!(
        "存活怪物：{} 只 {:?}",
        dist.values().sum::<usize>(),
        dist
    );
    if report.anomalies.is_empty() {
        println!("异常：无");
    } else {
        for a in &report.anomalies {
            println!("!! {a}");
        }
    }
    println!();
}

// ── 动作策略 ────────────────────────────────────────────

fn choose_command(
    world: &World,
    player: Entity,
    pos: (usize, usize),
    turn: usize,
) -> (PlayerCommand, &'static str) {
    let hp = health_of(world, player);
    let nearest = nearest_monster(world, pos);

    if let Some((mpos, dist)) = nearest {
        // 相邻（含对角）：打。
        if dist <= 1 {
            return (
                PlayerCommand::Move {
                    dx: (mpos.0 as isize) - (pos.0 as isize),
                    dy: (mpos.1 as isize) - (pos.1 as isize),
                },
                "相邻攻击",
            );
        }
        // 血量低且近：退。
        if hp.0 / hp.1 < 0.35 && dist <= 3 {
            let dx = (pos.0 as isize) - (mpos.0 as isize);
            let dy = (pos.1 as isize) - (mpos.1 as isize);
            let (dx, dy) = (dx.signum(), dy.signum());
            if can_step(world, pos, dx, dy) {
                return (PlayerCommand::Move { dx, dy }, "逃跑");
            }
        }
        // 否则朝它走（贪心；被挡就找别的方向）。
        let (dx, dy) = step_toward(pos, mpos);
        if can_step(world, pos, dx, dy) {
            return (PlayerCommand::Move { dx, dy }, "贪心接近");
        }
    }

    // 每 7 步等一次，采样"等待"这条路径（它是 AV 固定、不吃速度的那一类）。
    if turn % 7 == 3 {
        return (PlayerCommand::Wait, "采样等待");
    }

    // 否则随便走一个能走的方向（起点按回合轮转：被拒之后下一回合会换一条路，
    // 否则策略是确定性的，会一直重试同一个方向）。
    let dirs = [(1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1), (0, -1), (1, -1)];
    for k in 0..dirs.len() {
        let (dx, dy) = dirs[(k + turn) % dirs.len()];
        if can_step(world, pos, dx, dy) {
            return (PlayerCommand::Move { dx, dy }, "随机游走");
        }
    }
    (PlayerCommand::Wait, "无路可走")
}

fn nearest_monster(world: &World, pos: (usize, usize)) -> Option<((usize, usize), usize)> {
    let mut query = world.try_query::<&Position>()?;
    let monsters: Vec<(usize, usize)> = {
        let mut q = world.try_query::<(&Position, &Monster)>()?;
        q.iter(world).map(|(p, _)| p.to_tuple()).collect()
    };
    let _ = &mut query;
    monsters
        .into_iter()
        .map(|m| {
            let d = m.0.abs_diff(pos.0).max(m.1.abs_diff(pos.1));
            (m, d)
        })
        .min_by_key(|(_, d)| *d)
}

fn step_toward(from: (usize, usize), to: (usize, usize)) -> (isize, isize) {
    (
        (to.0 as isize - from.0 as isize).signum(),
        (to.1 as isize - from.1 as isize).signum(),
    )
}

/// 能不能走这一格——**直接用游戏自己的规则**（`moved_position`），
/// 不自己写一套：第一版自己写"目标格可走且未占用"，漏了对角穿墙规则，
/// 结果脚本一直朝同一个非法对角撞墙，400 条命令全被拒绝。
fn can_step(world: &World, pos: (usize, usize), dx: isize, dy: isize) -> bool {
    let map = world.resource::<Map>();
    let occupancy = world.resource::<ecs_core::OccupancyMap>();
    ecs_core::action::execution::movement::moved_position(
        map,
        occupancy,
        Position::new(pos.0, pos.1),
        dx,
        dy,
    )
    .is_some()
}

// ── 观测辅助 ────────────────────────────────────────────

fn check_invariants(
    report: &mut Report,
    world: &mut World,
    player: Entity,
    turn: usize,
    pos: (usize, usize),
) {
    let map = world.resource::<Map>();
    if !map.tiles[pos.1][pos.0].walkable() {
        report.anomalies.push(format!(
            "t{turn} 玩家站在不可走格 {:?} ({:?})",
            pos,
            map.tiles[pos.1][pos.0]
        ));
    }

    let (hp, max) = health_of(world, player);
    if !hp.is_finite() || hp < 0.0 || hp > max {
        report.anomalies.push(format!("t{turn} HP 越界 {hp}/{max}"));
    }

    // 空闲却还挂着 ActiveAction 子实体 = 泄漏。
    let idle = world.get::<Active>(player).is_none();
    if idle {
        let mut q = world.query::<(&ActionOf, &ActiveAction)>();
        let owned = q.iter(world).filter(|(of, _)| of.parent() == player).count();
        if owned > 0 {
            report
                .anomalies
                .push(format!("t{turn} 玩家空闲但仍有 {owned} 个 ActiveAction 子实体"));
        }
    }

    // 中毒效果必须挂在还活着的实体上（游客实体已 despawn 时应级联回收）。
    let poisons: Vec<(Entity, Entity)> = {
        let mut q = world.query::<(Entity, &Poison, &ecs_core::EffectOf)>();
        q.iter(world)
            .map(|(e, _, of)| (e, of.parent()))
            .collect()
    };
    for (effect, owner) in poisons {
        if world.get_entity(owner).is_err() {
            report
                .anomalies
                .push(format!("t{turn} 中毒效果 {effect:?} 的拥有者已消失（级联失效）"));
        }
    }

    // 泥潭常量应被真的用上（读一下，顺带防"内容没接线"）。
    let _ = CellMoveCost::new(1.0);
    let _ = GroundItemAt::new(0, 0);
}

/// 玩家当前的状态摘要（只用于诊断）。
/// **不变量：占用图必须与 `Position` 一致。**
///
/// 返回不一致的样本（最多 3 条）。这条检查是驱动器真正的价值所在——
/// 单看日志看不出"某个格子明明站着怪却是空的"。
fn occupancy_mismatches(world: &mut World) -> Vec<String> {
    let mut q = world.query::<(
        Entity,
        &Position,
        Option<&ecs_core::Stairs>,
        Option<&Monster>,
        Option<&Player>,
    )>();
    let rows: Vec<(Entity, (usize, usize), bool, String)> = q
        .iter(world)
        .map(|(e, p, stairs, monster, is_player)| {
            let role = if is_player.is_some() {
                "玩家".to_string()
            } else if monster.is_some() {
                "怪物".to_string()
            } else if stairs.is_some() {
                "楼梯".to_string()
            } else {
                "其他".to_string()
            };
            (e, p.to_tuple(), stairs.is_some(), role)
        })
        .collect();

    let mut out = Vec::new();
    let mut seen: BTreeMap<(usize, usize), usize> = BTreeMap::new();
    for (_, pos, _, _) in &rows {
        *seen.entry(*pos).or_insert(0) += 1;
    }
    for (entity, pos, _, role) in rows {
        if role == "楼梯" {
            continue;
        }
        if seen.get(&pos).copied().unwrap_or(0) > 1 {
            out.push(format!(
                "{pos:?} 有多个实体共用同一格（{entity:?} 角色={role}）"
            ));
        }
        {
            let occ = world.resource::<ecs_core::OccupancyMap>();
            let raw = if pos.1 < ecs_core::MAP_HEIGHT && pos.0 < ecs_core::MAP_WIDTH {
                Some(occ.cells[pos.1][pos.0])
            } else {
                None
            };
            let via_api = occ.entity_at(pos.0, pos.1);
            if via_api.is_none() {
                let total = occ.cells.iter().flatten().filter(|c| c.is_some()).count();
                out.push(format!(
                    "{pos:?} 上的实体 {entity:?}（角色={role}）查不到；raw={raw:?} api={via_api:?} 全图登记={total}"
                ));
            }
        }
        if out.len() >= 3 {
            break;
        }
    }
    out
}

fn describe_player(world: &mut World, player: Entity) -> String {
    let game_over = world.resource::<TurnManager>().game_over;
    let active = world.get::<Active>(player).is_some();
    let idle = world.get::<Idle>(player).is_some();
    let mut q = world.query::<(
        Entity,
        &ActionOf,
        Option<&ecs_core::ActionName>,
        Option<&ActionTimer>,
        Has<Ready>,
        Has<ActiveAction>,
        Has<ecs_core::Candidate>,
    )>();
    let rows: Vec<String> = q
        .iter(world)
        .filter(|(_, of, ..)| of.parent() == player)
        .map(|(e, _, name, timer, ready, act, cand)| {
            format!(
                "{e:?} name={:?} av={:?} ready={ready} active={act} cand={cand}",
                name.map(|n| n.0),
                timer.map(|t| t.remaining_av)
            )
        })
        .collect();
    format!("game_over={game_over} Active={active} Idle={idle} 子实体={rows:?}")
}

fn player_entity(world: &mut World) -> Entity {
    let mut q = world.query_filtered::<Entity, With<Player>>();
    q.iter(world).next().expect("有玩家")
}

fn player_pos(world: &mut World) -> (usize, usize) {
    let player = player_entity(world);
    world.get::<Position>(player).expect("玩家有位置").to_tuple()
}

fn health_of(world: &World, entity: Entity) -> (f64, f64) {
    world
        .get::<Health>(entity)
        .map(|h| (h.current, h.max))
        .unwrap_or((0.0, 0.0))
}

fn is_poisoned(world: &World, player: Entity) -> bool {
    let Some(owned) = world.get::<ecs_core::OwnedEffects>(player) else {
        return false;
    };
    owned.iter().any(|e| world.get::<Poison>(e).is_some())
}

fn count<T: Component>(world: &mut World) -> usize {
    let mut q = world.query_filtered::<Entity, With<T>>();
    q.iter(world).count()
}

fn kinds(world: &mut World) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    let mut q = world.query::<&MonsterKindId>();
    for kind in q.iter(world) {
        *out.entry(format!("{kind:?}")).or_insert(0) += 1;
    }
    out
}

fn stat(world: &World, entity: Entity, which: &str) -> f64 {
    match which {
        "attack" => world.get::<ecs_core::Attack>(entity).map_or(0.0, |a| a.0),
        "defense" => world.get::<ecs_core::Defense>(entity).map_or(0.0, |d| d.0),
        "speed" => world
            .get::<ecs_core::MoveSpeed>(entity)
            .map_or(0.0, |s| s.0),
        _ => 0.0,
    }
}

fn query_count<Q: bevy_ecs::query::QueryData>(q: &mut QueryState<Q>, world: &World) -> usize {
    q.iter(world).count()
}
