//! 死亡判定与 `DeathEvent` 的生产。
//!
//! 职责边界：本模块**只负责**「谁死了、把它从世界里拿掉、并带上死亡时应结算的
//! 数值（经验奖励）」。经验与升级在 `experience` 模块，掉落/成就等未来消费者
//! 各自订阅 `DeathEvent`。
//!
//! 奖励必须写进事件里（而不是留在世界某处让下游去查）：实体在这个系统里就被
//! `despawn`，事后拿不到它的 `ExperienceReward`。旧实现为此保留了一个
//! `PendingExp` 旁路资源，结果同一件事同时存在事件与旁路两条路——Phase E 已删除。

use crate::components::*;
use crate::entity_cls::Player;
use crate::events::DeathEvent;
use crate::resources::{EventLog, EventMessage, TurnManager};
use bevy_ecs::prelude::*;

/// 死亡候选：实体 + 生命 + （玩家？经验奖励？名字？）。
type DeathCandidate = (
    Entity,
    &'static Health,
    Option<&'static Player>,
    Option<&'static ExperienceReward>,
    Option<&'static EntityName>,
);

/// 必须最后执行：把死亡实体转为 `DeathEvent`，并处理玩家失败/怪物经验。
///
/// 玩家死亡走 `TurnManager::game_over`，不发奖励；怪物死亡 `despawn` 并把
/// `ExperienceReward` 抄进事件。
pub fn check_death_system(
    mut commands: Commands,
    query: Query<DeathCandidate>,
    mut death_events: EventWriter<DeathEvent>,
    mut turn_manager: ResMut<TurnManager>,
    mut event_log: ResMut<EventLog>,
) {
    for (entity, health, player, reward, name) in query.iter() {
        if health.is_alive() {
            continue;
        }

        // 玩家没有经验奖励（`ExperienceReward` 只挂在怪物上）。
        let reward_amount = reward.map(|r| r.0).unwrap_or(0.0);
        death_events.write(DeathEvent {
            entity,
            reward: reward_amount,
        });

        if player.is_some() {
            log::warn!("玩家死亡");
            turn_manager.game_over = true;
            event_log.push(EventMessage::danger("你死了"));
            continue;
        }

        let name = name.map(|n| n.0.as_str()).unwrap_or("怪物");
        log::info!("实体死亡: {name} ({entity:?})");
        event_log.push(EventMessage::combat(format!("{name} 倒下了")));

        commands.entity(entity).despawn();
    }
}