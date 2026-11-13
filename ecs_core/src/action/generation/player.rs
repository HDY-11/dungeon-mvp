//! 玩家路径生成（C2）：把已确认的玩家命令翻译成 **active action 实体**。
//!
//! 玩家输入不走 AI 生成/仲裁：`PlayerActionRequest` -> 直接产出 active action。

use bevy_ecs::prelude::*;

use crate::action::entity::{
    ActionName, ActionPriority, ActionSource, ActiveAction, MovementContext, PlayerWorld,
    PRIORITY_PLAYER, SpeedModifiers, SpeedRule, actor_speeds, begin_action,
};
use crate::action::ownership::ActionOf;
use crate::components::ActionTimer;
use crate::action::player_command::PlayerCommand;
use crate::balance::{THROW_DURATION_AV, UNARMED_ATTACK_DURATION, WAIT_DURATION};
use crate::components::{BasicAttack, Move, Wait};
use crate::effect::EffectContext;
use crate::map::{MAP_HEIGHT, MAP_WIDTH};
use crate::spell::SpellId;
use crate::throw::{ThrowAction, throw_reachable};

// ── 玩家路径生成（C2） ────────────────────────────────
//
// 玩家输入不走 AI 生成/仲裁：`PlayerActionRequest` → 直接产出 active action。

/// 玩家行动请求（C2）：与迁移前旧链路的 `PlayerActionRequest` 同形，
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
    /// 施法（DsnE26）：法术编号在生成期固定，目标留给校验阶段选。
    Cast(SpellId),
    /// 投掷（Gm9）：目标格在生成期固定（落点必中，落点上站谁由校验/结算处理）。
    Throw {
        target: (usize, usize),
    },
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
/// [`moved_position`]: crate::action::movement::moved_position
pub fn player_action_generation_system(
    mut commands: Commands,
    mut request: ResMut<PlayerActionRequest>,
    world: PlayerWorld,
    movement: MovementContext,
    effects: EffectContext,
    mut event_log: ResMut<crate::resources::EventLog>,
) {
    // 被拒绝时给玩家一句话（ISSUES SYN6）：此前三条拒绝路径都只写 `log::debug`，
    // 玩家看不到任何反馈，无法区分"撞墙""被挡住""卡住了"。
    // 连续重复会被 `EventLog::push` 合并成 (×N)（SYN8）。
    fn refuse(log: &mut crate::resources::EventLog, reason: &str) {
        log.push(crate::resources::EventMessage::system(reason));
    }    let Some(command) = request.command.take() else {
        return;
    };

    let Ok((player, position, stats)) = world.players.single() else {
        log::warn!("玩家请求无法处理（玩家不存在或已有行动）: {command:?}");
        return;
    };

    let (action, rule, duration) = match command {
        PlayerCommand::Wait => (PlayerAction::Wait, SpeedRule::Fixed, WAIT_DURATION),
        // 下楼在 `apply_player_command` 里就被拦走了（它不是行动）。真走到这里说明
        // 有人绕过那个入口塞了命令进来——记一条错误并按"没做成"处理，而不是
        // 静默当成 `Wait`（那会让"按了 > 却没下楼"变成一个查不到的迷）。
        PlayerCommand::Descend => {
            log::error!("下楼命令不该进入行动生成：它没有行动实体");
            refuse(&mut event_log, "下楼要在楼梯上按 >");
            return;
        }
        PlayerCommand::Move { dx, dy } => {
            let (nx, ny) = position.offset(dx, dy);
            if nx >= MAP_WIDTH || ny >= MAP_HEIGHT {
                log::debug!("玩家移动越界，拒绝请求: ({dx},{dy})");
                refuse(&mut event_log, "那边是墙外，走不过去");
                return;
            }
            if let Some(occupant) = movement.occupancy.entity_at(nx, ny) {
                if world.monsters.get(occupant).is_ok() {
                    // 走向怪物 = 声明攻击（与旧实现一致）。
                    (
                        PlayerAction::BasicAttack(occupant),
                        SpeedRule::Attack,
                        UNARMED_ATTACK_DURATION,
                    )
                } else {
                    log::debug!("玩家移动目标被非怪物占用，拒绝请求: ({nx},{ny})");
                    refuse(&mut event_log, "那里有东西挡着");
                    return;
                }
            } else if crate::action::movement::moved_position(
                &movement.map, &movement.occupancy, *position, dx, dy,
            )
            .is_none()
            {
                log::debug!("玩家移动非法，拒绝请求: ({dx},{dy})");
                refuse(&mut event_log, "那边过不去");
                return;
            } else {
                (
                    PlayerAction::Move(Move { dx, dy }),
                    SpeedRule::Move,
                    UNARMED_ATTACK_DURATION,
                )
            }
        }
        PlayerCommand::Cast { slot } => {
            let Ok((book, mana, viewshed)) = world.books.get(player) else {
                log::warn!("玩家没有法术书，拒绝施法");
                refuse(&mut event_log, "你还没有学会任何法术");
                return;
            };
            let Some(spell_id) = book.get(usize::from(slot)) else {
                refuse(&mut event_log, "那个法术格是空的");
                return;
            };
            let spell = spell_id.def();
            if mana.current < f64::from(spell.mp_cost) {
                refuse(&mut event_log, "法力不足，念不出这个法术");
                return;
            }
            // 目标在别处校验：生成期只拒绝"连个能选的目标都没有"的情形，
            // 免得玩家花一个吟唱才被告知什么都打不到。
            if spell.targeting == crate::spell::SpellTargeting::NearestEnemy
                && crate::spell::nearest_enemy_in_range(
                    *position,
                    viewshed,
                    spell.range,
                    world
                        .monster_positions
                        .iter()
                        .map(|(entity, position)| (entity, *position)),
                )
                .is_none()
            {
                refuse(&mut event_log, "视野里没有可施法的目标");
                return;
            }
            (PlayerAction::Cast(spell_id), SpeedRule::Fixed, spell.cast_av)
        }
        PlayerCommand::Throw { dx, dy } => {
            let (nx, ny) = position.offset(dx, dy);
            // 取用口径（Gm9）：看副手，不看快捷键——副手没有可投掷物就拒绝；
            // 行囊里的石子由应用层在进入瞄准前自动装填，核心不替输入层做这件事。
            let off_hand_throwable = world
                .bodies
                .get(player)
                .ok()
                .and_then(|body| body.slot(crate::item::off_hand_slot_index()))
                .is_some_and(|item| world.throwables.get(item).is_ok());
            if !off_hand_throwable {
                refuse(&mut event_log, "副手没有可投掷的东西");
                return;
            }
            if !throw_reachable(&movement.map, position.to_tuple(), (nx, ny)) {
                refuse(&mut event_log, "那个位置投不到：太远或视线被挡住");
                return;
            }
            (
                PlayerAction::Throw { target: (nx, ny) },
                SpeedRule::Fixed,
                THROW_DURATION_AV,
            )
        }
    };

    // 收桶（求值第 ② 步）：玩家脚下这一格的修正 + 玩家名下效果声明的无视来源。
    let collected = effects.collect(player, position.to_tuple());
    // 武器攻速（GAME.md Gm7「武器攻速」）：主手武器覆盖攻击的基础耗时。
    // 移动仍用 `UNARMED_ATTACK_DURATION`——Gm7 的旧口径把"移动/攻击耗时"都挂在武器上，
    // 那是旧系统的一个副作用；把移动交给移速是这一版有意的改动（记在 DsnE11 的代价里）。
    let duration = match action {
        PlayerAction::BasicAttack(_) => effects
            .main_hand_weapon_speed(player)
            .unwrap_or(UNARMED_ATTACK_DURATION),
        _ => duration,
    };
    let remaining_av = rule.action_av_ignoring(
        duration,
        actor_speeds(player, stats),
        SpeedModifiers {
            actor: &collected.actor,
            cell: &collected.cell,
        },
        &collected.ignored,
    );

    let mut action_cmd = commands.spawn((
        ActionOf(player),
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
        PlayerAction::Cast(spell) => {
            // 目标不在生成期固定：校验阶段按"此刻视野里最近的敌人"重新选，
            // 吟唱期间目标跑动/换目标都由此兜住（DsnE26）。
            action_cmd.insert((ActionName("Cast"), crate::spell::CastSpell { spell }));
        }
        PlayerAction::Throw { target } => {
            action_cmd.insert((ActionName("Throw"), ThrowAction { target }));
        }
    }

    let mut player_cmd = commands.entity(player);
    begin_action(&mut player_cmd);
    log::debug!("玩家行动已挂载: {action:?} player={player:?}");
}
