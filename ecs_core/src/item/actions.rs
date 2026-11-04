//! 背包页的**动作**：在「身体 / 行囊 / 地面」之间搬动物品（DsnE18）。
//!
//! # 为什么不是"行动"
//!
//! 换装不产生 action 实体、不占 AV：它没有施法者与目标的时序关系，也**不消耗回合**。
//! 将来若要"换装耗时"，那是给这些函数加一次世界推进，而不是把它们塞进行动链路
//! （`DsnE18` 记录）。
//!
//! # 动件的形状
//!
//! 每个动作都是"取出来 → 放进去"，且**全部检查通过才动**：
//!
//! - 换装：目标部位已被占 → 先把旧的收回行囊（**交换**，不是丢弃）；
//! - 卸下：行囊满了就**放回去**，玩家的装备不会凭空消失；
//! - 丢弃：物品落到玩家脚下（`GroundItemAt`），容器里不再有它；
//! - 使用：消耗品用掉 1 个，数量归零才销毁实体；
//! - 搜刮：把脚下尸体容器里的一件东西拿进行囊（尸体不自动清空，玩家自己翻）。
//!
//! 每次失败都回滚到"没发生"，所以调用方只需看 [`ItemActionError`]。

use crate::components::{Health, Magic, Position};
use crate::entity_cls::Player;
use crate::item::{
    Amount, Container, ConsumeEffect, GroundItemAt, Item, ItemDef, ItemId, ItemTag,
    pack_slot_index, stack_weight,
};
use bevy_ecs::prelude::*;

/// 动作失败的原因（每条都能给玩家一句话）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemActionError {
    /// 世界里没有玩家。
    NoPlayer,
    /// 玩家身上没有行囊。
    NoPack,
    /// 选中的槽位是空的。
    EmptySlot,
    /// 选中槽位里不是可装备的物品。
    NotEquippable,
    /// 行囊放不下。
    NoRoom,
    /// 不是消耗品，或现在用了没用（资源已满）。
    NotUsable,
    /// 想动「背包槽」本身（行囊不能塞进自己）。
    CannotNestPack,
    /// 脚下没有可以搜刮的尸体。
    NoCorpse,
    /// 行囊的载重不够（DsnE20）。
    Overweight,
}

impl ItemActionError {
    /// 给玩家看的一句话。
    pub fn describe(self) -> &'static str {
        match self {
            ItemActionError::NoPlayer => "找不到你的角色",
            ItemActionError::NoPack => "你没有可用的行囊",
            ItemActionError::EmptySlot => "那是空的",
            ItemActionError::NotEquippable => "这个东西穿不上",
            ItemActionError::NoRoom => "行囊里放不下了",
            ItemActionError::NotUsable => "现在用不上它",
            ItemActionError::CannotNestPack => "行囊不能塞进它自己",
            ItemActionError::NoCorpse => "脚下没有可以搜刮的尸体",
            ItemActionError::Overweight => "行囊超重了，背不动更多东西",
        }
    }
}

/// 把行囊第 `pack_slot` 格的装备穿到身体对应部位。
///
/// 部位已被占时**交换**：旧的收回行囊，新的穿上。交换是原子的——任一步失败都回滚。
pub fn equip_from_pack(world: &mut World, pack_slot: usize) -> Result<(), ItemActionError> {
    let player = player_entity(world)?;
    let pack = pack_entity(world, player)?;
    let item = slot_of(world, pack, pack_slot)?;
    let def = item_def(world, item)?;
    if !def.is_equipment() {
        return Err(ItemActionError::NotEquippable);
    }
    // 有空位就穿；被占就交换。两者都是"目标槽位 + 可选旧装备"。
    let (body_slot, old) = match free_body_slot(world, player, def.tags) {
        Ok(slot) => (slot, None),
        Err(_) => {
            let (slot, occupant) = occupied_body_slot(world, player, def.tags)
                .ok_or(ItemActionError::NotEquippable)?;
            (slot, Some(occupant))
        }
    };

    // 先摘下来，再放进去：任意一步失败都把它塞回原处。
    // 交换前先看行囊背不背得动旧装备——背不动就整笔拒绝，不做"换到一半"。
    if let Some(old) = old {
        let old_def = item_def(world, old)?;
        let old_amount = item_amount(world, old);
        if !pack_holds_weight(world, pack, stack_weight(old_def.id, old_amount)) {
            return Err(ItemActionError::Overweight);
        }
    }
    let taken = take_from(world, pack, pack_slot).ok_or(ItemActionError::EmptySlot)?;
    if taken != item {
        // 读与写之间世界不该变；真变了就当作没发生（不 panic）。
        return Err(ItemActionError::EmptySlot);
    }
    if let Some(old) = old {
        let _ = take_from(world, player, body_slot);
        if put_back(world, pack, pack_slot, old).is_err() {
            // 行囊塞不下旧装备：整体回滚。
            let _ = take_from(world, pack, pack_slot);
            let _ = put_into(world, player, body_slot, old, &[]);
            let _ = put_back(world, pack, pack_slot, item);
            return Err(ItemActionError::NoRoom);
        }
    }
    if put_into(world, player, body_slot, item, def.tags).is_err() {
        // 放不回去就把新装备还回行囊（世界保持一致）。
        let _ = put_back(world, pack, pack_slot, item);
        return Err(ItemActionError::NotEquippable);
    }
    Ok(())
}

/// 把身体第 `body_slot` 格的装备收回行囊。
pub fn unequip_to_pack(world: &mut World, body_slot: usize) -> Result<(), ItemActionError> {
    let player = player_entity(world)?;
    let pack = pack_entity(world, player)?;
    if body_slot == pack_slot_index() {
        return Err(ItemActionError::CannotNestPack);
    }
    let item = slot_of(world, player, body_slot)?;
    let def = item_def(world, item)?;
    if !pack_holds_weight(world, pack, stack_weight(def.id, item_amount(world, item))) {
        return Err(ItemActionError::Overweight);
    }
    let free = free_slot_in(world, pack, def.tags).ok_or(ItemActionError::NoRoom)?;
    let taken = take_from(world, player, body_slot).ok_or(ItemActionError::EmptySlot)?;
    if put_into(world, pack, free, taken, def.tags).is_err() {
        let _ = put_into(world, player, body_slot, taken, def.tags);
        return Err(ItemActionError::NoRoom);
    }
    Ok(())
}

/// 把行囊第 `pack_slot` 格的物品丢到玩家脚下。
pub fn drop_from_pack(world: &mut World, pack_slot: usize) -> Result<(), ItemActionError> {
    let player = player_entity(world)?;
    let pack = pack_entity(world, player)?;
    let position = world
        .get::<Position>(player)
        .map(Position::to_tuple)
        .ok_or(ItemActionError::NoPlayer)?;
    let item = take_from(world, pack, pack_slot).ok_or(ItemActionError::EmptySlot)?;
    world
        .entity_mut(item)
        .insert(GroundItemAt::new(position.0, position.1));
    Ok(())
}

/// 使用行囊第 `pack_slot` 格的消耗品（用掉 1 个）。
pub fn use_from_pack(world: &mut World, pack_slot: usize) -> Result<(), ItemActionError> {
    let player = player_entity(world)?;
    let pack = pack_entity(world, player)?;
    let item = slot_of(world, pack, pack_slot)?;
    let def = item_def(world, item)?;
    let Some(effect) = def.consume_effect() else {
        return Err(ItemActionError::NotUsable);
    };
    if !apply_effect(world, player, effect) {
        return Err(ItemActionError::NotUsable);
    }
    let remaining = world.get::<Amount>(item).map(|amount| amount.0).unwrap_or(1);
    if remaining <= 1 {
        let _ = take_from(world, pack, pack_slot);
        world.entity_mut(item).despawn();
    } else {
        world.entity_mut(item).insert(Amount::new(remaining - 1));
    }
    Ok(())
}

/// 把脚下尸体第 `corpse_slot` 格的物品拿进行囊。
///
/// 尸体**不再自动搜刮**（玩家反馈：默认全拾取太粗暴）：踩上去什么都不发生，
/// 打开背包页在「脚下」面板里自己翻。并堆 / 开新格沿用行囊同一套规则，
/// 任一失败都把东西放回尸体，保证"要么拿走、要么还在原地"。
pub fn take_from_corpse(world: &mut World, corpse_slot: usize) -> Result<ItemId, ItemActionError> {
    let player = player_entity(world)?;
    let position = world
        .get::<Position>(player)
        .map(Position::to_tuple)
        .ok_or(ItemActionError::NoPlayer)?;
    let corpse = corpse_underfoot(world, position).ok_or(ItemActionError::NoCorpse)?;
    let item = slot_of(world, corpse, corpse_slot)?;
    let def = item_def(world, item)?;
    let id = def.id;
    let amount = world.get::<Amount>(item).map(|amount| amount.0).unwrap_or(1);
    let pack = pack_entity(world, player)?;
    // 空槽位只是"放得进"，还得"背得动"（DsnE20）。
    if !pack_holds_weight(world, pack, stack_weight(id, amount)) {
        return Err(ItemActionError::Overweight);
    }

    // 先在行囊里找同类、且**装得下整堆**的格子：整堆并进去最简单，也不会留下半个实体。
    let merged = world.get::<Container>(pack).and_then(|pack| {
        pack.iter().find_map(|(_, occupant)| {
            let occupant_item = world.get::<Item>(occupant)?;
            let occupant_amount = world.get::<Amount>(occupant)?;
            (occupant_item.id == id && occupant_amount.room(occupant_item.def()) >= amount)
                .then_some((occupant, occupant_amount.0))
        })
    });

    let taken = take_from(world, corpse, corpse_slot).ok_or(ItemActionError::EmptySlot)?;
    if taken != item {
        // 读与写之间世界不该变；真变了就当作没发生（不 panic）。
        return Err(ItemActionError::EmptySlot);
    }

    if let Some((occupant, current)) = merged {
        let merged_amount = current + amount;
        if let Some(mut held) = world.get_mut::<Amount>(occupant) {
            held.0 = merged_amount;
        }
        world.entity_mut(item).despawn();
        return Ok(id);
    }

    let Some(free) = free_slot_in(world, pack, def.tags) else {
        let _ = put_back(world, corpse, corpse_slot, item);
        return Err(ItemActionError::NoRoom);
    };
    if put_into(world, pack, free, item, def.tags).is_err() {
        let _ = put_back(world, corpse, corpse_slot, item);
        return Err(ItemActionError::NoRoom);
    }
    Ok(id)
}

/// 玩家脚下那具尸体（`Position` 相同且带 `Corpse`）。
fn corpse_underfoot(world: &mut World, at: (usize, usize)) -> Option<Entity> {
    let mut query = world.query_filtered::<(Entity, &Position), With<crate::monster::Corpse>>();
    query
        .iter(world)
        .find(|(_, position)| position.to_tuple() == at)
        .map(|(entity, _)| entity)
}

// ── 小工具（每个都只借一次 World，避免同时可变借用两个容器） ──

fn player_entity(world: &World) -> Result<Entity, ItemActionError> {
    let mut query = world
        .try_query_filtered::<Entity, With<Player>>()
        .ok_or(ItemActionError::NoPlayer)?;
    query.iter(world).next().ok_or(ItemActionError::NoPlayer)
}

fn pack_entity(world: &World, player: Entity) -> Result<Entity, ItemActionError> {
    let body = world
        .get::<Container>(player)
        .ok_or(ItemActionError::NoPack)?;
    body.slot(pack_slot_index()).ok_or(ItemActionError::NoPack)
}

fn slot_of(world: &World, container: Entity, slot: usize) -> Result<Entity, ItemActionError> {
    world
        .get::<Container>(container)
        .and_then(|container| container.slot(slot))
        .ok_or(ItemActionError::EmptySlot)
}

fn item_def(world: &World, item: Entity) -> Result<&'static ItemDef, ItemActionError> {
    world
        .get::<Item>(item)
        .map(|item| item.id.def())
        .ok_or(ItemActionError::EmptySlot)
}

fn free_body_slot(
    world: &World,
    player: Entity,
    tags: &[ItemTag],
) -> Result<usize, ItemActionError> {
    world
        .get::<Container>(player)
        .and_then(|body| body.free_slot_for(tags))
        .ok_or(ItemActionError::NoRoom)
}

/// 接受这些标签**并且已被占用**的身体槽位（交换用）。
fn occupied_body_slot(
    world: &World,
    player: Entity,
    tags: &[ItemTag],
) -> Option<(usize, Entity)> {
    let body = world.get::<Container>(player)?;
    body.iter().find_map(|(index, occupant)| {
        let accepts = body.def().slots[index].accept.accepts(tags);
        (accepts && index != pack_slot_index()).then_some((index, occupant))
    })
}

fn free_slot_in(world: &World, container: Entity, tags: &[ItemTag]) -> Option<usize> {
    world
        .get::<Container>(container)
        .and_then(|container| container.free_slot_for(tags))
}

/// 容器当前总重（`&World` 版；系统里用 `item::container_weight`，口径同一份）。
fn container_weight(world: &World, container: Entity) -> f32 {
    world
        .get::<Container>(container)
        .map(|container| {
            container.total_weight(|entity| {
                world
                    .get::<Item>(entity)
                    .map(|stack| {
                        let amount = world.get::<Amount>(entity).map(|amount| amount.0).unwrap_or(1);
                        stack_weight(stack.id, amount)
                    })
                    .unwrap_or(0.0)
            })
        })
        .unwrap_or(0.0)
}

/// 行囊在现有载重上再放 `extra` 重会不会超载。
fn pack_holds_weight(world: &World, pack: Entity, extra: f32) -> bool {
    let current = container_weight(world, pack);
    world
        .get::<Container>(pack)
        .is_none_or(|pack| pack.accepts_weight(current, extra))
}

fn take_from(world: &mut World, container: Entity, slot: usize) -> Option<Entity> {
    world.get_mut::<Container>(container)?.take_slot(slot)
}

fn put_into(
    world: &mut World,
    container: Entity,
    slot: usize,
    item: Entity,
    tags: &[ItemTag],
) -> Result<(), ItemActionError> {
    let mut container = world
        .get_mut::<Container>(container)
        .ok_or(ItemActionError::NoPack)?;
    container
        .insert(slot, item, tags)
        .map(|_| ())
        .map_err(|_| ItemActionError::NoRoom)
}

/// 放回"原槽位优先，否则任意空格"——交换失败回滚用。
fn put_back(
    world: &mut World,
    container: Entity,
    preferred: usize,
    item: Entity,
) -> Result<(), ItemActionError> {
    let tags = world
        .get::<Item>(item)
        .map(|item| item.tags())
        .unwrap_or(&[]);
    if let Some(container_ref) = world.get::<Container>(container)
        && container_ref.is_slot_free(preferred)
        && container_ref.def().slots[preferred].accept.accepts(tags)
    {
        return put_into(world, container, preferred, item, tags);
    }
    let free = free_slot_in(world, container, tags).ok_or(ItemActionError::NoRoom)?;
    put_into(world, container, free, item, tags)
}

/// 应用一次消耗品效果；返回是否真的生效（资源已满 = 没生效）。
fn apply_effect(world: &mut World, player: Entity, effect: ConsumeEffect) -> bool {
    match effect {
        ConsumeEffect::Heal(amount) => {
            let Some(mut health) = world.get_mut::<Health>(player) else {
                return false;
            };
            if health.current >= health.max {
                return false;
            }
            health.current = (health.current + amount).min(health.max);
            true
        }
        ConsumeEffect::RestoreMagic(amount) => {
            let Some(mut magic) = world.get_mut::<Magic>(player) else {
                return false;
            };
            if magic.current >= magic.max {
                return false;
            }
            magic.current = (magic.current + amount).min(magic.max);
            true
        }
    }
}

/// 物品编号的便捷查询（供调用方记日志）。
pub fn item_name(world: &World, item: Entity) -> Option<&'static str> {
    world.get::<Item>(item).map(|item| item.id.def().name)
}

/// 给日志用：`Amount` 的数量（没有就当 1）。
pub fn item_amount(world: &World, item: Entity) -> u32 {
    world.get::<Amount>(item).map(|amount| amount.0).unwrap_or(1)
}

/// 行囊里第 `pack_slot` 格的物品编号（`None` = 空格）。
pub fn pack_item_id(world: &World, pack_slot: usize) -> Option<ItemId> {
    let player = player_entity(world).ok()?;
    let pack = pack_entity(world, player).ok()?;
    let item = slot_of(world, pack, pack_slot).ok()?;
    world.get::<Item>(item).map(|item| item.id)
}

#[cfg(test)]
#[path = "actions_tests.rs"]
mod tests;
