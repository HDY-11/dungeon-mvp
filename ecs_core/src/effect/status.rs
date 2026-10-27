//! 状态效果（C3）：**中毒**。
//!
//! # 这是 DsnE12 第 4 条的第二个半句第一次落地
//!
//! 设计里写的是"**每类效果恰好一个 owner 系统**负责它的全部终结路径（过期、被解除、
//! 被替换）；其他系统只能改它的数据，不能 `despawn` 它"。本模块之前，效果实体只有
//! **测试**在用，生命周期系统一个也没有——`CellEffects` 每轮重建算是特例（格子效果没有
//! 独立生命周期）。中毒是第一个真正"活一段时间后自己消失"的效果。
//!
//! 于是两个系统分工明确：
//!
//! | 系统 | 职责 | 什么时候跑 |
//! |---|---|---|
//! | [`poison_apply_system`] | 把"蝎子打中了谁"翻译成一条中毒效果（刷新，不叠层） | 伤害结算之后 |
//! | [`poison_tick_system`] | 每轮扣血、减回合、**到期时回收实体**（唯一回收者） | 死亡判定之前 |
//!
//! 顺序不是随手排的：tick 必须在 `check_death_system` **之前**，否则"被毒死"要晚一轮
//! 才被判定（玩家会看到血量归零却没死）。这条由 `build_core_schedule` 的 `.chain()` 固定，
//! 且 `ambiguity_detection = Error` 会逼着新增系统把顺序写出来。

use crate::components::*;
use crate::effect::{EffectOf, OwnedEffects};
use crate::events::AttackEvent;
use crate::monster::MonsterKindId;
use crate::resources::{EventLog, EventMessage};
use bevy_ecs::prelude::*;

/// 每回合中毒伤害。[⃞试调] 内容参数（数值口径属 GAME.md Gm5）。
pub const POISON_DAMAGE_PER_ROUND: f64 = 2.0;

/// 中毒持续回合数。[⃞试调]
pub const POISON_ROUNDS: u32 = 3;

/// 中毒状态。挂在**效果实体**上（经 [`EffectOf`] 归属到受害者）。
///
/// 字段是"还剩多少"而不是"什么时候到期"：回合数是离散的，用差值比用时间戳好推理，
/// 也让存档不需要冻结时钟口径。
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Poison {
    pub damage_per_round: f64,
    pub rounds_left: u32,
}

impl Poison {
    /// 一剂标准中毒（内容参数见本模块常量）。
    pub fn standard() -> Self {
        Self {
            damage_per_round: POISON_DAMAGE_PER_ROUND,
            rounds_left: POISON_ROUNDS,
        }
    }
}

/// **施加**：蝎子的近战命中会给目标挂中毒。
///
/// 为什么用 `AttackEvent` 而不是在战斗公式里加字段：战斗公式是纯函数（DsnE9），
/// 而"这一击带不带毒"属**内容**（物种特性），不是公式的一部分。订阅事件让公式保持
/// 对"毒"一无所知。
///
/// 重复命中**刷新**回合数而不是叠层：叠层会让多只蝎子的伤害呈平方增长，而目前
/// 没有任何设计支撑那个曲线。将来若要做叠层，改动点就是这里的 `rounds_left.max`。
pub fn poison_apply_system(
    mut commands: Commands,
    mut attacks: EventReader<AttackEvent>,
    attackers: Query<&MonsterKindId>,
    victims: Query<(&Health, Option<&OwnedEffects>)>,
    mut poisons: Query<&mut Poison>,
    mut event_log: ResMut<EventLog>,
) {
    for event in attacks.read() {
        // 只有蝎子带毒（内容口径，见 GAME.md Gm8 的物种设定）。
        let Ok(&kind) = attackers.get(event.attacker) else {
            continue;
        };
        if kind != MonsterKindId::Scorpion {
            continue;
        }
        // 目标必须是可受伤的活物。
        //
        // **`OwnedEffects` 用 `Option` 查，不能当"目标合法"的判据**：关系目标组件只在
        // 第一个孩子出现时才被创建，所以"这条效果也没有"表现为**组件缺失**，而不是
        // 空列表。把它写成必需组件，会让"第一次中毒"永远挂不上（本模块的第一版就踩了：
        // 断言失败在"蝎子命中必须挂毒"上）。
        let Ok((_, owned)) = victims.get(event.target) else {
            continue;
        };

        let already: Option<Entity> = owned
            .into_iter()
            .flat_map(|owned| owned.iter())
            .find(|effect| poisons.get(*effect).is_ok());
        match already {
            Some(effect) => {
                if let Ok(mut poison) = poisons.get_mut(effect) {
                    poison.rounds_left = poison.rounds_left.max(POISON_ROUNDS);
                }
            }
            None => {
                commands.spawn((EffectOf(event.target), Poison::standard()));
                event_log.push(EventMessage::danger("你中毒了"));
                log::debug!("施加中毒: victim={:?}", event.target);
            }
        }
    }
}

/// **运转与终结**：每轮扣血、减回合，到期 `despawn`（唯一回收者，DsnE12 第 4 条）。
///
/// 受害者被 despawn 时，效果实体由 `EffectOf` 的 `linked_spawn` 级联回收——
/// 这正是"两个关系类型分开"（ECS42）要的语义：状态效果**应该**跟着人一起消失。
pub fn poison_tick_system(
    mut commands: Commands,
    mut poisons: Query<(Entity, &mut Poison, &EffectOf)>,
    mut healths: Query<&mut Health>,
    names: Query<&EntityName>,
    mut event_log: ResMut<EventLog>,
) {
    for (effect, mut poison, owner) in &mut poisons {
        let victim = owner.parent();
        let damage = poison.damage_per_round;

        if let Ok(mut health) = healths.get_mut(victim) {
            *health = health.damage(damage);
            let name = names
                .get(victim)
                .map(|name| name.0.as_str())
                .unwrap_or("目标");
            event_log.push(EventMessage::combat(format!(
                "{name} 受到 {damage:.0} 点中毒伤害"
            )));
        }

        poison.rounds_left = poison.rounds_left.saturating_sub(1);
        if poison.rounds_left == 0 {
            commands.entity(effect).despawn();
            log::debug!("中毒消退: victim={victim:?}");
        }
    }
}

#[cfg(test)]
#[path = "status_tests.rs"]
mod tests;
