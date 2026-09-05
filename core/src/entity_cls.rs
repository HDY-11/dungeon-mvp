//! 实体范畴与身份标记。
//!
//! 设计约定：
//! - 范畴（enum）回答“这类实体使用哪套公式/生命周期”。
//! - 身份（ZST 组件）回答“这个实体是谁”，用于查询过滤和决定挂载哪些组件。
//! - 一个实体可以没有身份标记，但应至少有一个范畴组件。

use bevy_ecs::prelude::*;

/// 顶层实体范畴：决定实体参与哪类系统。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityClass {
    /// 生物/行动者。
    Actor,
    /// 物品（背包、装备、地面物品）。本轮暂不迁移物品规则，仅预留范畴。
    Item,
    /// Buff/状态效果子实体。本轮暂不迁移，仅预留范畴。
    Buff,
    /// 投射物。
    Projectile,
    /// 区域/地面效果。
    Field,
}

/// 生物范畴：用于同类生物共享的公式与行为。
///
/// 注意：物种级差异（如老鼠与蝎子的成长曲线）仍由 `monster::MonsterKindId`
/// 或具体身份 marker 参与分派；本枚举只表达高层公式族。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreatureKind {
    Humanoid,
    Beast,
    Plant,
    MagicCreature,
    Construct,
    Aquatic,
}

/// 玩家标记。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Player;

/// 怪物通用标记。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Monster;

/// 楼梯标记。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Stairs;

// ── 身份标记（ZST） ──────────────────────────────────
// 身份标记用于“它是谁”；不承载数值。需要物种数据时查 `monster::MonsterTemplate`。

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rat;

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Scorpion;

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Goblin;

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Sporeling;

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MushroomGolem;

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CaveFish;

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CaveCrab;

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DeepEel;
