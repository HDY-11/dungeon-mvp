//! 效果：**载体侧**（效果活在哪里、由谁终结）+ **收桶**（求值第 ①② 步）。
//! Phase H9 / H11。
//!
//! 求值侧的第 ③④⑤ 步（过滤 / 折叠 / 纯函数）在 `rules::modifier`（DsnE10）；
//! 本模块只回答"效果存在哪、从哪读出来"，不解释任何具体规则。
//!
//! # 三个落点（DsnE12 定案，2026-09）
//!
//! | 落点 | 载体 | 拥有者消失时 |
//! |---|---|---|
//! | actor 拥有的持续效果（buff、技能授予） | [`EffectOf`] / [`OwnedEffects`] 子实体 | **级联**消失（`linked_spawn`） |
//! | 装备 | [`EquippedBy`] / [`EquippedItems`] 子实体 | **存活**（不级联，由 owner 系统改写为"掉在地上"） |
//! | 格子拥有的效果（泥潭、冻结块） | 每格**一个**世界级实体 + [`CellEffects`] 格索引 | 与 actor 无父子关系 |
//!
//! 三者都在**世界实体空间**里，因此存档可以统一枚举（DsnE12 第 6 条）；
//! 而 `EffectContext` 是唯一把这三处读成纯数据的地方（ISSUES ECS28 的那条接缝）。
//!
//! # 本轮的两个具体效果（H11 最小闭环）
//!
//! DsnE12 明确"不定义任何具体效果类型"——效果只是位。H11 破例**一对**，因为
//! "格子侧减速 × actor 侧无视"必须真的跑通一次才能证明机制成立：
//!
//! | 效果 | 侧 | 作用 |
//! |---|---|---|
//! | [`CellMoveCost`] | 格子 | 贡献 `Terrain` 桶的速度乘区（`2.0` = 速度减半 → AV 翻倍） |
//! | [`IgnoresSource`] | actor | 让求值第 ③ 步丢掉指定来源桶（"无视地形减速"） |
//!
//! 这一对的验收看 `effect_tests.rs`：同一格、同一 actor，**没有** `IgnoresSource` 时
//! AV 是平地两倍，**有**时回到平地值——而规则层（`rules::modifier`）一行没改。

pub mod cell;
pub mod collect;
// `ownership` 不设为 `pub mod`：`action` 已有一个同名子模块，两个 `pub use X::*`
// 会在 `lib.rs` 的 `pub use effect::*` 处撞成 `ambiguous_glob_reexports`
// （模块名不是 API —— 四个关系类型才是）。所以这里只转出类型。
mod ownership;

pub use cell::{
    CellAttributes, CellEffectAt, CellEffects, CellMoveCost, cell_attributes, cell_speed_modifier,
    rebuild_cell_effects_system, speed_multiplier_for, tile_at,
};
pub use collect::{CollectedModifiers, EffectContext};
pub use ownership::{EffectOf, EquippedBy, EquippedItems, OwnedEffects};

use crate::rules::EffectSource;
use bevy_ecs::prelude::*;

/// "无视某一类来源的修正"（H11 最小闭环的 **actor 侧**）。
///
/// 挂在效果实体上（经 [`EffectOf`] 归属到 actor）。装备提供的无视要由装备的
/// owner 系统转成一条效果——物品系统未迁移，这条缝留到 R2/S4。
///
/// 它是**数据**而不是规则分支：`rules::modifier::apply_modifiers` 收到的是
/// `ignored: &[EffectSource]`，规则里永远不会出现"若带了某装备则……"。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct IgnoresSource(pub EffectSource);

impl IgnoresSource {
    /// 本地形减速的"踏水靴"用的那一条（最小闭环的固定取值）。
    pub const TERRAIN: Self = Self(EffectSource::Terrain);

    /// 是否无视这个来源。
    pub fn covers(&self, source: EffectSource) -> bool {
        self.0 == source
    }
}

#[cfg(test)]
#[path = "effect_tests.rs"]
mod tests;
