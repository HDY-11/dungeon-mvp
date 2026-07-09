//! 背包界面（双栏：左侧装备+背包，右侧地面物品）

use std::io;
use std::time::Instant;

use ratatui::{
    layout::{Alignment, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Terminal,
};
use crossterm::event::{self, Event, KeyCode};

use bevy_ecs::prelude::*;
use dungeon_core::{
    ops, Equipment, EquipmentSlot, EventLog, Inventory, ItemPickup, ItemStack,
    Player, Position, Renderable,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum InvPanel { Left, Right }

#[derive(Clone, Copy, PartialEq, Eq)]
enum DetailSource { LeftInv, LeftEquip, Right }

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    List(InvPanel),
    Detail(DetailSource, usize),
}

fn collect_ground_items_in(world: &World) -> Vec<(ItemStack, Entity)> {
    let pp = world.try_query::<(&Player, &Position)>().expect("Player+Position registered at init").iter(world).next().map(|(_, p)| (p.x, p.y)).unwrap_or((0, 0));
    let mut items = Vec::new();
    let mut q = world.try_query::<(Entity, &ItemPickup, &Position)>().expect("Entity+ItemPickup+Position registered at init");
    for (entity, pickup, pos) in q.iter(world) {
        if pos.x == pp.0 && pos.y == pp.1 {
            items.push((pickup.stack.clone(), entity));
        }
    }
    items
}

pub fn open_inventory(
    terminal: &mut Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
    world: &mut World,
    game_start: Instant,
) -> io::Result<()> {
    let mut left_sel: usize = 0;
    let mut right_sel: usize = 0;
    let mut page = Page::List(InvPanel::Left);

    let left_count = |_eq: &Equipment, inv: &Inventory| -> usize { 4 + inv.stacks.len() };

    loop {
        let (inv_stacks, inv_cap, equip, ground) = {
            let mut q = world.try_query::<(&Inventory, &Equipment)>().expect("Inventory+Equipment registered at init");
            let (inv, eq) = q.iter(world).next()
                .map(|(i, e)| (i.clone(), e.clone())).unwrap_or_default();
            let ground = collect_ground_items_in(world);
            (inv.stacks, inv.capacity, eq, ground)
        };

        let left_total = left_count(&equip, &Inventory { stacks: inv_stacks.clone(), capacity: inv_cap });
        if page == Page::List(InvPanel::Left) && left_total > 0 && left_sel >= left_total {
            left_sel = left_total - 1;
        }
        if page == Page::List(InvPanel::Right) && !ground.is_empty() && right_sel >= ground.len() {
            right_sel = ground.len() - 1;
        }

        terminal.draw(|frame| {
            let area = frame.area();
            let block = Block::default()
                .title("  背包  ").title_alignment(Alignment::Center)
                .borders(Borders::ALL).border_style(Style::default().fg(Color::Cyan));
            frame.render_widget(block, area);
            let inner = Rect { x: area.x + 1, y: area.y + 1, width: area.width.saturating_sub(2), height: area.height.saturating_sub(2) };

            if let Page::Detail(dsrc, idx) = &page {
                let (stack, source_label) = match dsrc {
                    DetailSource::LeftEquip => ([&equip.main_hand, &equip.off_hand, &equip.armor, &equip.ring][*idx].as_ref(), "装备"),
                    DetailSource::LeftInv => (inv_stacks.get(*idx), "背包"),
                    DetailSource::Right => (ground.get(*idx).map(|(s, _)| s), "地面"),
                };
                if let Some(item) = stack {
                    let mut lines = vec![
                        Line::from(Span::styled(format!(" ── {} ──", source_label), Style::default().fg(Color::DarkGray))),
                        Line::from(Span::raw("")),
                        Line::from(Span::styled(format!(" {}", item.name()), Style::default().fg(Color::Yellow).bold())),
                    ];
                    if item.count > 1 {
                        lines.push(Line::from(Span::styled(format!(" 数量: {}", item.count), Style::default().fg(Color::White))));
                    }
                    if let Some(d) = item.def() {
                        let class_str = d.class.display_name();
                        let slot_str = d.slot.map(|s| format!("{:?}", s)).unwrap_or_default();
                        lines.push(Line::from(vec![
                            Span::styled(format!(" 类别: {}", class_str), Style::default().fg(Color::DarkGray)),
                            if !slot_str.is_empty() { Span::styled(format!(" [{}]", slot_str), Style::default().fg(Color::DarkGray)) } else { Span::raw("") },
                        ]));
                        let b = &d.bonus;
                        let mut parts = Vec::new();
                        if b.attack != 0 { parts.push(format!("攻击{:+}", b.attack)); }
                        if b.defense != 0 { parts.push(format!("防御{:+}", b.defense)); }
                        if b.magic_mastery != 0 { parts.push(format!("法术精通{:+}", b.magic_mastery)); }
                        if b.agility != 0 { parts.push(format!("敏捷{:+}", b.agility)); }
                        if b.hp != 0 { parts.push(format!("HP{:+}", b.hp)); }
                        if b.crit_rate != 0.0 { parts.push(format!("暴击率{:.0}%", b.crit_rate * 100.0)); }
                        if !parts.is_empty() {
                            lines.push(Line::from(Span::styled(format!(" {}", parts.join(" ")), Style::default().fg(Color::Green))));
                        }
                    }
                    lines.push(Line::from(Span::raw("")));
                    let desc = item.description();
                    if !desc.is_empty() {
                        lines.push(Line::from(Span::styled(format!(" {}", desc), Style::default().fg(Color::DarkGray))));
                    }
                    lines.push(Line::from(Span::raw("")));
                    match dsrc {
                        DetailSource::LeftEquip => lines.push(Line::from(Span::styled(" u:卸载装备", Style::default().fg(Color::DarkGray)))),
                        DetailSource::LeftInv => {
                            if item.def().is_some_and(|d| d.slot.is_some()) {
                                lines.push(Line::from(Span::styled(" e:装备  d:丢弃  r:使用/学习", Style::default().fg(Color::DarkGray))));
                            } else {
                                lines.push(Line::from(Span::styled(" d:丢弃  r:使用/学习", Style::default().fg(Color::DarkGray))));
                            }
                        }
                        DetailSource::Right => lines.push(Line::from(Span::styled(" g:拾取", Style::default().fg(Color::DarkGray)))),
                    }
                    lines.push(Line::from(Span::styled(" Esc:返回", Style::default().fg(Color::DarkGray))));
                    frame.render_widget(Paragraph::new(lines), inner);
                }
            } else {
                let half = inner.width / 2;
                let left_area = Rect { x: inner.x, y: inner.y, width: half, height: inner.height };
                let right_area = Rect { x: inner.x + half, y: inner.y, width: inner.width - half, height: inner.height };
                // Left panel
                {
                    let mut lines = vec![];
                    let act = page == Page::List(InvPanel::Left);
                    let ts = if act { Style::default().fg(Color::Cyan).bold() } else { Style::default().fg(Color::DarkGray) };
                    lines.push(Line::from(Span::styled(" ── 装备 ──", ts)));
                    for i in 0..4 {
                        let item = [&equip.main_hand, &equip.off_hand, &equip.armor, &equip.ring][i];
                        let name = item.as_ref().map(|s| s.name()).unwrap_or("(空)".into());
                        let p = if act && left_sel == i { "▸" } else { " " };
                        lines.push(Line::from(vec![
                            Span::styled(format!("{}{}", p, i), Style::default().fg(Color::Yellow)),
                            Span::styled([" [主]", " [副]", " [防]", " [戒]"][i], Style::default().fg(Color::DarkGray)),
                            Span::raw(format!(" {}", name)),
                        ]));
                    }
                    lines.push(Line::from(Span::styled(format!(" ── 背包 ({}/{}) ──", inv_stacks.len(), inv_cap), ts)));
                    if inv_stacks.is_empty() {
                        lines.push(Line::from(Span::styled(" (空)", Style::default().fg(Color::DarkGray))));
                    } else {
                        let ps = (left_area.height as usize).saturating_sub(8).min(15);
                        for i in 0..inv_stacks.len().min(ps) {
                            let real = i + 4;
                            let stack = &inv_stacks[i];
                            let p = if act && left_sel == real { "▸" } else { " " };
                            let hk = if i < 10 { char::from_digit(i as u32, 10).expect("i < 10") } else { char::from(b'a' + (i - 10) as u8) };
                            let cl = if stack.count > 1 { format!(" x{}", stack.count) } else { String::new() };
                            lines.push(Line::from(vec![
                                Span::styled(format!("{}{}", p, hk), Style::default().fg(Color::Yellow)),
                                Span::raw(format!(" {}{}", stack.name(), cl)),
                            ]));
                        }
                    }
                    if act { lines.push(Line::from(Span::styled(" 0-9a-z:选中 Enter:查看 e:装备 d:丢弃", Style::default().fg(Color::DarkGray)))); }
                    frame.render_widget(Paragraph::new(lines), left_area);
                }
                // Right panel
                {
                    let mut lines = vec![];
                    let act = page == Page::List(InvPanel::Right);
                    let ts = if act { Style::default().fg(Color::Cyan).bold() } else { Style::default().fg(Color::DarkGray) };
                    lines.push(Line::from(Span::styled(" ── 地面 ──", ts)));
                    if ground.is_empty() {
                        lines.push(Line::from(Span::styled(" (无物品)", Style::default().fg(Color::DarkGray))));
                    } else {
                        for i in 0..ground.len().min(10) {
                            let (stack, _) = &ground[i];
                            let p = if act && right_sel == i { "▸" } else { " " };
                            let cl = if stack.count > 1 { format!(" x{}", stack.count) } else { String::new() };
                            lines.push(Line::from(vec![
                                Span::styled(p.to_string(), Style::default().fg(Color::Yellow)),
                                Span::raw(format!(" {}{}", stack.name(), cl)),
                            ]));
                        }
                    }
                    if act && !ground.is_empty() {
                        lines.push(Line::from(Span::styled(" Enter:查看  g:拾取全部", Style::default().fg(Color::DarkGray))));
                    }
                    frame.render_widget(Paragraph::new(lines), right_area);
                }
            }
        })?;

        if let Event::Key(key) = event::read()? {
            match (&page, key.code) {
                (Page::Detail(_, _), KeyCode::Esc | KeyCode::Char('q')) => {
                    page = Page::List(InvPanel::Left);
                }
                (Page::List(_), KeyCode::Esc | KeyCode::Char('q')) => break,
                (Page::List(_), KeyCode::Left) => page = Page::List(InvPanel::Left),
                (Page::List(_), KeyCode::Right) => page = Page::List(InvPanel::Right),
                (Page::List(InvPanel::Left), KeyCode::Up) => { left_sel = left_sel.saturating_sub(1); }
                (Page::List(InvPanel::Right), KeyCode::Up) => { right_sel = right_sel.saturating_sub(1); }
                (Page::List(InvPanel::Left), KeyCode::Down) => { if left_sel + 1 < left_total { left_sel += 1; } }
                (Page::List(InvPanel::Right), KeyCode::Down) => { if right_sel + 1 < ground.len() { right_sel += 1; } }
                (Page::List(InvPanel::Left), KeyCode::Enter) => {
                    if left_sel < 4 {
                        if [&equip.main_hand, &equip.off_hand, &equip.armor, &equip.ring][left_sel].is_some() {
                            page = Page::Detail(DetailSource::LeftEquip, left_sel);
                        }
                    } else if left_sel - 4 < inv_stacks.len() {
                        page = Page::Detail(DetailSource::LeftInv, left_sel - 4);
                    }
                }
                (Page::List(InvPanel::Right), KeyCode::Enter) => {
                    if !ground.is_empty() {
                        page = Page::Detail(DetailSource::Right, right_sel);
                    }
                }
                // 列表页热键
                (Page::List(InvPanel::Left), KeyCode::Char(ch)) if ch.is_ascii_lowercase() || ch.is_ascii_digit() => {
                    let idx = if ch.is_ascii_digit() { ch as usize - '0' as usize } else { ch as usize - 'a' as usize + 10 };
                    let real = idx + 4;
                    if real < left_total {
                        left_sel = real;
                        if real < 4 {
                            if [&equip.main_hand, &equip.off_hand, &equip.armor, &equip.ring][real].is_some() {
                                page = Page::Detail(DetailSource::LeftEquip, real);
                            }
                        } else if real - 4 < inv_stacks.len() {
                            page = Page::Detail(DetailSource::LeftInv, real - 4);
                        }
                    }
                }
                // 详情页操作 — 石子投掷（自动装副手→瞄准→消耗副手）
                (Page::Detail(DetailSource::LeftInv, idx), KeyCode::Char('r'))
                    if world.query::<(&Inventory,)>().iter(world).next()
                        .and_then(|(inv,)| inv.stacks.get(*idx))
                        .map(|s| s.item_id == dungeon_core::ITEM_STONE)
                        .unwrap_or(false) =>
                {
                    // 从背包取出石子，装到副手（旧副手物品放回背包）
                    if ops::player_entity(world).is_some() {
                        let mut q = world.query::<(&mut Inventory, &mut Equipment)>();
                        if let Some((mut inv, mut eq)) = q.iter_mut(world).next() {
                            if let Some(stack) = inv.stacks.get(*idx).cloned() {
                                // 旧副手回背包
                                if let Some(old) = eq.off_hand.take() {
                                    inv.add(old.item_id, old.count);
                                }
                                // 从背包移除（此时 idx 可能已变，重查位置）
                                if let Some(pos) = inv.stacks.iter().position(|s| s.item_id == stack.item_id) {
                                    inv.stacks.remove(pos);
                                }
                                eq.off_hand = Some(stack);
                            }
                        }
                    }
                    crate::throw::open_throw_aim(terminal, world, game_start)?;
                    page = Page::List(InvPanel::Left);
                }
                (Page::Detail(DetailSource::LeftInv, idx), KeyCode::Char('e')) => {
                    let w2 = &mut *world;
                    let mut q = w2.query::<(&mut Inventory, &mut Equipment)>();
                    if let Some((mut inv, mut eq)) = q.iter_mut(w2).next()
                        && let Some(stack) = inv.stacks.get(*idx) {
                            let def = stack.def();
                            if let Some(slot) = def.and_then(|d| d.slot) {
                                let stack = stack.clone();
                                inv.remove(*idx, 1);
                                let old = match slot {
                                    EquipmentSlot::MainHand => eq.main_hand.replace(stack),
                                    EquipmentSlot::OffHand => eq.off_hand.replace(stack),
                                    EquipmentSlot::Armor => eq.armor.replace(stack),
                                    EquipmentSlot::Ring => eq.ring.replace(stack),
                                };
                                if let Some(old_stack) = old {
                                    inv.add(old_stack.item_id, old_stack.count);
                                }
                                w2.resource_mut::<EventLog>().push(format!("装备了{}", def.expect("Item def should exist").name));
                            } else {
                                w2.resource_mut::<EventLog>().push("该物品无法装备");
                            }
                        }
                    page = Page::List(InvPanel::Left);
                }
                (Page::Detail(DetailSource::LeftInv, idx), KeyCode::Char('d')) => {
                    let w2 = &mut *world;
                    let mut q = w2.query::<(&mut Inventory, &Position)>();
                    if let Some((mut inv, pos)) = q.iter_mut(w2).next()
                        && let Some(stack) = inv.drop_stack(*idx) {
                            let (px, py) = (pos.x, pos.y);
                            let name = stack.name();
                            w2.spawn((
                                ItemPickup { stack: stack.clone() },
                                Position { x: px, y: py },
                                Renderable { glyph: stack.glyph(), color: stack.color() },
                            ));
                            w2.resource_mut::<EventLog>().push(format!("丢弃了{}x{}在脚下", name, stack.count));
                        }
                    page = Page::List(InvPanel::Left);
                }
                (Page::Detail(DetailSource::LeftInv, idx), KeyCode::Char('r')) => {
                    // 获取物品和玩家
                    let item_id = {
                        let mut q = world.query::<(&Inventory,)>();
                        q.iter(world).next()
                            .and_then(|(inv,)| inv.stacks.get(*idx))
                            .map(|s| s.item_id)
                    };
                    let player = ops::player_entity(world);
                    // 通过集中分派函数使用物品
                    let consumed = item_id.is_some_and(|id| {
                        player.is_some_and(|p| dungeon_core::use_item(id, world, p))
                    });
                    if consumed {
                        let mut q = world.query::<(&mut Inventory,)>();
                        if let Some((mut inv,)) = q.iter_mut(world).next() {
                            inv.remove(*idx, 1);
                        }
                    } else {
                        world.resource_mut::<EventLog>().push("无法使用该物品".to_string());
                    }
                    page = Page::List(InvPanel::Left);
                }
                (Page::Detail(DetailSource::LeftEquip, idx), KeyCode::Char('u')) => {
                    let w2 = &mut *world;
                    let mut q = w2.query::<(&mut Inventory, &mut Equipment)>();
                    if let Some((mut inv, mut eq)) = q.iter_mut(w2).next() {
                        let slot = match idx {
                            0 => &mut eq.main_hand,
                            1 => &mut eq.off_hand,
                            2 => &mut eq.armor,
                            _ => &mut eq.ring,
                        };
                        let can_add = slot.as_ref().is_some_and(|s| inv.can_add(s.item_id, s.count));
                        if can_add {
                            let stack = slot.take().expect("Slot was checked as can_add");
                            inv.add(stack.item_id, stack.count);
                        } else if slot.is_some() {
                            w2.resource_mut::<EventLog>().push("背包已满");
                        }
                    }
                    page = Page::List(InvPanel::Left);
                }
                (Page::Detail(DetailSource::Right, _idx), KeyCode::Char('g')) => {
                    ops::pickup_ground(world);
                    page = Page::List(InvPanel::Left);
                }
                (Page::Detail(_, _), _) => {}
                _ => {}
            }
        }
    }
    Ok(())
}
