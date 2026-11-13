//! AI 生成系统：只 spawn 候选，不写 actor 行动状态。
//!
//! 四个系统分别对应四种能力组件；是否有资格产出候选由 `AiActor` 过滤条件决定。
//! 速度口径见 `SpeedRule`，AV 在候选创建时就固化进 `ActionTimer`。

use bevy_ecs::prelude::*;

use crate::action::entity::{
    ActionName, ActionPriority, ActionSource, AiActor, AiActorData, Candidate, PRIORITY_CHASE,
    PRIORITY_FLEE, PRIORITY_WAIT, PRIORITY_WANDER, SpeedModifiers, SpeedRule, actor_speeds,
};
use crate::action::ownership::ActionOf;
use crate::components::ActionTimer;
use crate::balance::{
    CHASE_DURATION, FLEE_DURATION, FLEE_HP_RATIO, WAIT_DURATION, WANDER_DURATION,
};
use crate::components::{
    CanChase, CanFlee, CanWait, CanWander, Chase, Flee, Health, Position, Stats, Viewshed, Wait,
    Wander,
};
use crate::effect::EffectContext;
use crate::entity_cls::Player;
use crate::monster::Aggro;

// ── 生成系统 ─────────────────────────────────────────
//
// 生成系统只 spawn 候选：不写 actor 的 `Idle/Active/Failure`，不写计时器。

/// 游荡候选：空闲/失败且具 `CanWander` 的怪物各产出一个 `Wander` 候选。
pub fn wander_generation_system(
    mut commands: Commands,
    actors: Query<AiActorData, AiActor<CanWander>>,
    effects: EffectContext,
) {
    for (actor, stats, position) in &actors {
        let collected = effects.collect(actor, position.to_tuple());
        commands.spawn((
            ActionOf(actor),
            ActionPriority(PRIORITY_WANDER),
            ActionSource::Ai,
            ActionName("Wander"),
            ActionTimer {
                remaining_av: SpeedRule::Move.action_av_ignoring(
                    WANDER_DURATION,
                    actor_speeds(actor, stats),
                    SpeedModifiers {
                        actor: &collected.actor,
                        cell: &collected.cell,
                    },
                    &collected.ignored,
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
    actors: Query<(Entity, Option<&Stats>, &Health, &Position), AiActor<CanFlee>>,
    effects: EffectContext,
) {
    for (actor, stats, health, position) in &actors {
        if health.ratio() >= FLEE_HP_RATIO {
            continue;
        }
        let collected = effects.collect(actor, position.to_tuple());
        commands.spawn((
            ActionOf(actor),
            ActionPriority(PRIORITY_FLEE),
            ActionSource::Ai,
            ActionName("Flee"),
            ActionTimer {
                remaining_av: SpeedRule::Move.action_av_ignoring(
                    FLEE_DURATION,
                    actor_speeds(actor, stats),
                    SpeedModifiers {
                        actor: &collected.actor,
                        cell: &collected.cell,
                    },
                    &collected.ignored,
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
            ActionOf(actor),
            ActionPriority(PRIORITY_WAIT),
            ActionSource::Ai,
            ActionName("Wait"),
            // 固定耗时：等待不受任何速度倍率影响（REFACTOR.md §11.6 第 4 项），
            // 因此也**不传修正器**——`Fixed` 分支根本不读它。
            ActionTimer {
                remaining_av: SpeedRule::Fixed.action_av(
                    WAIT_DURATION,
                    actor_speeds(actor, None),
                    SpeedModifiers::EMPTY,
                ),
            },
            Wait,
            Candidate,
        ));
    }
}

/// 追击候选（C5）：可见玩家、仍有记忆、或仇恨表里还有目标（DsnE29）。
///
/// 旧条件只有"看见玩家或有最后已知位置"；仇恨表加入后，被其他来源打过的怪物
/// 即使看不见玩家也会产生追击候选（记忆由 `record_threat_system` 写入，
/// 具体锁谁在 `validate_chase_system` 里按威胁值决定）。
pub fn chase_generation_system(
    mut commands: Commands,
    actors: Query<AiActorData, AiActor<CanChase>>,
    viewsheds: Query<&Viewshed>,
    aggro: Query<&Aggro>,
    player: Query<&Position, With<Player>>,
    effects: EffectContext,
) {
    let player_position = player.single().ok().map(Position::to_tuple);

    for (actor, stats, position) in &actors {
        let awareness = aggro.get(actor).ok();
        let can_see_player = player_position.is_some_and(|player| {
            viewsheds
                .get(actor)
                .ok()
                .is_some_and(|viewshed| viewshed.can_see(player))
        });
        let has_aggro = awareness.is_some_and(|aggro| !aggro.is_empty());
        let has_memory = awareness.is_some_and(|aggro| aggro.last_known().is_some());
        if !can_see_player && !has_aggro && !has_memory {
            continue;
        }
        let collected = effects.collect(actor, position.to_tuple());
        commands.spawn((
            ActionOf(actor),
            ActionPriority(PRIORITY_CHASE),
            ActionSource::Ai,
            ActionName("Chase"),
            ActionTimer {
                remaining_av: SpeedRule::Move.action_av_ignoring(
                    CHASE_DURATION,
                    actor_speeds(actor, stats),
                    SpeedModifiers {
                        actor: &collected.actor,
                        cell: &collected.cell,
                    },
                    &collected.ignored,
                ),
            },
            Chase,
            Candidate,
        ));
    }
}
