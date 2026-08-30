//! 背包页（双栏：装备+背包 / 地面）+ 详情页操作（e 装备 / r 使用 / d 丢弃 / u 卸载 / g 拾取）

use std::io;

use bevy_ecs::prelude::*;
use crossterm::event::KeyCode;
use dungeon_core::OptionLogExt;
use dungeon_core::{
    Equipment, EventLog, Inventory, InventoryUI, ItemPickup, ItemStack, Player, Position,
    Renderable, ops,
};

/// 背包页面按键处理
pub(super) fn process_inventory_key(code: KeyCode, world: &mut World) -> io::Result<bool> {
    let detail;
    let detail_source;
    let detail_idx;
    let panel;
    let left_sel;
    let right_sel;
    let left_total;
    let ground_total;
    {
        let inv_state = world.resource::<InventoryUI>();
        detail = inv_state.detail;
        detail_source = inv_state.detail_source;
        detail_idx = inv_state.detail_idx;
        panel = inv_state.panel;
        left_sel = inv_state.left_sel;
        right_sel = inv_state.right_sel;
    } // drop inv_state immutable borrow
    {
        left_total = 4 + world
            .try_query::<(&Player, &Inventory)>()
            .map(|mut q| {
                q.iter(world)
                    .next()
                    .map(|(_, inv)| inv.stacks.len())
                    .unwrap_or(0)
            })
            .unwrap_or(0);
    }
    {
        let mut q = world
            .try_query::<(Entity, &Position, &ItemPickup)>()
            .expect_log("ItemPickup+Pos reg");
        let px = world
            .try_query::<(&Player, &Position)>()
            .expect_log("Player+Pos reg")
            .iter(world)
            .next()
            .map(|(_, p)| (p.x, p.y));
        ground_total = px
            .map(|(px, py)| {
                q.iter(world)
                    .filter(|(_, p, _)| p.x == px && p.y == py)
                    .count()
            })
            .unwrap_or(0);
    } // drop query borrows

    match code {
        KeyCode::Esc => {
            if detail {
                world.resource_mut::<InventoryUI>().detail = false;
            } else {
                world.resource_mut::<dungeon_action::PageStack>().pop();
            }
        }
        KeyCode::Left => world.resource_mut::<InventoryUI>().panel = false,
        KeyCode::Right => world.resource_mut::<InventoryUI>().panel = true,
        KeyCode::Up => {
            let mut s = world.resource_mut::<InventoryUI>();
            if !s.detail {
                if !panel {
                    s.left_sel = left_sel.saturating_sub(1);
                } else {
                    s.right_sel = right_sel.saturating_sub(1);
                }
            }
        }
        KeyCode::Down => {
            let mut s = world.resource_mut::<InventoryUI>();
            if !s.detail {
                if !panel {
                    s.left_sel = (left_sel + 1).min(left_total.saturating_sub(1));
                } else {
                    s.right_sel = (right_sel + 1).min(ground_total.saturating_sub(1));
                }
            }
        }
        KeyCode::Enter => {
            let mut s = world.resource_mut::<InventoryUI>();
            if !panel && left_total > 0 {
                s.detail = true;
                if left_sel < 4 {
                    s.detail_source = 1;
                    s.detail_idx = left_sel;
                } else {
                    s.detail_source = 0;
                    s.detail_idx = left_sel - 4;
                }
            } else if panel && ground_total > 0 {
                s.detail = true;
                s.detail_source = 2;
                s.detail_idx = right_sel;
            }
        }
        KeyCode::Char('e') if detail && detail_source == 0 => {
            // 装备：背包物品 → 装备槽（I57 无 slot 不可装备；I58 原子预检防旧装备丢失）
            if let Some(p) = dungeon_core::ops::player_entity(world) {
                let stack = world
                    .get::<Inventory>(p)
                    .and_then(|inv| inv.stacks.get(detail_idx).cloned());
                if let Some(s) = stack {
                    // I66/L47: 可用性判定与 UI 提示同源（detail_item_actions）
                    let actions = dungeon_core::detail_item_actions(Some(&s), 0);
                    if !actions.contains(&dungeon_core::ItemAction::Equip) {
                        world
                            .resource_mut::<EventLog>()
                            .push(dungeon_core::EventMessage::system(
                                "该物品不能装备".to_string(),
                            ));
                        world.resource_mut::<InventoryUI>().detail = false;
                        return Ok(false);
                    }
                    // actions 含 Equip ⟹ def.slot 必为 Some（同一判定来源，安全 unwrap）
                    let slot = s
                        .def()
                        .and_then(|d| d.slot)
                        .expect_log("Equip in actions implies slot");
                    let equip_name = s.def().map(|d| d.name.clone()).unwrap_or_default();
                    // I58/Dsn10 原子预检：换下的旧装备必须能放回背包，否则放弃本次操作
                    let old_stack = {
                        let eq = world.get::<Equipment>(p);
                        match slot {
                            dungeon_core::EquipmentSlot::MainHand => {
                                eq.and_then(|e| e.main_hand.clone())
                            }
                            dungeon_core::EquipmentSlot::OffHand => {
                                eq.and_then(|e| e.off_hand.clone())
                            }
                            dungeon_core::EquipmentSlot::Armor => eq.and_then(|e| e.armor.clone()),
                            dungeon_core::EquipmentSlot::Ring => eq.and_then(|e| e.ring.clone()),
                        }
                    };
                    if let Some(old) = &old_stack {
                        let can = world
                            .get::<Inventory>(p)
                            .map(|inv| inv.can_add(old.item_id, old.count))
                            .unwrap_or(false);
                        if !can {
                            world.resource_mut::<EventLog>().push(
                                dungeon_core::EventMessage::item("背包已满，无法换装".to_string()),
                            );
                            world.resource_mut::<InventoryUI>().detail = false;
                            return Ok(false);
                        }
                    }
                    // 执行换装（预检保证 add 不失败）
                    let mut inv = world
                        .get_mut::<Inventory>(p)
                        .expect_log("Player inventory exists");
                    inv.remove(detail_idx, 1);
                    let mut eq = world
                        .get_mut::<Equipment>(p)
                        .expect_log("Player equipment exists");
                    let old = match slot {
                        dungeon_core::EquipmentSlot::MainHand => eq.main_hand.replace(s),
                        dungeon_core::EquipmentSlot::OffHand => eq.off_hand.replace(s),
                        dungeon_core::EquipmentSlot::Armor => eq.armor.replace(s),
                        dungeon_core::EquipmentSlot::Ring => eq.ring.replace(s),
                    };
                    if let Some(old_stack) = old {
                        world
                            .get_mut::<Inventory>(p)
                            .expect_log("Player inventory exists")
                            .add(old_stack.item_id, old_stack.count);
                    }
                    world
                        .resource_mut::<EventLog>()
                        .push(dungeon_core::EventMessage::item(format!(
                            "装备了{}",
                            equip_name
                        )));
                }
            }
            world.resource_mut::<InventoryUI>().detail = false;
        }
        KeyCode::Char('d') if detail && detail_source == 0 => {
            // 丢弃背包物品（I66: 仅背包详情——地面详情用 g 拾取，装备槽用 u 卸载）
            let player = dungeon_core::ops::player_entity(world);
            let pos = player
                .and_then(|p| world.get::<Position>(p).map(|pp| (pp.x, pp.y)))
                .unwrap_or((0, 0));
            let idx = detail_idx;
            if let Some(p) = player {
                let stack = world.get_mut::<Inventory>(p).and_then(|mut inv| {
                    if idx < inv.stacks.len() {
                        Some(inv.stacks.remove(idx))
                    } else {
                        None
                    }
                });
                if let Some(s) = stack {
                    world.spawn((
                        ItemPickup {
                            stack: ItemStack::new(s.item_id, s.count),
                        },
                        Position { x: pos.0, y: pos.1 },
                        Renderable {
                            glyph: '?',
                            color: (180, 180, 180),
                        },
                    ));
                    world
                        .resource_mut::<EventLog>()
                        .push(dungeon_core::EventMessage::item("丢弃了物品"));
                }
            }
            world.resource_mut::<InventoryUI>().detail = false;
        }
        KeyCode::Char('u') if detail && detail_source == 1 => {
            // 卸载装备：Equipment 槽位 → 背包
            if let Some(p) = dungeon_core::ops::player_entity(world) {
                // I73: 槽位访问统一走 Equipment::slot/slot_mut
                let (can_unequip, slot_empty, item_name, item_id, item_count) = {
                    let eq = world.get::<Equipment>(p);
                    let inv = world.get::<Inventory>(p);
                    match (eq, inv) {
                        (Some(eq), Some(inv)) => match eq.slot(detail_idx).as_ref() {
                            Some(s) => (
                                inv.can_add(s.item_id, s.count),
                                false,
                                s.name(),
                                s.item_id,
                                s.count,
                            ),
                            None => (false, true, String::new(), 0, 0),
                        },
                        _ => (false, true, String::new(), 0, 0),
                    }
                };
                if can_unequip {
                    let mut eq = world
                        .get_mut::<Equipment>(p)
                        .expect_log("Player equipment exists");
                    eq.slot_mut(detail_idx).take();
                    world
                        .get_mut::<Inventory>(p)
                        .expect_log("Player inventory exists")
                        .add(item_id, item_count);
                    world
                        .resource_mut::<EventLog>()
                        .push(dungeon_core::EventMessage::item(format!(
                            "卸载了{}",
                            item_name
                        )));
                } else if slot_empty {
                    // I66: 空槽位按 u 不应误报"背包已满"
                    world
                        .resource_mut::<EventLog>()
                        .push(dungeon_core::EventMessage::system(
                            "该槽位没有装备".to_string(),
                        ));
                } else {
                    world
                        .resource_mut::<EventLog>()
                        .push(dungeon_core::EventMessage::item("背包已满".to_string()));
                }
            }
            world.resource_mut::<InventoryUI>().detail = false;
        }
        KeyCode::Char('r') if detail && detail_source == 0 => {
            // I61: 使用/学习物品（卷轴学习技能 / 模板碎片合成）；不可用物品提示
            if let Some(p) = dungeon_core::ops::player_entity(world) {
                let item_id = world
                    .get::<Inventory>(p)
                    .and_then(|inv| inv.stacks.get(detail_idx).map(|s| s.item_id));
                if let Some(id) = item_id {
                    if dungeon_core::is_usable(id) {
                        // 合成/学习失败的原因由 use_item 内部推送（如材料不足）
                        let consumed = dungeon_core::use_item(id, world, p);
                        if consumed && let Some(mut inv) = world.get_mut::<Inventory>(p) {
                            inv.remove(detail_idx, 1);
                        }
                    } else {
                        world
                            .resource_mut::<EventLog>()
                            .push(dungeon_core::EventMessage::system(
                                "该物品不能直接使用".to_string(),
                            ));
                    }
                }
            }
            world.resource_mut::<InventoryUI>().detail = false;
        }
        KeyCode::Char('g') if detail && detail_source == 2 => {
            // 拾取当前查看的地面物品（I66: 与 UI 提示 g 对齐——原实现详情页按 g 无反应）
            if let Some(p) = dungeon_core::ops::player_entity(world) {
                let pos = world
                    .get::<Position>(p)
                    .map(|pp| (pp.x, pp.y))
                    .unwrap_or((0, 0));
                let idx = detail_idx;
                let target = {
                    let mut q = world
                        .try_query::<(Entity, &ItemPickup, &Position)>()
                        .expect_log("ItemPickup+Position registered at init");
                    let items: Vec<Entity> = q
                        .iter(world)
                        .filter(|(_, _, pp)| pp.x == pos.0 && pp.y == pos.1)
                        .map(|(e, _, _)| e)
                        .collect();
                    items.get(idx).copied()
                };
                if let Some(e) = target {
                    if dungeon_core::ops::pickup_ground_item(world, e) {
                        world
                            .resource_mut::<EventLog>()
                            .push(dungeon_core::EventMessage::item("拾取了物品"));
                    } else {
                        world
                            .resource_mut::<EventLog>()
                            .push(dungeon_core::EventMessage::item("背包已满".to_string()));
                    }
                }
            }
            world.resource_mut::<InventoryUI>().detail = false;
        }
        KeyCode::Char('g') if !panel && !detail => {
            ops::pickup_ground(world);
        }
        // README 声称的 0-9/a-z 快捷选中（L47 违规：UI 一直显示热键但处理器不响应——G27 补上）。
        // 放在 'g' 分支之后：'g' 的拾取语义优先；'e'/'d'/'u'/'r' 均有 detail guard，非详情页时落到此分支。
        KeyCode::Char(c) if c.is_ascii_alphanumeric() && !detail => {
            if let Some(idx) = hotkey_to_idx(c) {
                let mut s = world.resource_mut::<InventoryUI>();
                if !panel {
                    // 背包段：热键 0-9/a-z ↔ 背包第 0-35 个物品（与 ui.rs 热键显示一致）
                    if idx + 4 < left_total {
                        s.left_sel = 4 + idx;
                    }
                } else if idx < ground_total {
                    s.right_sel = idx;
                }
            }
        }
        _ => {}
    }
    Ok(false)
}

/// 热键字符 → 列表索引（G27）：'0'-'9' → 0-9，'a'-'z' → 10-35（背包容量 36 恰好覆盖）。
fn hotkey_to_idx(c: char) -> Option<usize> {
    if c.is_ascii_digit() {
        Some(c as usize - '0' as usize)
    } else if c.is_ascii_lowercase() {
        Some(c as usize - 'a' as usize + 10)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::hotkey_to_idx;

    #[test]
    fn test_hotkey_mapping() {
        assert_eq!(hotkey_to_idx('0'), Some(0));
        assert_eq!(hotkey_to_idx('9'), Some(9));
        assert_eq!(hotkey_to_idx('a'), Some(10));
        assert_eq!(hotkey_to_idx('z'), Some(35));
        // 大写与符号不是热键（输入线程已统一小写，但防御性验证）
        assert_eq!(hotkey_to_idx('A'), None);
        assert_eq!(hotkey_to_idx(' '), None);
    }
}
