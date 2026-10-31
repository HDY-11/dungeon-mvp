//! 堆叠与背包：`(物品编号, 数量)` 对 + 一个定长格子数组（GAME.md Gm7「背包」）。
//!
//! # 为什么背包是**数据**而装备是**实体**
//!
//! 两者看着都是"玩家身上的一堆物品"，但它们的**语义**不同：
//!
//! | | 背包 | 装备 |
//! |---|---|---|
//! | 有没有位置/身份 | 没有——`(id, count)` 就是一格 | 有——它在哪个槽位、谁带的、卸下后去哪 |
//! | 会不会被别的系统查询 | 不会（只有 UI 与拾取/使用读它） | 会——求值侧按 `EquippedBy` 收它的修正 |
//! | 生命周期 | 与拥有者同生共死 | owner 死亡时**掉出来**（DsnE12/E42 的 `EquippedBy` 不级联） |
//!
//! 所以装备是实体（`EquippedBy` 关系 + 一个 `EquippedItem` 组件），背包是组件里的定长数组。
//! 把背包也做成实体是纯亏：要遍历、要排序、要处理"格子实体泄漏"，换来的只有"统一"两个字。
//!
//! # 堆叠规则
//!
//! - 同一种物品优先并进**已有**且未满的那一堆（避免背包里出现 5 堆各 1 个石子）；
//! - 放不下就整堆拒绝并返回剩余数量——**不做部分堆叠**。
//!   理由：部分堆叠会让"捡起 3 个、背包只剩 2 格"变成"地上留 1 个"，调用方要再写一遍
//!   "把剩下的放回地上"，而那条路径几乎没人测。宁可整堆留在原地。
//! - 装备的 `max_stack` 恒为 1（GAME.md Gm7），所以"一格一件"是它的自然结果。

use crate::item::registry::{ItemDef, ItemId};
use bevy_ecs::prelude::*;

/// 背包格子数。
///
/// 旧实现是 24 格（`archive/dungeon-core/src/components.rs`）；这里沿用。
/// 它不是"数值平衡"，而是 UI 一屏能列多少行——改它要同时改详情页的翻页。
pub const INVENTORY_SLOTS: usize = 24;

/// 一堆物品：`(编号, 数量)`。
///
/// 数量恒 ≥ 1：`ItemStack` 存在就代表"至少有一个"。数量归零应该变成 `None`，
/// 由 [`Inventory`] 的方法负责，而不是留一个 `count: 0` 的空壳（那种空壳会让
/// "格子已用/已空"的判定到处都要多想一遍）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemStack {
    pub id: ItemId,
    pub count: u32,
}

impl ItemStack {
    /// 构建一堆；数量为 0 是调用错误（见类型注释）。
    pub fn new(id: ItemId, count: u32) -> Self {
        assert!(count > 0, "ItemStack 的数量必须 ≥ 1");
        Self { id, count }
    }

    /// 这堆物品的静态定义。
    pub fn def(&self) -> &'static ItemDef {
        self.id.def()
    }

    /// 还能再装几个（到 `max_stack` 为止）。
    pub fn room(&self) -> u32 {
        self.def().max_stack.saturating_sub(self.count)
    }

    /// 显示用："蘑菇 ×3"（`1` 个时不带数量）。
    pub fn label(&self) -> String {
        let def = self.def();
        if self.count > 1 {
            format!("{} ×{}", def.name, self.count)
        } else {
            def.name.to_string()
        }
    }
}

/// 玩家的背包（定长、可堆叠）。
///
/// 用 `Vec<Option<ItemStack>>` 而不是 `[Option<ItemStack>; N]`：
/// `Option<ItemStack>` 只有 16 字节，但定长数组要 24 次逐元素初始化，
/// 而且未来"背包扩容"（卷轴/升级）会想改容量——`Vec` 让容量成为一个可变的数。
#[derive(Component, Debug, Clone, Default, PartialEq, Eq)]
pub struct Inventory {
    slots: Vec<Option<ItemStack>>,
}

impl Inventory {
    /// 空背包（标准容量）。
    pub fn new() -> Self {
        Self {
            slots: vec![None; INVENTORY_SLOTS],
        }
    }

    /// 当前容量。
    pub fn capacity(&self) -> usize {
        self.slots.len()
    }

    /// 所有非空格子（按槽位顺序）。
    pub fn iter(&self) -> impl Iterator<Item = &ItemStack> {
        self.slots.iter().flatten()
    }

    /// 已用格数。
    pub fn used(&self) -> usize {
        self.slots.iter().flatten().count()
    }

    /// 是否完全没有空间（连一堆新材料也放不下）。
    pub fn is_full(&self) -> bool {
        self.slots.iter().all(Option::is_some)
    }

    /// 某种物品的总数（合成/UI 用）。
    pub fn count_of(&self, id: ItemId) -> u32 {
        self.iter()
            .filter(|stack| stack.id == id)
            .map(|stack| stack.count)
            .sum()
    }

    /// 尝试放入 `count` 个：**要么全放得下，要么一个都不放**。
    ///
    /// 返回没放进去的数量：`0` = 全部放进去了；`count` = 完全没放（空间不足）。
    /// 不做部分堆叠的理由见模块头。
    pub fn try_add(&mut self, id: ItemId, count: u32) -> u32 {
        if count == 0 {
            return 0;
        }
        let max_stack = id.def().max_stack;
        // 先算"腾挪之后放不放得下"，再动手——两遍扫描换来"要么全放要么不放"。
        let mut free = 0u64;
        for slot in self.slots.iter().flatten() {
            if slot.id == id {
                free += u64::from(slot.room());
            }
        }
        let empty = self.slots.iter().filter(|slot| slot.is_none()).count();
        free += empty as u64 * u64::from(max_stack);
        if free < u64::from(count) {
            return count;
        }

        let mut remaining = count;
        // ① 并进已有堆
        for slot in self.slots.iter_mut().flatten() {
            if remaining == 0 {
                break;
            }
            if slot.id != id {
                continue;
            }
            let add = slot.room().min(remaining);
            slot.count += add;
            remaining -= add;
        }
        // ② 开新格
        for slot in self.slots.iter_mut() {
            if remaining == 0 {
                break;
            }
            if slot.is_some() {
                continue;
            }
            let add = max_stack.min(remaining);
            *slot = Some(ItemStack::new(id, add));
            remaining -= add;
        }
        debug_assert_eq!(remaining, 0, "空间检查通过之后必然放得下");
        remaining
    }

    /// 取出 `count` 个（从后往前取，先消耗零散小堆）。
    ///
    /// 返回实际取出的数量（≤ `count`）：调用方要用它决定"材料够不够"。
    pub fn remove(&mut self, id: ItemId, mut count: u32) -> u32 {
        let mut removed = 0;
        for slot in self.slots.iter_mut().rev() {
            if count == 0 {
                break;
            }
            let Some(stack) = slot else { continue };
            if stack.id != id {
                continue;
            }
            let take = stack.count.min(count);
            stack.count -= take;
            count -= take;
            removed += take;
            if stack.count == 0 {
                *slot = None;
            }
        }
        removed
    }

    /// 取出第 `index` 格（UI 用它挑"选中那一件"）。
    pub fn take_slot(&mut self, index: usize) -> Option<ItemStack> {
        self.slots.get_mut(index)?.take()
    }

    /// 第 `index` 格的内容。
    pub fn slot(&self, index: usize) -> Option<ItemStack> {
        self.slots.get(index).copied().flatten()
    }

    /// 把一堆直接塞进**第一个空格**（不堆叠）；满了返回那一堆本身。
    ///
    /// 与 [`Self::try_add`] 的区别：这里**保留两堆分开**。装备从槽位卸下、
    /// 或玩家把一格单独丢回背包时用它——那种场景下"并进别的堆"是错的
    /// （一件装备永远该占自己一格）。
    pub fn push_slot(&mut self, stack: ItemStack) -> Option<ItemStack> {
        for slot in self.slots.iter_mut() {
            if slot.is_none() {
                *slot = Some(stack);
                return None;
            }
        }
        Some(stack)
    }
}

#[cfg(test)]
#[path = "inventory_tests.rs"]
mod tests;
