//! 物品：**一件物品 = 一个实体**；装备/背包装在**容器**里（DESIGN DsnE16 / DsnE18）。
//!
//! # 四件事、四种存法
//!
//! | 部件 | 存法 | 判据 |
//! |---|---|---|
//! | 物品定义 | 静态表 [`registry::ITEMS`] | 数据、无中央分派 |
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
//! | 捡到什么 | 做什么 |
//! |---|---|
//! | 装备且对应部位空着 | 进身体槽位（= 穿上） |
//! | 装备但部位已占 | 进行囊 |
//! | 消耗品且对应资源未满 | **立即使用 1 个**，剩下的留在原地 |
//! | 其他 | 进行囊（先并堆、再开新格） |
//! | 行囊超重 | 与"满"同一条口径：**留在原地**并记一条日志（DsnE20） |
//! | 放不下 | **留在原地**并记一条日志（不许静默失败，ISSUES SYN6） |
//!
//! **尸体不自动搜刮**：踩上尸体什么都不会发生，玩家打开背包页在「脚下」面板里
//! 自己翻（`actions::take_from_corpse` 的注释记录了这条玩家决策）。

pub mod actions;
pub mod container;
pub mod loot;
pub mod registry;

use crate::components::*;
use crate::entity_cls::Player;
use crate::resources::{EventLog, EventMessage};
use bevy_ecs::prelude::*;

pub use container::{
    BODY, CORPSE, Container, ContainerDef, InsertError, ItemTag, SlotAccept, SlotDef, TRAVELER_PACK,
};
pub use actions::{
    ItemActionError, drop_from_pack, equip_from_pack, take_from_corpse, unequip_to_pack,
    use_from_pack,
};
pub use loot::{LootEntry, roll_loot};
pub use registry::{
    ConsumeEffect, ITEMS, ItemDef, ItemId, ItemKind, TRAVELER_PACK_ITEM, item_by_name, stack_weight,
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

/// 造一件物品实体（不含位置）：`Item + Amount`，行囊类物品还会带上自己的 `Container`。
///
/// "物品出现"的**唯一入口**（地上、容器里都走它）：于是"物品实体一定带 `Item + Amount`"
/// 与"行囊一定带 `Container`"都是结构保证。
pub fn spawn_item(commands: &mut Commands, id: ItemId, count: u32) -> Entity {
    let mut entity = commands.spawn((Item::new(id), Amount::new(count)));
    if let Some(def) = id.def().container_def() {
        entity.insert(Container::new(def));
    }
    entity.id()
}

/// 在地上放一堆物品，返回实体。
pub fn spawn_ground_item(
    commands: &mut Commands,
    id: ItemId,
    count: u32,
    at: (usize, usize),
) -> Entity {
    let entity = spawn_item(commands, id, count);
    commands
        .entity(entity)
        .insert(GroundItemAt::new(at.0, at.1));
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

/// 玩家的身体视图：位置 + 身体容器 + 两条会被消耗品改的资源。
type PickupPlayer = (
    Entity,
    &'static Position,
    &'static mut Container,
    &'static mut Health,
    Option<&'static mut Magic>,
);

/// 玩家踩到物品就捡起。
pub fn pickup_system(
    mut commands: Commands,
    mut players: Query<PickupPlayer, With<Player>>,
    mut others: Query<&mut Container, Without<Player>>,
    items: Query<(&Item, &Amount)>,
    ground: Query<(Entity, &GroundItemAt, &Item, &Amount)>,
    mut event_log: ResMut<EventLog>,
) {
    let Ok((_, position, mut body, mut health, mut magic)) = players.single_mut() else {
        return;
    };
    let here = position.to_tuple();

    for (entity, at, item, amount) in &ground {
        if at.to_tuple() != here {
            continue;
        }
        let def = item.def();
        let mut left = amount.0;

        // ① 装备：身体上对应部位空着就穿上（部位由 `tags` 求交得到）。
        if def.is_equipment()
            && let Some(index) = body.free_slot_for(def.tags)
        {
            let _ = body.insert(index, entity, def.tags);
            commands.entity(entity).remove::<GroundItemAt>();
            event_log.push(EventMessage::item(format!(
                "你装备了{}（{}）",
                def.name,
                BODY.slots[index].name
            )));
            continue;
        }

        // ② 消耗品：对应资源没满就立刻用掉 1 个，剩下的留在原地。
        if let Some(effect) = def.consume_effect()
            && consume_one(effect, def.name, &mut health, magic.as_deref_mut(), &mut event_log)
        {
            left -= 1;
            if left == 0 {
                commands.entity(entity).despawn();
            } else {
                commands.entity(entity).insert(Amount::new(left));
            }
            continue;
        }

        // ③ 其余进**行囊**：先并堆、再开新格。
        let Some(pack_entity) = body.slot(pack_slot_index()) else {
            event_log.push(EventMessage::system("你没有可用的行囊，东西只能留在原地"));
            break;
        };
        let Ok(mut pack) = others.get_mut(pack_entity) else {
            event_log.push(EventMessage::system("你的行囊不见了，东西只能留在原地"));
            break;
        };

        // 并堆：找同类且有空间的槽位。行囊有载重上限（DsnE20），并堆也算重量。
        let mut weight = container_weight(&pack, &items);
        let merge_target = pack.iter().find_map(|(_, occupant)| {
            let (occupant_item, occupant_amount) = items.get(occupant).ok()?;
            (occupant_item.id == def.id && occupant_amount.room(def) > 0)
                .then_some((occupant, occupant_amount.0, occupant_amount.room(def)))
        });
        if let Some((occupant, current, room)) = merge_target {
            let merged = room.min(left);
            let merged_weight = stack_weight(def.id, merged);
            if pack.accepts_weight(weight, merged_weight) {
                commands.entity(occupant).insert(Amount::new(current + merged));
                weight += merged_weight;
                left -= merged;
            }
        }
        // 开新格（一堆一个实体；超过 `max_stack` 的部分继续找下一格）。
        // 空槽位只是"放得进"，还得"背得动"：载重超了就留在原地。
        let mut moved_original = false;
        let mut overweight = false;
        while left > 0 {
            let Some(index) = pack.free_slot_for(def.tags) else {
                break;
            };
            let put = def.max_stack.min(left);
            let put_weight = stack_weight(def.id, put);
            if !pack.accepts_weight(weight, put_weight) {
                overweight = true;
                break;
            }
            let holder = if put == left {
                moved_original = true;
                entity
            } else {
                spawn_item(&mut commands, def.id, put)
            };
            let _ = pack.insert(index, holder, def.tags);
            weight += put_weight;
            left -= put;
        }

        if moved_original {
            commands.entity(entity).remove::<GroundItemAt>();
            event_log.push(EventMessage::item(format!("你捡起了{}", def.name)));
        } else if left == amount.0 {
            let reason = if overweight { "行囊超重" } else { "行囊满了" };
            event_log.push(EventMessage::system(format!(
                "{reason}，{}留在了地上",
                def.name
            )));
        } else if left > 0 {
            commands.entity(entity).insert(Amount::new(left));
            event_log.push(EventMessage::system(format!(
                "行囊放不下全部{}，剩下的留在地上",
                def.name
            )));
        } else {
            commands.entity(entity).despawn();
            event_log.push(EventMessage::item(format!("你捡起了{}", def.name)));
        }
    }
}

/// 用掉 1 个消耗品；返回是否真的用掉了（资源已满 = 没用到，留着）。
fn consume_one(
    effect: ConsumeEffect,
    name: &str,
    health: &mut Health,
    magic: Option<&mut Magic>,
    event_log: &mut EventLog,
) -> bool {
    match effect {
        ConsumeEffect::Heal(amount) => {
            if health.current >= health.max {
                return false;
            }
            let before = health.current;
            health.current = (health.current + amount).min(health.max);
            event_log.push(EventMessage::item(format!(
                "你吃下{name}，回复 {:.0} 点生命",
                health.current - before
            )));
            true
        }
        ConsumeEffect::RestoreMagic(amount) => {
            let Some(magic) = magic else {
                return false;
            };
            if magic.current >= magic.max {
                return false;
            }
            let before = magic.current;
            magic.current = (magic.current + amount).min(magic.max);
            event_log.push(EventMessage::item(format!(
                "你喝下{name}，回复 {:.0} 点法力",
                magic.current - before
            )));
            true
        }
    }
}

#[cfg(test)]
#[path = "item_tests.rs"]
mod tests;
