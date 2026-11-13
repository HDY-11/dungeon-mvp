//! 行动的完成、回收与调度装配。

use bevy_ecs::prelude::*;

use crate::action::arbitration::{action_arbitration_system, tick_action_timers_system};
use crate::action::entity::{ActiveAction, finish_action};
use crate::action::ownership::ActionOf;
use crate::components::{ActionTimer, Ready};
use crate::action::generation::{
    chase_generation_system, flee_generation_system, player_action_generation_system,
    wait_generation_system, wander_generation_system,
};
use crate::action::settle::settle_action_system;
use crate::action::validation::{
    validate_basic_attack_system, validate_cast_system, validate_chase_system,
    validate_flee_system, validate_move_system, validate_throw_system, validate_wander_system,
};
use crate::events::{ActionFailedEvent, ActionSucceededEvent};
use crate::schedule::{ActionPocSchedule, PlayerMountSchedule};

// ── Completion ───────────────────────────────────────

/// 消费 `ActionSucceeded` / `ActionFailedEvent`：despawn action 实体，actor 回 `Idle`/`Failure`。
///
/// 这是 `ActionSucceeded/FailedEvent` 从零消费者的死事件变成真实状态回转机制的地方
/// （§3.6.6）；它也是唯一允许写 actor `Idle`/`Failure` 的地方之一。
///
/// H1 之后职责边界更清楚了：**终态出口（[`crate::action::entity::ActionMessages`]）负责"结束行动"**
/// （清 `Ready` + 恰好一个事件），**本系统负责"回收并回转 actor"**。
/// 这里的 `remove::<Ready>()` 因此只剩防御意义（执行器已清过），保留它是为了
/// 万一有别的路径给 actor 挂了 `Ready`（actor 的 `Ready` 与 action 实体的 `Ready`
/// 是同一个组件类型）。
pub fn action_completion_system(
    mut commands: Commands,
    mut succeeded: MessageReader<ActionSucceededEvent>,
    mut failed: MessageReader<ActionFailedEvent>,
    actions: Query<(Entity, &ActionOf), With<ActiveAction>>,
) {
    let done: Vec<(Entity, bool)> = succeeded
        .read()
        .map(|event| (event.entity, true))
        .chain(failed.read().map(|event| (event.entity, false)))
        .collect();

    for (actor, is_success) in done {
        // 该 actor 的所有 action 实体：despawn 的实体无法再查询，
        // 因此按“归属 + 仍是 ActiveAction”逐个回收。
        //
        // 这里**不需要**先判断 actor 是否还在：`ActionOf` 是 `linked_spawn` 关系，
        // actor 被 despawn 时它名下的行动实体已经一并消失（见 `action::ownership`）。
        let targets: Vec<Entity> = actions
            .iter()
            .filter(|(_, action_of)| action_of.parent() == actor)
            .map(|(action, _)| action)
            .collect();
        for action in targets {
            commands.entity(action).despawn();
        }

        if let Ok(mut actor_cmd) = commands.get_entity(actor) {
            // 行动状态的收尾只有一处实现（见 `finish_action`）：清 `Active`/计时器/`Ready`，
            // 并把 `Idle` 与 `Failure` 置成互斥的两个状态。
            finish_action(&mut actor_cmd, is_success);
            actor_cmd.remove::<ActionTimer>();
            actor_cmd.remove::<Ready>();
        }
    }
}

// ── 调度 ─────────────────────────────────────────────

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
    // `core_schedule`（而不是 `Schedule::new`）：构建时强制无歧义，见 `schedule.rs`。
    let mut schedule = crate::schedule::core_schedule(PlayerMountSchedule);
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

/// 行动链路的角色分组（Phase H3）：调度里唯一需要登记"位置"的地方。
///
/// 每个角色是一个**独立扩展点**——新增一个执行器只要往
/// [`ActionPhase::Execute`] 的列表里加一行，不需要读懂整条链路的顺序，也不必修改
/// 生成/仲裁/tick/完成 任何一处。
///
/// **顺序由这里定义一次**（`Generate → Arbitrate → Tick → Execute → Complete`），
/// 由 [`build_action_poc_schedule`] 用 `.chain()` 钉住；角色之间必须保持这个全序，
/// 因为后一个角色的输入正是前一个角色的输出。
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActionPhase {
    /// 生成候选 / 挂载玩家行动（只 spawn，不写 actor 行动状态）。
    Generate,
    /// 每个 actor 挑一个候选转正（唯一写 actor 行动状态的系统）。
    Arbitrate,
    /// 推进 `ActiveAction` 的 `ActionTimer`，归零加 `Ready`。
    Tick,
    /// **定案 + 预留**：每个行动一个校验器，决定"落在哪 / 打谁 / 还该做吗"，
    /// 并把目标格立刻占住（串行阶段，所以同一轮不会有人抢同一格）。
    ///
    /// 没有校验器 = 永远有效（`Wait` 就是这样）。
    Validate,
    /// 落地：把上一阶段定下的结果写进世界，**不再判定**。
    Execute,
    /// 回收 action 实体并把 actor 转回 `Idle` / `Failure`。
    Complete,
}

/// 组建行动链路调度：生成 → 仲裁 → tick → 执行 → 完成。
///
/// H3 把原先「13 个系统一条 `.chain()`」改成**按角色分组的链**。两者语义等价
/// （分组之间仍是全序，组内保持插入顺序），但扩展点的形状变了：
///
/// | | H3 之前 | H3 之后 |
/// |---|---|---|
/// | 加一个执行器 | 在 13 项的长链里找位置，并确认它与前后系统的顺序 | 往 [`ActionPhase::Execute`] 加一行 |
/// | 加一个生成器 | 同上 | 往 [`ActionPhase::Generate`] 加一行 |
/// | 顺序在哪定义 | 隐含在一条长元组的书写顺序里 | [`ActionPhase`] 的全序 + 本函数的 `.chain()`，一处 |
///
/// **为什么仍未合并成一组**（对应 LESSONS `LECS22`）：调度里**顺序协调的中央点**
/// 是 ECS 消除不掉的那一类中央点。分组能把它显式化（"谁先谁后"变成一个有名字的
/// 枚举），但不能把它变没——所以这里的目标是"加第 N 个只改一处"，不是"零处"。
///
/// **`ApplyDeferred` 的位置不能省**：生成器用 `Commands` spawn 候选，仲裁用 `Query`
/// 读候选；不把命令落盘，仲裁会看到空世界（Phase C 的 `.chain()` 已包含这一点，
/// 分组后必须逐处保留）。
pub fn build_action_poc_schedule() -> Schedule {
    let mut schedule = crate::schedule::core_schedule(ActionPocSchedule);
    schedule.add_systems(
        (
            (
                // ---- Generate：只 spawn，不写 actor 行动状态 ----
                (
                    player_action_generation_system,
                    wait_generation_system,
                    chase_generation_system,
                    wander_generation_system,
                    flee_generation_system,
                )
                    .chain()
                    .in_set(ActionPhase::Generate),
                // 仲裁要看到候选实体，所以候选必须在这里落盘。
                ApplyDeferred,
                // ---- Arbitrate ----
                action_arbitration_system.in_set(ActionPhase::Arbitrate),
                // 仲裁的 spawn/despawn 落盘后再推进计时器。
                ApplyDeferred,
                // ---- Tick ----
                tick_action_timers_system.in_set(ActionPhase::Tick),
                // 校验阶段写的组件（`Landing`）要先落盘，执行器才看得到。
                ApplyDeferred,
                // ---- Validate：定案 + 预留。加一个行动只改这一段（外加它自己的生成器） ----
                (
                    validate_move_system,
                    validate_basic_attack_system,
                    validate_cast_system,
                    validate_throw_system,
                    validate_chase_system,
                    validate_flee_system,
                    validate_wander_system,
                )
                    .chain()
                    .in_set(ActionPhase::Validate),
                // 校验器用 Commands 挂 `Landing`，执行器要用。
                ApplyDeferred,
                // ---- Execute：一个通用落地器（判定已在上一阶段完成） ----
                settle_action_system.in_set(ActionPhase::Execute),
                // completion 要读到执行器写的终态事件。
                ApplyDeferred,
                // ---- Complete ----
                action_completion_system.in_set(ActionPhase::Complete),
            )
                .chain(),
        ),
    );
    schedule
}
