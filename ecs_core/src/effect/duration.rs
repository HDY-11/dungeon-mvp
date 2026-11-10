//! 效果生命周期（DsnE12 第 4 条）：**时长推进**与**到期回收**的通用实现。
//!
//! 单独拆出来，是因为"计时 + 回收"是所有有时长的效果共用的骨架：
//! 中毒之前把两件事都塞在自己的 tick 系统里；第二个有时长的效果（buff、污染、
//! 燃烧地面）出现时，就会复制一份计时/回收逻辑，回收入口也就不再唯一。
//!
//! # 两个系统，一条顺序契约
//!
//! | 系统 | 职责 | 在结算链中的位置 |
//! |---|---|---|
//! | [`advance_effect_durations_system`] | 按 `WorldClock.last_step_av` 把所有效果推进同样的 AV | **payload 系统之前** |
//! | [`expire_effects_system`] | `remaining_av <= 0` 的效果统一 `despawn` | **payload 系统之后** |
//!
//! payload 系统（如 `poison_tick_system`）只读推进后的 [`EffectDuration`] 决定"这一轮
//! 该不该发作"，并且只改自己的数据；它**不得**回收实体。顺序由
//! `build_core_schedule` 的 `.chain()` 固定——把 `advance` 放到 payload 之后，时长会
//! 晚一轮；把 `expire` 放到 payload 之前，会吞掉最后一次发作。
//!
//! # 时长单位
//!
//! **AV**（世界时钟推进量），不是结算轮次：轮次长度随场上怪物数量漂移（ISSUES ECS46）。
//! 内容数值（毒 400 AV × 3 次等）仍在 GAME.md 与具体效果模块，这里只管"还剩多少"。
//!
//! **格子效果不接这条路径**：`CellEffectAt` 每层按 `(seed, floor)` 重建、没有独立
//! 生命周期（DsnE12 的"格子效果"落点是索引 + 每轮重建），它们不挂 [`EffectDuration`]。

use crate::resources::WorldClock;
use bevy_ecs::prelude::*;

/// 效果还能活多久（AV）。挂在**效果实体**上，由 [`expire_effects_system`] 统一回收。
///
/// 字段是"还剩多少"而不是"什么时候到期"：与 `ActionTimer` 同理，用差值比用时间戳好
/// 推理；也避免把时钟绝对值写进组件（将来存档冻结时钟口径时不必迁移语义）。
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct EffectDuration {
    /// 距离到期还剩多少 AV（按世界时钟扣减，不是按结算轮次）。
    pub remaining_av: f64,
}

impl EffectDuration {
    /// 一条时长为 `total_av` 的新效果。
    pub const fn new(total_av: f64) -> Self {
        Self {
            remaining_av: total_av,
        }
    }

    /// 刷新（重置）为完整时长。**刷新策略由 payload 决定**：这里只提供原语。
    pub fn reset(&mut self, total_av: f64) {
        self.remaining_av = total_av;
    }

    /// 是否已经到期（`expire_effects_system` 的判据）。
    pub fn is_expired(&self) -> bool {
        self.remaining_av <= 0.0
    }
}

/// **推进**：所有效果按本轮世界时间（`last_step_av`）一起走。
///
/// 必须排在 payload 系统（如 `poison_tick_system`）**之前**：payload 读到的
/// `remaining_av` 必须是"这一轮已经走完"的值，否则发作会整体晚一轮。
pub fn advance_effect_durations_system(
    clock: Res<WorldClock>,
    mut durations: Query<&mut EffectDuration>,
) {
    let step = clock.last_step_av;
    if step <= 0.0 {
        return;
    }
    for mut duration in &mut durations {
        duration.remaining_av -= step;
    }
}

/// **到期回收**：唯一按时间终结效果实体的地方（DsnE12 第 4 条）。
///
/// 必须排在 payload 系统**之后**：最后一次发作发生在 `remaining_av` 从正数跨到 0 的
/// 那一轮，payload 先结算，再回收。其他终结路径（被解除、被替换）将来也走这里：
/// 先由解除方把效果标记成到期/移除来源，再由本系统统一回收。
pub fn expire_effects_system(mut commands: Commands, durations: Query<(Entity, &EffectDuration)>) {
    for (effect, duration) in &durations {
        if duration.is_expired() {
            commands.entity(effect).despawn();
            log::debug!("效果到期: effect={effect:?}");
        }
    }
}

#[cfg(test)]
#[path = "duration_tests.rs"]
mod tests;
