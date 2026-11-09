//! 容器与槽位：**身体 / 行囊 / 尸体 / 箱子都是"带过滤的槽位集合"**（DESIGN DsnE18）。
//!
//! # 两个概念，两种存法
//!
//! | 概念 | 存法 | 判据（DsnE13） |
//! |---|---|---|
//! | **容器布局**（有几个槽、各接什么） | 静态 `ContainerDef` | 内容：同类容器布局相同，不需要身份 |
//! | **容器当前内容**（哪个槽有谁） | 组件 `Container { slots: Vec<Option<Entity>> }` | 运行时状态 |
//!
//! 槽位**不做成实体**：一个 27 格箱子就是 27 个实体，而槽位除了"这个位置接什么 + 现在有谁"
//! 之外没有独立身份。要"给槽位挂组件"（按部位受伤等）时再加一份并行数组，别为它付实体成本。
//!
//! # 过滤：两份同源词汇求交
//!
//! 槽位声明它**接受哪些标签**（[`SlotAccept`]），物品从自己的定义里带来**自己是什么标签**
//! （[`ItemDef::tags`](crate::item::ItemDef::tags)），插入时求交。**不能**做成"槽位带一个过滤组件、
//! 物品带同种组件，然后问物品有没有这个组件"：Rust 没有把组件类型存成值再查询的能力（除非引入
//! `Reflect`），那样只能退化成 `match 槽位种类` —— 中央分派，正是本项目一直在消灭的形状。
//!
//! # 「满」是**派生**，不是权威标记
//!
//! [`Container::free_slot_for`] 是唯一判据。为什么不给槽位挂一个 `Full` 标记：
//!
//! 1. 它与"这个槽有谁"是**同一个事实的两种写法**，权威标记 = 第二个真相（ISSUES ECS25 / ECS42）；
//! 2. "满"依赖**要放进什么**：有过滤的容器对头盔可能满、对靴子却空着，一个静态标记表达不了。
//!
//! 将来若需要"快速筛空容器"（UI 灰显 / AI 搜刮），做法是**由系统重建**一份派生标记
//! （照 `OccupancyMap` / `CellEffects` 的先例），而不是手维护。
//!
//! # 权威只有一处：**容器格子**
//!
//! 物品**不挂**"我在哪个容器"的反向关系：谁在这里，只由 `Container.slots` 记录。
//! 反向关系（如 `ContainedIn`）会立刻产生第二个真相，而且删除/转移的每条路径都要对称维护。
//! 需要"某件物品在哪个容器"时用查询（遍历容器）得到——容器数量是十来级，不是上千。
//!
//! # 环形包含
//!
//! 容器可以是**物品**（行囊就是），于是"把行囊塞进自己"是可能的。本模块只管槽位（纯数据），
//! 环检查属于操作容器的系统：转移前必须确认"目标容器不是来源容器自己的子孙"
//! （`world::transfer::would_nest` 一类函数），否则会出现无法遍历、也无法保存的结构。

use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};

/// 物品的**可装位标签**：回答"这件东西能装进哪类槽位"。
///
/// 与 [`ItemPart`](crate::item::ItemPart) 是两件事：部件回答"它是什么、怎么用"，
/// 本枚举回答"它能进哪个部位"。战士的剑与法杖都是 `Weapon`，但用法不同。
///
/// **只增不改**：标签是数据，改一个旧值等于让所有旧存档里的物品换了个部位。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemTag {
    /// 头部（头盔、帽子）。
    Head,
    /// 躯干（甲、袍）。
    Chest,
    /// 主手（武器）。
    Weapon,
    /// 副手（盾、副手武器）。
    Shield,
    /// 脚（鞋、靴）。
    Feet,
    /// 手指（戒指）。
    Ring,
    /// 背包类（行囊本身就是一件能装进身体「背包」槽的物品）。
    Pack,
}

/// 一个槽位**接受什么**。
///
/// `Any` 是"什么都收"（行囊、尸体、箱子的格子）；`Only` 是白名单（身体各部位）。
/// 用 `&'static [ItemTag]` 而不是 `Vec`：布局是编译期内容，运行期只读。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotAccept {
    /// 任何物品都可以放。
    Any,
    /// 只接受带这些标签之一的物品。
    Only(&'static [ItemTag]),
}

impl SlotAccept {
    /// 这件物品（按它的标签）能不能放进这个槽位。
    ///
    /// 求交：物品有任何一枚标签在槽位的白名单里就成立。空标签的物品只能进 `Any`。
    pub fn accepts(&self, item_tags: &[ItemTag]) -> bool {
        match self {
            SlotAccept::Any => true,
            SlotAccept::Only(accepted) => item_tags.iter().any(|tag| accepted.contains(tag)),
        }
    }

    /// 给日志/UI 的一句话。
    pub fn describe(&self) -> String {
        match self {
            SlotAccept::Any => "任意物品".to_string(),
            SlotAccept::Only(tags) => {
                let names: Vec<&str> = tags.iter().map(|tag| tag.name()).collect();
                names.join("/")
            }
        }
    }
}

impl ItemTag {
    /// 显示名（日志与 UI 用）。
    pub fn name(self) -> &'static str {
        match self {
            ItemTag::Head => "头",
            ItemTag::Chest => "躯干",
            ItemTag::Weapon => "主手武器",
            ItemTag::Shield => "副手",
            ItemTag::Feet => "脚",
            ItemTag::Ring => "手指",
            ItemTag::Pack => "背包",
        }
    }
}

/// 一个槽位的定义：叫什么、收什么、能装几个。
///
/// **容量现在就写成字段**（今天恒为 1）：将来引入"重量/堆叠"时，`Full` 的判据从
/// "这个格子有谁"变成"占用 ≥ 容量"，改数据即可、不必改形状。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotDef {
    pub name: &'static str,
    pub accept: SlotAccept,
    pub capacity: u32,
}

impl SlotDef {
    /// 单个槽位、指定接受范围的构造器（最常用）。
    pub const fn one(name: &'static str, accept: SlotAccept) -> Self {
        Self {
            name,
            accept,
            capacity: 1,
        }
    }

    /// 什么都收的单槽（行囊/尸体/箱子的格子）。
    pub const fn any(name: &'static str) -> Self {
        Self::one(name, SlotAccept::Any)
    }
}

/// 一类容器的**布局**（静态内容表）。
///
/// 类型 + 名字 + 槽位表 + 载重上限。加一类容器 = 加一个常量；加一个槽位 = 在该常量的槽位列表里加一行。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContainerDef {
    pub name: &'static str,
    pub slots: &'static [SlotDef],
    /// 这个容器最重能装多少（`None` = 不限重）。
    ///
    /// 行囊有限重、身体与尸体不限：装备的重量不该让玩家"脱不下来"
    /// （重量将来还要参与移动速度，那时再由平衡表决定）。
    pub max_weight: Option<f32>,
}

impl ContainerDef {
    /// 槽位数量。
    pub const fn len(&self) -> usize {
        self.slots.len()
    }

    /// 是不是空容器（0 槽）。
    pub const fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }
}

// ── 容器布局（内容） ─────────────────────────────────

/// **身体**：装备挂在这里，一个部位一件。
///
/// 取代旧的 `EquippedBy` + `EquippedItem.slot`（见 DsnE18 决策 1 待拍板）：槽位由这张表给出，
/// 物品不再自己声明"我占的是主手还是副手"。
pub const BODY: ContainerDef = ContainerDef {
    name: "身体",
    max_weight: None,
    slots: &[
        SlotDef::one("头", SlotAccept::Only(&[ItemTag::Head])),
        SlotDef::one("躯干", SlotAccept::Only(&[ItemTag::Chest])),
        SlotDef::one("主手", SlotAccept::Only(&[ItemTag::Weapon])),
        SlotDef::one("副手", SlotAccept::Only(&[ItemTag::Shield])),
        SlotDef::one("脚", SlotAccept::Only(&[ItemTag::Feet])),
        SlotDef::one("手指", SlotAccept::Only(&[ItemTag::Ring])),
        SlotDef::one("背包", SlotAccept::Only(&[ItemTag::Pack])),
    ],
};

/// **旅者行囊**：没有过滤的 12 格。玩家开局就带着（作为一件物品放进身体的「背包」槽）。
pub const TRAVELER_PACK: ContainerDef = ContainerDef {
    name: "旅者行囊",
    max_weight: Some(20.0),
    slots: &[
        SlotDef::any("格 1"),
        SlotDef::any("格 2"),
        SlotDef::any("格 3"),
        SlotDef::any("格 4"),
        SlotDef::any("格 5"),
        SlotDef::any("格 6"),
        SlotDef::any("格 7"),
        SlotDef::any("格 8"),
        SlotDef::any("格 9"),
        SlotDef::any("格 10"),
        SlotDef::any("格 11"),
        SlotDef::any("格 12"),
    ],
};

/// **尸体**：怪物倒下后留在原地的一具容器（DsnE18 第②片）。
///
/// 只装它**身上的装备**；战利品仍按 Gm8 掉在地上（用户选的 (a) 档：改动最小、
/// 之后要改成"全部进尸体"只需把 `death` 里的地面掉落搬进这里）。
/// 四个格子够用：现在一只怪最多带一件，留出余量给将来的"多件装备/战利品"。
///
/// 尸体**不占格**（没有 `Blocks`），所以踩上去不会挡路；搜刮在拾取系统里做。
pub const CORPSE: ContainerDef = ContainerDef {
    name: "尸体",
    max_weight: None,
    slots: &[
        SlotDef::any("遗留 1"),
        SlotDef::any("遗留 2"),
        SlotDef::any("遗留 3"),
        SlotDef::any("遗留 4"),
    ],
};

/// 往槽位里放东西失败的原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertError {
    /// 槽位号越界（布局只有 N 个槽）。
    NoSuchSlot(usize),
    /// 该槽位已经有人了（`capacity == 1` 时即"满"）。
    Occupied,
    /// 物品的标签不被这个槽位接受。
    Filtered,
}

/// 一个**容器实例**：按布局开好的槽位，每个槽位至多一件物品（一件物品 = 一个实体）。
///
/// 物品是实体（DsnE18 决策「物品实体化」）：于是"实例数据"就是物品实体上的组件
/// （耐久、附魔…），不需要一个 `ItemData` 大结构，也不会出现"数据堆 + 实体"两种表示。
#[derive(Component, Debug, Clone, PartialEq)]
pub struct Container {
    def: &'static ContainerDef,
    slots: Vec<Option<Entity>>,
}

impl Container {
    /// 按布局开一个空容器。
    pub fn new(def: &'static ContainerDef) -> Self {
        Self {
            def,
            slots: vec![None; def.slots.len()],
        }
    }

    /// 布局。
    pub fn def(&self) -> &'static ContainerDef {
        self.def
    }

    /// 槽位数量。
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// 是否没有任何槽位（0 格容器）。
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// 已占用槽位数。
    pub fn used(&self) -> usize {
        self.slots.iter().flatten().count()
    }

    /// 所有**非空**槽位：`(槽位号, 物品实体)`，按槽位顺序（UI 需要稳定顺序）。
    pub fn iter(&self) -> impl Iterator<Item = (usize, Entity)> + '_ {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(index, slot)| slot.map(|item| (index, item)))
    }

    /// 第 `index` 个槽位里是谁。
    pub fn slot(&self, index: usize) -> Option<Entity> {
        self.slots.get(index).copied().flatten()
    }

    /// 这个槽位空着吗。
    pub fn is_slot_free(&self, index: usize) -> bool {
        self.slots.get(index).is_some_and(Option::is_none)
    }

    /// 哪件物品在容器里（返回槽位号）。
    pub fn slot_of(&self, item: Entity) -> Option<usize> {
        self.slots.iter().position(|slot| *slot == Some(item))
    }

    /// 这件物品在这个容器里吗。
    pub fn contains(&self, item: Entity) -> bool {
        self.slot_of(item).is_some()
    }

    /// **「满」的唯一判据**：找第一个能接住这些标签的空槽位。
    ///
    /// 找不到 = 对这件物品而言已经满了。注意这是**相对于物品**的判断：有过滤的容器
    /// 可能对头盔满、对靴子空着——这正是"满"不能做成静态标记的原因（DndE18 评估第 4 条）。
    pub fn free_slot_for(&self, item_tags: &[ItemTag]) -> Option<usize> {
        self.slots.iter().enumerate().find_map(|(index, slot)| {
            if slot.is_some() {
                return None;
            }
            self.def.slots[index]
                .accept
                .accepts(item_tags)
                .then_some(index)
        })
    }

    /// 对这件物品而言是否满了。
    pub fn is_full_for(&self, item_tags: &[ItemTag]) -> bool {
        self.free_slot_for(item_tags).is_none()
    }

    /// 这个容器能装多重的上限（`None` = 不限重）。
    pub fn max_weight(&self) -> Option<f32> {
        self.def.max_weight
    }

    /// 容器里所有物品的总重。
    ///
    /// `weight_of` 由调用方给：系统持有 `Query<(&Item, &Amount)>`，动作函数只有 `&World`，
    /// 两边共用这一处遍历，不必各写一遍求和（重量是派生数据，不存第二份）。
    pub fn total_weight(&self, weight_of: impl Fn(Entity) -> f32) -> f32 {
        self.iter().map(|(_, item)| weight_of(item)).sum()
    }

    /// 在 `current` 的基础上再装 `extra` 重，会不会超过上限。
    ///
    /// 槽位是否空着由 [`Self::free_slot_for`] 判断；两个判据合起来才是"装得下"。
    pub fn accepts_weight(&self, current: f32, extra: f32) -> bool {
        self.def
            .max_weight
            .is_none_or(|max| current + extra <= max)
    }

    /// 放进指定槽位。
    ///
    /// 三道检查都要过：槽位存在 → 空着 → 物品标签被接受。**不做"自动挑最合适的槽"**：
    /// 那是调用方的意图（拾取想找空位就先用 [`Self::free_slot_for`]），放进这个函数会让人
    /// 分不清"我要它放这里"和"随便放个地方"。
    pub fn insert(
        &mut self,
        index: usize,
        item: Entity,
        item_tags: &[ItemTag],
    ) -> Result<usize, InsertError> {
        let def = self.def.slots.get(index).ok_or(InsertError::NoSuchSlot(index))?;
        if self.slots[index].is_some() {
            return Err(InsertError::Occupied);
        }
        if !def.accept.accepts(item_tags) {
            return Err(InsertError::Filtered);
        }
        self.slots[index] = Some(item);
        Ok(index)
    }

    /// 取出某件物品（返回它原来的槽位号）。
    pub fn remove(&mut self, item: Entity) -> Option<usize> {
        let index = self.slot_of(item)?;
        self.slots[index] = None;
        Some(index)
    }

    /// 取出某个槽位里的东西。
    pub fn take_slot(&mut self, index: usize) -> Option<Entity> {
        let slot = self.slots.get_mut(index)?;
        slot.take()
    }
}

#[cfg(test)]
#[path = "container_tests.rs"]
mod tests;
