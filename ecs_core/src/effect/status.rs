//! 状态效果（C3）：**中毒**。
//!
//! # 这是 DsnE12 第 4 条第一次出现"有时长、会自己消失"的效果
//!
//! 设计里写的是"**效果实体由恰好一个 owner 系统负责回收**"。中毒落地时曾把
//! "扣时间 + 回收"都塞在 [`poison_tick_system`] 里；DsnE27 之后、在第二个有时长的
//! 效果出现之前，把这件事拆成通用骨架（`effect::duration`）：
//!
//! | 系统 | 职责 | 什么时候跑 |
//! |---|---|---|
//! | [`crate::effect::advance_effect_durations_system`] | 按 `WorldClock.last_step_av` 推进所有 `EffectDuration` | 本模块之前 |
//! | [`poison_tick_system`] | 只读推进后的剩余时长，判断发作点、扣血；**不回收实体** | 死亡判定之前 |
//! | [`poison_apply_system`] | 把"蝎子打中了谁"翻译成一条中毒效果（刷新，不叠层） | 发作结算之后 |
//! | [`crate::effect::expire_effects_system`] | 到期统一回收（含中毒） | 本模块之后 |
//!
//! 顺序不是随手排的：tick 必须在 `check_death_system` **之前**，否则"被毒死"要晚一轮
//! 才被判定（玩家会看到血量归零却没死）；`expire` 必须在 tick **之后**，否则最后一次
//! 发作会被"到期"吞掉。这条由 `build_core_schedule` 的 `.chain()` 固定，且
//! `ambiguity_detection = Error` 会逼着新增系统把顺序写出来。
//!
//! 中毒自己的计时现在是**派生**的：`Poison` 只存"还剩几次发作"，时长在
//! [`EffectDuration`] 上。"第 N 次发作发生在 `ticks_left == N` 且剩余时长跨过
//! `(N-1) * POISON_TICK_AV`"——这样一个时钟，两个消费者（tick 与到期）不会漂移。

use crate::components::*;
use crate::effect::{EffectDuration, EffectOf, OwnedEffects};
use crate::events::AttackEvent;
use crate::monster::MonsterKindId;
use crate::resources::{EventLog, EventMessage};
use bevy_ecs::prelude::*;

/// 两次中毒发作之间隔多少 **AV**。[⃞试调] 内容参数（数值口径属 GAME.md Gm5）。
///
/// 用 AV 而不是"轮次"：玩家一次行动会跨若干个世界轮，按轮次计时会让
/// "毒多久"随场上怪物数量漂移（ISSUES ECS46）。参考 GAME.md Gm5 的旧口径
/// "1000 AV ≈ 3 次行动"，400 AV 大约相当于 1.5 次玩家移动。
pub const POISON_TICK_AV: f64 = 400.0;

/// 一共发作几次。[⃞试调]
pub const POISON_TICKS: u32 = 3;

/// 每次发作伤害。[⃞试调] 不吃防御（毒不是攻击），但**受魔法抗性影响**
///（m08882：中毒是魔法伤害的一种；抗性上限由词表 `magic_resist` 管，0.75）。
pub const POISON_DAMAGE_PER_TICK: f64 = 2.0;

/// 一剂标准中毒的总时长（AV）＝发作间隔 × 次数。
pub const POISON_TOTAL_AV: f64 = POISON_TICK_AV * POISON_TICKS as f64;

/// 中毒状态。挂在**效果实体**上（经 [`EffectOf`] 归属到受害者）。
///
/// 只剩"还剩几次发作 + 每次伤害"；**时长**由同一实体上的 [`EffectDuration`] 承担，
/// 由通用到期系统回收。这样第二个有时长的效果出现时，计时/回收不需要各写一份。
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Poison {
    /// 还剩几次发作（总时长由 `EffectDuration` 单独推进）。
    pub ticks_left: u32,
    /// 每次发作的伤害。
    pub damage_per_tick: f64,
}

impl Poison {
    /// 一剂标准中毒（内容参数见本模块常量）。
    pub fn standard() -> Self {
        Self {
            ticks_left: POISON_TICKS,
            damage_per_tick: POISON_DAMAGE_PER_TICK,
        }
    }
}

/// **施加**：蝎子的近战命中会给目标挂中毒。
///
/// 为什么用 `AttackEvent` 而不是在战斗公式里加字段：战斗公式是纯函数（DsnE9），
/// 而"这一击带不带毒"属**内容**（物种特性），不是公式的一部分。订阅事件让公式保持
/// 对"毒"一无所知。
///
/// 重复命中**刷新**而不是叠层：叠层会让多只蝎子的伤害呈平方增长，而目前
/// 没有任何设计支撑那个曲线。刷新 = 次数回满、剩余时长重置（不叠层；将来若要做
/// 叠层，改动点就是这里的次数与时长策略）。
pub fn poison_apply_system(
    mut commands: Commands,
    mut attacks: EventReader<AttackEvent>,
    attackers: Query<&MonsterKindId>,
    victims: Query<(&Health, Option<&OwnedEffects>)>,
    mut poisons: Query<&mut Poison>,
    mut durations: Query<&mut EffectDuration>,
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
                    // 刷新 = 重新计时、回满次数（不叠层：叠层会让多只蝎子的伤害呈平方增长）。
                    poison.ticks_left = POISON_TICKS;
                }
                if let Ok(mut duration) = durations.get_mut(effect) {
                    duration.reset(POISON_TOTAL_AV);
                }
            }
            None => {
                commands.spawn((
                    EffectOf(event.target),
                    Poison::standard(),
                    EffectDuration::new(POISON_TOTAL_AV),
                ));
                event_log.push(EventMessage::danger("你中毒了"));
                log::debug!("施加中毒: victim={:?}", event.target);
            }
        }
    }
}

/// **运转**：只读推进后的剩余时长，结清本轮跨过的发作点。
///
/// 受害者被 despawn 时，效果实体由 `EffectOf` 的 `linked_spawn` 级联回收——
/// 这正是"两个关系类型分开"（ECS42）要的语义：状态效果**应该**跟着人一起消失。
/// 时间到期的回收则在 `expire_effects_system`（`effect::duration`），本系统不再
/// `despawn` 任何实体（DsnE12 第 4 条的唯一回收入口）。
pub fn poison_tick_system(
    mut poisons: Query<(&mut Poison, &EffectDuration, &EffectOf)>,
    mut healths: Query<&mut Health>,
    victims: Query<(&Position, Option<&Stats>, &EntityName)>,
    effects: crate::effect::EffectContext,
    mut event_log: ResMut<EventLog>,
) {
    for (mut poison, duration, owner) in &mut poisons {
        let victim = owner.parent();

        // 可能一轮跨过多个发作点（例如玩家做了一次很长的行动）。
        // 第 k 次发作的判据：还剩 k 次时，剩余时长已跨过 `(k-1) 个间隔`。
        while poison.ticks_left > 0
            && duration.remaining_av <= (poison.ticks_left.saturating_sub(1) as f64) * POISON_TICK_AV
        {
            // 毒是魔法伤害：魔法抗性按"独立乘区"折进来（m08882）。
            // 基数来自 `Stats`，装备/状态修正走与攻击同一条求值路径；位置用于收脚下格修正。
            let (base, collected) = victims.get(victim).map_or_else(
                |_| (0.0, crate::effect::CollectedModifiers::default()),
                |(position, stats, _)| {
                    let base = stats
                        .map(|stats| stats.get(crate::rules::StatId::MAGIC_RESIST))
                        .unwrap_or(0.0);
                    (base, effects.collect(victim, (position.x, position.y)))
                },
            );
            let resist = crate::rules::clamp_resistance(crate::rules::apply_modifiers(
                crate::rules::StatId::MAGIC_RESIST,
                base,
                &collected.actor,
                &collected.ignored,
            ));
            let damage = (poison.damage_per_tick * (1.0 - resist)).max(1.0);
            if let Ok(mut health) = healths.get_mut(victim) {
                *health = health.damage(damage);
                let name = victims
                    .get(victim)
                    .map(|(_, _, name)| name.0.as_str())
                    .unwrap_or("目标");
                event_log.push(EventMessage::combat(format!(
                    "{name} 受到 {damage:.0} 点中毒伤害"
                )));
            }
            poison.ticks_left -= 1;
        }
    }
}

#[cfg(test)]
#[path = "status_tests.rs"]
mod tests;
