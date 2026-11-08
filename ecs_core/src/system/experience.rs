//! 经验与升级。
//!
//! 经验来源是 `DeathEvent`：每轮结算里 `death` 模块先发事件，本模块把它们
//! 加成总经验发给玩家。**奖励数值随事件而来**，不需要任何旁路资源
//! （旧 `PendingExp` 已随 Phase E 删除；见 `events.rs::DeathEvent` 的说明）。
//!
//! 升级只做两件事：清空阈值内的经验、把等级 +1；生命/法力的**上限**交给
//! `rules::vitals::apply_maxima`（DsnE23 的唯一公式）。升级**不回满**（DsnE25
//! 决策 2）：生命只能靠消耗品/治疗泉恢复，法力走随时间回复——升级不再附带续航，
//! 否则"升一级=回满"会把资源取舍变成免费重置。

use crate::balance::exp_to_next_level;
use crate::components::*;
use crate::effect::EffectContext;
use crate::entity_cls::Player;
use crate::events::{DeathEvent, LevelUpEvent};
use crate::resources::{EventLog, EventMessage};
use crate::rules::apply_maxima;
use bevy_ecs::prelude::*;

/// 升级玩家：经验/等级/生命/法力。上限不在这里算（DsnE23）。
type ExpPlayer = (
    Entity,
    &'static mut Experience,
    &'static mut Level,
    &'static mut Health,
    &'static mut Mana,
);

/// 消费 `DeathEvent` 的经验奖励，累加到玩家身上并处理升级。
///
/// 非玩家实体的死亡奖励会**被忽略**（当前设计里只有玩家吃经验）；若未来要让
/// 召唤物/宠物分经验，就在这里按实体筛选，而不是在死亡侧改奖励数值。
pub fn apply_exp_system(
    mut death_events: EventReader<DeathEvent>,
    mut players: Query<ExpPlayer, With<Player>>,
    effects: EffectContext,
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

    for (entity, mut exp, mut level, mut health, mut mana) in players.iter_mut() {
        // 死人不吃经验（结算链是 `death → exp`，同轮里玩家可能刚被怪物打死）：
        // 若放行，升级会把 `health.current` 回满，HUD 上出现"死了却满血"，
        // 存档也会把尸体当活人写进盘。游戏结束后等级/经验都不该再变。
        if !health.is_alive() {
            continue;
        }
        exp.add(gained);
        while exp.overflow() > 0.0 {
            exp.exp = exp.overflow();
            level.0 += 1;

            // 上限公式只有一个实现（DsnE23）：等级 + 装备/状态修正。
            // DsnE25 起升级**不回满**——只重算上限、夹取 current。
            apply_maxima(
                level.0,
                &effects.collect_actor(entity),
                &mut health,
                &mut mana,
                false,
            );
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
