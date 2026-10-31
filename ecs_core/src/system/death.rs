//! 死亡判定、`DeathEvent` 的生产，以及**掉落**。
//!
//! 职责边界：本模块负责「谁死了、把它从世界里拿掉、并带上死亡时应结算的数值」。
//! 经验与升级在 `experience` 模块；掉落在这里就地结算。
//!
//! # 为什么奖励写进事件、掉落却不写
//!
//! 经验**必须**写进 `DeathEvent`：实体在这个系统里就被 `despawn` 了，下游
//! （`apply_exp_system`）事后拿不到它的 `ExperienceReward`。掉落不同——它需要
//! `Commands` 去 spawn 新实体，而事件消费者拿到的只是一份数据；与其让下游再拿
//! "死者的位置 + 物种"去查表，不如在这里一次做完（掷骰是纯函数，见 `item::loot`）。
//!
//! 顺序也有意义：先掷骰生成掉落、再 `despawn` 死者。反过来会读到已消失的实体。

use crate::components::*;
use crate::entity_cls::Player;
use crate::events::DeathEvent;
use crate::item::{GroundItem, GroundItemAt, roll_loot};
use crate::monster::MonsterKindId;
use crate::resources::{EventLog, EventMessage, GameRng, TurnManager};
use bevy_ecs::prelude::*;

/// 死亡候选：实体 + 生命 + （玩家？经验奖励？名字？位置？物种？）。
type DeathCandidate = (
    Entity,
    &'static Health,
    Option<&'static Player>,
    Option<&'static ExperienceReward>,
    Option<&'static EntityName>,
    Option<&'static Position>,
    Option<&'static MonsterKindId>,
);

/// 必须最后执行：把死亡实体转为 `DeathEvent`，处理玩家失败/怪物经验与掉落。
///
/// 玩家死亡走 `TurnManager::game_over`，不发奖励、不掉落；怪物死亡按模板掉落表
/// 掷骰，把结果作为地面物品放在它倒下的那一格。
pub fn check_death_system(
    mut commands: Commands,
    query: Query<DeathCandidate>,
    mut death_events: EventWriter<DeathEvent>,
    mut turn_manager: ResMut<TurnManager>,
    mut rng: ResMut<GameRng>,
    mut event_log: ResMut<EventLog>,
) {
    for (entity, health, player, reward, name, position, kind) in query.iter() {
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

        // 掉落：掷骰 → 生成地面物品。位置缺失（理论上不该发生）就跳过掉落但照常回收，
        // 免得"一只怪没位置"把整个死亡流程卡住。
        if let (Some(kind), Some(position)) = (kind, position) {
            let table = crate::monster::monster_template(*kind).loot;
            for (item, count) in roll_loot(table, &mut rng) {
                commands.spawn((
                    GroundItemAt::new(position.x, position.y),
                    GroundItem::of(item, count),
                ));
            }
        }

        commands.entity(entity).despawn();
    }
}
