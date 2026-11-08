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
//! - `restore = true` 只给"出生满池"这类一次性初始化用；**升级不再回满**
//!   （DsnE25 决策 2 取代了 DsnE23 的"升级回满"）。
//!
//! 生命回复只有两类来源（DsnE25）：消耗品与治疗泉；法力按 AV 回复见
//! [`mana_regen_system`]。上限公式本身不负责回复。

use bevy_ecs::prelude::*;
use bevy_ecs::system::RunSystemOnce;

use crate::balance::MANA_REGEN_AV;
use crate::components::{Health, Level, Mana, ManaRegen};
use crate::effect::EffectContext;
use crate::entity_cls::Player;
use crate::resources::WorldClock;
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
/// - `restore = false`：只夹取 `current`（装备变化/读档/升级/每轮兜底）；
/// - `restore = true`：回满（只给出生这类一次性初始化）。
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
/// 升级回满**不在这里做**——DsnE25 起升级也不回满，`apply_exp_system` 只调
/// `apply_maxima(..., restore = false)`；这里同样只做"夹取"，避免系统之间争谁是写者。
pub fn refresh_vitals_system(
    effects: EffectContext,
    mut players: Query<(Entity, &Level, &mut Health, &mut Mana), With<Player>>,
) {
    for (entity, level, mut health, mut mana) in players.iter_mut() {
        let modifiers = effects.collect_actor(entity);
        apply_maxima(level.0, &modifiers, &mut health, &mut mana, false);
    }
}

/// 法力随时间回复（DsnE25 决策 1）：每累计 [`MANA_REGEN_AV`] AV 回 1 点。
///
/// 只在存活时累计；MP 已满时把进度清零——**不攒爆发**：否则玩家可以挂机攒满一大截，
/// 开战后连续施法，等于把"随时间回"变成"开局送蓝"。进度不足 1 点的零头保留，
/// 所以回复节奏不会因为一次行动跨了多轮而丢失。
///
/// 死亡（`Health` 已空）不回复：尸体不该回蓝，且读档/结算链里这条路径本身也可达。
pub fn mana_regen_system(
    clock: Res<WorldClock>,
    mut players: Query<(&Health, &mut Mana, &mut ManaRegen), With<Player>>,
) {
    if clock.last_step_av <= 0.0 {
        return;
    }
    for (health, mut mana, mut regen) in players.iter_mut() {
        if !health.is_alive() {
            continue;
        }
        if mana.current >= mana.max {
            regen.progress_av = 0.0;
            continue;
        }
        regen.progress_av += clock.last_step_av;
        let points = (regen.progress_av / MANA_REGEN_AV).floor();
        if points < 1.0 {
            continue;
        }
        let restored = mana.restore(points);
        // 只扣掉真正换到的点数；满池时多余的进度直接丢弃（不攒爆发）。
        regen.progress_av -= restored * MANA_REGEN_AV;
        if mana.current >= mana.max {
            regen.progress_av = 0.0;
        }
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

