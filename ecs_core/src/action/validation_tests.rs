//! `action::validation`（`ActionPhase::Validate`）的测试。
//!
//! 这一层要钉住三件事，缺一条这条链路就白拆了：
//!
//! 1. **判定只有一处**：每个行动恰好一个校验器，且只认领自己那一种行动；
//! 2. **预留是真的**：判定通过就立刻占住目标格，所以**同一轮**里第二个人抢不到；
//! 3. **落地只有一处**：`settle_action_system` 只写结果，不再判定（所以它无法
//!    把一个被挡的移动落地）。
//!
//! 这些用例之所以能写出来，正是因为判定从执行器里搬了出来：以前"这一步为什么被拒"
//! 藏在四个执行器内部，只能通过"位置有没有变"间接观察。

// `super::*` 已经带出校验器用到的组件（`Move`/`Chase`/`Flee`/`Wander`/`BasicAttack`/
// `Ready`/`Position`/`Health`/`LastKnownPlayerPos`）与三个资源；这里只补它没有的。
use super::*;
use crate::action::entity::{ActionPriority, ActionSource, ActiveAction};
use crate::action::settle::settle_action_system;
use crate::action::ownership::ActionOf;
use crate::components::{ActionTimer, Active, BasicAttack, Blocks, Idle, Ready, Wait};
use crate::events::{ActionFailedEvent, ActionSucceededEvent, AttackIntentEvent};
use crate::map::Tile;
use crate::schedule::ActionPocSchedule;
use crate::test_util::{fill_map, single_tile_scene, spawn_test_monster};
use bevy_ecs::event::Events;
use bevy_ecs::system::RunSystemOnce;

// ── 场景脚手架 ───────────────────────────────────────

/// 全平地 + 玩家 `(10, 10)`：绝大多数用例只需要"没有障碍的空场地"。
///
/// 顺带把事件缓冲清空：`single_tile_scene` 里的 `run_settle_systems` 可能已经产生过
/// 事件，而下面每条用例都在数"本轮产生了几个"。
fn open_world() -> (World, Entity) {
    let (mut world, player) = single_tile_scene();
    fill_map(&mut world, Tile::Floor);
    world.entity_mut(player).insert(Position::new(10, 10));
    crate::system::run_settle_systems(&mut world);
    clear_events(&mut world);
    (world, player)
}

/// 清空行动链路关心的三个事件缓冲（数"本轮产生了什么"之前必须先归零）。
fn clear_events(world: &mut World) {
    world
        .resource_mut::<Events<ActionSucceededEvent>>()
        .clear();
    world.resource_mut::<Events<ActionFailedEvent>>().clear();
    world
        .resource_mut::<Events<AttackIntentEvent>>()
        .clear();
}

/// 在 `actor` 名下挂一个行动实体，`ready` 决定它是否已到点。
///
/// 直接造"到点 / 排队中"状态而不是跑生成/仲裁/tick：这一层测的是校验器，不是调度。
fn mount_action<B: Bundle>(world: &mut World, actor: Entity, action: B, ready: bool) -> Entity {
    let mut entity = world.spawn((
        ActionOf(actor),
        ActionPriority(0),
        ActionSource::Ai,
        ActionTimer {
            remaining_av: if ready { 0.0 } else { 5.0 },
        },
        ActiveAction,
        action,
    ));
    if ready {
        entity.insert(Ready);
    }
    let entity = entity.id();
    world.entity_mut(actor).remove::<Idle>().insert(Active);
    entity
}

/// 已到点的行动（校验器会对它**定案**）。
fn mount_ready<B: Bundle>(world: &mut World, actor: Entity, action: B) -> Entity {
    mount_action(world, actor, action, true)
}

/// 排队中的行动（`ActiveAction` 但没有 `Ready`）：校验器只对它做**保活**。
fn mount_queued<B: Bundle>(world: &mut World, actor: Entity, action: B) -> Entity {
    mount_action(world, actor, action, false)
}

/// 把一只怪物放在某格（`Blocks` + `Health` + `Position` 齐备）。
fn monster_at(world: &mut World, pos: (usize, usize)) -> Entity {
    spawn_test_monster(
        world,
        crate::monster::MonsterKindId::Rat,
        pos,
        10.0,
        4.0,
    )
}

/// 只跑校验阶段（按调度里的顺序），不落地。
fn run_validators(world: &mut World) {
    let mut schedule = crate::schedule::core_schedule(ActionPocSchedule);
    schedule.add_systems(
        (
            validate_move_system,
            validate_basic_attack_system,
            validate_chase_system,
            validate_flee_system,
            validate_wander_system,
        )
            .chain(),
    );
    schedule.run(world);
}

/// 跑「校验 → 落地」。
fn run_validate_and_settle(world: &mut World) {
    run_validators(world);
    let _ = world.run_system_once(settle_action_system);
}

/// 占用图里被占住的格子（用于断言"预留恰好一格、多余的都退掉了"）。
fn occupied_cells(world: &World) -> Vec<(usize, usize)> {
    let occupancy = world.resource::<OccupancyMap>();
    let mut cells: Vec<(usize, usize)> = Vec::new();
    for (y, row) in occupancy.cells.iter().enumerate() {
        for (x, cell) in row.iter().enumerate() {
            if cell.is_some() {
                cells.push((x, y));
            }
        }
    }
    cells
}

fn position_of(world: &World, entity: Entity) -> (usize, usize) {
    world.get::<Position>(entity).unwrap().to_tuple()
}

// ── 1. 预留：同一轮里第二个人抢不到同一格 ───────────

/// **ECS45 的校验层复现**：两个 actor 同一轮都想进 `(10, 11)`。
///
/// 第一个校验器定案并**立刻占住**，所以第二个看到的是"这格已经有人"——即使两个
/// 行动是在**同一轮**里被判定的。这条性质以前靠"执行器发意图 → 应用系统按顺序写穿"
/// 才成立，现在靠阶段顺序（串行校验）成立，强度更高：占用图在做第二个判定**之前**
/// 就已经是对的。
#[test]
fn the_second_claimant_of_a_cell_loses_before_anyone_moves() {
    let (mut world, _player) = open_world();
    let first = monster_at(&mut world, (4, 10));
    let second = monster_at(&mut world, (4, 12));
    // 两人都朝 `(4, 11)` 走。
    let first_action = mount_ready(&mut world, first, Move { dx: 0, dy: 1 });
    let second_action = mount_ready(&mut world, second, Move { dx: 0, dy: -1 });

    run_validators(&mut world);

    assert_eq!(
        world.get::<MoveTarget>(first_action),
        Some(&MoveTarget(Position::new(4, 11))),
        "先判定的那个必须拿到落点"
    );
    assert!(
        world.get::<MoveTarget>(second_action).is_none(),
        "同一轮里第二个想进同一格的行动不得拿到落点"
    );
    // 预留发生在**落地之前**：这正是"判定只有一处"能成立的原因。
    assert_eq!(
        world.resource::<OccupancyMap>().entity_at(4, 11),
        Some(first),
        "校验阶段就该把目标格占住（不是等到落地才写）"
    );
    assert_eq!(
        position_of(&world, first),
        (4, 10),
        "校验阶段不得移动任何人——写位置是落地阶段的事"
    );
    assert_eq!(
        world.resource::<Events<ActionFailedEvent>>().len(),
        1,
        "被挡的 `Move` 必须失败（`Move` 的语义是「走到那一步」）"
    );
}

// ── 2. 落地：只写结果 ────────────────────────────────

/// 落地把预留兑现：`Position` 变、旧格腾出、目标格仍归本人。
#[test]
fn settle_lands_the_reserved_step_and_frees_the_old_cell() {
    let (mut world, _player) = open_world();
    let actor = monster_at(&mut world, (8, 8));
    let action = mount_ready(&mut world, actor, Move { dx: 1, dy: 0 });

    run_validate_and_settle(&mut world);

    assert_eq!(position_of(&world, actor), (9, 8), "落地必须移动一格");
    assert_eq!(
        world.resource::<OccupancyMap>().entity_at(9, 8),
        Some(actor),
        "新格必须归本人"
    );
    assert_eq!(
        world.resource::<OccupancyMap>().entity_at(8, 8),
        None,
        "旧格必须腾出（否则这个人变成两格）"
    );
    assert_eq!(
        world.resource::<Events<ActionSucceededEvent>>().len(),
        1,
        "落地之后恰好一个成功事件"
    );
    assert!(world.get::<Ready>(action).is_none(), "终态出口要清 Ready");
}

/// 落地器**不判定**：没有 `MoveTarget` 就意味着"原地"，绝不会自己算一步。
///
/// 这条是"判定只有一处"的反面保证：如果有人把判定塞回落地器，被挡的移动会
/// 自己找一格走掉，这条用例就会失败（`Wait` 没有校验器 → 必须原地）。
#[test]
fn settle_never_invents_a_step_without_a_decision() {
    let (mut world, _player) = open_world();
    let actor = monster_at(&mut world, (8, 8));
    mount_ready(&mut world, actor, Wait);

    run_validate_and_settle(&mut world);

    assert_eq!(position_of(&world, actor), (8, 8), "没有定案就必须原地");
    assert_eq!(
        world.resource::<Events<ActionSucceededEvent>>().len(),
        1,
        "`Wait` 没有校验器 = 永远有效（无条件成功）"
    );
    assert_eq!(
        world.resource::<Events<ActionFailedEvent>>().len(),
        0,
        "`Wait` 不得失败"
    );
}

// ── 3. 被挡的语义：只有 `Move` 会失败 ────────────────

/// 撞墙的玩家必须**看到原因**（ISSUES SYN6），而且原因来自规则本身。
#[test]
fn a_blocked_player_move_fails_and_reports_the_reason() {
    let (mut world, player) = open_world();
    // 玩家东边砌墙：`Wall` → "前面过不去"。
    world.resource_mut::<Map>().tiles[10][11] = Tile::Wall;
    let action = mount_ready(&mut world, player, Move { dx: 1, dy: 0 });

    run_validators(&mut world);

    assert!(
        world.get::<MoveTarget>(action).is_none(),
        "被墙挡住的移动不得定案"
    );
    assert_eq!(world.resource::<Events<ActionFailedEvent>>().len(), 1);
    let log = world.resource::<EventLog>();
    let reason = super::super::execution::movement::BlockReason::Wall.describe();
    assert!(
        log.messages.iter().any(|message| message.text == reason),
        "玩家的失败原因必须进 EventLog（期望含「{reason}」）：{:?}",
        log.messages.iter().map(|m| &m.text).collect::<Vec<_>>()
    );
}

/// 游荡被挡**不算失败**：随机试探本来就是"试了，没走成"。
///
/// 这条守的是"被挡"在两类行动里的不同语义——把 `Wander` 也判成失败会让怪物
/// 在角落里反复刷 `Failure` 状态，进而改变生成系统的过滤结果。
#[test]
fn a_blocked_wander_stays_put_but_still_succeeds() {
    let (mut world, _player) = open_world();
    let actor = monster_at(&mut world, (1, 1));
    // 把 `(1, 1)` 四周砌墙：八个方向全被挡。
    {
        let mut map = world.resource_mut::<Map>();
        for (dx, dy) in [(-1isize, -1isize), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)]
        {
            let x = 1isize.wrapping_add(dx) as usize;
            let y = 1isize.wrapping_add(dy) as usize;
            map.tiles[y][x] = Tile::Wall;
        }
    }
    let before = world.resource::<GameRng>().steps;
    mount_ready(&mut world, actor, Wander);

    run_validate_and_settle(&mut world);

    assert_eq!(position_of(&world, actor), (1, 1), "全被挡时只能原地");
    assert_eq!(
        world.resource::<Events<ActionFailedEvent>>().len(),
        0,
        "游荡被挡不得记成失败"
    );
    assert_eq!(
        world.resource::<Events<ActionSucceededEvent>>().len(),
        1,
        "游荡被挡也算这一步做完"
    );
    assert_eq!(
        world.resource::<GameRng>().steps,
        before + 1,
        "每个游荡行动恰好消耗一次随机数（与旧执行器同序）"
    );
}

// ── 4. 校验器只认领自己的行动 ────────────────────────

/// 五个行动同轮到期：每个都被**自己那个**校验器认领，互不串味。
///
/// 这条用例的载荷是"过滤器是 `ReadyAction<A>`"这件事本身——`validate_and_settle`
/// 之类的写法（以及生产调度里五个校验器串行跑）都依赖它：如果某个校验器漏了
/// `With<A>`，它就会替别人的行动做判定，落点/目标会串到错误的实体上。
#[test]
fn each_validator_only_claims_its_own_action_type() {
    let (mut world, player) = open_world();

    // A：`Move` —— 空地一步。
    let mover = monster_at(&mut world, (20, 20));
    let move_action = mount_ready(&mut world, mover, Move { dx: 0, dy: 1 });

    // B：`BasicAttack` —— 与玩家相邻。
    let biter = monster_at(&mut world, (11, 10));
    let attack_action = mount_ready(&mut world, biter, BasicAttack { target: player });

    // C：`Chase` —— 看不见但有记忆，朝记忆点走一步。
    let chaser = monster_at(&mut world, (30, 30));
    world.entity_mut(chaser).insert(LastKnownPlayerPos(Some((30, 32))));
    let chase_action = mount_ready(&mut world, chaser, Chase);

    // D：`Flee` —— 低血（低于滞回退出阈值），朝离玩家最远的合法方向逃。
    let coward = monster_at(&mut world, (5, 10));
    world.get_mut::<Health>(coward).unwrap().current = 1.0;
    let flee_action = mount_ready(&mut world, coward, Flee);

    // E：`Wander` —— 随机一步。
    let wanderer = monster_at(&mut world, (40, 40));
    let wander_action = mount_ready(&mut world, wanderer, Wander);

    let rng_before = world.resource::<GameRng>().steps;
    // 让占用图与真实世界一致（怪物是刚 spawn 的，得重建一次占用图）。
    crate::system::run_settle_systems(&mut world);
    clear_events(&mut world);
    run_validators(&mut world);

    assert_eq!(
        world.get::<MoveTarget>(move_action),
        Some(&MoveTarget(Position::new(20, 21))),
        "`Move` 应当按 payload 定案"
    );
    assert_eq!(
        world.get::<AttackTarget>(attack_action),
        Some(&AttackTarget(player)),
        "`BasicAttack` 应当定案为攻击玩家"
    );
    assert_eq!(
        world.get::<MoveTarget>(chase_action),
        Some(&MoveTarget(Position::new(30, 31))),
        "`Chase` 应当朝最后已知位置走 A* 首步"
    );
    assert_eq!(
        world.get::<MoveTarget>(flee_action),
        Some(&MoveTarget(Position::new(4, 9))),
        "`Flee` 应当选最远的合法方向（平局取方向表里更靠前的）"
    );
    assert!(
        world.get::<MoveTarget>(wander_action).is_some(),
        "空地上游荡必须拿到一个落点"
    );
    assert_eq!(
        world.resource::<GameRng>().steps,
        rng_before + 1,
        "只有 `Wander` 消费随机数（其余四个校验器都不许碰 RNG）"
    );
    assert_eq!(
        world.resource::<Events<ActionFailedEvent>>().len(),
        0,
        "五个行动都不该失败"
    );
}

// ── 5. 各行动的定案规则 ─────────────────────────────

/// `Chase`：无路可走时**原地**，而且仍然算成功。
///
/// 与 `Move` 的差别就在这里：追击的语义是"尽量靠近"，靠近不了不算没做成；
/// 判成失败会让追击者在被堵的那一轮丢掉 `Idle`，进而影响下一轮的候选生成。
///
/// 这里构造的是「A\* 找不到路」这一支——注意**不能**用"唯一的下一格被占"来构造：
/// A\* 本来就会绕开占用格，它会先绕路而不是失败。另一支（A\* 给了对角首步但
/// `plan_step` 判它穿角）同样落到原地。
#[test]
fn a_chase_with_no_path_stays_put_and_succeeds() {
    let (mut world, _player) = open_world();
    let chaser = monster_at(&mut world, (30, 30));
    world.entity_mut(chaser).insert(LastKnownPlayerPos(Some((30, 32))));
    // 把追击者四周砌死：A* 没有首步可用。
    {
        let mut map = world.resource_mut::<Map>();
        for (dx, dy) in [
            (-1isize, -1isize),
            (0, -1),
            (1, -1),
            (-1, 0),
            (1, 0),
            (-1, 1),
            (0, 1),
            (1, 1),
        ] {
            let x = 30isize.wrapping_add(dx) as usize;
            let y = 30isize.wrapping_add(dy) as usize;
            map.tiles[y][x] = Tile::Wall;
        }
    }
    crate::system::run_settle_systems(&mut world);
    clear_events(&mut world);
    let action = mount_ready(&mut world, chaser, Chase);

    run_validate_and_settle(&mut world);

    assert!(
        world.get::<MoveTarget>(action).is_none(),
        "无路可走时不得定案（不能凭空造一步）"
    );
    assert_eq!(position_of(&world, chaser), (30, 30), "追击者原地不动");
    assert_eq!(
        world.resource::<Events<ActionFailedEvent>>().len(),
        0,
        "被挡的追击不算失败"
    );
    assert_eq!(
        world.resource::<Events<ActionSucceededEvent>>().len(),
        1,
        "被挡的追击仍算这一步做完"
    );
}

/// `Chase` 保活：既看不见玩家、也没有记忆 → 行动失败。
#[test]
fn a_chase_without_sight_or_memory_fails() {
    let (mut world, _player) = open_world();
    let chaser = monster_at(&mut world, (30, 30));
    let action = mount_ready(&mut world, chaser, Chase);

    run_validators(&mut world);

    assert!(world.get::<MoveTarget>(action).is_none());
    assert_eq!(
        world.resource::<Events<ActionFailedEvent>>().len(),
        1,
        "看不见又没记忆的追击必须失败"
    );
}

/// `Flee` 保活：血量回到滞回退出阈值以上 → 不再逃。
#[test]
fn a_flee_above_the_exit_threshold_fails() {
    let (mut world, _player) = open_world();
    let coward = monster_at(&mut world, (5, 10));
    // 满血（ratio = 1.0 > FLEE_HP_RATIO_EXIT）：滞回区间已经退出。
    let action = mount_ready(&mut world, coward, Flee);

    run_validators(&mut world);

    assert!(world.get::<MoveTarget>(action).is_none(), "不该再逃");
    assert_eq!(world.resource::<Events<ActionFailedEvent>>().len(), 1);
}

/// `Flee` 的预留只有**一格**：八个方向都试算过，但只有选中的那个被占住。
///
/// 这条守的是一个实测到的失败模式：如果试算顺手预留，`plan_step` 的**穿角**检查会
/// 读到刚占下的格子，后面的对角方向被误判成 `CornerCutting`——选出来的就不是"最远方向"
/// 而是"没被自己挡死的方向里最靠后的那个"（实测把 `(4, 9)` 换成了 `(4, 11)`）。
/// 所以 `Reservation` 必须把「试算」与「预留」分成 `probe` / `claim` 两步。
#[test]
fn a_flee_claims_exactly_one_cell() {
    let (mut world, _player) = open_world();
    let coward = monster_at(&mut world, (5, 10));
    world.get_mut::<Health>(coward).unwrap().current = 1.0;
    crate::system::run_settle_systems(&mut world);
    clear_events(&mut world);
    mount_ready(&mut world, coward, Flee);

    run_validate_and_settle(&mut world);

    // 不变式：落地之后占用图里的格子**恰好**与"占格实体"一一对应——没有任何幽灵预留。
    // 这比逐格列出来更强，也不受场景里还有哪些实体影响（玩家、角落里的怪物都在）。
    //
    // 为什么必须落地之后看：校验阶段一个人合法地占**两格**（原格 + 预留格），落地才腾出
    // 原格。所以"一人一格"是对**落地结果**的不变式，不是对校验中间态的不变式。
    let blocks = world
        .query_filtered::<Entity, With<Blocks>>()
        .iter(&world)
        .count();
    let occupied = occupied_cells(&world).len();
    assert_eq!(
        occupied, blocks,
        "占用图里出现了比分身更多的格子：试算方向留下了幽灵预留"
    );
    assert_eq!(
        world.resource::<OccupancyMap>().entity_at(4, 9),
        Some(coward),
        "选中的那一格必须归逃跑者（平局取方向表里更靠前的那个方向）"
    );
    assert_eq!(
        world.resource::<OccupancyMap>().entity_at(5, 10),
        None,
        "逃跑者的原格必须腾出"
    );
}

/// `BasicAttack`：相邻 + 目标存活才定案；不相邻、或目标已死 → 失败。
#[test]
fn a_basic_attack_needs_adjacency_and_a_living_target() {
    // 相邻 + 存活 → 定案。
    {
        let (mut world, player) = open_world();
        let target = monster_at(&mut world, (11, 10));
        let action = mount_ready(&mut world, player, BasicAttack { target });
        run_validators(&mut world);
        assert_eq!(
            world.get::<AttackTarget>(action),
            Some(&AttackTarget(target)),
            "相邻且存活时必须定案为攻击"
        );
        assert_eq!(world.resource::<Events<ActionFailedEvent>>().len(), 0);
    }

    // 不相邻 → 失败。
    {
        let (mut world, player) = open_world();
        let target = monster_at(&mut world, (14, 10));
        let action = mount_ready(&mut world, player, BasicAttack { target });
        run_validators(&mut world);
        assert!(world.get::<AttackTarget>(action).is_none(), "够不着不得定案");
        assert_eq!(world.resource::<Events<ActionFailedEvent>>().len(), 1);
    }

    // 相邻但目标已死 → 失败。
    {
        let (mut world, player) = open_world();
        let target = monster_at(&mut world, (11, 10));
        world.get_mut::<Health>(target).unwrap().current = 0.0;
        let action = mount_ready(&mut world, player, BasicAttack { target });
        run_validators(&mut world);
        assert!(
            world.get::<AttackTarget>(action).is_none(),
            "打尸体不得定案"
        );
        assert_eq!(world.resource::<Events<ActionFailedEvent>>().len(), 1);
    }
}

// ── 6. 攻击意图由落地阶段发出 ────────────────────────

/// 定案为攻击之后，**落地**才发 `AttackIntentEvent`（判定与声明分开）。
#[test]
fn settle_emits_the_attack_intent_for_an_attack_decision() {
    let (mut world, player) = open_world();
    let target = monster_at(&mut world, (11, 10));
    mount_ready(&mut world, player, BasicAttack { target });

    run_validate_and_settle(&mut world);

    let intents = world.resource::<Events<AttackIntentEvent>>();
    assert_eq!(intents.len(), 1, "一次攻击恰好一个意图");
    let intents = world.resource_mut::<Events<AttackIntentEvent>>();
    let mut cursor = intents.get_cursor();
    let collected: Vec<(Entity, Entity)> = cursor
        .read(&intents)
        .map(|intent| (intent.attacker, intent.target))
        .collect();
    assert_eq!(
        collected,
        vec![(player, target)],
        "意图必须是「玩家打那只怪物」"
    );
}

// ── 7. 保活覆盖范围：排队中的行动也要查（ISSUES ECS43） ──
//
// 一个行动从拿到 `ActiveAction` 到真正执行之间可能隔若干回合（`tick` 每轮只推进最小正 AV）。
// 只在这一刻之前不查，失效的行动就会一直占住 actor 唯一的槽位，而生成系统被 `Without<Active>`
// 挡住——actor 于是空转。下面四条分别钉住"保活生效"与"定案不提前"。

/// 排队中的追击：一旦既看不见又没记忆，**立刻**取消（不等到到点）。
#[test]
fn a_queued_chase_is_cancelled_the_moment_it_loses_the_target() {
    let (mut world, _player) = open_world();
    let chaser = monster_at(&mut world, (30, 30));
    // 没有 Viewshed、没有记忆：追击的成立条件从一开始就不满足。
    let action = mount_queued(&mut world, chaser, Chase);
    assert!(
        world.get::<Ready>(action).is_none(),
        "这条用例的前提是行动**还在排队**"
    );

    run_validators(&mut world);

    assert_eq!(
        world.resource::<Events<ActionFailedEvent>>().len(),
        1,
        "排队中的追击失去目标必须立刻取消（ECS43：否则空占槽位）"
    );
    assert!(
        world.get::<MoveTarget>(action).is_none(),
        "取消它，而不是替它算一步"
    );
}

/// 排队中的逃跑：血量回到滞回退出阈值以上 → 立刻取消。
#[test]
fn a_queued_flee_is_cancelled_when_health_recovers() {
    let (mut world, _player) = open_world();
    let coward = monster_at(&mut world, (5, 10));
    // 满血：`ratio() >= FLEE_HP_RATIO_EXIT`，逃跑的成立条件已经不在了。
    let action = mount_queued(&mut world, coward, Flee);

    run_validators(&mut world);

    assert_eq!(
        world.resource::<Events<ActionFailedEvent>>().len(),
        1,
        "排队中的逃跑在血量恢复后必须立刻取消"
    );
    assert!(world.get::<MoveTarget>(action).is_none());
}

/// 排队中的攻击：目标走开 → 立刻取消。
#[test]
fn a_queued_basic_attack_is_cancelled_when_the_target_walks_away() {
    let (mut world, player) = open_world();
    let target = monster_at(&mut world, (14, 10)); // 够不着
    let action = mount_queued(&mut world, player, BasicAttack { target });

    run_validators(&mut world);

    assert_eq!(
        world.resource::<Events<ActionFailedEvent>>().len(),
        1,
        "排队中的攻击在目标离开后必须立刻取消"
    );
    assert!(world.get::<AttackTarget>(action).is_none());
}

/// **定案与预留不提前**：排队中的行动即使仍然有效，也不许定方向、不许占格、不许取随机数。
///
/// 三条都很要紧：
/// - 提前定案会拿到一个"到时候已经过期"的落点；
/// - 提前预留会白占一格（还会挡住别人这一整轮）；
/// - 提前取随机数会让**同一颗种子的整局结果漂移**（随机流的分配时机变了）。
#[test]
fn a_queued_action_is_never_decided_reserved_or_randomised() {
    let (mut world, _player) = open_world();

    // 仍然有效的排队追击（有最后已知位置，保活成立，只是还没到点）：不该定案。
    let chaser = monster_at(&mut world, (30, 30));
    world
        .entity_mut(chaser)
        .insert(LastKnownPlayerPos(Some((30, 32))));
    let chase = mount_queued(&mut world, chaser, Chase);

    // 排队中的游荡：不该取随机数、不该占格。
    let wanderer = monster_at(&mut world, (40, 40));
    let wander = mount_queued(&mut world, wanderer, Wander);

    crate::system::run_settle_systems(&mut world);
    clear_events(&mut world);
    let occupied_before = occupied_cells(&world);
    let rng_before = world.resource::<GameRng>().steps;

    run_validators(&mut world);

    assert!(
        world.get::<MoveTarget>(chase).is_none(),
        "排队中的追击不得提前定案"
    );
    assert!(
        world.get::<MoveTarget>(wander).is_none(),
        "排队中的游荡不得提前定案"
    );
    assert_eq!(
        occupied_cells(&world).len(),
        occupied_before.len(),
        "排队中的行动不得预留格子（占用图的变化等于提前占位）"
    );
    assert_eq!(
        world.resource::<GameRng>().steps,
        rng_before,
        "排队中的行动不得消耗随机数（否则整局随机流会漂）"
    );
    assert_eq!(
        world.resource::<Events<ActionFailedEvent>>().len(),
        0,
        "仍然有效的排队行动不该被取消"
    );
}
