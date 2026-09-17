//! 行动实体链路（REFACTOR.md §3.6 / §11.3 Phase C；DESIGN Dsn27）。
//!
//! **一个行动 = 一个 actor 的瞬态子实体**，取代中央分派的 `ActionKind`：
//!
//! ```text
//! Generation（每个行为一个生成系统，只 spawn 候选，不碰 actor 状态）
//!     ↓ ApplyDeferred
//! Arbitration（唯一写入 actor 行动状态的系统）
//!     ↓
//! Tick（推进 ActiveAction 的 ActionTimer，归零加 Ready）
//!     ↓
//! Execution（每个行动一个专用 query 系统，零中央 match；只发事件）
//!     ↓ ActionSucceeded / ActionFailedEvent
//! Completion（唯一 despawn action 实体 + 回转 actor `Idle` / `Failure` 的系统）
//! ```
//!
//! # 现状
//!
//! - **已接主循环**（C7）：`world/loop_.rs` 每轮运行
//!   [`build_action_poc_schedule`]，玩家命令先经 [`build_player_mount_schedule`]；
//! - **旧模型已删除**（C8）：`ActionKind` / `mount_action` 中央 match / actor 上的
//!   行动 ZST / 独占执行系统都不复存在；
//! - 调度标签仍叫 `ActionPocSchedule`（Phase B 遗留名），为少改调用点保留。
//!
//! # 与 actor 组件模型的关系
//!
//! `Can*` 仍是 actor 上的 ZST 组件（回答“能不能做”）；action 实体只回答
//! “正在考虑/执行什么”。actor 的 `Idle`/`Active`/`Failure` 仍是行动状态，
//! 但**只有仲裁与 completion 可以写**：生成系统一律不碰，且 `Idle`/`Failure` 互斥。
//!
//! # 不存档
//!
//! action 实体是瞬态子实体；存档只存 actor 状态，读档后重新生成行动
//! （见 REFACTOR.md §10.7）。

use crate::action::generation::player::PlayerCommand;
use crate::balance::{
    CHASE_DURATION, FLEE_DURATION, FLEE_HP_RATIO, FLEE_HP_RATIO_EXIT, UNARMED_ATTACK_DURATION,
    WAIT_DURATION, WANDER_DURATION, action_av,
};
use crate::components::{
    ActionTimer, Active, AttackSpeed, BasicAttack, CanChase, CanFlee, CanWait, CanWander, Chase,
    Failure, Flee, Health, Idle, LastKnownPlayerPos, Move, MoveSpeed, Position, Ready, Viewshed,
    Wait, Wander,
};
use crate::entity_cls::{Monster, Player};
use crate::events::{ActionFailedEvent, ActionSucceededEvent, AttackIntentEvent};
use crate::map::{MAP_HEIGHT, MAP_WIDTH, Map};
use crate::resources::{GameRng, OccupancyMap};
use crate::schedule::{ActionPocSchedule, PlayerMountSchedule};
use bevy_ecs::prelude::*;
use bevy_ecs::query::Or;
use std::collections::{HashMap, HashSet};

// ── 优先级表（REFACTOR.md §3.5） ──────────────────────
//
// 仲裁取**最小** `(ActionPriority, to_bits())`，所以优先级越高 = 数值越小。
// 与 §3.5 一一对应：Flee 200 > Chase 100 > Wander 50 > Wait 0。

pub const PRIORITY_FLEE: i32 = -200;
pub const PRIORITY_CHASE: i32 = -100;
pub const PRIORITY_WANDER: i32 = -50;
pub const PRIORITY_WAIT: i32 = 0;

/// 玩家行动不进入 AI 仲裁（§3.5「玩家行动不进入此表」），因此这里给一个
/// 明确凌驾于 AI 之上的值：即使未来玩家行动被写进仲裁路径，也不会被 AI 覆盖。
pub const PRIORITY_PLAYER: i32 = -1000;

// ── action 实体组件 ──────────────────────────────────

/// 仲裁排序键。**越小越优先**；同值由 `action_entity.to_bits()` 升序打破平局。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ActionPriority(pub i32);

/// 行动来源。玩家行动不参与 AI 仲裁（Phase C 才做），这里先只用于调试/日志。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionSource {
    Ai,
    Player,
}

/// 生成系统产出、等待仲裁的候选。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Candidate;

/// 仲裁选中、正在计时/执行的行动。一个 actor 至多一个。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ActiveAction;

/// 人类可读的行动名，只用于日志（§3.6.9 调试建议）。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionName(pub &'static str);

// ── 行动类别 → 速度组件（Phase D / REFACTOR.md §2.6） ──
//
// 「哪一类行动受哪个速度影响」是**生成期的口径**，所以映射放在这里，
// 而不是散在各生成系统里。执行器完全不关心速度：AV 在挂载时就固化进
// `ActionTimer`，之后 tick 与执行只看剩余值。

/// 行动类别：决定用哪个速度组件把 `base_duration` 换算成 AV。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpeedCategory {
    /// 用 `MoveSpeed`：`Move` / `Chase` / `Flee` / `Wander`。
    Move,
    /// 用 `AttackSpeed`：`BasicAttack`。
    Attack,
    /// 固定耗时、不受任何速度影响：`Wait`（REFACTOR.md §11.6 第 4 项）。
    Fixed,
}

impl SpeedCategory {
    /// 计算该类别行动的 AV。
    ///
    /// 缺失速度组件时回退到 `1.0` 基准并告警——见 [`action_av_of`] 的说明。
    pub fn action_av(self, base_duration: f64, move_speed: f64, attack_speed: f64) -> f64 {
        match self {
            SpeedCategory::Move => action_av(base_duration, move_speed),
            SpeedCategory::Attack => action_av(base_duration, attack_speed),
            SpeedCategory::Fixed => base_duration,
        }
    }
}

/// 速度组件缺失时的回退值（= 基准 1.0）。
///
/// 生成系统用 `Option<&MoveSpeed>` 查询而不是必需组件：**Bevy 的查询遇到
/// 不匹配的 archetype 会静默返回空**（LESSONS.md L49）。若写成必需组件，
/// 一个漏挂速度组件的 actor 会直接从 AI 里消失、既不行动也不报错；
/// 回退到基准 + 告警则让这种漏挂可见且不至于卡死行为。
const FALLBACK_SPEED: f64 = 1.0;

/// 计算某个 actor 的某类行动 AV，速度组件缺失时回退到 [`FALLBACK_SPEED`]。
///
/// 速度组件缺失**不应**被静默忽略（LESSONS.md L49 的同类问题：查询不匹配就是
/// 静静地什么都不做）。调用方用 [`speed_or_warn`] 解析组件，回退前先告警，
/// 让「漏挂速度组件」在日志里可见。
fn action_av_of(
    category: SpeedCategory,
    base_duration: f64,
    move_speed: f64,
    attack_speed: f64,
) -> f64 {
    category.action_av(base_duration, move_speed, attack_speed)
}

/// 解析速度组件；缺失时告警并回退到基准 [`FALLBACK_SPEED`]。
///
/// `name` 只用于日志。用 `Option<&MoveSpeed>` 而不是必需组件查询，是因为
/// **Bevy 的查询遇到不匹配的 archetype 会静默返回空**（LESSONS.md L49）：
/// 写成必需组件的话，一个漏挂速度组件的 actor 会直接从 AI 里消失。
fn speed_or_warn(speed: Option<&MoveSpeed>, actor: Entity) -> f64 {
    speed.map_or_else(
        || {
            log::warn!("actor={actor:?} 缺少 MoveSpeed，按基准 {FALLBACK_SPEED} 计算 AV");
            FALLBACK_SPEED
        },
        |speed| speed.0,
    )
}

/// [`speed_or_warn`] 的 `AttackSpeed` 版本。
fn attack_speed_or_warn(speed: Option<&AttackSpeed>, actor: Entity) -> f64 {
    speed.map_or_else(
        || {
            log::warn!("actor={actor:?} 缺少 AttackSpeed，按基准 {FALLBACK_SPEED} 计算 AV");
            FALLBACK_SPEED
        },
        |speed| speed.0,
    )
}

// ── 生成系统 ─────────────────────────────────────────
//
// 生成系统只 spawn 候选：不写 actor 的 `Idle/Active/Failure`，不写计时器。

/// 游荡候选：空闲/失败且具 `CanWander` 的怪物各产出一个 `Wander` 候选。
pub fn wander_generation_system(
    mut commands: Commands,
    actors: Query<
        (Entity, Option<&MoveSpeed>),
        (
            With<Monster>,
            With<CanWander>,
            Without<Active>,
            Or<(With<Idle>, With<Failure>)>,
        ),
    >,
) {
    for (actor, move_speed) in &actors {
        commands.spawn((
            ChildOf(actor),
            ActionPriority(PRIORITY_WANDER),
            ActionSource::Ai,
            ActionName("Wander"),
            ActionTimer {
                remaining_av: action_av_of(
                    SpeedCategory::Move,
                    WANDER_DURATION,
                    speed_or_warn(move_speed, actor),
                    FALLBACK_SPEED,
                ),
            },
            Wander,
            Candidate,
        ));
    }
}

/// 逃跑候选：生命占比低于进入阈值且具 `CanFlee`。
///
/// 存在意义有两个：让仲裁真的需要比较优先级（Flee 200 > Wander 50），
/// 以及给 §3.6.4「每个行为一个生成系统」提供第二个样本。
pub fn flee_generation_system(
    mut commands: Commands,
    actors: Query<
        (Entity, Option<&MoveSpeed>, &Health),
        (
            With<Monster>,
            With<CanFlee>,
            Without<Active>,
            Or<(With<Idle>, With<Failure>)>,
        ),
    >,
) {
    for (actor, move_speed, health) in &actors {
        if health.ratio() >= FLEE_HP_RATIO {
            continue;
        }
        commands.spawn((
            ChildOf(actor),
            ActionPriority(PRIORITY_FLEE),
            ActionSource::Ai,
            ActionName("Flee"),
            ActionTimer {
                remaining_av: action_av_of(
                    SpeedCategory::Move,
                    FLEE_DURATION,
                    speed_or_warn(move_speed, actor),
                    FALLBACK_SPEED,
                ),
            },
            Flee,
            Candidate,
        ));
    }
}

/// 等待候选（C1）：兜底行为，任何空闲且具 `CanWait` 的 actor 都会产出一个 `Wait`。
///
/// 与 `Wander` / `Flee` 的差别：**永远有候选**，因此仲裁必然有结果，
/// 有能力的 actor 不会出现「没有任何行动」的空转（§3.3 的兜底语义）。
pub fn wait_generation_system(
    mut commands: Commands,
    actors: Query<
        Entity,
        (
            With<Monster>,
            With<CanWait>,
            Without<Active>,
            Or<(With<Idle>, With<Failure>)>,
        ),
    >,
) {
    for actor in &actors {
        commands.spawn((
            ChildOf(actor),
            ActionPriority(PRIORITY_WAIT),
            ActionSource::Ai,
            ActionName("Wait"),
            // 固定耗时：等待不受任何速度倍率影响（REFACTOR.md §11.6 第 4 项）。
            ActionTimer {
                remaining_av: SpeedCategory::Fixed.action_av(
                    WAIT_DURATION,
                    FALLBACK_SPEED,
                    FALLBACK_SPEED,
                ),
            },
            Wait,
            Candidate,
        ));
    }
}

/// 追击候选（C5）：玩家可见、或仍有最后已知位置（§3.3 表）。
///
/// 条件与旧 `ai.rs::choose_action` 的 `chase_condition` 一致：
/// `player_visible_to(actor) || LastKnownPlayerPos.0.is_some()`。
pub fn chase_generation_system(
    mut commands: Commands,
    actors: Query<
        (Entity, Option<&MoveSpeed>),
        (
            With<Monster>,
            With<CanChase>,
            Without<Active>,
            Or<(With<Idle>, With<Failure>)>,
        ),
    >,
    viewsheds: Query<&Viewshed>,
    last_known: Query<&LastKnownPlayerPos>,
    player: Query<&Position, With<Player>>,
) {
    let Ok(player_position) = player.single() else {
        return;
    };
    let player_position = player_position.to_tuple();

    for (actor, move_speed) in &actors {
        let can_see = viewsheds
            .get(actor)
            .ok()
            .is_some_and(|viewshed| viewshed.can_see(player_position));
        let has_memory = last_known
            .get(actor)
            .ok()
            .is_some_and(|known| known.0.is_some());
        if !can_see && !has_memory {
            continue;
        }
        commands.spawn((
            ChildOf(actor),
            ActionPriority(PRIORITY_CHASE),
            ActionSource::Ai,
            ActionName("Chase"),
            ActionTimer {
                remaining_av: action_av_of(
                    SpeedCategory::Move,
                    CHASE_DURATION,
                    speed_or_warn(move_speed, actor),
                    FALLBACK_SPEED,
                ),
            },
            Chase,
            Candidate,
        ));
    }
}

// ── 玩家路径生成（C2） ────────────────────────────────
//
// 玩家输入不走 AI 生成/仲裁：`PlayerActionRequest` → 直接产出 active action。

/// 玩家行动请求（C2）：与旧 `generation::player::PlayerActionRequest` 同形，
/// 但由 action 实体链路消费——**玩家路径直接产出 active action，不进 AI 仲裁**
/// （§3.6.4）。C3 补上「走向怪物 = 攻击」的分支。
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

/// 玩家路径的行动种类（内部用，避免与 `ActionKind` 混淆）。
#[derive(Debug, Clone, Copy)]
enum PlayerAction {
    Wait,
    Move(Move),
    BasicAttack(Entity),
}

/// 玩家行动生成（C2/C3）：把已确认的玩家命令翻译成 **active action 实体**。
///
/// 与 AI 生成系统的三条差别（都是 §3.6.4 的规定）：
///
/// 1. 过滤 `With<Player>` 而不是 `With<Monster>`——玩家与怪物互不覆盖；
/// 2. 直接产出 `ActiveAction` 并写 actor 的 `Active`，**不经过仲裁**；
/// 3. 产出前先做同一套合法性检查（越界 / 占用 / [`moved_position`]），
///    失败即拒绝请求（等价旧 `player_action_generation_system` 的「请求无效，未挂载」）。
///
/// **「走向怪物 = 攻击」**（C3）：按旧实现的口径——目标格被占用时，占用者是怪物
/// 就产出 `BasicAttack`，否则拒绝请求。
///
/// **忙碌判定的依据**：查询用 `Without<Active>` 而不是 `Without<ActiveAction>`。
/// `ActiveAction` 挂在**子实体**上，actor 身上看不到它；actor 自己的 `Active`
/// 才是「正在行动」的权威标记（与仲裁系统用 `Without<Active>` 过滤 AI 候选一致）。
///
/// 计时迁移期沿用旧口径：`Move`/`BasicAttack` 用 `UNARMED_ATTACK_DURATION`、
/// `Wait` 用 `WAIT_DURATION`，都乘敏捷系数（Phase D 才换成倍率组件）。
///
/// [`moved_position`]: crate::action::execution::movement::moved_position
pub fn player_action_generation_system(
    mut commands: Commands,
    mut request: ResMut<PlayerActionRequest>,
    players: Query<
        (Entity, &Position, Option<&MoveSpeed>, Option<&AttackSpeed>),
        (With<Player>, Without<Active>),
    >,
    monsters: Query<(), With<Monster>>,
    map: Res<Map>,
    occupancy: Res<OccupancyMap>,
) {
    let Some(command) = request.command.take() else {
        return;
    };

    let Ok((player, position, move_speed, attack_speed)) = players.single() else {
        log::warn!("玩家请求无法处理（玩家不存在或已有行动）: {command:?}");
        return;
    };

    let (action, category, duration) = match command {
        PlayerCommand::Wait => (PlayerAction::Wait, SpeedCategory::Fixed, WAIT_DURATION),
        PlayerCommand::Move { dx, dy } => {
            let (nx, ny) = position.offset(dx, dy);
            if nx >= MAP_WIDTH || ny >= MAP_HEIGHT {
                log::debug!("玩家移动越界，拒绝请求: ({dx},{dy})");
                return;
            }
            if let Some(occupant) = occupancy.entity_at(nx, ny) {
                if monsters.get(occupant).is_ok() {
                    // 走向怪物 = 声明攻击（与旧实现一致）。
                    (
                        PlayerAction::BasicAttack(occupant),
                        SpeedCategory::Attack,
                        UNARMED_ATTACK_DURATION,
                    )
                } else {
                    log::debug!("玩家移动目标被非怪物占用，拒绝请求: ({nx},{ny})");
                    return;
                }
            } else if crate::action::execution::movement::moved_position(
                &map, &occupancy, *position, dx, dy,
            )
            .is_none()
            {
                log::debug!("玩家移动非法，拒绝请求: ({dx},{dy})");
                return;
            } else {
                (
                    PlayerAction::Move(Move { dx, dy }),
                    SpeedCategory::Move,
                    UNARMED_ATTACK_DURATION,
                )
            }
        }
    };

    let remaining_av = action_av_of(
        category,
        duration,
        speed_or_warn(move_speed, player),
        attack_speed_or_warn(attack_speed, player),
    );

    let mut action_cmd = commands.spawn((
        ChildOf(player),
        ActionPriority(PRIORITY_PLAYER),
        ActionSource::Player,
        ActionTimer { remaining_av },
        ActiveAction,
    ));
    match action {
        PlayerAction::Wait => {
            action_cmd.insert((ActionName("Wait"), Wait));
        }
        PlayerAction::Move(action_move) => {
            action_cmd.insert((ActionName("Move"), action_move));
        }
        PlayerAction::BasicAttack(target) => {
            action_cmd.insert((ActionName("BasicAttack"), BasicAttack { target }));
        }
    }

    let mut player_cmd = commands.entity(player);
    player_cmd.remove::<Idle>();
    player_cmd.remove::<Failure>();
    player_cmd.insert(Active);
    log::debug!("玩家行动已挂载: {action:?} player={player:?}");
}

/// 挂载玩家行动（C2/C3）：生成 + 仲裁两段，**不含** tick/执行/completion。
///
/// 主循环（`world/loop_.rs::apply_player_command`）在推进世界之前先单独运行这一次，
/// 用来「确认命令是否被接受」：
///
/// - 旧实现同样是先 `player_action_generation_system` 挂载、再进推进循环；
/// - 若把挂载和推进放在同一次调度里，玩家会在这一轮就把行动跑完，
///   调用方随后看到的 `player_is_busy == false` 只是「已经做完了」，
///   无法区分「命令被拒绝」（本文件第一版就踩了这个坑）。
pub fn build_player_mount_schedule() -> Schedule {
    let mut schedule = Schedule::new(PlayerMountSchedule);
    schedule.add_systems(
        (
            player_action_generation_system,
            ApplyDeferred,
            action_arbitration_system,
        )
            .chain(),
    );
    schedule
}

// ── 仲裁系统 ─────────────────────────────────────────

/// 每个 actor 至多留下一个 `ActiveAction`，其余候选与落后候选一律 despawn。
///
/// 比较器是 `(ActionPriority, action_entity.to_bits())` 的**全序**：不含随机数。
/// 同优先级时 entity bits 小者胜出，保证结果可复现（REFACTOR.md §3.4）。
pub fn action_arbitration_system(
    mut commands: Commands,
    candidates: Query<
        (Entity, &ActionPriority, &ChildOf),
        (With<Candidate>, Without<ActiveAction>),
    >,
    active_actions: Query<&ChildOf, With<ActiveAction>>,
) {
    // 已经持有 `ActiveAction` 的 actor：本轮不得再被授予行动。
    // （候选查询里的 `Without<ActiveAction>` 只保证候选自身未激活，挡不住这种情况。）
    let busy_actors: HashSet<Entity> = active_actions.iter().map(ChildOf::parent).collect();

    // 每个 actor 选一个赢家：(priority, bits) 最小者。
    let mut winners: HashMap<Entity, (Entity, i32, u64)> = HashMap::new();
    let mut losers: Vec<Entity> = Vec::new();

    for (action, priority, child_of) in &candidates {
        let actor = child_of.parent();
        if busy_actors.contains(&actor) {
            losers.push(action);
            continue;
        }
        let bits = action.to_bits();
        match winners.get(&actor) {
            Some((_, best_priority, best_bits))
                if (*best_priority, *best_bits) <= (priority.0, bits) =>
            {
                losers.push(action);
            }
            Some((previous, ..)) => {
                losers.push(*previous);
                winners.insert(actor, (action, priority.0, bits));
            }
            None => {
                winners.insert(actor, (action, priority.0, bits));
            }
        }
    }

    for (actor, (winner, ..)) in &winners {
        commands
            .entity(*winner)
            .remove::<Candidate>()
            .insert(ActiveAction);
        let mut actor_cmd = commands.entity(*actor);
        actor_cmd.remove::<Idle>();
        actor_cmd.remove::<Failure>();
        actor_cmd.insert(Active);
    }
    for loser in losers {
        commands.entity(loser).despawn();
    }
}

// ── Tick ─────────────────────────────────────────────

/// 本轮要推进的最小正 AV（只统计 `ActiveAction`）。
///
/// 与 Phase A 的 `execution::positive_timer_delta` 同源；Phase C 会用本函数替换它。
pub fn active_action_timer_delta(world: &mut World) -> f64 {
    let mut query = world.query_filtered::<&ActionTimer, With<ActiveAction>>();
    query
        .iter(world)
        .map(|timer| timer.remaining_av)
        .filter(|remaining| *remaining > 0.0)
        .min_by(|a, b| a.partial_cmp(b).expect("ActionTimer must not be NaN"))
        .unwrap_or(0.0)
}

/// 推进所有 `ActiveAction` 的计时器；归零者加 `Ready`（执行门禁）。
pub fn tick_action_timers_system(world: &mut World) {
    let delta = active_action_timer_delta(world);

    let mut ready_actions = Vec::new();
    {
        let mut query = world.query_filtered::<(Entity, &mut ActionTimer), With<ActiveAction>>();
        for (action, mut timer) in query.iter_mut(world) {
            if delta > 0.0 && timer.remaining_av > 0.0 {
                timer.remaining_av = (timer.remaining_av - delta).max(0.0);
            }
            if timer.remaining_av <= 0.0 {
                ready_actions.push(action);
            }
        }
    }

    for action in ready_actions {
        world.entity_mut(action).insert(Ready);
    }
}

// ── 执行系统 ─────────────────────────────────────────
//
// 每个行动一个专用 query，执行层零中央 match。执行系统只发事件：
// **不 despawn action 实体、不写 actor 的 `Idle`/`Failure`**——那是 completion 的事。

/// 执行到期的 `Wander`：随机选一个 8 方向走一步；被挡/越界则原地不动。
///
/// 与旧 `execution::execute_wander_system` 行为一致：随机方向**照常消耗随机数**
/// （保证 RNG 步数与旧实现同序），只有合法时才改 `Position`；被挡也算行动完成
/// （游荡本来就是随机试探）。
pub fn execute_wander_system(
    mut commands: Commands,
    mut rng: ResMut<GameRng>,
    actions: Query<(Entity, &ChildOf), (With<ActiveAction>, With<Ready>, With<Wander>)>,
    actors: Query<&Position>,
    map: Res<Map>,
    occupancy: Res<OccupancyMap>,
    mut succeeded: EventWriter<ActionSucceededEvent>,
    mut failed: EventWriter<ActionFailedEvent>,
) {
    use crate::action::execution::movement::moved_position;

    const DIRECTIONS: [(isize, isize); 8] = [
        (0, -1),
        (0, 1),
        (-1, 0),
        (1, 0),
        (-1, -1),
        (1, -1),
        (-1, 1),
        (1, 1),
    ];

    for (action, child_of) in &actions {
        let actor = child_of.parent();
        let Ok(position) = actors.get(actor) else {
            // actor 已消失：清掉残留 action 实体并结束行动。
            commands.entity(action).despawn();
            failed.write(ActionFailedEvent { entity: actor });
            continue;
        };

        let index =
            (rng.random_range(0, DIRECTIONS.len() as u64) as usize).min(DIRECTIONS.len() - 1);
        let (dx, dy) = DIRECTIONS[index];

        match moved_position(&map, &occupancy, *position, dx, dy) {
            Some(next) => {
                commands.entity(actor).insert(Position::new(next.x, next.y));
                log::debug!(
                    "PoC 游荡: actor={actor:?} 方向=({dx},{dy}) → ({},{})",
                    next.x,
                    next.y
                );
            }
            None => {
                log::debug!("PoC 游荡: actor={actor:?} 方向=({dx},{dy}) 被挡，原地不动");
            }
        }
        succeeded.write(ActionSucceededEvent { entity: actor });
    }
}

/// 执行到期的 `Wait`（C1）：什么都不做，直接算完成。
///
/// 等价旧 `execution::execute_wait_system` 的语义：**无条件成功**——
/// 回合照常推进（AV 已经付过），actor 回到 `Idle`。
///
/// 这里刻意不 `.remove::<Ready>()`：completion 会 `despawn` 整个 action 实体，
/// 它上面的 `Ready` 随之消失；而「执行器不得回收实体」这条分层约束
/// （见 [`execute_move_system`]）在 `Wait` 上同样成立。
pub fn execute_wait_system(
    actions: Query<(Entity, &ChildOf), (With<ActiveAction>, With<Ready>, With<Wait>)>,
    mut succeeded: EventWriter<ActionSucceededEvent>,
) {
    for (action, child_of) in &actions {
        let actor = child_of.parent();
        log::debug!("PoC 等待: actor={actor:?}（action={action:?}）");
        succeeded.write(ActionSucceededEvent { entity: actor });
    }
}

/// 执行到期的 `Flee`（C6）：与旧 `execution::execute_flee_system` 逐条对齐。
///
/// 1. **保活**：仍处于逃跑滞回区间（`Health.ratio() < FLEE_HP_RATIO_EXIT`），
///    否则 `ActionFailedEvent`；
/// 2. 在 8 个方向里选**合法且曼哈顿距离玩家最远**的落点（严格 `>` 比较，
///    平局保留方向表里更靠前的那个——顺序与原实现一致）；
/// 3. 有可逃方向 → 走过去；全被堵住时，若**相邻且可见**则改为声明攻击
///    （顶到墙角也要反咬一口），否则原地不动；
/// 4. 逃跑总是以成功结束。
pub fn execute_flee_system(
    actions: Query<(Entity, &ChildOf), (With<ActiveAction>, With<Ready>, With<Flee>)>,
    mut positions: Query<&mut Position>,
    healths: Query<&Health>,
    viewsheds: Query<&Viewshed>,
    player: Query<Entity, With<Player>>,
    map: Res<Map>,
    occupancy: Res<OccupancyMap>,
    mut intents: EventWriter<AttackIntentEvent>,
    mut succeeded: EventWriter<ActionSucceededEvent>,
    mut failed: EventWriter<ActionFailedEvent>,
) {
    const DIRECTIONS: [(isize, isize); 8] = [
        (0, -1),
        (0, 1),
        (-1, 0),
        (1, 0),
        (-1, -1),
        (1, -1),
        (-1, 1),
        (1, 1),
    ];

    for (action, child_of) in &actions {
        let actor = child_of.parent();

        // 保活：滞回退出阈值。
        if !healths
            .get(actor)
            .is_ok_and(|health| health.ratio() < FLEE_HP_RATIO_EXIT)
        {
            failed.write(ActionFailedEvent { entity: actor });
            continue;
        }

        let Ok(player_entity) = player.single() else {
            failed.write(ActionFailedEvent { entity: actor });
            continue;
        };
        let Ok(player_position) = positions.get(player_entity).map(Position::to_tuple) else {
            failed.write(ActionFailedEvent { entity: actor });
            continue;
        };
        let Ok(self_position) = positions.get(actor).map(Position::to_tuple) else {
            failed.write(ActionFailedEvent { entity: actor });
            continue;
        };
        let player_tile = Position::new(player_position.0, player_position.1);

        let mut best: Option<Position> = None;
        let mut best_distance = 0usize;
        for (dx, dy) in DIRECTIONS {
            if crate::action::execution::movement::can_move_to(
                &map,
                &occupancy,
                self_position.0,
                self_position.1,
                dx,
                dy,
            ) {
                let (nx, ny) = Position::new(self_position.0, self_position.1).offset(dx, dy);
                let candidate = Position::new(nx, ny);
                let distance = candidate.manhattan(player_tile);
                if distance > best_distance {
                    best_distance = distance;
                    best = Some(candidate);
                }
            }
        }

        match best {
            Some(next) => {
                if let Ok(mut position) = positions.get_mut(actor) {
                    position.x = next.x;
                    position.y = next.y;
                }
            }
            None => {
                // 无路可逃：相邻且看得见玩家就反咬一口（与旧实现一致）。
                let near = Position::new(self_position.0, self_position.1).is_near(player_tile);
                let sees = viewsheds
                    .get(actor)
                    .ok()
                    .is_some_and(|viewshed| viewshed.can_see(player_position));
                if near && sees {
                    intents.write(AttackIntentEvent {
                        attacker: actor,
                        target: player_entity,
                    });
                }
            }
        }

        log::debug!("PoC 逃跑: actor={actor:?} action={action:?}");
        succeeded.write(ActionSucceededEvent { entity: actor });
    }
}

/// 执行到期的 `Chase`（C5）：与旧 `execution::execute_chase_system` 逐条对齐。
///
/// 1. **保活**：仍可见玩家，或仍有最后已知位置；否则 `ActionFailedEvent`；
/// 2. 可见时把玩家当前位置写入 `LastKnownPlayerPos`（执行器的合法写入之一，
///    旧实现同样在这里写）；
/// 3. 目标 = 可见时玩家实时位置，否则最后已知位置；
/// 4. 可见且相邻 → 写 `AttackIntentEvent`（伤害仍归结算链路）；
///    否则 A* 走一步（`astar(..., Some(occupancy))`，与旧实现同一调用口径）；
/// 5. 不可见且已抵达最后已知位置（切比雪夫 ≤2）→ 清空 `LastKnownPlayerPos`。
///
/// 追击总是以成功结束（保活通过就算这一步做完），与旧实现一致。
#[allow(clippy::too_many_arguments)]
pub fn execute_chase_system(
    mut positions: Query<&mut Position>,
    viewsheds: Query<&Viewshed>,
    mut last_known: Query<&mut LastKnownPlayerPos>,
    player: Query<Entity, With<Player>>,
    map: Res<Map>,
    occupancy: Res<OccupancyMap>,
    actions: Query<(Entity, &ChildOf), (With<ActiveAction>, With<Ready>, With<Chase>)>,
    mut intents: EventWriter<AttackIntentEvent>,
    mut succeeded: EventWriter<ActionSucceededEvent>,
    mut failed: EventWriter<ActionFailedEvent>,
) {
    use crate::spatial::pathfinding::astar;

    for (action, child_of) in &actions {
        let actor = child_of.parent();

        let Ok(player_entity) = player.single() else {
            failed.write(ActionFailedEvent { entity: actor });
            continue;
        };
        let Ok(player_position) = positions.get(player_entity).map(Position::to_tuple) else {
            failed.write(ActionFailedEvent { entity: actor });
            continue;
        };
        let Ok(self_position) = positions.get(actor).map(Position::to_tuple) else {
            failed.write(ActionFailedEvent { entity: actor });
            continue;
        };

        let can_see = viewsheds
            .get(actor)
            .ok()
            .is_some_and(|viewshed| viewshed.can_see(player_position));
        let memory = last_known.get(actor).ok().and_then(|known| known.0);
        // 保活：既看不见又没有记忆 → 行动失败。
        if !can_see && memory.is_none() {
            failed.write(ActionFailedEvent { entity: actor });
            continue;
        }

        if can_see && let Ok(mut known) = last_known.get_mut(actor) {
            known.0 = Some(player_position);
        }

        let target = if can_see {
            Some(player_position)
        } else {
            memory
        };
        if let Some((tx, ty)) = target {
            let self_tile = Position::new(self_position.0, self_position.1);
            let target_tile = Position::new(tx, ty);
            if can_see && self_tile.is_near(target_tile) {
                // 相邻且可见：声明攻击，伤害交给结算链路。
                intents.write(AttackIntentEvent {
                    attacker: actor,
                    target: player_entity,
                });
            } else if let Some((nx, ny)) =
                astar(self_position, (tx, ty), &map.tiles, Some(&occupancy))
                    .and_then(|path| path.first().copied())
                && let Ok(mut position) = positions.get_mut(actor)
            {
                position.x = nx;
                position.y = ny;
            }

            // 不可见且已抵达最后已知位置附近 → 记忆失效。
            if !can_see
                && let Ok(mut known) = last_known.get_mut(actor)
                && let Some((kx, ky)) = known.0
                && self_position.0.abs_diff(kx) <= 2
                && self_position.1.abs_diff(ky) <= 2
            {
                known.0 = None;
            }
        }

        log::debug!("PoC 追击: actor={actor:?} action={action:?} can_see={can_see}");
        succeeded.write(ActionSucceededEvent { entity: actor });
    }
}

/// 执行到期的 `BasicAttack { target }`（C3）：保活检查 → 发 `AttackIntentEvent`。
///
/// 与旧 `execution::execute_basic_attack_system` **完全同语义**：
///
/// - 保活检查用 [`crate::combat::can_attack`]（8 方向相邻 + 目标存活）；
/// - 通过 → 写 `AttackIntentEvent`（伤害仍由结算链路 `resolve_attack_system` →
///   `apply_damage_system` → `check_death_system` 负责，执行器不算伤害）；
/// - 不通过 → `ActionFailedEvent`（等价旧的 `finish_action_failure`）。
///
/// 因此攻击的「只结算一次」保证（I90）不受迁移影响：执行器依旧只发一次意图。
pub fn execute_basic_attack_system(
    actions: Query<(Entity, &ChildOf, &BasicAttack), (With<ActiveAction>, With<Ready>)>,
    positions: Query<&Position>,
    healths: Query<&Health>,
    mut intents: EventWriter<AttackIntentEvent>,
    mut succeeded: EventWriter<ActionSucceededEvent>,
    mut failed: EventWriter<ActionFailedEvent>,
) {
    for (action, child_of, attack) in &actions {
        let actor = child_of.parent();
        let target = attack.target;

        let ok = positions
            .get(actor)
            .ok()
            .zip(positions.get(target).ok())
            .zip(healths.get(target).ok())
            .is_some_and(|((attacker_position, target_position), target_health)| {
                crate::combat::can_attack_positions(
                    *attacker_position,
                    *target_position,
                    target_health,
                )
            });

        log::debug!(
            "PoC 攻击保活检查: attacker={actor:?} target={target:?} ok={ok} action={action:?}"
        );
        if !ok {
            failed.write(ActionFailedEvent { entity: actor });
            continue;
        }

        intents.write(AttackIntentEvent {
            attacker: actor,
            target,
        });
        succeeded.write(ActionSucceededEvent { entity: actor });
    }
}

/// 执行到期的 `Move { dx, dy }`：**参数化普通系统**（不再独占 `&mut World`）。
///
/// 为什么这样写是安全的（Phase B 时这里曾是 exclusive，属过度保守）：
///
/// - 驱动实体是 **action 实体**（`ActiveAction + Ready + Move` 都在它身上），
///   `ChildOf` 只作**读**，用来拿 actor id；
/// - 被写的是**另一个实体**的 `Position`，因此 `Query<&mut Position>` 的
///   per-entity 唯一可变访问没有被违反；
/// - 规则由纯函数 [`crate::action::execution::movement::moved_position`] 提供，
///   不需要 `World` 句柄。
///
/// 之前必须 exclusive，唯一原因是复用了 `movement::execute_move(&mut World, ...)`
/// ——那个签名把「读资源 + 读组件 + 写组件」揉进一次 `&mut World` 调用。
/// 抽取纯规则后这个约束自动消失，A41 的边界随之收窄。
pub fn execute_move_system(
    mut commands: Commands,
    mut actions: Query<(Entity, &ChildOf, &Move), (With<ActiveAction>, With<Ready>)>,
    mut actors: Query<&mut Position>,
    map: Res<Map>,
    occupancy: Res<OccupancyMap>,
    mut succeeded: EventWriter<ActionSucceededEvent>,
    mut failed: EventWriter<ActionFailedEvent>,
) {
    use crate::action::execution::movement::moved_position;

    for (action, child_of, action_move) in &mut actions {
        let actor = child_of.parent();
        let Ok(mut position) = actors.get_mut(actor) else {
            // actor 已消失：清掉残留 action 实体并结束行动。
            commands.entity(action).despawn();
            failed.write(ActionFailedEvent { entity: actor });
            continue;
        };

        let moved = moved_position(&map, &occupancy, *position, action_move.dx, action_move.dy);
        match moved {
            Some(next) => {
                position.x = next.x;
                position.y = next.y;
                log::debug!(
                    "PoC 移动: actor={actor:?} 方向=({},{}) → ({},{})",
                    action_move.dx,
                    action_move.dy,
                    next.x,
                    next.y
                );
            }
            None => {
                log::debug!(
                    "PoC 移动: actor={actor:?} 方向=({},{}) 被挡，原地不动",
                    action_move.dx,
                    action_move.dy
                );
            }
        }

        commands.entity(action).remove::<Ready>();
        if moved.is_some() {
            succeeded.write(ActionSucceededEvent { entity: actor });
        } else {
            failed.write(ActionFailedEvent { entity: actor });
        }
    }
}

// ── Completion ───────────────────────────────────────

/// 消费 `ActionSucceeded` / `ActionFailedEvent`：despawn action 实体，actor 回 `Idle`/`Failure`。
///
/// 这是 `ActionSucceeded/FailedEvent` 从零消费者的死事件变成真实状态回转机制的地方
/// （§3.6.6）；它也是唯一允许写 actor `Idle`/`Failure` 的地方之一。
pub fn action_completion_system(
    mut commands: Commands,
    mut succeeded: EventReader<ActionSucceededEvent>,
    mut failed: EventReader<ActionFailedEvent>,
    actions: Query<(Entity, &ChildOf), With<ActiveAction>>,
) {
    let done: Vec<(Entity, bool)> = succeeded
        .read()
        .map(|event| (event.entity, true))
        .chain(failed.read().map(|event| (event.entity, false)))
        .collect();

    for (actor, is_success) in done {
        // 该 actor 的所有 action 子实体：despawn 的实体无法再查询，
        // 因此按“归属 + 仍是 ActiveAction”逐个回收。
        let targets: Vec<Entity> = actions
            .iter()
            .filter(|(_, child_of)| child_of.parent() == actor)
            .map(|(action, _)| action)
            .collect();
        for action in targets {
            commands.entity(action).despawn();
        }

        if let Ok(mut actor_cmd) = commands.get_entity(actor) {
            actor_cmd.remove::<Active>();
            actor_cmd.remove::<ActionTimer>();
            actor_cmd.remove::<Ready>();
            // `Idle` 与 `Failure` 互斥：必须显式清掉另一个再插入，
            // 否则 actor 会同时持有两者（旧 `finish_action_success/failure` 也是这样清的）。
            if is_success {
                actor_cmd.remove::<Failure>();
                actor_cmd.insert(Idle);
            } else {
                actor_cmd.remove::<Idle>();
                actor_cmd.insert(Failure);
            }
        }
    }
}

// ── PoC 调度（Phase C 逐步扩成正式链路） ──────────────
//
// 生成（AI 候选 + 玩家 active）→ 仲裁 → tick → 执行 → completion。
//
// 全链路都是**普通系统**：没有 exclusive `&mut World` 系统，因此这条调度可以被
// 自由组合（见 `entity_tests.rs` 里把它追加进 `CoreSettleSchedule` 的共存测试）。
//
// **已接主循环**（C7）：`world/loop_.rs` 每轮运行它；玩家命令先经
// [`build_player_mount_schedule`] 挂载，再进推进循环。
pub fn build_action_poc_schedule() -> Schedule {
    let mut schedule = Schedule::new(ActionPocSchedule);
    schedule.add_systems(
        (
            (
                player_action_generation_system,
                wait_generation_system,
                chase_generation_system,
                wander_generation_system,
                flee_generation_system,
            )
                .chain(),
            ApplyDeferred,
            action_arbitration_system,
            ApplyDeferred,
            tick_action_timers_system,
            execute_wait_system,
            execute_move_system,
            execute_basic_attack_system,
            execute_chase_system,
            execute_flee_system,
            execute_wander_system,
            ApplyDeferred,
            action_completion_system,
        )
            .chain(),
    );
    schedule
}

// ── 测试 ─────────────────────────────────────────────

#[cfg(test)]
mod tests {
    //! 行动实体链路的测试。两种放法：
    //!
    //! - `entity_tests.rs`：链路级用例（生成/仲裁/tick/执行/completion/parity）；
    //! - 本文件内联：**纯函数**（AV 公式、速度映射）的用例，不需要 World。
    //!
    //! 两者都写在同一个 `mod tests` 下，测试名统一是
    //! `action::entity::tests::<name>`（历史用例名不变）。

    use super::*;

    /// 浮点比较容差。
    ///
    /// 不能用 `assert_eq!`：新式是 `duration / speed`、旧式是
    /// `reaction + duration * factor`，两者结合律不同，最后几位必然不同
    /// （实测相对误差 ~1e-13），这是浮点运算顺序的差异，不是口径差异。
    const EPS: f64 = 1e-9;

    /// 相对误差比较（`expected` 为 0 时退化成绝对误差）。
    fn approx_eq(actual: f64, expected: f64, what: &str) {
        let tolerance = EPS * expected.abs().max(1.0);
        assert!(
            (actual - expected).abs() <= tolerance,
            "{what}: actual={actual} expected={expected} diff={}",
            (actual - expected).abs()
        );
    }

    // ── Phase D1：新旧 AV 对比（`Agility` 仍保留作对照） ──

    /// 旧口径的「耗时系数」`max(1 - agility*0.02, 0.5)`。
    fn old_factor(agility: f64) -> f64 {
        (1.0 - agility * 0.02).max(0.5)
    }

    /// D1 核心等价：迁移映射下，新 AV **恰好**等于旧 AV 里的耗时项
    /// `duration × 耗时系数`，丢掉的是等量叠加的反应时。
    ///
    /// 这就是「按旧敏捷保行为」的精确含义，也是 D4 里 GAME.md 数值表的来源。
    #[test]
    fn speed_mapping_reproduces_legacy_duration_term() {
        let cases = [
            (3.0, "哥布林"),
            (4.0, "蝎子/蘑菇傀儡"),
            (5.0, "老鼠"),
            (8.0, "孢子怪"),
            (10.0, "玩家/深鳗"),
            (14.0, "洞穴鱼"),
        ];
        for (agility, name) in cases {
            let speed = crate::balance::agility_to_speed(agility);
            assert_eq!(
                speed,
                1.0 / old_factor(agility),
                "{name}: 速度必须是旧耗时系数的倒数"
            );

            for duration in [UNARMED_ATTACK_DURATION, CHASE_DURATION, WANDER_DURATION] {
                let legacy = crate::balance::legacy_action_av(duration, agility);
                let reaction = crate::balance::agility_to_reaction(agility);
                let new = action_av(duration, speed);
                approx_eq(
                    new,
                    legacy - reaction,
                    &format!("{name} duration={duration}: 新 AV 应等于旧 AV 减去反应时"),
                );
            }
        }
    }

    /// 新 AV 去掉的是**常数项**，所以旧口径里「反应时占比随耗时缩短而升高」的
    /// 挤压效应消失：`BasicAttack` 不再被反应时拖累得最狠。
    ///
    /// 注意「新/旧」之比**不是**常数——旧式是 `reaction + duration × factor`，
    /// 新式是 `duration × factor`（因为 `duration / speed` 在迁移映射下正好等于
    /// 旧式的耗时项），所以
    ///
    /// ```text
    /// 新/旧 = duration × factor / (reaction + duration × factor)
    /// ```
    ///
    /// 随 `duration` 增大而增大（反应时被摊薄），恒 `< 1`。不变的是
    /// `新 AV` 恒等于 `duration × 耗时系数`，这正是上面第一条测试钉住的性质。
    ///
    /// 这是 Phase D 唯一有意为之的行为改动（§11.6 第 3 项：先删反应时）。
    #[test]
    fn dropping_reaction_time_removes_the_flat_penalty() {
        let agility = 10.0;
        let speed = crate::balance::agility_to_speed(agility);
        let reaction = crate::balance::agility_to_reaction(agility);
        let factor = old_factor(agility);

        for duration in [UNARMED_ATTACK_DURATION, CHASE_DURATION, WANDER_DURATION] {
            let legacy = crate::balance::legacy_action_av(duration, agility);
            let new = action_av(duration, speed);
            let expected_ratio = duration * factor / (reaction + duration * factor);

            approx_eq(
                new / legacy,
                expected_ratio,
                &format!("duration={duration}: 新/旧之比"),
            );
            assert!(new < legacy, "去掉反应时后 AV 只会变短");
        }

        // 反应时是常数项：行动越短，被它拖累得越狠。
        let short = UNARMED_ATTACK_DURATION;
        let long = WANDER_DURATION;
        let short_ratio = action_av(short, speed) / crate::balance::legacy_action_av(short, agility);
        let long_ratio = action_av(long, speed) / crate::balance::legacy_action_av(long, agility);
        assert!(
            short_ratio < long_ratio,
            "短行动的新/旧之比必须更小（被常数反应时拖累更多）：{short_ratio} < {long_ratio}"
        );

        // 新口径下 AV 之比只由 base_duration 决定，与速度无关。
        let new_ratio = action_av(long, speed) / action_av(short, speed);
        approx_eq(new_ratio, long / short, "新口径下 长/短 行动 AV 之比");
        let legacy_ratio =
            crate::balance::legacy_action_av(long, agility) / crate::balance::legacy_action_av(short, agility);
        assert!(
            new_ratio > legacy_ratio,
            "去掉常数反应时后，长/短行动的比值应回升（{new_ratio} > {legacy_ratio}）"
        );
    }

    /// 玩家初始速度的两个来源必须一致：常量的字面值 vs 旧敏捷 10 的映射。
    ///
    /// 写成断言而不是注释，是因为「迁移映射的产物被当成设计值」正是最容易
    /// 悄悄漂移的一类错误（GAME.md 里重新校准时两边必须一起改）。
    #[test]
    fn player_initial_speed_matches_agility_ten_mapping() {
        let mapped = crate::balance::agility_to_speed(10.0);
        assert_eq!(crate::balance::PLAYER_MOVE_SPEED, mapped);
        assert_eq!(crate::balance::PLAYER_ATTACK_SPEED, mapped);
        assert_eq!(mapped, 1.25, "旧敏捷 10 → 耗时系数 0.80 → 速度 1.25");
    }

    /// 行动类别 → 速度组件的映射（`Move` 用移动速度、`Attack` 用攻击速度）。
    ///
    /// 玩家移动耗时沿用 `UNARMED_ATTACK_DURATION`（旧口径就是这样，共享同一个
    /// 基础耗时），所以这里用 `WANDER_DURATION` 区分两类速度更直白。
    #[test]
    fn speed_category_selects_the_matching_component() {
        let (move_speed, attack_speed) = (0.5, 2.0);

        assert_eq!(
            SpeedCategory::Move.action_av(WANDER_DURATION, move_speed, attack_speed),
            WANDER_DURATION / move_speed,
            "移动类行动必须读 MoveSpeed"
        );
        assert_eq!(
            SpeedCategory::Attack.action_av(UNARMED_ATTACK_DURATION, move_speed, attack_speed),
            UNARMED_ATTACK_DURATION / attack_speed,
            "攻击类行动必须读 AttackSpeed"
        );
        assert_eq!(
            SpeedCategory::Fixed.action_av(WAIT_DURATION, move_speed, attack_speed),
            WAIT_DURATION,
            "Wait 固定耗时，不受任何速度影响"
        );
    }
}

// 链路级用例：独立的 `#[cfg(test)]` 兄弟模块（历史路径 `entity::tests::entity_tests::*`）。
#[cfg(test)]
#[path = "entity_tests.rs"]
mod entity_tests;
