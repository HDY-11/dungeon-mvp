//! 物品：**一件物品 = 一个实体**（DESIGN DsnE16 / DsnE18）。
//!
//! # 为什么是实体
//!
//! 物品会需要**自己的数据**（耐久、附魔…）。数据堆（`(id, count)`）撑不住这件事：一旦某一堆
//! 要记"这把剑还剩 7 点耐久"，就得给堆结构加一个可选字段，而那份字段迟早长成半个物品系统。
//! 实体化之后，实例数据就是**这个实体上的组件**——ECS 的自然答案。
//!
//! | 部件 | 存法 | 判据 |
//! |---|---|---|
//! | 物品定义 | 静态表 [`registry::ITEMS`] | 数据、无中央分派 |
//! | 物品实例 | 实体 + [`Item`] + [`Amount`] + 0..n 个实例组件 | 需要身份与实例态 |
//! | 所在位置 | 地上挂 [`GroundItemAt`]；**进容器时必须摘掉** | 位置是"在哪"，不是"是什么" |
//! | 装备 | 实体 + `EquippedBy` 关系 | 求值侧按关系收加成（DsnE12/E42） |
//!
//! # 过渡期：背包还是数据堆
//!
//! 玩家的背包暂时仍是 [`Inventory`]（`(id, count)` 堆）。这是**唯一**一处"物品有两种表示"的缝：
//! 拾取时把实体转成数据堆，因为背包页与容器化还没接上（DsnE18 的第①片）。
//! 已有的 [`Container`] 是那片落地的形状——届时背包改成容器、实体直接移进去，
//! 这条缝随之消失（`item_tests` 里有标注）。
//!
//! 刻意**不**让这条缝扩大：世界侧（地面、尸体、箱子）一律是实体，只有"玩家背包"是数据。
//!
//! # 堆叠
//!
//! [`Amount`] 是"这一堆有几个"，所以"一堆血肉 ×3"是**一个实体**而不是三个；
//! 将来进容器时，一个槽位放一个实体、槽位容量约束数量上限。
//!
//! # 拾取口径（背包页落地前的临时规则）
//!
//! | 捡到什么 | 做什么 |
//! |---|---|
//! | 装备且对应部位空着 | 直接穿上（旧四槽位模型；DsnE18 第①片换成身体槽位） |
//! | 消耗品且对应资源未满 | **立即使用 1 个**，剩下的留在原地 |
//! | 其他 | 进背包（按 `max_stack` 堆叠） |
//! | 背包放不下 | **留在原地**并记一条日志（不许静默失败，ISSUES SYN6） |

pub mod container;
pub mod inventory;
pub mod loot;
pub mod registry;

use crate::components::*;
use crate::effect::EquippedBy;
use crate::entity_cls::Player;
use crate::resources::{EventLog, EventMessage};
use bevy_ecs::prelude::*;

pub use container::{
    BODY, Container, ContainerDef, InsertError, ItemTag, SlotAccept, SlotDef, TRAVELER_PACK,
};
pub use inventory::{INVENTORY_SLOTS, Inventory, ItemStack};
pub use loot::{LootEntry, roll_loot};
pub use registry::{ConsumeEffect, EquipSlot, ITEMS, ItemDef, ItemId, ItemKind, item_by_name};

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

/// 一件**装备中**的物品记录：哪个部位、哪一件（旧四槽位模型，`EquippedBy` 的子实体挂它）。
///
/// 只记 `(slot, id)`，数值一律回注册表取——于是"卸下装备"就是 `despawn` 这个实体，
/// 加成自动消失，不需要再去清理"它授予过什么"。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct EquippedItem {
    pub slot: EquipSlot,
    pub id: ItemId,
}

impl EquippedItem {
    pub const fn new(slot: EquipSlot, id: ItemId) -> Self {
        Self { slot, id }
    }

    /// 这件装备的定义。
    pub fn def(self) -> &'static ItemDef {
        self.id.def()
    }
}
/// 物品所在的格子。
///
/// **专用组件**（不用 `Position`）：地上的东西不该被"位置 = 占格 = 会动"那一套语义牵连。
/// 收进容器时**必须移除**它——否则这个实体在说"我既在容器里，又在 (x,y)"。
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

/// 在地上放一堆物品，返回实体。
///
/// "物品出现在世界里"的**唯一入口**（生成期与死亡掉落都走它）：于是"地上物品一定带
/// `Item + Amount + GroundItemAt`"是结构保证，而不是各处的自觉。
pub fn spawn_ground_item(
    commands: &mut Commands,
    id: ItemId,
    count: u32,
    at: (usize, usize),
) -> Entity {
    commands
        .spawn((
            Item::new(id),
            Amount::new(count),
            GroundItemAt::new(at.0, at.1),
        ))
        .id()
}

/// 拾取系统的玩家视图：位置 + 背包 + 两条会被消耗品改的资源。
///
/// 抽成别名既解决 clippy::type_complexity，也让"拾取会改哪些资源"一眼可见。
type PickupPlayer = (
    Entity,
    &'static Position,
    &'static mut Inventory,
    &'static mut Health,
    Option<&'static mut Magic>,
);

/// 玩家踩到物品就捡起。
pub fn pickup_system(
    mut commands: Commands,
    mut players: Query<PickupPlayer, With<Player>>,
    equipped: Query<(Entity, &EquippedBy, &EquippedItem)>,
    ground: Query<(Entity, &GroundItemAt, &Item, &Amount)>,
    mut event_log: ResMut<EventLog>,
) {
    let Ok((player, position, mut inventory, mut health, mut magic)) = players.single_mut() else {
        return;
    };
    let here = position.to_tuple();

    for (entity, at, item, amount) in &ground {
        if at.to_tuple() != here {
            continue;
        }
        let def = item.def();
        let mut left = amount.0;

        // ① 装备：对应部位空着就直接穿（旧四槽位模型）。
        if let Some(slot) = def.equip_slot()
            && !slot_is_taken(&equipped, player, slot)
        {
            commands.entity(entity).despawn();
            commands.spawn((EquippedBy(player), EquippedItem::new(slot, def.id)));
            event_log.push(EventMessage::item(format!(
                "你装备了{}（{}）",
                def.name,
                slot.name()
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

        // ③ 其余进背包（数据堆；容器化时这里换成"把实体移进容器"）。
        let placed = if left > 0 {
            let overflow = inventory.try_add(def.id, left);
            left - overflow
        } else {
            0
        };
        left -= placed;

        if left == 0 {
            commands.entity(entity).despawn();
            if placed > 0 {
                event_log.push(EventMessage::item(format!(
                    "你捡起了{} ×{placed}",
                    def.name
                )));
            }
        } else if placed == 0 {
            event_log.push(EventMessage::system(format!(
                "背包满了，{}留在了地上",
                def.name
            )));
        } else {
            commands.entity(entity).insert(Amount::new(left));
            event_log.push(EventMessage::system(format!(
                "背包放不下全部{}，剩下的留在地上",
                def.name
            )));
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

/// 这个槽位是否已被占用（按 `EquippedBy` 关系查；旧四槽位模型专用）。
fn slot_is_taken(
    equipped: &Query<(Entity, &EquippedBy, &EquippedItem)>,
    owner: Entity,
    slot: EquipSlot,
) -> bool {
    equipped
        .iter()
        .any(|(_, of, item)| of.owner() == owner && item.slot == slot)
}

#[cfg(test)]
#[path = "item_tests.rs"]
mod tests;
