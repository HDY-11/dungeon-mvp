//! 物品：**一件物品 = 一个实体**；装备/背包装在**容器**里（DESIGN DsnE16 / DsnE18）。
//!
//! # 四件事、四种存法
//!
//! | 部件 | 存法 | 判据 |
//! |---|---|---|
//! | 物品定义 | 数据文件 `assets/items.json`（内嵌 + 加载校验，[`registry::ITEMS`]） | 数据、无中央分派 |
//! | 物品实例 | 实体 + [`Item`] + [`Amount`] + 0..n 个实例组件 | 需要身份与实例态 |
//! | 所在位置 | 地上挂 [`GroundItemAt`]；**进容器时必须摘掉** | 位置是"在哪"，不是"是什么" |
//! | 装备 / 背包 | 都是 [`Container`]：身体（玩家身上）+ 行囊（一件物品） | 一组带过滤的槽位 |
//!
//! # 装备 = 身体槽位里的物品
//!
//! 没有 `EquippedBy`、没有 `EquipSlot`：**部位由 `BODY` 的槽位给出**，物品带
//! [`ItemTag`] 说明自己能进哪类部位，两边求交。于是"穿上"= 移进身体某个槽位，
//! "卸下"= 从槽位移出；求值侧（`effect::collect`）遍历身体槽位读物品定义。
//!
//! 行囊是**一件物品**（`ItemKind::Pack`），被塞在身体的「背包」槽里；它自己带一个
//! `Container`。所以"背包也是容器"不是特例，而是同一条规则的另一个实例。
//!
//! # 容器里没有位置
//!
//! 物品被收进容器时**必须去掉 `GroundItemAt`**：还带着"我在 (x,y)"就是在说谎，
//! 而渲染抽取、拾取、存档都会按那个谎言行动。
//!
//! # 堆叠
//!
//! [`Amount`] 是"这一堆有几个"，所以"一堆血肉 ×3"是**一个实体**；容器一个槽位放一个实体，
//! 槽位容量（`SlotDef::capacity`）约束数量上限。
//!
//! # 拾取口径
//!
//! **踩上去不会自动捡**（DsnE21 阶段 4）：玩家在背包页的「地面」栏按 `Enter`
//! 拿一件（`actions::take_from_ground`），每按一次走一次下面的路由：
//!
//! | 捡到什么 | 做什么 |
//! |---|---|
//! | 装备且对应部位空着 | 进身体槽位（= 穿上） |
//! | 装备但部位已占 | 进行囊 |
//! | 消耗品且对应资源未满 | **立即使用 1 个**，剩下的留在原地 |
//! | 其他 | 进行囊（先并堆、再开新格） |
//! | 行囊超重 | 与"满"同一条口径：**留在原地**并记一条事件（DsnE20） |
//! | 放不下 | **留在原地**并记一条事件（不许静默失败，ISSUES SYN6） |
//!
//! **尸体也不自动搜刮**：踩上尸体什么都不会发生，玩家打开背包页在「脚下」面板里
//! 自己翻（`actions::take_from_corpse` 的注释记录了这条玩家决策）。

pub mod actions;
pub mod container;
pub mod loot;
pub mod registry;

#[cfg(test)]
use crate::resources::EventLog;
use bevy_ecs::prelude::*;

pub use container::{
    BODY, CORPSE, Container, ContainerDef, InsertError, ItemTag, SlotAccept, SlotDef, TRAVELER_PACK,
};
pub use actions::{
    ItemActionError, drop_from_pack, equip_from_pack, take_from_corpse, take_from_ground,
    unequip_to_pack, use_from_pack,
};
pub use loot::{LootEntry, roll_loot};
pub use registry::{
    Behavior, BehaviorEffect, BehaviorTrigger, ITEMS, ITEMS_SCHEMA_VERSION, ItemDef, ItemId,
    ItemKind, TRAVELER_PACK_ITEM, item_by_name, stack_weight,
};

/// 一件物品（实体上的身份组件）：它是**哪一件**东西。
///
/// 只装编号：属性在 `ItemDef`（静态）或这个实体上的其它组件（实例态）。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Item {
    pub id: ItemId,
}

impl Item {
    pub const fn new(id: ItemId) -> Self {
        Self { id }
    }

    /// 这一件物品的定义。
    pub fn def(self) -> &'static ItemDef {
        self.id.def()
    }

    /// 它能进哪些部位的槽位（DsnE18 的过滤词汇）。
    pub fn tags(self) -> &'static [ItemTag] {
        self.def().tags
    }
}

/// 这一堆有几个（`1` = 不堆叠的单个物品）。
///
/// 数量归零时实体应当被销毁，**不留** `Amount(0)` 空壳：空壳会让"这里有没有东西"
/// 变成"看 Amount"，那就是第二个真相。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Amount(pub u32);

impl Amount {
    pub const fn new(count: u32) -> Self {
        Self(if count == 0 { 1 } else { count })
    }

    /// 还能再堆几个（按物品的 `max_stack`）。
    pub fn room(self, def: &ItemDef) -> u32 {
        def.max_stack.saturating_sub(self.0)
    }
}

/// 物品所在的格子。
///
/// **专用组件**（不用 `Position`）：地上的东西不该被"位置 = 占格 = 会动"那一套语义牵连。
/// 收进容器时**必须移除**它。
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

/// 物品实例的耐久（实例数据的第一项，DsnE22 阶段 ④）。
///
/// 只出现在蓝图声明了 `durability_max` 的物品上（稀疏组件）：模板装配时插满，
/// 存档只保存"与蓝图不同的部分"（当前值）。磨损规则尚未接入——本阶段先把
/// "实例数据进得了档、读档后与模板装配一致"这条地基落下来。
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Durability {
    pub current: f32,
    pub max: f32,
}

impl Durability {
    /// 满耐久（模板装配时用）。
    pub fn new(max: f32) -> Self {
        let max = max.max(0.0);
        Self { current: max, max }
    }

    /// 带当前值（读档套用 overrides 时用）：当前值夹在 `[0, max]`。
    pub fn with_current(current: f32, max: f32) -> Self {
        let max = max.max(0.0);
        Self {
            current: if current.is_finite() {
                current.clamp(0.0, max)
            } else {
                max
            },
            max,
        }
    }

    /// 当前值占上限的比例；上限为 0 时按"满"处理。
    pub fn ratio(self) -> f32 {
        if self.max <= 0.0 {
            1.0
        } else {
            self.current / self.max
        }
    }
}

/// 物品实体装配器：让系统里的 `Commands` 与读档时的 `World` 共用同一段装配代码。
///
/// 不写成两个 `spawn_item` 是因为"物品蓝图 → 实体组件"必须只有一条实现
/// （DsnE22：装配是蓝图与 ECS 之间的唯一桥）；读档走 `World` 只是为了绕开
/// `Commands` 的生命周期。
pub trait ItemSpawner {
    /// 只造基础组件（`Item + Amount`）。
    fn spawn_item_base(&mut self, id: ItemId, count: u32) -> Entity;
    /// 按蓝图补上可选组件（容器布局、满耐久……）。
    fn insert_blueprint_parts(&mut self, entity: Entity, id: ItemId);
}

impl ItemSpawner for Commands<'_, '_> {
    fn spawn_item_base(&mut self, id: ItemId, count: u32) -> Entity {
        self.spawn((Item::new(id), Amount::new(count))).id()
    }

    fn insert_blueprint_parts(&mut self, entity: Entity, id: ItemId) {
        let def = id.def();
        if let Some(layout) = def.container_def() {
            self.entity(entity).insert(Container::new(layout));
        }
        if let Some(max) = def.durability_max {
            self.entity(entity).insert(Durability::new(max));
        }
    }
}

impl ItemSpawner for World {
    fn spawn_item_base(&mut self, id: ItemId, count: u32) -> Entity {
        self.spawn((Item::new(id), Amount::new(count))).id()
    }

    fn insert_blueprint_parts(&mut self, entity: Entity, id: ItemId) {
        let def = id.def();
        if let Some(layout) = def.container_def() {
            self.entity_mut(entity).insert(Container::new(layout));
        }
        if let Some(max) = def.durability_max {
            self.entity_mut(entity).insert(Durability::new(max));
        }
    }
}

/// 按蓝图装配一件物品实体（唯一入口）。
pub fn spawn_item_assembled<S: ItemSpawner>(spawner: &mut S, id: ItemId, count: u32) -> Entity {
    let entity = spawner.spawn_item_base(id, count);
    spawner.insert_blueprint_parts(entity, id);
    entity
}

/// 造一件物品实体（不含位置）：`Item + Amount` + 蓝图声明的实例组件。
///
/// "物品出现"的**唯一入口**（地上、容器里都走它）：于是"物品实体一定带 `Item + Amount`"
/// 与"行囊一定带 `Container`"都是结构保证。读档走 [`spawn_item_assembled`] 的 `World` 实现。
pub fn spawn_item(commands: &mut Commands, id: ItemId, count: u32) -> Entity {
    spawn_item_assembled(commands, id, count)
}

/// 在地上放一堆物品，返回实体。
///
/// 地面物品挂 [`crate::components::Observable`]（DsnE21 阶段 3）：它们能产生
/// "你捡起了 X"这类事件，是信息栏候选；容器里的物品没有位置，不挂。
pub fn spawn_ground_item(
    commands: &mut Commands,
    id: ItemId,
    count: u32,
    at: (usize, usize),
) -> Entity {
    let entity = spawn_item(commands, id, count);
    commands
        .entity(entity)
        .insert((GroundItemAt::new(at.0, at.1), crate::components::Observable));
    entity
}

/// 身体上「背包」那一个槽位的下标（由 `BODY` 的过滤表推出，不手写常数）。
pub fn pack_slot_index() -> usize {
    BODY.slots
        .iter()
        .position(|slot| slot.accept.accepts(&[ItemTag::Pack]))
        .expect("BODY 必须有能装背包的槽位")
}

/// 容器当前总重（系统查询版）。
///
/// 槽位判"装得下"、载重判"装不装得动"，两个判据合起来才是可放置；
/// 动作函数那边用 `&World` 的版本（`item::actions` 内部），逻辑同一份。
pub fn container_weight(container: &Container, items: &Query<(&Item, &Amount)>) -> f32 {
    container.total_weight(|entity| {
        items
            .get(entity)
            .map(|(item, amount)| stack_weight(item.id, amount.0))
            .unwrap_or(0.0)
    })
}

#[cfg(test)]
#[path = "item_tests.rs"]
mod tests;
