//! 物品系统：**注册表 + 堆叠/背包 + 装备槽 + 掉落**（GAME.md Gm4 / Gm7）。
//!
//! # 四个部件，四种存法
//!
//! | 部件 | 存法 | 为什么 |
//! |---|---|---|
//! | 物品定义 | 静态表（[`registry`]） | 数据可以编译进去，关键是**没有中央分派**（加物品 = 加一行） |
//! | 地面物品 | 实体 + [`GroundItemAt`] | 它占位置但不占格；用专用组件而不是 `Position`，否则玩家走不进去（第三次踩同一个坑） |
//! | 背包 | 组件里的定长数组（[`Inventory`]） | 它没有身份也没有位置，做成实体是纯亏 |
//! | 装备 | 实体 + `EquippedBy` 关系 | 它要被求值侧按关系收进来，且 owner 死亡时要**掉出来**（DsnE12/E42） |
//!
//! # 装备的加成怎么生效
//!
//! 装备**不起新实体来承载效果**：`EquippedItem` 只记 `(槽位, 物品编号)`，求值侧
//! （`effect::collect`）顺着 `EquippedBy` 找到它们，再去注册表里取这件装备的
//! [`Modifier`](crate::rules::Modifier) 列表。于是：
//!
//! - 一件装备的数值只有**一处**（注册表那一行），不会出现"实体上的修正与表里的不一致"；
//! - 卸下装备 = `despawn` 那个实体，效果**自动**消失，不需要"把授予的效果再找出来删掉"；
//! - 状态效果仍走 `EffectOf` 实体（它们有自己的时间轴与解除路径），两条关系各管一摊。
//!
//! 这与 C4 的第一版不同：那一版把装备的无视声明**实例化**成 `IgnoresSource` 子实体，
//! 于是"卸下"必须去找回那些子实体——而 C4 干脆没做卸下。改成"求值侧读注册表"之后，
//! 缝被补上了（`effect/collect.rs` 里那句"这条缝故意留着不做"可以删了）。
//!
//! # 拾取口径（背包页落地前的临时规则）
//!
//! 踩到就捡，且**尽量让内容立刻生效**（否则物品系统对玩家不可见）：
//!
//! | 捡到什么 | 做什么 |
//! |---|---|
//! | 装备且槽位空 | 直接装备 |
//! | 装备且槽位已占 | 进背包 |
//! | 消耗品且对应资源未满 | **立即使用**（临时口径，等背包页做成 `r` 键使用） |
//! | 其他（材料/已满的消耗品） | 进背包 |
//! | 背包满 | **留在原地**并记一条日志（不许静默失败，ISSUES SYN6） |

pub mod inventory;
pub mod loot;
pub mod registry;

use crate::components::*;
use crate::effect::EquippedBy;
use crate::entity_cls::Player;
use crate::resources::{EventLog, EventMessage};
use bevy_ecs::prelude::*;

pub use inventory::{INVENTORY_SLOTS, Inventory, ItemStack};
pub use loot::{LootEntry, roll_loot};
pub use registry::{ConsumeEffect, EquipSlot, ITEMS, ItemDef, ItemId, ItemKind, item_by_name};

/// 一件躺在地上的物品（可能是一堆）。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct GroundItem {
    pub stack: ItemStack,
}

impl GroundItem {
    pub fn new(stack: ItemStack) -> Self {
        Self { stack }
    }

    /// 便捷构造：按编号与数量。
    pub fn of(id: ItemId, count: u32) -> Self {
        Self::new(ItemStack::new(id, count))
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

/// 一件**装备中**的物品（挂在带 `EquippedBy` 的实体上）。
///
/// 只记"哪个槽位、哪件物品"。数值**不抄一份到这里**：抄了就会出现
/// "实体上的加成"与"注册表里的定义"两个真相（ISSUES ECS25 那类问题）。
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

/// 拾取系统的玩家查询：位置 + 背包 + 两条会被消耗品改的资源。
///
/// 抽成别名既解决 `clippy::type_complexity`，也让"拾取会改哪些资源"一眼可见
/// （加一条会改的资源 = 改这一行，而不是往系统签名里再塞一个元组成员）。
type PickupPlayer = (
    Entity,
    &'static Position,
    &'static mut Inventory,
    &'static mut Health,
    Option<&'static mut Magic>,
);

/// 玩家踩到物品就捡起。
///
/// 命令顺序有意义：先让物品离地，再决定去哪（装备 / 背包 / 立即使用）。
/// 全部走 `Commands`，因此同一轮里不会出现"半拾取"状态被别的系统读到。
pub fn pickup_system(
    mut commands: Commands,
    mut players: Query<PickupPlayer, With<Player>>,
    equipped: Query<(Entity, &EquippedBy, &EquippedItem)>,
    items: Query<(Entity, &GroundItemAt, &GroundItem)>,
    mut event_log: ResMut<EventLog>,
) {
    let Ok((player, position, mut inventory, mut health, mut magic)) = players.single_mut() else {
        return;
    };

    for (item_entity, at, ground) in &items {
        if at.to_tuple() != position.to_tuple() {
            continue;
        }
        let stack = ground.stack;
        let def = stack.def();

        // ① 装备：槽位空就直接穿上，否则进背包。
        if let Some(slot) = def.equip_slot()
            && !slot_is_taken(&equipped, player, slot)
        {
            commands.entity(item_entity).despawn();
            commands.spawn((EquippedBy(player), EquippedItem::new(slot, stack.id)));
            event_log.push(EventMessage::item(format!(
                "你装备了{}（{}）",
                def.name,
                slot.name()
            )));
            // 装备可能改上限（防御/生命），这里只记日志，重算留给 owner 系统。
            continue;
        }

        // ② 消耗品：对应资源没满就立刻用掉（临时口径，见模块头）。
        if let ItemKind::Consumable(effect) = def.kind {
            let consumed = match effect {
                ConsumeEffect::Heal(amount) => {
                    if health.current >= health.max {
                        false
                    } else {
                        let before = health.current;
                        health.current = (health.current + amount).min(health.max);
                        event_log.push(EventMessage::item(format!(
                            "你吃下{}，回复 {:.0} 点生命",
                            def.name,
                            health.current - before
                        )));
                        true
                    }
                }
                ConsumeEffect::RestoreMagic(amount) => {
                    let mut used = false;
                    // `magic.as_mut()` 而不是把 `Option<&mut Magic>` 移进 `if let`：
                    // 后者会在第一次循环迭代就把值移走，第二件物品时编译不过。
                    if let Some(magic) = magic.as_mut()
                        && magic.current < magic.max
                    {
                        let before = magic.current;
                        magic.current = (magic.current + amount).min(magic.max);
                        event_log.push(EventMessage::item(format!(
                            "你喝下{}，回复 {:.0} 点法力",
                            def.name,
                            magic.current - before
                        )));
                        used = true;
                    }
                    used
                }
            };
            if consumed {
                // 这一堆只用掉 1 个：剩下多少个就还剩多少个。
                if stack.count > 1 {
                    commands.entity(item_entity).insert(GroundItem::of(stack.id, stack.count - 1));
                } else {
                    commands.entity(item_entity).despawn();
                }
                continue;
            }
        }

        // ③ 其余全部进背包；放不下就留在原地（**不静默**）。
        if inventory.try_add(stack.id, stack.count) == 0 {
            commands.entity(item_entity).despawn();
            event_log.push(EventMessage::item(format!("你捡起了{}", stack.label())));
        } else if inventory.is_full() {
            event_log.push(EventMessage::system(format!(
                "背包满了，{}留在了地上",
                stack.label()
            )));
        }
    }
}

/// 这个槽位是否已被占用（按 `EquippedBy` 关系查，不另存一份"哪个槽有什么"）。
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
