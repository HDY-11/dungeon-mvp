//! 生命/法力上限的**唯一公式**（DESIGN DsnE23）。
//!
//! # 为什么上限要可重算
//!
//! 上限是派生数据：`max = 等级曲线 + 装备/附魔的修正`。它**不进存档**，
//! 读档后按当前规则重算——否则以后改曲线或加一件 +生命上限的装备时，
//! 老档里的 `max` 会与新规则撕裂（DsnE23 拍板的 V3 口径）。
//!
//! # 单一公式、一处夹取
//!
//! [`max_health`] / [`max_mana`] 是唯一算上限的纯函数，折叠走
//! [`crate::rules::apply_modifiers`]（所以 `max_health` 修正与其它属性共用同一套
//! `Add / MultiplyBase / MultiplyTotal` 语义）。[`apply_maxima`] 是唯一的"写上限"
//! 函数：
//!
//! - 普通刷新（装备变化、读档、每轮兜底）只把 `current` 夹到上限内，**不回血**；
//! - 升级时传 `restore = true`，回满 HP/MP（存活者的既有行为，DsnE23 确认）。
//!
//! 防御不再顺带加血：防御只做减伤，血量上限只看等级曲线与显式修正。

use bevy_ecs::prelude::*;
use bevy_ecs::system::RunSystemOnce;

use crate::components::{Health, Level, Mana};
use crate::effect::EffectContext;
use crate::entity_cls::Player;
use crate::rules::{Modifier, StatId, apply_modifiers};

/// 等级 → 生命上限的基础曲线：Lv1 40，每级 +8（Lv10 = 112）。
///
/// 曲线是**内容数值**，调它不需要改任何系统；`GAME.md` 是玩家口径的权威。
pub fn hp_curve(level: u64) -> f64 {
    40.0 + 8.0 * level.saturating_sub(1) as f64
}

/// 等级 → 法力上限的基础曲线：Lv1 12，每级 +2（Lv10 = 30）。
pub fn mp_curve(level: u64) -> f64 {
    12.0 + 2.0 * level.saturating_sub(1) as f64
}

/// 生命上限 = 等级曲线 + `max_health` 修正（至少 1，避免零血上限）。
pub fn max_health(level: u64, modifiers: &[Modifier]) -> f64 {
    apply_modifiers(StatId::MAX_HEALTH, hp_curve(level), modifiers, &[]).max(1.0)
}

/// 法力上限 = 等级曲线 + `max_mana` 修正（至少 1）。
pub fn max_mana(level: u64, modifiers: &[Modifier]) -> f64 {
    apply_modifiers(StatId::MAX_MANA, mp_curve(level), modifiers, &[]).max(1.0)
}

/// 把上限写进生命/法力组件（[`max_health`] / [`max_mana`] 的唯一调用口）。
///
/// - `restore = false`：只夹取 `current`（装备变化/读档/每轮兜底）；
/// - `restore = true`：回满（升级与出生）。
///
/// 不回血这件事是刻意的：把 +生命上限的装备穿上再脱下，不能变成免费治疗。
pub fn apply_maxima(
    level: u64,
    modifiers: &[Modifier],
    health: &mut Health,
    mana: &mut Mana,
    restore: bool,
) {
    health.max = max_health(level, modifiers);
    mana.max = max_mana(level, modifiers);
    if restore {
        health.current = health.max;
        mana.current = mana.max;
    } else {
        health.current = health.current.min(health.max);
        mana.current = mana.current.min(mana.max);
    }
}

/// 每轮结算的兜底刷新：等级/装备变化一个系统周期内没被即时刷新时，这里补上。
///
/// 只处理玩家：怪物血量来自模板（`MonsterStats`），不走等级曲线。
/// 升级回满不在这里做——`apply_exp_system` 升级时直接带 `restore = true` 调
/// [`apply_maxima`]，这里只做"夹取"，避免系统之间争谁是写者。
pub fn refresh_vitals_system(
    effects: EffectContext,
    mut players: Query<(Entity, &Level, &mut Health, &mut Mana), With<Player>>,
) {
    for (entity, level, mut health, mut mana) in players.iter_mut() {
        let modifiers = effects.collect_actor(entity);
        apply_maxima(level.0, &modifiers, &mut health, &mut mana, false);
    }
}

/// 立刻按当前规则重算玩家上限（装备/卸下/读档等不进结算链的操作调用它）。
///
/// 内部跑一次 [`refresh_vitals_system`]，所以"算上限"只有一份实现。
/// 失败（世界里没有玩家）时静默返回：调用点都是玩家相关动作，没有玩家时
/// 本身就已经走不到的路径。
pub fn refresh_player_vitals(world: &mut World) {
    let _ = world.run_system_once(refresh_vitals_system);
}

#[cfg(test)]
#[path = "vitals_tests.rs"]
mod tests;

