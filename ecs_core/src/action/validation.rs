//! **校验阶段**（`ActionPhase::Validate`）：决定"这一步落在哪 / 打谁 / 还该做吗"，并把落点**占住**。
//!
//! # 为什么要单独一个阶段
//!
//! 判定与写入原先粘在同一个系统里：执行器先 `plan_step` 再发 `MoveRequest`，
//! 由 `apply_move_system` 写入。那解决了同一轮抢格（ISSUES ECS45），但**判定藏在执行器内部**，
//! 读代码的人要钻进六个执行器才知道"什么情况下这一步会被拒"。改成显式阶段之后：
//!
//! | 阶段 | 回答的问题 | 允许干什么 |
//! |---|---|---|
//! | Generate | 有哪些候选？ | 只 spawn |
//! | Arbitrate | 谁转正？ | 写 actor 行动状态 |
//! | Tick | 谁到点了？ | 推进 AV |
//! | **Validate** | **还该做吗？这一步落在哪？** | **保活 + 判定 + 预留格子**（不写 `Position`） |
//! | Execute | 落地。 | 只写结果（`Position` / 攻击意图） |
//! | Complete | 收尾。 | 回收实体、置状态 |
//!
//! 好处是**职责单一且对称**：每个行动恰好一个校验器（这里）+ 一个落地动作（`settle_action_system`），
//! 两者一一对应；而"判定"这件事在整个链路里只有这一处——**判定只有一处，落地也只有一处**。
//!
//! # 一个校验器管两件**范围不同**的事（ISSUES ECS43）
//!
//! | 事 | 对谁生效 | 为什么 |
//! |---|---|---|
//! | **保活**（"还该做吗"） | **所有**待执行行动（到点与否） | 一个行动从被授予 `ActiveAction` 到真正执行之间可能隔若干回合（`tick` 每轮只推进最小正 AV）。在这段等待里世界变了（目标跑远、血量回升），若只在执行前检查，失效的行动会一直占住 actor **唯一的**槽位，而生成系统被 `Without<Active>` 挡住 → actor 空转 |
//! | **定案**（"落在哪 / 打谁" + **预留**） | 只有**到点**的行动 | 提前定案没意义（落点在被兑现前可能过期），而且会白占一格 |
//!
//! 所以查询过滤器是 [`PendingAction`]（不带 `With<Ready>`），是否定案由数据里的
//! [`Has<Ready>`](ActionEntry) 决定。这也让 `GameRng` 的消耗时机保持不变：只有游荡的**定案**
//! 取随机方向，仍只发生在到点那一轮。
//!
//! # 为什么保活不会让 actor 来回抖
//!
//! 生成条件与保活条件**故意不同阈值**（滞回）：逃跑在 `ratio < `[`FLEE_HP_RATIO`] 时生成、
//! 在 `ratio >= `[`FLEE_HP_RATIO_EXIT`] 时取消；追击在"看得见或记得"时生成、在"既看不见又没记忆"
//! 时取消。中间那段区间里既不重新生成也不取消，所以不会"生成 → 立刻取消 → 再生成"。
//!
//! # 预留：为什么判定阶段就占格
//!
//! 校验阶段是**串行**的（`.chain()`），而落地阶段在其后。所以校验器判定"这一格能进"
//! 时就立刻 `occupancy.set`，后面的校验器就能看见——**同一轮里不会有两个人抢到同一格**，
//! 到了落地阶段已经没有判定可做（这也是"判定只有一处"成立的原因）。
//!
//! 预留的生命周期上限是**一轮**：占用图每轮结算都会整轮重建
//! （`system::occupancy::rebuild_occupancy_system`），所以哪怕某个预留没有被兑现
//! （例如行动在落地前就失败），它也活不过这一轮——这是"拿占用图当预留表"的安全边界。
//!
//! # 校验器也负责"决定方向"
//!
//! `Chase`/`Flee`/`Wander` 的方向不能脱离合法性单独决定（A\*、扫描、随机都同时回答
//! "往哪走"和"那一格能不能走"），所以校验器做的是**定案**而不只是"检查"。
//! 方向的三个来源分别对应：A\* 首步 / 最远合法方向 / 随机方向。
//!
//! # 失败语义（沿用旧执行器，未改行为）
//!
//! - `Move`：被挡 = 行动**失败**（"走到那一步"没做成）；
//! - `Chase`/`Flee`/`Wander`：被挡 = 这一步原地站着，仍然**成功**；
//! - `BasicAttack`：前置条件不成立 = 失败；
//! - `Wait`：没有校验器 = 永远成立（"没有校验器"就是"无条件有效"的表达）。

use crate::action::entity::{ActionEntry, ActionEntryPayload, ActionEvents, PendingAction};
use crate::balance::FLEE_HP_RATIO_EXIT;
use crate::combat::can_attack_positions;
use crate::components::{
    BasicAttack, Chase, Flee, Health, LastKnownPlayerPos, Move, Position, Viewshed, Wander,
};
use crate::entity_cls::Player as PlayerMarker;
use crate::map::Map;
use crate::resources::{EventLog, EventMessage, GameRng, OccupancyMap};
use bevy_ecs::prelude::*;
use bevy_ecs::system::SystemParam;

use super::movement::{BlockReason, MovePlan, plan_step};

/// 校验阶段定下的**落点**。落地阶段只负责把它写进 `Position`。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct MoveTarget(pub Position);

/// 校验阶段定下的**攻击目标**。落地阶段只负责发 `AttackIntentEvent`。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttackTarget(pub Entity);

/// 一次"定案"的结果：要么落脚、要么打人、要么原地不动（仍然算这一步做完）。
enum Decision {
    Move(Position),
    Attack(Entity),
    Stay,
}

/// 把决策落到 action 实体上：`MoveTarget` / `AttackTarget` / 什么都不加（= 原地不动）。
fn apply_decision(cmd: &mut EntityCommands, decision: Decision) {
    match decision {
        Decision::Move(to) => {
            cmd.insert(MoveTarget(to));
        }
        Decision::Attack(target) => {
            cmd.insert(AttackTarget(target));
        }
        Decision::Stay => {}
    }
}

/// 校验器共用的「规则 + 预留表」：判定要读地图与占用图，定案要**写**占用图。
///
/// 合成一个 `SystemParam` 而不是两个独立参数：这两样永远一起用（判定离不开地图，
/// 预留离不开占用图），合起来就不存在"传了地图忘了占用图"的调用形状。
#[derive(SystemParam)]
pub struct Reservation<'w> {
    map: Res<'w, Map>,
    occupancy: ResMut<'w, OccupancyMap>,
}

impl Reservation<'_> {
    /// 只判定、不写状态（**探针**）。
    ///
    /// 与 [`Self::step`] 分开是必要的，不是洁癖：`plan_step` 的**穿角**检查会读占用图
    /// （对角两步都要空），所以"试探时顺手预留"会让先试探的方向把自己后面的方向判死。
    /// 逃跑要在八个方向里挑最远的那个——如果前几个方向已经占了格子，后面的对角方向
    /// 就会被自己挡出的假占用判成 `CornerCutting`，选出来的不是最远方向而是"顺序上最靠后
    /// 的合法方向"。**试算必须无副作用**，判定与预留要分两步（LESSONS LECS24）。
    fn probe(&self, from: Position, dx: isize, dy: isize) -> MovePlan {
        plan_step(&self.map, &self.occupancy, from, dx, dy)
    }

    /// 占住一格（预留）。只该对**最终选中**的落点调用一次。
    fn claim(&mut self, actor: Entity, at: Position) {
        self.occupancy.set(at.x, at.y, actor);
    }

    /// 判定 + 预留合一：合法就**立刻占住**目标格。
    fn step(
        &mut self,
        actor: Entity,
        from: Position,
        dx: isize,
        dy: isize,
    ) -> Result<Position, BlockReason> {
        match self.probe(from, dx, dy) {
            MovePlan::Allowed(to) => {
                self.claim(actor, to);
                Ok(to)
            }
            MovePlan::Blocked(reason) => Err(reason),
        }
    }
}

// ── 每个行动一个校验器 ──────────────────────────────

/// `Move`：用 payload 里的方向定案。
///
/// 被挡 → 行动**失败**（`Move` 的语义是"走到那一步"，走不到就是没做成）；
/// 这是四个移动类行动里唯一会失败的一个，`Chase`/`Flee`/`Wander` 被挡都只算"这一步做完了"。
///
/// **`Move` 没有保活条件**：它的"条件"就是这一步能不能走，而那要等落地那一刻才算数
/// ——队列期间目标格被占用并不代表到时候走不了（挡路的人可能先走开）。所以排队中的
/// `Move` 不做任何检查（这是 ECS43 的刻意例外，不是漏了）。
pub fn validate_move_system(
    mut commands: Commands,
    mut reservation: Reservation,
    mut events: ActionEvents,
    actions: Query<ActionEntryPayload<Move>, PendingAction<Move>>,
    actors: Query<(&Position, Has<PlayerMarker>)>,
    mut event_log: ResMut<EventLog>,
) {
    for (action, child_of, action_move, ready) in &actions {
        if !ready {
            continue;
        }
        let actor = child_of.parent();
        let Ok((position, is_player)) = actors.get(actor) else {
            events.fail(action, actor);
            continue;
        };
        match reservation.step(actor, *position, action_move.dx, action_move.dy) {
            Ok(to) => apply_decision(&mut commands.entity(action), Decision::Move(to)),
            Err(reason) => {
                // 玩家必须知道"为什么没动"（ISSUES SYN6）；原因来自规则，不在这里手写。
                if is_player {
                    event_log.push(EventMessage::system(reason.describe()));
                }
                events.fail(action, actor);
            }
        }
    }
}

/// `Chase`：保活（还看得见或有记忆）→ A\* 首步 / 相邻则攻击 / 否则原地。
///
/// 保活对**所有**待执行追击生效（ECS43）：看不见又没记忆时立刻取消，不等到它到点。
/// 定案只在到点那一轮做——A\* 是"此刻"的路径，提前算出来的首步没有意义。
///
/// 十个参数里 `positions`/`viewsheds`/`last_known`/`player` 语义各异且都是可变访问点，
/// 压不进一个 `SystemParam`（同旧执行器的取舍）。
#[allow(clippy::too_many_arguments)]
pub fn validate_chase_system(
    mut commands: Commands,
    mut reservation: Reservation,
    mut events: ActionEvents,
    actions: Query<ActionEntry, PendingAction<Chase>>,
    positions: Query<&Position>,
    viewsheds: Query<&Viewshed>,
    mut last_known: Query<&mut LastKnownPlayerPos>,
    player: Query<Entity, With<PlayerMarker>>,
) {
    use crate::spatial::pathfinding::astar;

    for (action, child_of, ready) in &actions {
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
        // 保活：既看不见又没有记忆 → 这个追击不再成立（排队中也立刻取消）。
        if !can_see && memory.is_none() {
            events.fail(action, actor);
            continue;
        }

        // 以下是**定案**：只对到点的那一轮做。
        if !ready {
            continue;
        }
        if can_see && let Ok(mut known) = last_known.get_mut(actor) {
            known.0 = Some(player_position);
        }

        let self_tile = Position::new(self_position.0, self_position.1);
        let target = if can_see { Some(player_position) } else { memory };
        let decision = match target {
            Some((tx, ty)) => {
                let target_tile = Position::new(tx, ty);
                if can_see && self_tile.is_near(target_tile) {
                    // 相邻且可见：定案为攻击（伤害仍归结算链路）。
                    Decision::Attack(player_entity)
                } else {
                    match astar(
                        self_position,
                        (tx, ty),
                        &reservation.map.tiles,
                        Some(&reservation.occupancy),
                    )
                    .and_then(|path| path.first().copied())
                    {
                        Some((nx, ny)) => {
                            let step = (
                                nx as isize - self_position.0 as isize,
                                ny as isize - self_position.1 as isize,
                            );
                            match reservation.step(actor, self_tile, step.0, step.1) {
                                Ok(to) => Decision::Move(to),
                                // 被挡（同轮里别人刚占住）：这一步站在原地。
                                Err(_) => Decision::Stay,
                            }
                        }
                        None => Decision::Stay,
                    }
                }
            }
            None => Decision::Stay,
        };

        // 不可见且已抵达最后已知位置附近 → 记忆失效。
        if !can_see
            && let Ok(mut known) = last_known.get_mut(actor)
            && let Some((kx, ky)) = known.0
            && self_position.0.abs_diff(kx) <= 2
            && self_position.1.abs_diff(ky) <= 2
        {
            known.0 = None;
        }

        apply_decision(&mut commands.entity(action), decision);
    }
}

/// `Flee`：保活（仍在滞回区间）→ 最远的合法方向 / 无路可逃则反咬 / 否则原地。
///
/// 保活对所有待执行逃跑生效（ECS43）：血量回到 [`FLEE_HP_RATIO_EXIT`] 以上立刻取消，
/// 不等到到点——否则一个已经不必逃的怪物会继续占着槽位。
#[allow(clippy::too_many_arguments)]
pub fn validate_flee_system(
    mut commands: Commands,
    mut reservation: Reservation,
    mut events: ActionEvents,
    actions: Query<ActionEntry, PendingAction<Flee>>,
    positions: Query<&Position>,
    healths: Query<&Health>,
    viewsheds: Query<&Viewshed>,
    player: Query<Entity, With<PlayerMarker>>,
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

    for (action, child_of, ready) in &actions {
        let actor = child_of.parent();

        // 保活：血量回到滞回退出阈值以上 → 不再逃。
        if !healths
            .get(actor)
            .is_ok_and(|health| health.ratio() < FLEE_HP_RATIO_EXIT)
        {
            events.fail(action, actor);
            continue;
        }

        // 以下是**定案**：只对到点的那一轮做。
        if !ready {
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
        let self_tile = Position::new(self_position.0, self_position.1);
        let player_tile = Position::new(player_position.0, player_position.1);

        // 选**最远**的合法方向（严格 `>` 比较：平局保留方向表里更靠前的那个）。
        //
        // 两步走：先只用 [`Reservation::probe`] 试算（**不写占用图**），选出最远的那个之后
        // 才 [`Reservation::claim`] 一次。不能"边试边占"——`plan_step` 的穿角检查会读占用图，
        // 试探性预留会把后面的对角方向判成 `CornerCutting`，于是选出来的不是最远方向，
        // 而是"没被自己挡死的方向里最靠后的那个"（这条实测过，见 validation_tests）。
        let mut best: Option<(Position, usize)> = None;
        for (dx, dy) in DIRECTIONS {
            let MovePlan::Allowed(candidate) = reservation.probe(self_tile, dx, dy) else {
                continue;
            };
            let distance = candidate.manhattan(player_tile);
            if best.is_none_or(|(_, best_distance)| distance > best_distance) {
                best = Some((candidate, distance));
            }
        }

        let decision = match best {
            Some((to, _)) => {
                // 定案才预留：逃跑者最终只占一格，不是八格。
                reservation.claim(actor, to);
                Decision::Move(to)
            }
            None => {
                // 无路可逃：相邻且看得见就反咬一口（与旧实现一致）。
                let near = self_tile.is_near(player_tile);
                let sees = viewsheds
                    .get(actor)
                    .ok()
                    .is_some_and(|viewshed| viewshed.can_see(player_position));
                if near && sees {
                    Decision::Attack(player_entity)
                } else {
                    Decision::Stay
                }
            }
        };
        apply_decision(&mut commands.entity(action), decision);
    }
}

/// `Wander`：随机一个方向（**消耗一次随机数**，与旧实现同序）→ 能走就定案，否则原地。
///
/// 游荡被挡**不算失败**：随机试探本来就是"试了，没走成"。
///
/// 游荡没有保活条件（随机试探没有"还该做吗"可言），但**定案照旧只在到点那一轮**：
/// 随机数必须等到真正要做这一步时才取，否则同一颗种子的整局结果会漂。
pub fn validate_wander_system(
    mut commands: Commands,
    mut reservation: Reservation,
    mut rng: ResMut<GameRng>,
    mut events: ActionEvents,
    actions: Query<ActionEntry, PendingAction<Wander>>,
    actors: Query<&Position>,
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

    for (action, child_of, ready) in &actions {
        if !ready {
            continue;
        }
        let actor = child_of.parent();
        let Ok(position) = actors.get(actor) else {
            events.fail(action, actor);
            continue;
        };

        // [`GameRng::random_range`] 是两参形式 `(lo, hi)`，不是 `rand` 的
        // `Rng::random_range(区间)`；不要 `use rand::RngExt`，否则同名方法会抢先生效。
        let index = GameRng::random_range(rng.as_mut(), 0, DIRECTIONS.len() as u64) as usize;
        let (dx, dy) = DIRECTIONS[index];
        let decision = match reservation.step(actor, *position, dx, dy) {
            Ok(to) => Decision::Move(to),
            Err(_) => Decision::Stay,
        };
        apply_decision(&mut commands.entity(action), decision);
    }
}

/// `BasicAttack`：前置条件是 [`can_attack_positions`]（8 方向相邻 + 目标存活）。
///
/// 它同时是保活条件（ECS43）：目标在排队期间走开或死掉 → 立刻取消，不等到到点。
/// 到点且条件仍成立时才**定案**为攻击。
pub fn validate_basic_attack_system(
    mut commands: Commands,
    mut events: ActionEvents,
    actions: Query<ActionEntryPayload<BasicAttack>, PendingAction<BasicAttack>>,
    positions: Query<&Position>,
    healths: Query<&Health>,
) {
    for (action, child_of, attack, ready) in &actions {
        let actor = child_of.parent();
        let ok = match (
            positions.get(actor),
            positions.get(attack.target),
            healths.get(attack.target),
        ) {
            (Ok(attacker), Ok(target), Ok(health)) => {
                can_attack_positions(*attacker, *target, health)
            }
            // 缺位置或缺血量 = 前置条件不成立（不是"默认允许"）。
            _ => false,
        };
        if !ok {
            events.fail(action, actor);
            continue;
        }
        if !ready {
            continue;
        }
        apply_decision(&mut commands.entity(action), Decision::Attack(attack.target));
    }
}

#[cfg(test)]
#[path = "validation_tests.rs"]
mod tests;
