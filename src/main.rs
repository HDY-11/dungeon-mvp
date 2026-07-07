use std::io::{self, stdout};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use bevy_ecs::prelude::*;
use bevy_ecs::system::RunSystemOnce;
use crossterm::event::{self, Event, KeyCode};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::ExecutableCommand;
use dungeon_core::{
    ops, Equipment, EquipmentSlot, EventLog, Inventory, ItemPickup, ItemStack,
    Player, Position, TurnManager,
};
use dungeon_action::{handle_player_direction, handle_wait, handle_skill};
use dungeon_world::{setup_world, descend, GameSave, fov_system, advance_and_settle_parallel as advance_and_settle};
use dungeon_render::{draw_title, render_ui};
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Terminal;

fn main() -> io::Result<()> {
    std::panic::set_hook(Box::new(|info| {
        let msg = format!("PANIC: {}\n", info);
        std::fs::write("panic.log", msg).ok();
    }));

    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let mut terminal = Terminal::new(ratatui::backend::CrosstermBackend::new(stdout()))?;
    let (mut world, game_start) = title_screen(&mut terminal)?;
    let result = run(&mut terminal, game_start, &mut world);
    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    result
}

fn run(
    terminal: &mut Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
    game_start: Instant,
    world: &mut World,
) -> io::Result<()> {
    ops::rebuild_occupancy(world);
    let _ = world.run_system_once(fov_system);
    ops::update_visible_memory(world);
    {
        let w: &World = &*world;
        terminal.draw(|frame| render_ui(frame, game_start, w))?;
    }

    let (tx, rx) = mpsc::channel::<KeyCode>();
    let modal_flag = Arc::new(AtomicBool::new(false));
    let thread_flag = modal_flag.clone();
    thread::spawn(move || {
        let mut last_code: KeyCode = KeyCode::Null;
        let mut last_time = Instant::now();
        loop {
            if thread_flag.load(Ordering::Relaxed) {
                thread::sleep(Duration::from_millis(16));
                continue;
            }
            if crossterm::event::poll(Duration::from_millis(16)).unwrap_or(false) {
                if let Event::Key(key) = crossterm::event::read().unwrap() {
                    let now = Instant::now();
                    if key.code == last_code && now - last_time < Duration::from_millis(50) {
                        continue;
                    }
                    last_code = key.code;
                    last_time = now;
                    if tx.send(key.code).is_err() { break; }
                }
            }
        }
    });

    loop {
        let has_action = match rx.try_recv() {
            Ok(code) => process_key(code, terminal, &modal_flag, world)?,
            Err(mpsc::TryRecvError::Empty) => {
                std::thread::sleep(Duration::from_millis(1));
                false
            }
            Err(mpsc::TryRecvError::Disconnected) => break Ok(()),
        };

        if has_action {
            advance_and_settle(world);
        }

        {
            let w: &World = &*world;
            terminal.draw(|frame| render_ui(frame, game_start, w))?;
        }

        if world.resource::<TurnManager>().wants_quit {
            break Ok(());
        }
    }
}

fn pickup_ground(world: &mut World) {
    ops::pickup_ground(world)
}

fn on_stairs(world: &World) -> bool {
    ops::on_stairs(world)
}

fn process_key(
    code: KeyCode,
    terminal: &mut Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
    modal_flag: &AtomicBool,
    world: &mut World,
) -> io::Result<bool> {
    match code {
        KeyCode::Up      => Ok(handle_player_direction(world, 0, -1)),
        KeyCode::Down    => Ok(handle_player_direction(world, 0, 1)),
        KeyCode::Left    => Ok(handle_player_direction(world, -1, 0)),
        KeyCode::Right   => Ok(handle_player_direction(world, 1, 0)),
        KeyCode::Home    => Ok(handle_player_direction(world, -1, -1)),
        KeyCode::End     => Ok(handle_player_direction(world, -1, 1)),
        KeyCode::PageUp  => Ok(handle_player_direction(world, 1, -1)),
        KeyCode::PageDown => Ok(handle_player_direction(world, 1, 1)),
        KeyCode::Char('.') => Ok(handle_wait(world)),
        KeyCode::Char('1') => Ok(handle_skill(world, 0)),
        KeyCode::Char('2') => Ok(handle_skill(world, 1)),
        KeyCode::Char('3') => Ok(handle_skill(world, 2)),
        KeyCode::Char('4') => Ok(handle_skill(world, 3)),
        KeyCode::Char('q') | KeyCode::Esc => {
            modal_flag.store(true, Ordering::Relaxed);
            let confirmed = open_modal(terminal, "确认退出？");
            modal_flag.store(false, Ordering::Relaxed);
            if confirmed { world.resource_mut::<TurnManager>().wants_quit = true; }
            Ok(false)
        }
        KeyCode::Char('e') | KeyCode::Char('E') => {
            modal_flag.store(true, Ordering::Relaxed);
            open_inventory(terminal, world)?;
            modal_flag.store(false, Ordering::Relaxed);
            Ok(false)
        }
        KeyCode::Char('g') | KeyCode::Char('G') => {
            pickup_ground(world);
            Ok(false)
        }
        KeyCode::F(5) => {
            if let Ok(data) = bincode::serialize(&GameSave::capture(world)) {
                std::fs::write("save.bin", data).ok();
                world.resource_mut::<EventLog>().push("已保存");
            }
            Ok(false)
        }
        KeyCode::F(9) => {
            if let Ok(data) = std::fs::read("save.bin") {
                if let Ok(save) = bincode::deserialize::<GameSave>(&data) {
                    save.restore(world);
                    world.resource_mut::<EventLog>().push("已读档");
                }
            }
            Ok(false)
        }
        KeyCode::Char('>') => {
            if on_stairs(world) {
                modal_flag.store(true, Ordering::Relaxed);
                let ok = open_modal(terminal, "确认下楼？");
                modal_flag.store(false, Ordering::Relaxed);
                if ok {
                    descend(world);
                    let _ = world.run_system_once(fov_system);
                    ops::update_map_memory(world);
                    ops::update_visible_memory(world);
                    ops::rebuild_occupancy(world);
                }
            }
            Ok(false)
        }
        _ => Ok(false),
    }
}

fn open_modal(
    terminal: &mut Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
    title: &str,
) -> bool {
    let _ = terminal.draw(|frame| {
        let area = frame.area();
        let msg = Paragraph::new(vec![
            Line::from(Span::styled(title, Style::default().fg(Color::Yellow).bold())),
            Line::from(Span::styled(" Y)是  N)否", Style::default().fg(Color::DarkGray))),
        ])
        .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::Yellow)))
        .alignment(Alignment::Center);
        frame.render_widget(msg, Rect {
            x: area.width / 2 - 12, y: area.height / 2,
            width: 24, height: 5,
        });
    });
    loop {
        if let Ok(Event::Key(k)) = event::read() {
            return matches!(k.code, KeyCode::Char('y') | KeyCode::Char('Y'));
        }
    }
}

fn title_screen(
    terminal: &mut Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
) -> io::Result<(World, Instant)> {
    loop {
        terminal.draw(|frame| draw_title(frame))?;
        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Enter | KeyCode::Char('\n') | KeyCode::Char('\r') => {
                    let mut world = setup_world();
                    let _ = world.run_system_once(fov_system);
                    ops::update_map_memory(&mut world);
                    ops::update_visible_memory(&mut world);
                    return Ok((world, Instant::now()));
                }
                KeyCode::F(9) => {
                    if let Ok(data) = std::fs::read("save.bin") {
                        if let Ok(save) = bincode::deserialize::<GameSave>(&data) {
                            let mut world = setup_world();
                            save.restore(&mut world);
                            let _ = world.run_system_once(fov_system);
                            ops::update_map_memory(&mut world);
                            ops::update_visible_memory(&mut world);
                            return Ok((world, Instant::now()));
                        }
                    }
                }
                KeyCode::Char('q') | KeyCode::Esc => {
                    disable_raw_mode()?;
                    stdout().execute(LeaveAlternateScreen)?;
                    std::process::exit(0);
                }
                _ => {}
            }
        }
    }
}

// ══════════════════════════════════════════════════════
// 背包界面
// ══════════════════════════════════════════════════════

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
    let pp = world.try_query::<(&Player, &Position)>().unwrap().iter(world).next().map(|(_, p)| (p.x, p.y)).unwrap_or((0, 0));
    let mut items = Vec::new();
    let mut q = world.try_query::<(Entity, &ItemPickup, &Position)>().unwrap();
    for (entity, pickup, pos) in q.iter(world) {
        if pos.x == pp.0 && pos.y == pp.1 {
            items.push((pickup.stack.clone(), entity));
        }
    }
    items
}

fn open_inventory(
    terminal: &mut Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
    world: &mut World,
) -> io::Result<()> {
    let mut left_sel: usize = 0;
    let _left_scroll: usize = 0;
    let mut right_sel: usize = 0;
    let _right_scroll: usize = 0;
    let mut page = Page::List(InvPanel::Left);

    fn left_count(_eq: &Equipment, inv: &Inventory) -> usize {
        3 + inv.stacks.len()
    }

    loop {
        let (inv_stacks, inv_cap, equip, ground) = {
            let mut q = world.try_query::<(&Inventory, &Equipment)>().unwrap();
            let (inv, eq) = q.iter(world).next()
                .map(|(i, e)| (i.clone(), e.clone())).unwrap_or_default();
            let ground = collect_ground_items_in(world);
            (inv.stacks, inv.capacity, eq, ground)
        };

        let left_total = left_count(&equip, &Inventory { stacks: inv_stacks.clone(), capacity: inv_cap });
        if page == Page::List(InvPanel::Left) && left_total > 0 && left_sel >= left_total {
            left_sel = left_total - 1;
        }
        if page == Page::List(InvPanel::Right) && ground.len() > 0 && right_sel >= ground.len() {
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
                let (stack, source_label, _is_equip_slot) = match dsrc {
                    DetailSource::LeftEquip => ([&equip.weapon, &equip.armor, &equip.ring][*idx].as_ref(), "装备", true),
                    DetailSource::LeftInv => (inv_stacks.get(*idx), "背包", false),
                    DetailSource::Right => (ground.get(*idx).map(|(s, _)| s), "地面", false),
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
                            if item.def().map_or(false, |d| d.slot.is_some()) {
                                lines.push(Line::from(Span::styled(" e:装备  d:丢弃", Style::default().fg(Color::DarkGray))));
                            } else {
                                lines.push(Line::from(Span::styled(" d:丢弃", Style::default().fg(Color::DarkGray))));
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
                    for i in 0..3 {
                        let item = [&equip.weapon, &equip.armor, &equip.ring][i];
                        let name = item.as_ref().map(|s| s.name()).unwrap_or("(空)".into());
                        let p = if act && left_sel == i { "▸" } else { " " };
                        lines.push(Line::from(vec![
                            Span::styled(format!("{}{}", p, i), Style::default().fg(Color::Yellow)),
                            Span::styled([" [武]", " [防]", " [戒]"][i], Style::default().fg(Color::DarkGray)),
                            Span::raw(format!(" {}", name)),
                        ]));
                    }
                    lines.push(Line::from(Span::styled(format!(" ── 背包 ({}/{}) ──", inv_stacks.len(), inv_cap), ts)));
                    if inv_stacks.is_empty() {
                        lines.push(Line::from(Span::styled(" (空)", Style::default().fg(Color::DarkGray))));
                    } else {
                        let ps = (left_area.height as usize).saturating_sub(8).min(15);
                        for i in 0..inv_stacks.len().min(ps) {
                            let real = i + 3;
                            let stack = &inv_stacks[i];
                            let p = if act && left_sel == real { "▸" } else { " " };
                            let hk = if i < 10 { char::from_digit(i as u32, 10).unwrap() } else { char::from(b'a' + (i - 10) as u8) };
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
                                Span::styled(format!("{}", p), Style::default().fg(Color::Yellow)),
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
                (Page::List(InvPanel::Left), KeyCode::Up) => { if left_sel > 0 { left_sel -= 1; } }
                (Page::List(InvPanel::Right), KeyCode::Up) => { if right_sel > 0 { right_sel -= 1; } }
                (Page::List(InvPanel::Left), KeyCode::Down) => { if left_sel + 1 < left_total { left_sel += 1; } }
                (Page::List(InvPanel::Right), KeyCode::Down) => { if right_sel + 1 < ground.len() { right_sel += 1; } }
                (Page::List(InvPanel::Left), KeyCode::Enter) => {
                    if left_sel < 3 {
                        if [&equip.weapon, &equip.armor, &equip.ring][left_sel].is_some() {
                            page = Page::Detail(DetailSource::LeftEquip, left_sel);
                        }
                    } else if left_sel - 3 < inv_stacks.len() {
                        page = Page::Detail(DetailSource::LeftInv, left_sel - 3);
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
                    let real = idx + 3;
                    if real < left_total {
                        left_sel = real;
                        // 自动打开详情
                        if real < 3 {
                            if [&equip.weapon, &equip.armor, &equip.ring][real].is_some() {
                                page = Page::Detail(DetailSource::LeftEquip, real);
                            }
                        } else if real - 3 < inv_stacks.len() {
                            page = Page::Detail(DetailSource::LeftInv, real - 3);
                        }
                    }
                }
                // 详情页操作
                (Page::Detail(DetailSource::LeftInv, idx), KeyCode::Char('e')) => {
                    let w2 = &mut *world;
                    let mut q = w2.query::<(&mut Inventory, &mut Equipment)>();
                    if let Some((mut inv, mut eq)) = q.iter_mut(w2).next() {
                        if let Some(stack) = inv.stacks.get(*idx) {
                            let def = stack.def();
                            if let Some(slot) = def.and_then(|d| d.slot) {
                                let stack = stack.clone();
                                inv.remove(*idx, 1);
                                let old = match slot {
                                    EquipmentSlot::Weapon => eq.weapon.replace(stack),
                                    EquipmentSlot::Armor => eq.armor.replace(stack),
                                    EquipmentSlot::Ring => eq.ring.replace(stack),
                                };
                                if let Some(old_stack) = old {
                                    inv.add(old_stack.item_id, old_stack.count);
                                }
                                w2.resource_mut::<EventLog>().push(format!("装备了{}", def.unwrap().name));
                            } else {
                                w2.resource_mut::<EventLog>().push("该物品无法装备");
                            }
                        }
                    }
                    page = Page::List(InvPanel::Left);
                }
                (Page::Detail(DetailSource::LeftInv, idx), KeyCode::Char('d')) => {
                    let w2 = &mut *world;
                    let mut q = w2.query::<(&mut Inventory,)>();
                    if let Some((mut inv,)) = q.iter_mut(w2).next() {
                        inv.drop_stack(*idx);
                    }
                    page = Page::List(InvPanel::Left);
                }
                (Page::Detail(DetailSource::LeftEquip, idx), KeyCode::Char('u')) => {
                    let w2 = &mut *world;
                    let mut q = w2.query::<(&mut Inventory, &mut Equipment)>();
                    if let Some((mut inv, mut eq)) = q.iter_mut(w2).next() {
                        let slot = match idx {
                            0 => &mut eq.weapon,
                            1 => &mut eq.armor,
                            _ => &mut eq.ring,
                        };
                        if let Some(stack) = slot.take() {
                            let leftover = inv.add(stack.item_id, stack.count);
                            if leftover > 0 {
                                slot.replace(stack);
                                w2.resource_mut::<EventLog>().push("背包已满");
                            }
                        }
                    }
                    page = Page::List(InvPanel::Left);
                }
                (Page::Detail(DetailSource::Right, _idx), KeyCode::Char('g')) => {
                    let mut collected = Vec::new();
                    let ppx = world.try_query::<(&Player, &Position)>().unwrap().iter(world).next().map(|(_, p)| (p.x, p.y)).unwrap_or((0, 0));
                    for (e, pu, po) in world.try_query::<(Entity, &ItemPickup, &Position)>().unwrap().iter(world) {
                        if po.x == ppx.0 && po.y == ppx.1 { collected.push((e, pu.stack.clone())); }
                    }
                    let mut logs = Vec::new();
                    let mut despawn = Vec::new();
                    for (e, s) in &collected {
                        let mut q = world.query::<(&mut Inventory,)>();
                        if let Some((mut inv,)) = q.iter_mut(world).next() {
                            let leftover = inv.add(s.item_id, s.count);
                            let picked = s.count - leftover;
                            if picked > 0 { logs.push(format!("拾取了{}x{}", s.name(), picked)); }
                            despawn.push(*e);
                        }
                    }
                    for e in despawn { world.entity_mut(e).despawn(); }
                    for msg in logs { world.resource_mut::<EventLog>().push(msg); }
                    page = Page::List(InvPanel::Left);
                }
                // 详情页其他键忽略
                (Page::Detail(_, _), _) => {}
                _ => {}
            }
        }
    }
    Ok(())
}
