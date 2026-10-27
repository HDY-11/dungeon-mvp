//! 最小物品形态（C4）：**地面上的一件装备**。
//!
//! # 这不是物品系统
//!
//! 物品注册表 / 背包 / 掉落表都没迁（REFACTOR §2.2 R2）。这里只做能验证架构的那一小块：
//! 一件装备存在于世界上 → 玩家走过去捡起来 → 它**开始生效**。三个环节各自验证一件我们
//! 花过设计成本的东西：
//!
//! | 环节 | 验证什么 |
//! |---|---|
//! | 地面物品用 [`GroundItemAt`] 而不是 `Position` | 复用通用组件 = 加入别人写的查询（本项目第三次踩：`ChildOf`、`Position`、现在这个） |
//! | 捡起后挂 [`EquippedBy`](crate::effect::EquippedBy) 关系 | 装备与状态效果的**级联语义相反**（ECS42），两个关系类型各管一边 |
//! | 装备授予的效果实例化到 actor 名下 | 求值层只看 `EffectOf` 那一组，所以"装备带来什么"必须**翻译成效果实体** |
//!
//! 第三条是设计里一直挂着的缝（`effect/collect.rs` 的注释写着"这条缝故意留着不做"）。
//! C4 把它补上了，但只补到"捡起即生效"为止——**没有卸下**，所以没有"效果随卸载消失"
//! 这条路径。等物品系统迁移时，正确的形状是：装备的 owner 系统按"已装备列表"维护对应的
//! 效果实体集合（增删都归它管，DsnE12 第 4 条）。

use crate::components::*;
use crate::effect::{EffectOf, EquippedBy, IgnoresSource};
use crate::entity_cls::Player;
use crate::resources::{EventLog, EventMessage};
use crate::rules::EffectSource;
use bevy_ecs::prelude::*;

/// 物品种类。当前只有一种——**加第二种物品要先想清楚"它授予什么效果"**，
/// 所以枚举与 [`ItemKind::granted_sources`] 成对出现，让"加物品"必然回答那个问题。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    /// 踏水靴：无视**地形**来源的移动代价（泥潭、浅水都对它无效）。
    TrailBoots,
}

impl ItemKind {
    /// 显示名（日志用）。
    pub fn name(self) -> &'static str {
        match self {
            ItemKind::TrailBoots => "踏水靴",
        }
    }

    /// 这件装备授予的效果：被无视的来源桶列表。
    ///
    /// 返回"桶"而不是具体修正，是因为求值层只认桶（DsnE10）。所以装备不需要知道
    /// "泥潭会让我变慢 2 倍"——它只知道"无视地形这一类"。
    pub fn granted_sources(self) -> &'static [EffectSource] {
        match self {
            ItemKind::TrailBoots => &[EffectSource::Terrain],
        }
    }
}

/// 一件物品（地面上的、装备中的，都是它）。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct GroundItem {
    pub kind: ItemKind,
}

impl GroundItem {
    pub const fn new(kind: ItemKind) -> Self {
        Self { kind }
    }
}

/// 物品所在的格子。**专用组件**，理由同 `CellEffectAt`：`rebuild_occupancy_system` 会把
/// 任何带 `Position` 的实体当作"占着格子"，地上的东西不能让玩家走不进去。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct GroundItemAt {
    pub x: usize,
    pub y: usize,
}

impl GroundItemAt {
    pub const fn new(x: usize, y: usize) -> Self {
        Self { x, y }
    }

    pub fn to_tuple(self) -> (usize, usize) {
        (self.x, self.y)
    }
}

/// 玩家踩到物品就捡起并**立即装备**（没有背包，所以没有"存起来"这一步）。
///
/// 三条命令的顺序有意义：先让物品离地，再挂装备关系，最后实例化它授予的效果。
/// 全部走 `Commands`，因此同一轮里不会出现"半装备"状态被别的系统读到。
pub fn pickup_system(
    mut commands: Commands,
    players: Query<(Entity, &Position), With<Player>>,
    items: Query<(Entity, &GroundItemAt, &GroundItem)>,
    mut event_log: ResMut<EventLog>,
) {
    let Ok((player, position)) = players.single() else {
        return;
    };

    for (item, at, ground) in &items {
        if at.to_tuple() != position.to_tuple() {
            continue;
        }
        commands.entity(item).despawn();
        commands.spawn((EquippedBy(player), GroundItem::new(ground.kind)));
        for source in ground.kind.granted_sources() {
            commands.spawn((EffectOf(player), IgnoresSource(*source)));
        }
        event_log.push(EventMessage::item(format!("你捡起了{}", ground.kind.name())));
        log::info!("拾取并装备: {:?}", ground.kind);
    }
}

#[cfg(test)]
#[path = "item_tests.rs"]
mod tests;
