//! 经验与升级。
//!
//! 经验来源是 `DeathEvent`：每轮结算里 `death` 模块先发事件，本模块把它们
//! 加成总经验发给玩家。**奖励数值随事件而来**，不需要任何旁路资源
//! （旧 `PendingExp` 已随 Phase E 删除；见 `events.rs::DeathEvent` 的说明）。

use crate::balance::{exp_to_next_level, max_hp_for, max_mp_for};
use crate::components::*;
use crate::entity_cls::Player;
use crate::events::{DeathEvent, LevelUpEvent};
use crate::resources::{EventLog, EventMessage};
use bevy_ecs::prelude::*;

/// 升级玩家：经验/等级/生命/法力 + 重算上限所需的防御与法术精通。
type ExpPlayer = (
    Entity,
    &'static mut Experience,
    &'static mut Level,
    &'static mut Health,
    &'static mut Magic,
    &'static Defense,
    &'static MagicMastery,
);

/// 消费 `DeathEvent` 的经验奖励，累加到玩家身上并处理升级。
///
/// 非玩家实体的死亡奖励会**被忽略**（当前设计里只有玩家吃经验）；若未来要让
/// 召唤物/宠物分经验，就在这里按实体筛选，而不是在死亡侧改奖励数值。
pub fn apply_exp_system(
    mut death_events: EventReader<DeathEvent>,
    mut players: Query<ExpPlayer, With<Player>>,
    mut event_log: ResMut<EventLog>,
    mut level_events: EventWriter<LevelUpEvent>,
) {
    let gained: f64 = death_events
        .read()
        .filter(|event| event.reward > 0.0)
        .map(|event| event.reward)
        .sum();
    if gained <= 0.0 {
        return;
    }

    for (entity, mut exp, mut level, mut health, mut magic, defense, mastery) in players.iter_mut() {
        exp.add(gained);
        while exp.overflow() > 0.0 {
            exp.exp = exp.overflow();
            level.0 += 1;

            let max_hp = max_hp_for(level.0, defense.0);
            let max_mp = max_mp_for(level.0, mastery.0);
            health.max = max_hp;
            health.current = max_hp;
            magic.max = max_mp;
            magic.current = max_mp;
            exp.exp_to_next = exp_to_next_level(level.0);

            log::debug!("升级: entity={entity:?} level={}", level.0);
            level_events.write(LevelUpEvent {
                entity,
                new_level: level.0,
            });
            event_log.push(EventMessage::system(format!(
                "升级！达到 Lv.{}",
                level.0
            )));
        }
    }
}