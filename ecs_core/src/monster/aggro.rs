//! 仇恨 / 威胁骨架（DsnE29）。
//!
//! 现状只做**玩家与怪物**的一层：谁对怪物造成了伤害，就记一条威胁；追击时优先
//! 锁定威胁最高的来源（活着且仍可寻路），其次才是"看见玩家"的旧逻辑。
//!
//! 为什么先做"威胁表"而不是"单目标仇恨"：内容侧第一层要的是群体仇恨/警戒传播/
//! 三方势力（见 `content/BESTIARY.md` 的 AI 档位讨论）。单目标字段升级到多目标
//! 要同时改读方；而 `Vec<ThreatEntry>` 从第一天就能容纳任意个来源，UI/AI 读的
//! 仍是同一个 `target()`。后续再补的三件事都不改现有读方：
//!
//! 1. 威胁衰减曲线/按类型倍率 → 改 [`Aggro::decay`] 与 [`record_threat_system`]；
//! 2. 群体仇恨/警戒传播 → 新增 `GroupId`/`AlertEvent`，由广播系统往同群 `Aggro`
//!    里 `record` 一份来源；本模块结构不变；
//! 3. 感知通道（听觉/气味） → 在 `record` 的调用方加来源，不在本模块加分支。
//!
//! 仇恨只影响**目标选择**，不新增行动原语：追击/攻击仍走已验证的行动链。
//!
//! ## 何时清理
//!
//! - 威胁值衰减到 0 → 来源从表里移除（[`threat_decay_system`]）；
//! - 来源死亡 → 立刻移除（同上，`Health` 不在存活态）；
//! - `last_known` 独立于威胁表：它表达"最后看见/被打的位置"，由追击校验器在
//!   抵达后清除（与旧 `LastKnownPlayerPos` 同一语义，DsnE29 把它并进来）。

use crate::balance::{THREAT_DECAY_PER_1000_AV, THREAT_PER_DAMAGE, THREAT_PER_HIT};
use crate::components::{Health, Position};
use crate::events::DamageTakenEvent;
use crate::resources::WorldClock;
use bevy_ecs::prelude::*;

/// 一条威胁记录：谁、多少。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThreatEntry {
    pub source: Entity,
    pub value: f64,
}

/// 怪物对世界其他实体的敌意记忆。
///
/// - `target()` 的行为口径：威胁值最高者；同值按实体编号升序（确定性）。
/// - 任何来源都可以记录，不限于玩家——怪物打怪物、召唤物、陷阱（将来）都复用。
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct Aggro {
    entries: Vec<ThreatEntry>,
    last_known: Option<(usize, usize)>,
}

impl Aggro {
    pub fn new() -> Self {
        Self::default()
    }

    /// 带一段记忆创建（读档、测试、以及"看见但没打"的情形）。
    pub fn with_memory(last_known: (usize, usize)) -> Self {
        Self {
            entries: Vec::new(),
            last_known: Some(last_known),
        }
    }

    /// 记一笔威胁。非有限值/非正值被忽略（伤害换算在调用方做）。
    ///
    /// 同一来源累加而不是覆盖：多次攻击比一次更"拉仇恨"。
    pub fn record(&mut self, source: Entity, value: f64) {
        if !value.is_finite() || value <= 0.0 {
            return;
        }
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.source == source) {
            entry.value += value;
        } else {
            self.entries.push(ThreatEntry { source, value });
        }
    }

    /// 对某个来源的当前威胁值。
    pub fn threat_against(&self, source: Entity) -> f64 {
        self.entries
            .iter()
            .find(|entry| entry.source == source)
            .map_or(0.0, |entry| entry.value)
    }

    /// 当前锁定的目标：威胁最高者，同值取实体编号小者。
    pub fn target(&self) -> Option<Entity> {
        self.entries
            .iter()
            .max_by(|a, b| {
                a.value.total_cmp(&b.value).then_with(|| {
                    // 值相同时让实体编号小者胜出（max_by 视角反转）。
                    // 0.20 的 to_bits 是倒序编码，不能用。
                    b.source.index_u32().cmp(&a.source.index_u32())
                })
            })
            .map(|entry| entry.source)
    }

    /// 最后已知位置（看见目标或被目标打中时写入）。
    pub fn last_known(&self) -> Option<(usize, usize)> {
        self.last_known
    }

    pub fn remember(&mut self, position: (usize, usize)) {
        self.last_known = Some(position);
    }

    pub fn forget(&mut self) {
        self.last_known = None;
    }

    /// 所有威胁值按 `amount` 衰减；归零的来源移除。
    pub fn decay(&mut self, amount: f64) {
        if !amount.is_finite() || amount <= 0.0 {
            return;
        }
        for entry in &mut self.entries {
            entry.value = (entry.value - amount).max(0.0);
        }
        self.entries.retain(|entry| entry.value > 0.0);
    }

    /// 丢掉某个来源（死亡、退场、被安抚）。
    pub fn drop_source(&mut self, source: Entity) {
        self.entries.retain(|entry| entry.source != source);
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.last_known = None;
    }

    pub fn entries(&self) -> &[ThreatEntry] {
        &self.entries
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// 把已落地的伤害换算成威胁（DsnE29）。
///
/// 读 [`DamageTakenEvent`]（近战/法术/投掷的统一出口），只对带 [`Aggro`] 的受击者
/// 生效——玩家没有该组件，所以玩家被打不会产生仇恨。
pub fn record_threat_system(
    mut damage_taken: MessageReader<DamageTakenEvent>,
    mut aggro: Query<&mut Aggro>,
    positions: Query<&Position>,
) {
    record_hits(&mut damage_taken, &mut aggro, &positions);
}

/// 「记威胁」的纯逻辑：两个系统共享同一实现，避免规则两份。
fn record_hits(
    damage_taken: &mut MessageReader<DamageTakenEvent>,
    aggro: &mut Query<&mut Aggro>,
    positions: &Query<&Position>,
) {
    for event in damage_taken.read() {
        let Some(source) = event.source else {
            continue;
        };
        if source == event.target {
            continue;
        }
        let Ok(mut aggro) = aggro.get_mut(event.target) else {
            continue;
        };
        let damage = if event.amount.is_finite() {
            event.amount.max(0.0)
        } else {
            0.0
        };
        let value = THREAT_PER_HIT.max(damage * THREAT_PER_DAMAGE);
        aggro.record(source, value);
        if let Ok(position) = positions.get(source) {
            aggro.remember(position.to_tuple());
        }
    }
}

/// 按 AV 衰减威胁，并清掉已死来源。
///
/// 与 `WorldClock.last_step_av` 同口径：一轮 tick 走多少 AV，就衰减多少。
/// 死亡清理放在衰减之前、且不受步长影响（0 AV 的轮次也要清）。
pub fn threat_decay_system(
    clock: Res<WorldClock>,
    mut aggro: Query<&mut Aggro>,
    healths: Query<&Health>,
) {
    decay_threat(&clock, &mut aggro, &healths);
}

/// 「衰减 + 清死来源」的纯逻辑。
fn decay_threat(clock: &WorldClock, aggro: &mut Query<&mut Aggro>, healths: &Query<&Health>) {
    for mut aggro in &mut *aggro {
        let dead: Vec<Entity> = aggro
            .entries()
            .iter()
            .filter(|entry| !healths.get(entry.source).is_ok_and(|health| health.is_alive()))
            .map(|entry| entry.source)
            .collect();
        for source in dead {
            aggro.drop_source(source);
        }
    }

    let step = clock.last_step_av;
    if !step.is_finite() || step <= 0.0 {
        return;
    }
    let decay = step / 1000.0 * THREAT_DECAY_PER_1000_AV;
    for mut aggro in &mut *aggro {
        aggro.decay(decay);
    }
}

/// 结算链里的合并系统：先记账、再衰减（DsnE29）。
///
/// 拆成两个系统会让结算链超过 bevy 的 20 元组上限，而且这一步本来没有别的系统
/// 需要插在两者之间——合成一个系统后，"本轮伤害先记、再按同一轮 AV 衰减"的
/// 顺序是函数体内的顺序，不需要调度保证。
pub fn threat_system(
    mut damage_taken: MessageReader<DamageTakenEvent>,
    mut aggro: Query<&mut Aggro>,
    positions: Query<&Position>,
    clock: Res<WorldClock>,
    healths: Query<&Health>,
) {
    record_hits(&mut damage_taken, &mut aggro, &positions);
    decay_threat(&clock, &mut aggro, &healths);
}

#[cfg(test)]
#[path = "aggro_tests.rs"]
mod tests;
