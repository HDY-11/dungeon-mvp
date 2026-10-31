//! 堆叠与背包的测试。
//!
//! 这一层全是**纯数据操作**（不碰 `World`），所以用例写得比 ECS 层细：
//! 堆叠、拆堆、满了怎么办、取出顺序——每一条都可能让玩家看见"东西没了"或"东西变多了"。

use super::*;

fn mushroom() -> ItemId {
    // 蘑菇：max_stack = 20
    ItemId(25)
}

fn flesh() -> ItemId {
    // 生物血肉：max_stack = 64
    ItemId(10)
}

fn sword() -> ItemId {
    // 锈铁剑：max_stack = 1
    ItemId(0)
}

/// 新背包是空的，且容量来自 `INVENTORY_SLOTS`。
#[test]
fn a_new_inventory_is_empty() {
    let inventory = Inventory::new();
    assert_eq!(inventory.capacity(), INVENTORY_SLOTS);
    assert_eq!(inventory.used(), 0);
    assert!(!inventory.is_full());
    assert_eq!(inventory.iter().count(), 0);
}

/// 同一物品先并进已有堆，不会一格一个。
#[test]
fn adding_the_same_item_merges_into_the_existing_stack() {
    let mut inventory = Inventory::new();
    assert_eq!(inventory.try_add(mushroom(), 3), 0);
    assert_eq!(inventory.try_add(mushroom(), 4), 0);
    assert_eq!(inventory.used(), 1, "同种物品必须并进同一堆");
    assert_eq!(inventory.count_of(mushroom()), 7);
}

/// 超过 `max_stack` 时开新格（而不是把一堆撑破上限）。
#[test]
fn overflowing_a_stack_opens_a_new_slot() {
    let mut inventory = Inventory::new();
    assert_eq!(inventory.try_add(mushroom(), 25), 0, "20 + 5 应当在两格里放得下");
    assert_eq!(inventory.used(), 2);
    assert_eq!(inventory.count_of(mushroom()), 25);
    let counts: Vec<u32> = inventory.iter().map(|stack| stack.count).collect();
    assert_eq!(counts, vec![20, 5], "先填满第一堆，再开第二堆");
}

/// **要么全放要么不放**：一格装不下 21 个蘑菇时，一个都不进背包。
///
/// 这条口径的价值在调用方：拾取系统只需要判断"返回值是否为 0"，
/// 不必处理"地上还剩几个"——那是最容易漏写的分支。
#[test]
fn a_partially_fitting_batch_is_rejected_as_a_whole() {
    let mut inventory = Inventory::new();
    // 只留一格空间：push_slot 一格一件（	ry_add 会并堆，这里要的是占格）。
    for _ in 0..INVENTORY_SLOTS - 1 {
        assert!(inventory.push_slot(ItemStack::new(sword(), 1)).is_none());
    }
    assert_eq!(inventory.used(), INVENTORY_SLOTS - 1);

    // 21 个蘑菇需要两格（20 + 1），只剩一格 → 整批拒绝。
    assert_eq!(inventory.try_add(mushroom(), 21), 21, "必须原样返回没放进去的数量");
    assert_eq!(inventory.count_of(mushroom()), 0, "拒绝时一个都不许进背包");

    // 20 个只需要一格 → 放得下。
    assert_eq!(inventory.try_add(mushroom(), 20), 0);
    assert!(inventory.is_full());
}

/// 装备（`max_stack == 1`）一格一件，绝不并堆。
#[test]
fn equipment_never_merges() {
    let mut inventory = Inventory::new();
    assert_eq!(inventory.try_add(sword(), 1), 0);
    assert_eq!(inventory.try_add(sword(), 1), 0);
    assert_eq!(inventory.used(), 2, "两把剑必须占两格");
}

/// 取出会先消耗零散小堆（从后往前），并把空掉的格子清成 `None`。
#[test]
fn removing_consumes_partial_stacks_first_and_clears_empty_slots() {
    let mut inventory = Inventory::new();
    inventory.try_add(flesh(), 64); // 格 0 满
    inventory.try_add(flesh(), 3); // 格 1 零散
    assert_eq!(inventory.used(), 2);

    assert_eq!(inventory.remove(flesh(), 2), 2);
    assert_eq!(inventory.count_of(flesh()), 65, "总数少了 2");
    assert_eq!(inventory.slot(1).map(|s| s.count), Some(1), "先消耗零散那堆");
    assert_eq!(inventory.slot(0).map(|s| s.count), Some(64), "满堆不动");

    assert_eq!(inventory.remove(flesh(), 1), 1);
    assert_eq!(inventory.slot(1), None, "空掉的格子必须清成 None");
    assert_eq!(inventory.used(), 1);
}

/// 取出比库存更多时，返回实际取出的数量（不 panic、不欠账）。
#[test]
fn removing_more_than_available_reports_what_was_taken() {
    let mut inventory = Inventory::new();
    inventory.try_add(sword(), 1);
    assert_eq!(inventory.remove(sword(), 5), 1);
    assert_eq!(inventory.count_of(sword()), 0);
    assert_eq!(inventory.remove(sword(), 1), 0, "空背包取出返回 0");
}

/// `push_slot` 保留两堆分开（装备卸载、单独丢回背包用），满了原样退回。
#[test]
fn push_slot_keeps_stacks_separate() {
    let mut inventory = Inventory::new();
    assert!(inventory.push_slot(ItemStack::new(mushroom(), 3)).is_none());
    assert!(inventory.push_slot(ItemStack::new(mushroom(), 3)).is_none());
    assert_eq!(inventory.used(), 2, "push_slot 不合并同类");

    let mut full = Inventory::new();
    for _ in 0..INVENTORY_SLOTS {
        full.push_slot(ItemStack::new(sword(), 1));
    }
    let rejected = full.push_slot(ItemStack::new(sword(), 1));
    assert_eq!(rejected, Some(ItemStack::new(sword(), 1)), "满背包要原样退回");
}

/// `take_slot` 只取指定格（UI 选中那一件），越界返回 `None`。
#[test]
fn take_slot_removes_only_the_selected_slot() {
    let mut inventory = Inventory::new();
    inventory.push_slot(ItemStack::new(sword(), 1));
    inventory.push_slot(ItemStack::new(flesh(), 5));
    assert_eq!(
        inventory.take_slot(1),
        Some(ItemStack::new(flesh(), 5))
    );
    assert_eq!(inventory.slot(1), None);
    assert_eq!(inventory.slot(0), Some(ItemStack::new(sword(), 1)));
    assert_eq!(inventory.take_slot(INVENTORY_SLOTS + 3), None);
}

/// `ItemStack` 的显示串：单个不带数量，多个带 `×N`。
#[test]
fn stack_labels_read_naturally() {
    assert_eq!(ItemStack::new(sword(), 1).label(), "锈铁剑");
    assert_eq!(ItemStack::new(mushroom(), 3).label(), "蘑菇 ×3");
}

/// 数量为 0 的堆是不合法的（构造即断言）：数量归零必须变成 `None`。
#[test]
#[should_panic(expected = "数量必须 ≥ 1")]
fn a_zero_count_stack_is_rejected() {
    let _ = ItemStack::new(mushroom(), 0);
}
