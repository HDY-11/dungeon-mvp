//! 行动实体链路（REFACTOR.md §3.6 / §11.3 Phase C；DESIGN DsnE8）。
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
use bevy_ecs::system::SystemParam;
use std::collections::{HashMap, HashSet};

// ── 查询与参数别名 ───────────────────────────────────
//
// 行动链路里同一段「actor 过滤条件」被五个生成系统重复，同一段
// 「到期行动过滤条件」被六个执行器重复，同一批「行动事件写入器」被三个执行器
// 重复。写成别名/`SystemParam` 之后，口径只有一份：改一次过滤条件不会漏改某个
// 生成系统或执行器。
//
// 顺带解决 `clippy::type_complexity` 与 `clippy::too_many_arguments`：
// 别名本身也是给读代码的人减少噪音，不只是为了让 lint 闭嘴。

/// AI 生成系统共同的 actor 过滤：是怪物、有对应能力、当前空闲或上次失败。
///
/// `Without<Active>` 挡的是「自己已经有行动」，注意它**挡不住**
/// 「actor 名下有 `ActiveAction` 子实体」——仲裁系统另有一道显式检查（§3.4）。
type AiActor<C> = (
    With<Monster>,
    With<C>,
    Without<Active>,
    Or<(With<Idle>, With<Failure>)>,
);

/// 玩家路径的 actor 数据：实体 id + 位置 + 两个速度。
type PlayerActorData = (
    Entity,
    &'static Position,
    Option<&'static MoveSpeed>,
    Option<&'static AttackSpeed>,
);

/// 一个 AI actor 的数据：实体 id + 移动速度。
///
/// 用 `Option<&MoveSpeed>` 而不是必需组件——理由见 [`actor_speeds`]：
/// 必需组件会让漏挂速度的 actor 直接从查询里消失（LESSONS.md LECS21）。
///
/// `With<C>` / `Without<Active>` / `Or<..>` 是 **filter**，不能写进数据元组，
/// 所以必须与 [`AiActor`] 一起作为 `Query` 的两个泛型参数给出去。
type AiActorData = (Entity, Option<&'static MoveSpeed>);

/// 仲裁的输入：尚未激活的候选（优先级 + 归属）。
type CandidateAction = (Entity, &'static ActionPriority, &'static ChildOf);

/// 已到期（`Ready`）的行动实体，按具体行动类型 `A` 过滤。
///
/// 只能在「filter 恰好就是 `(With<ActiveAction>, With<Ready>)`」时直接当
/// `Query` 的 filter 用；需要再叠一个 filter 时用 [`ReadyActionWith`]。
type ReadyAction<A> = (With<ActiveAction>, With<Ready>, With<A>);

/// 到期行动的数据：实体 id + 归属。
type ReadyActionEntity = (Entity, &'static ChildOf);

/// 带 payload 的到期行动（`Move` / `BasicAttack`）。
type ReadyActionPayload<A> = (Entity, &'static ChildOf, &'static A);

/// 行动终态的**唯一出口**：执行器只能经它结束一个行动。
///
/// # 为什么把两个 `EventWriter` 藏起来（Phase H1 / ECS35 / ECS36）
///
/// H1 之前，每个执行器手写三件事：`remove::<Ready>()`（只有 `Move` 写了）、
/// `write(ActionSucceededEvent)`、`write(ActionFailedEvent)`。这带来一个**编译器
/// 不会报错**的失败模式（ECS35）：新增执行器漏写终态事件 → actor 永久停在
/// `Active`（生成系统被 `Without<Active>` 挡住，不再为它产出候选）→ action 子实体
/// 永久泄漏，且无任何报错。
///
/// 现在**字段是私有的**：执行器拿不到 `EventWriter`，只能调 [`Self::succeed`] 或
/// [`Self::fail`]，两者都**同时**做「清 `Ready`」+「恰好写一个事件」。于是
/// 「恰好一个终态事件 + `Ready` 被消费」从**约定**变成了**类型层面的保证**。
///
/// 清理放在这里（而不是留给 completion 的 `despawn` 顺带做）还有一层意义：
/// 它去掉了「`Ready` 清理由谁负责」这一处只有注释兜着的不变式（ECS36）。
#[derive(SystemParam)]
pub struct ActionEvents<'w, 's> {
    succeeded: EventWriter<'w, ActionSucceededEvent>,
    failed: EventWriter<'w, ActionFailedEvent>,
    /// `Commands<'w, 's>` 需要两个生命周期：`'w` 借 `Entities`，`'s` 借延迟命令队列
    /// （bevy 0.16 的 `Commands` 定义即如此，所以本 `SystemParam` 也必须带两个参数）。
    commands: Commands<'w, 's>,
}

impl ActionEvents<'_, '_> {
    /// 行动成功结束：清 `Ready` + 发 [`ActionSucceededEvent`]（恰好一个）。
    pub fn succeed(&mut self, action: Entity, actor: Entity) {
        self.commands.entity(action).remove::<Ready>();
        self.succeeded.write(ActionSucceededEvent { entity: actor });
    }

    /// 行动失败结束：清 `Ready` + 发 [`ActionFailedEvent`]（恰好一个）。
    ///
    /// 失败**不回收实体**——回收仍是 completion 的职责（见模块头分层说明）。
    pub fn fail(&mut self, action: Entity, actor: Entity) {
        self.commands.entity(action).remove::<Ready>();
        self.failed.write(ActionFailedEvent { entity: actor });
    }
}

/// 移动类执行器共同的只读世界信息。
#[derive(SystemParam)]
pub struct MovementContext<'w> {
    pub map: Res<'w, Map>,
    pub occupancy: Res<'w, OccupancyMap>,
}

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

/// 行动的速度规则：哪些行动吃哪个速度，以及哪些行动固定耗时。
///
/// 规则**由生成系统在编译期写死**，actor 侧只提供两个速度数值——这样生成系统
/// 不必再判断行动种类，也不会出现两个生成系统对同一行动给出不同口径。
///
/// 枚举而不是常量集合，是为了让非法组合无法表达：`Fixed` 没有速度字段，
/// 也就不存在「固定耗时却读了某个速度」的状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpeedRule {
    /// 读 `MoveSpeed`：`Move` / `Chase` / `Flee` / `Wander`。
    Move,
    /// 读 `AttackSpeed`：`BasicAttack`。
    Attack,
    /// 固定耗时、不受任何速度影响：`Wait`（REFACTOR.md §11.6 第 4 项）。
    Fixed,
}

/// actor 的两个速度（缺失组件时已经是回退值）。
///
/// 与 [`SpeedRule`] 一起构成 AV 计算的**纯输入**：`action_av` 不需要
/// `World`/查询，因此口径可以在无 ECS 的单测里逐条钉住。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActorSpeeds {
    move_speed: f64,
    attack_speed: f64,
}

impl SpeedRule {
    /// 计算该规则下行动的 AV。
    pub fn action_av(self, base_duration: f64, speeds: ActorSpeeds) -> f64 {
        match self {
            SpeedRule::Move => action_av(base_duration, speeds.move_speed),
            SpeedRule::Attack => action_av(base_duration, speeds.attack_speed),
            SpeedRule::Fixed => base_duration,
        }
    }
}

/// 速度组件缺失时的回退值（= 基准 1.0）。
///
/// 生成系统用 `Option<&MoveSpeed>` 查询而不是必需组件：**Bevy 的查询遇到
/// 不匹配的 archetype 会静默返回空**（LESSONS.md LECS21）。若写成必需组件，
/// 一个漏挂速度组件的 actor 会直接从 AI 里消失、既不行动也不报错；
/// 回退到基准 + 告警则让这种漏挂可见且不至于卡死行为。
const FALLBACK_SPEED: f64 = 1.0;

/// 解析 actor 的速度组件；缺失时告警并回退到基准 [`FALLBACK_SPEED`]。
///
/// 缺失**不应**被静默忽略（LESSONS.md LECS21 的同类问题：查询不匹配就是静静地
/// 什么都不做），所以回退前先告警，让「漏挂速度组件」在日志里可见。
fn actor_speeds(
    actor: Entity,
    move_speed: Option<&MoveSpeed>,
    attack_speed: Option<&AttackSpeed>,
) -> ActorSpeeds {
    ActorSpeeds {
        move_speed: move_speed.map_or_else(
            || {
                log::warn!("actor={actor:?} 缺少 MoveSpeed，按基准 {FALLBACK_SPEED} 计算 AV");
                FALLBACK_SPEED
            },
            |speed| speed.0,
        ),
        attack_speed: attack_speed.map_or_else(
            || {
                log::warn!("actor={actor:?} 缺少 AttackSpeed，按基准 {FALLBACK_SPEED} 计算 AV");
                FALLBACK_SPEED
            },
            |speed| speed.0,
        ),
    }
}

// ── 生成系统 ─────────────────────────────────────────
//
// 生成系统只 spawn 候选：不写 actor 的 `Idle/Active/Failure`，不写计时器。

/// 游荡候选：空闲/失败且具 `CanWander` 的怪物各产出一个 `Wander` 候选。
pub fn wander_generation_system(
    mut commands: Commands,
    actors: Query<AiActorData, AiActor<CanWander>>,
) {
    for (actor, move_speed) in &actors {
        commands.spawn((
            ChildOf(actor),
            ActionPriority(PRIORITY_WANDER),
            ActionSource::Ai,
            ActionName("Wander"),
            ActionTimer {
                remaining_av: SpeedRule::Move.action_av(
                    WANDER_DURATION,
                    actor_speeds(actor, move_speed, None),
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
    actors: Query<(Entity, Option<&MoveSpeed>, &Health), AiActor<CanFlee>>,
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
                remaining_av: SpeedRule::Move.action_av(
                    FLEE_DURATION,
                    actor_speeds(actor, move_speed, None),
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
    actors: Query<Entity, AiActor<CanWait>>,
) {
    for actor in &actors {
        commands.spawn((
            ChildOf(actor),
            ActionPriority(PRIORITY_WAIT),
            ActionSource::Ai,
            ActionName("Wait"),
            // 固定耗时：等待不受任何速度倍率影响（REFACTOR.md §11.6 第 4 项）。
            ActionTimer {
                remaining_av: SpeedRule::Fixed.action_av(WAIT_DURATION, actor_speeds(actor, None, None)),
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
    actors: Query<AiActorData, AiActor<CanChase>>,
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
                remaining_av: SpeedRule::Move.action_av(
                    CHASE_DURATION,
                    actor_speeds(actor, move_speed, None),
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
    players: Query<PlayerActorData, (With<Player>, Without<Active>)>,
    monsters: Query<(), With<Monster>>,
    movement: MovementContext,
) {
    let Some(command) = request.command.take() else {
        return;
    };

    let Ok((player, position, move_speed, attack_speed)) = players.single() else {
        log::warn!("玩家请求无法处理（玩家不存在或已有行动）: {command:?}");
        return;
    };

    let (action, rule, duration) = match command {
        PlayerCommand::Wait => (PlayerAction::Wait, SpeedRule::Fixed, WAIT_DURATION),
        PlayerCommand::Move { dx, dy } => {
            let (nx, ny) = position.offset(dx, dy);
            if nx >= MAP_WIDTH || ny >= MAP_HEIGHT {
                log::debug!("玩家移动越界，拒绝请求: ({dx},{dy})");
                return;
            }
            if let Some(occupant) = movement.occupancy.entity_at(nx, ny) {
                if monsters.get(occupant).is_ok() {
                    // 走向怪物 = 声明攻击（与旧实现一致）。
                    (
                        PlayerAction::BasicAttack(occupant),
                        SpeedRule::Attack,
                        UNARMED_ATTACK_DURATION,
                    )
                } else {
                    log::debug!("玩家移动目标被非怪物占用，拒绝请求: ({nx},{ny})");
                    return;
                }
            } else if crate::action::execution::movement::moved_position(
                &movement.map, &movement.occupancy, *position, dx, dy,
            )
            .is_none()
            {
                log::debug!("玩家移动非法，拒绝请求: ({dx},{dy})");
                return;
            } else {
                (
                    PlayerAction::Move(Move { dx, dy }),
                    SpeedRule::Move,
                    UNARMED_ATTACK_DURATION,
                )
            }
        }
    };

    let remaining_av = rule.action_av(duration, actor_speeds(player, move_speed, attack_speed));

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
    candidates: Query<CandidateAction, (With<Candidate>, Without<ActiveAction>)>,
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
    actions: Query<ReadyActionEntity, ReadyAction<Wander>>,
    actors: Query<&Position>,
    movement: MovementContext,
    mut events: ActionEvents,
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
            // actor 已消失：这个 action 没有可回转的对象，直接失败收场
            // （终态出口顺带清 `Ready`，实体交给 completion 回收）。
            events.fail(action, actor);
            continue;
        };

        let index =
            (rng.random_range(0, DIRECTIONS.len() as u64) as usize).min(DIRECTIONS.len() - 1);
        let (dx, dy) = DIRECTIONS[index];

        match moved_position(&movement.map, &movement.occupancy, *position, dx, dy) {
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
        events.succeed(action, actor);
    }
}

/// 执行到期的 `Wait`（C1）：什么都不做，直接算完成。
///
/// 等价旧 `execution::execute_wait_system` 的语义：**无条件成功**——
/// 回合照常推进（AV 已经付过），actor 回到 `Idle`。
///
/// `Ready` 由 [`ActionEvents::succeed`] 统一清掉（H1），实体仍由 completion 回收：
/// 「执行器不得回收实体」这条分层约束（见 [`execute_move_system`]）在 `Wait` 上同样成立。
pub fn execute_wait_system(
    actions: Query<ReadyActionEntity, ReadyAction<Wait>>,
    mut events: ActionEvents,
) {
    for (action, child_of) in &actions {
        let actor = child_of.parent();
        log::debug!("PoC 等待: actor={actor:?}（action={action:?}）");
        events.succeed(action, actor);
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
///
/// 参数偏多（8 个）是 ECS 系统的固有形状：每个 `Query`/`Res`/`EventWriter` 都是
/// 一个**类型不同**的入参，无法折成 DTO 而不丢查询语义（对比 `execute_chase_system`
/// 用 `MovementContext` + `ActionEvents` 把 12 个压到 7 个——压缩到这里就压不动了：
/// `positions`/`healths`/`viewsheds` 都是 `Query`，语义各异且都是可变访问点）。
#[allow(clippy::too_many_arguments)]
pub fn execute_flee_system(
    actions: Query<ReadyActionEntity, ReadyAction<Flee>>,
    mut positions: Query<&mut Position>,
    healths: Query<&Health>,
    viewsheds: Query<&Viewshed>,
    player: Query<Entity, With<Player>>,
    movement: MovementContext,
    mut intents: EventWriter<AttackIntentEvent>,
    mut events: ActionEvents,
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
            events.fail(action, actor);
            continue;
        }

        let Ok(player_entity) = player.single() else {
            events.fail(action, actor);
            continue;
        };
        let Ok(player_position) = positions.get(player_entity).map(Position::to_tuple) else {
            events.fail(action, actor);
            continue;
        };
        let Ok(self_position) = positions.get(actor).map(Position::to_tuple) else {
            events.fail(action, actor);
            continue;
        };
        let player_tile = Position::new(player_position.0, player_position.1);

        let mut best: Option<Position> = None;
        let mut best_distance = 0usize;
        for (dx, dy) in DIRECTIONS {
            if crate::action::execution::movement::can_move_to(
                &movement.map,
                &movement.occupancy,
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
        events.succeed(action, actor);
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
    movement: MovementContext,
    actions: Query<ReadyActionEntity, ReadyAction<Chase>>,
    mut intents: EventWriter<AttackIntentEvent>,
    mut events: ActionEvents,
) {
    use crate::spatial::pathfinding::astar;

    for (action, child_of) in &actions {
        let actor = child_of.parent();

        let Ok(player_entity) = player.single() else {
            events.fail(action, actor);
            continue;
        };
        let Ok(player_position) = positions.get(player_entity).map(Position::to_tuple) else {
            events.fail(action, actor);
            continue;
        };
        let Ok(self_position) = positions.get(actor).map(Position::to_tuple) else {
            events.fail(action, actor);
            continue;
        };

        let can_see = viewsheds
            .get(actor)
            .ok()
            .is_some_and(|viewshed| viewshed.can_see(player_position));
        let memory = last_known.get(actor).ok().and_then(|known| known.0);
        // 保活：既看不见又没有记忆 → 行动失败。
        if !can_see && memory.is_none() {
            events.fail(action, actor);
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
                astar(self_position, (tx, ty), &movement.map.tiles, Some(&movement.occupancy))
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
        events.succeed(action, actor);
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
    actions: Query<ReadyActionPayload<BasicAttack>, (With<ActiveAction>, With<Ready>)>,
    positions: Query<&Position>,
    healths: Query<&Health>,
    mut intents: EventWriter<AttackIntentEvent>,
    mut events: ActionEvents,
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
            events.fail(action, actor);
            continue;
        }

        intents.write(AttackIntentEvent {
            attacker: actor,
            target,
        });
        events.succeed(action, actor);
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
///
/// H1 之后这里也**不再需要 `Commands`**：`Ready` 清理与终态事件都由
/// [`ActionEvents`] 的终态出口一并负责，执行器不再自己回收实体。
pub fn execute_move_system(
    mut actions: Query<ReadyActionPayload<Move>, (With<ActiveAction>, With<Ready>)>,
    mut actors: Query<&mut Position>,
    movement: MovementContext,
    mut events: ActionEvents,
) {
    use crate::action::execution::movement::moved_position;

    for (action, child_of, action_move) in &mut actions {
        let actor = child_of.parent();
        let Ok(mut position) = actors.get_mut(actor) else {
            // actor 已消失：这个 action 没有可回转的对象，直接失败收场。
            events.fail(action, actor);
            continue;
        };

        let moved = moved_position(&movement.map, &movement.occupancy, *position, action_move.dx, action_move.dy);
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

        if moved.is_some() {
            events.succeed(action, actor);
        } else {
            events.fail(action, actor);
        }
    }
}

// ── Completion ───────────────────────────────────────

/// 消费 `ActionSucceeded` / `ActionFailedEvent`：despawn action 实体，actor 回 `Idle`/`Failure`。
///
/// 这是 `ActionSucceeded/FailedEvent` 从零消费者的死事件变成真实状态回转机制的地方
/// （§3.6.6）；它也是唯一允许写 actor `Idle`/`Failure` 的地方之一。
///
/// H1 之后职责边界更清楚了：**终态出口（[`ActionEvents`]）负责"结束行动"**
/// （清 `Ready` + 恰好一个事件），**本系统负责"回收并回转 actor"**。
/// 这里的 `remove::<Ready>()` 因此只剩防御意义（执行器已清过），保留它是为了
/// 万一有别的路径给 actor 挂了 `Ready`（actor 的 `Ready` 与 action 实体的 `Ready`
/// 是同一个组件类型）。
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
    //! - 本文件内联：**纯函数**（AV 公式、速度规则）的用例，不需要 World。
    //!
    //! 两者都在同一个 `mod tests` 下；链路级用例的历史路径是
    //! `action::entity::entity_tests::<name>`（D1 引入嵌套模块前是
    //! `action::entity::tests::<name>`，只影响 `--exact` 过滤）。

    use super::*;
    use crate::balance::{MAX_SPEED, MIN_SPEED, clamp_speed};

    /// 浮点比较容差。
    ///
    /// 纯函数用例里尽量用 `assert_eq!`（这些比值的浮点结果恰好精确）；
    /// 需要跨运算顺序比较时才用它——`duration / speed` 与 `duration * factor`
    /// 的结合律不同，末几位会有 ~1e-13 的相对差异，那是浮点顺序而非口径差异。
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

    // ── Phase D：AV 公式不变式（旧 `Agility` 口径已在 D3 删除） ──

    /// AV 与速度严格成反比、与基础耗时严格成正比。
    ///
    /// 旧口径 `AV = 反应时 + duration × 敏捷系数` 两项都破坏了「AV 与 duration
    /// 成正比」：反应时是常数项，行动越短被它拖累得越狠。D3 之后这条不变式
    /// 成立，也是 GAME.md 能用「耗时 ÷ 速度」一句话写清数值的原因。
    #[test]
    fn av_is_inversely_proportional_to_speed_and_linear_in_duration() {
        // 与速度成反比：AV(speed) × speed 恒定。
        for speed in [0.5, 1.0, 1.25, 2.0, 3.0] {
            approx_eq(
                action_av(WANDER_DURATION, speed) * speed,
                WANDER_DURATION,
                &format!("speed={speed}: AV × speed 必须等于 base_duration"),
            );
        }

        // 单调：速度越高 AV 越短。
        let mut previous = f64::INFINITY;
        for speed in [0.25, 0.5, 1.0, 1.5, 2.0, 4.0] {
            let av = action_av(WANDER_DURATION, speed);
            assert!(av < previous, "speed={speed} 的 AV 必须严格更短");
            previous = av;
        }

        // 与基础耗时成正比：AV 之比只由 base_duration 决定，与速度无关。
        for speed in [0.25, 1.0, 4.0] {
            approx_eq(
                action_av(WANDER_DURATION, speed) / action_av(UNARMED_ATTACK_DURATION, speed),
                WANDER_DURATION / UNARMED_ATTACK_DURATION,
                &format!("speed={speed}: AV 之比必须等于 base_duration 之比"),
            );
        }
    }

    /// `clamp_speed` 必须夹住两端，并把非有限值兜成下限。
    ///
    /// 最后一条防的是「AV 变 NaN → `ActionTimer` 的 `partial_cmp().expect()` panic」
    /// 这条实测过的崩溃路径（`active_action_timer_delta` 里那个 expect）。
    #[test]
    fn clamp_speed_bounds_both_ends_and_non_finite_inputs() {
        assert_eq!(clamp_speed(1.0), 1.0, "区间内原样返回");
        assert_eq!(clamp_speed(MIN_SPEED), MIN_SPEED);
        assert_eq!(clamp_speed(MAX_SPEED), MAX_SPEED);

        assert_eq!(clamp_speed(0.0), MIN_SPEED, "0 速度必须夹到下限");
        assert_eq!(clamp_speed(-3.0), MIN_SPEED, "负速度必须夹到下限");
        assert_eq!(clamp_speed(1e9), MAX_SPEED, "过大速度必须夹到上限");

        assert_eq!(clamp_speed(f64::NAN), MIN_SPEED, "NaN 必须兜成下限");
        assert_eq!(clamp_speed(f64::INFINITY), MIN_SPEED, "inf 必须兜成下限");
        assert_eq!(
            clamp_speed(f64::NEG_INFINITY),
            MIN_SPEED,
            "-inf 必须兜成下限"
        );

        // 兜底的结果本身必须能算出有限 AV（否则只是把 NaN 推后一步）。
        for raw in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 0.0, -1.0] {
            assert!(
                action_av(WANDER_DURATION, raw).is_finite(),
                "raw={raw} 的 AV 必须有限"
            );
        }
    }

    /// 玩家初始速度是 1.25 —— 迁移映射的产物，不是设计值。
    ///
    /// 写成断言而不是注释，是因为「迁移来的数被当成设计基准」正是最容易悄悄
    /// 漂移的一类错误。GAME.md 里做 `[试调]` 重新校准（把玩家基准调回 1.0 并
    /// 重新配平怪物）时，这条断言必须一起改——那就是一次有意的决策。
    #[test]
    fn player_initial_speed_is_the_migration_value() {
        assert_eq!(crate::balance::PLAYER_MOVE_SPEED, 1.25);
        assert_eq!(crate::balance::PLAYER_ATTACK_SPEED, 1.25);
        assert_eq!(
            crate::balance::PLAYER_MOVE_SPEED,
            crate::balance::PLAYER_ATTACK_SPEED,
            "玩家移动与攻击同速（与怪物模板的 uniform() 一致）"
        );
        assert_eq!(
            action_av(UNARMED_ATTACK_DURATION, crate::balance::PLAYER_MOVE_SPEED),
            240.0,
            "玩家一次移动/攻击 = 300/1.25 = 240 AV"
        );
        // `action_av` 是通用公式，它**不**知道行动类别；「Wait 不吃速度」是
        // `SpeedRule::Fixed` 的语义，必须经由规则求值才成立。
        assert_eq!(
            SpeedRule::Fixed.action_av(
                WAIT_DURATION,
                ActorSpeeds {
                    move_speed: crate::balance::PLAYER_MOVE_SPEED,
                    attack_speed: crate::balance::PLAYER_ATTACK_SPEED,
                },
            ),
            WAIT_DURATION,
            "Wait 必须固定耗时，即使玩家速度是 1.25"
        );
    }
    /// 速度规则 → 速度组件的映射（`Move` 用移动速度、`Attack` 用攻击速度）。
    ///
    /// 玩家移动耗时沿用 `UNARMED_ATTACK_DURATION`（旧口径就是这样，共享同一个
    /// 基础耗时），所以这里用 `WANDER_DURATION` 区分两类速度更直白。
    #[test]
    fn speed_rule_selects_the_matching_component() {
        let speeds = ActorSpeeds {
            move_speed: 0.5,
            attack_speed: 2.0,
        };

        assert_eq!(
            SpeedRule::Move.action_av(WANDER_DURATION, speeds),
            WANDER_DURATION / speeds.move_speed,
            "移动类行动必须读 MoveSpeed"
        );
        assert_eq!(
            SpeedRule::Attack.action_av(UNARMED_ATTACK_DURATION, speeds),
            UNARMED_ATTACK_DURATION / speeds.attack_speed,
            "攻击类行动必须读 AttackSpeed"
        );
        assert_eq!(
            SpeedRule::Fixed.action_av(WAIT_DURATION, speeds),
            WAIT_DURATION,
            "Wait 固定耗时，不受任何速度影响"
        );
    }
}

// 链路级用例：独立的 `#[cfg(test)]` 兄弟模块（历史路径 `entity::tests::entity_tests::*`）。
#[cfg(test)]
#[path = "entity_tests.rs"]
mod entity_tests;
