use std::io::{self, stdout};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use bevy_ecs::prelude::Entity;
use bevy_ecs::system::RunSystemOnce;
use dungeon_core::world;
use crossterm::event::{self, Event, KeyCode};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::ExecutableCommand;
use dungeon_core::{
    check_death_system, descend,
    fov_system, rebuild_occupancy, save::GameSave,
    setup_world, update_map_memory, update_visible_memory,
    Equipment, EquipmentSlot, EventLog, Inventory, ItemPickup, ItemStack,
    Player, Position, Renderable, Skills, Stairs, TurnManager,
};
use dungeon_core::action::{ActionKindV3, PlayerPreview, ActionQueue};
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
    let game_start = title_screen(&mut terminal)?;
    let result = run(&mut terminal, game_start);
    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    result
}

fn player_entity() -> Option<Entity> {
    let mut w = world!(mut);
    let mut q = w.query::<(Entity, &Player)>();
    q.iter(&mut *w).next().map(|(e, _)| e)
}

// ══════════════════════════════════════════════════════
// 主循环
// ══════════════════════════════════════════════════════

fn run(
    terminal: &mut Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
    game_start: Instant,
) -> io::Result<()> {
    rebuild_occupancy();
    world!(mut).run_system_once(fov_system);
    update_visible_memory();
    terminal.draw(|frame| render_ui(frame, game_start))?;

    // ── 独立输入线程：16ms 限流轮询，250ms 按键去重 ──
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
                    if key.code == last_code && now - last_time < Duration::from_millis(250) {
                        continue; // 250ms 内相同按键 → 去重
                    }
                    last_code = key.code;
                    last_time = now;
                    if tx.send(key.code).is_err() {
                        break;
                    }
                }
            }
        }
    });

    // ── 主循环 ──
    loop {
        // 检查游戏状态
        {
            let w = world!();
            if w.resource::<TurnManager>().game_over || w.resource::<TurnManager>().wants_quit {
                break Ok(());
            }
        }

        // 接收输入（16ms 超时 = 至少 60fps 渲染）
        let has_action = match rx.try_recv() {
            Ok(code) => process_key(code, terminal, &modal_flag)?,
            Err(mpsc::TryRecvError::Empty) => {
                std::thread::sleep(Duration::from_millis(1));
                false
            }
            Err(mpsc::TryRecvError::Disconnected) => break Ok(()),
        };

        if has_action {
            advance_and_settle();
        }

        terminal.draw(|frame| render_ui(frame, game_start))?;
    }
}

/// 持续推进直到玩家的行动被执行
fn advance_until_player_acted() {
    use dungeon_core::action::{advance_action_queue, ActionQueue};
    loop {
        let dist = advance_action_queue();
        if dist <= 0.0 { break; }
        let player_done = {
            let mut w = world!(mut);
            let player = w.query::<(Entity, &Player)>().iter(&mut *w).next().map(|(e, _)| e);
            match player {
                Some(p) => !w.resource::<ActionQueue>().has_entity(p),
                None => true,
            }
        };
        if player_done { break; }
    }
}

fn advance_and_settle() {
    use dungeon_core::action::run_monster_decision;

    advance_until_player_acted();
    run_monster_decision();

    rebuild_occupancy();
    world!(mut).run_system_once(fov_system);
    update_map_memory();
    update_visible_memory();
    world!(mut).run_system_once(check_death_system);
    // Buff 随行动推进衰减
    let _ = world!(mut).run_system_once(dungeon_core::buff_tick_system);
}

// ══════════════════════════════════════════════════════
// 拾取地面物品（g 键）
// ══════════════════════════════════════════════════════

fn pickup_ground() {
    let (ppx, ppy) = {
        let mut w = world!(mut);
        let mut q = w.query::<(&Player, &Position)>();
        q.iter(&mut *w).next().map(|(_, p)| (p.x, p.y)).unwrap_or((0, 0))
    };
    let ground: Vec<(Entity, ItemStack)> = {
        let mut w = world!(mut);
        let mut q = w.query::<(Entity, &ItemPickup, &Position)>();
        q.iter(&mut *w)
            .filter(|(_, _, pos)| pos.x == ppx && pos.y == ppy)
            .map(|(e, p, _)| (e, p.stack.clone()))
            .collect()
    };
    if ground.is_empty() { return; }
    let mut logs = Vec::new();
    let mut despawn = Vec::new();
    for (entity, stack) in &ground {
        let mut w = world!(mut);
        let mut q = w.query::<(&mut Inventory,)>();
        if let Some((mut inv,)) = q.iter_mut(&mut *w).next() {
            let leftover = inv.add(stack.item_id, stack.count);
            let picked = stack.count - leftover;
            if picked > 0 { logs.push(format!("拾取了{}x{}", stack.name(), picked)); }
            despawn.push(*entity);
        }
    }
    for e in despawn { let mut w = world!(mut); w.entity_mut(e).despawn(); }
    for msg in logs { let mut w = world!(mut); w.resource_mut::<EventLog>().push(msg); }
}

// ══════════════════════════════════════════════════════
// 核心输入处理（主线程）
// ── 返回 true 表示确认了一个耗时行动，调用方应推进队列
// ══════════════════════════════════════════════════════

fn process_key(
    code: KeyCode,
    terminal: &mut Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
    modal_flag: &AtomicBool,
) -> io::Result<bool> {
    match code {
        KeyCode::Up    => Ok(handle_player_direction(0, -1)),
        KeyCode::Down  => Ok(handle_player_direction(0, 1)),
        KeyCode::Left  => Ok(handle_player_direction(-1, 0)),
        KeyCode::Right => Ok(handle_player_direction(1, 0)),
        KeyCode::Char('.') => Ok(handle_wait()),
        KeyCode::Char('1') | KeyCode::Char('2') | KeyCode::Char('3') | KeyCode::Char('4') => {
            Ok(handle_skill(code))
        }
        KeyCode::Char('q') | KeyCode::Esc => {
            modal_flag.store(true, Ordering::Relaxed);
            let confirmed = open_modal(terminal, "确认退出？");
            modal_flag.store(false, Ordering::Relaxed);
            if confirmed { world!(mut).resource_mut::<TurnManager>().wants_quit = true; }
            Ok(false)
        }
        KeyCode::Char('e') | KeyCode::Char('E') => {
            modal_flag.store(true, Ordering::Relaxed);
            open_inventory(terminal)?;
            modal_flag.store(false, Ordering::Relaxed);
            Ok(false)
        }
        KeyCode::Char('g') | KeyCode::Char('G') => {
            pickup_ground();
            Ok(false)
        }
        KeyCode::F(5) => {
            if let Ok(data) = bincode::serialize(&GameSave::capture()) {
                std::fs::write("save.bin", data).ok();
                world!(mut).resource_mut::<EventLog>().push("已保存");
            }
            Ok(false)
        }
        KeyCode::F(9) => {
            if let Ok(data) = std::fs::read("save.bin") {
                if let Ok(save) = bincode::deserialize::<GameSave>(&data) {
                    save.restore();
                    world!(mut).resource_mut::<EventLog>().push("已读档");
                }
            }
            Ok(false)
        }
        KeyCode::Char('>') => {
            if on_stairs() {
                modal_flag.store(true, Ordering::Relaxed);
                let ok = open_modal(terminal, "确认下楼？");
                modal_flag.store(false, Ordering::Relaxed);
                if ok { descend(); }
            }
            Ok(false)
        }
        _ => Ok(false),
    }
}

/// 简单 Y/N 模态对话框（输入线程已暂停）
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
            let result = matches!(k.code, KeyCode::Char('y') | KeyCode::Char('Y'));
            return result;
        }
    }
}

// ══════════════════════════════════════════════════════
// 方向键 tap-tap（预览 / 确认）
// ══════════════════════════════════════════════════════

fn handle_player_direction(dx: isize, dy: isize) -> bool {
    use dungeon_core::{Map, Tile, OccupancyMap, MAP_WIDTH, MAP_HEIGHT, Monster};
    use dungeon_core::action::{Reaction, CanMove, ActionKindV3};

    let Some(entity) = player_entity() else { return false };

    let kind = {
        let w = world!();
        let Some(pos) = w.get::<Position>(entity) else { return false };
        let nx = pos.x.wrapping_add_signed(dx);
        let ny = pos.y.wrapping_add_signed(dy);
        if nx >= MAP_WIDTH || ny >= MAP_HEIGHT { return false; }
        let tile = w.resource::<Map>().tiles[ny][nx];
        let has_enemy = w.resource::<OccupancyMap>().cells[ny][nx]
            .and_then(|e| if w.get::<Monster>(e).is_some() { Some(e) } else { None });
        if tile != Tile::Floor && has_enemy.is_none() { return false; }
        if let Some(target) = has_enemy {
            ActionKindV3::Attack { target }
        } else {
            ActionKindV3::Move { dx, dy }
        }
    };

    let reaction_time = world!().get::<Reaction>(entity).map(|r| r.time).unwrap_or(50.0);
    let duration = world!().get::<CanMove>(entity).map(|m| m.duration).unwrap_or(300.0);
    handle_timed_action(entity, kind, reaction_time + duration)
}

/// tap-tap 核心：返回 true 表示确认入队
fn handle_timed_action(entity: Entity, kind: ActionKindV3, av: f32) -> bool {
    let is_confirm = {
        let w = world!();
        let preview = w.resource::<PlayerPreview>();
        match (&preview.kind, &kind) {
            (Some(ActionKindV3::Move { dx: pd, dy: pd2 }), ActionKindV3::Move { dx, dy })
                if *pd == *dx && *pd2 == *dy => true,
            (Some(ActionKindV3::Wait), ActionKindV3::Wait) => true,
            (Some(ActionKindV3::Skill(a)), ActionKindV3::Skill(b)) if *a == *b => true,
            (Some(ActionKindV3::Attack { .. }), ActionKindV3::Attack { .. }) => true,
            _ => false,
        }
    };

    if is_confirm {
        world!(mut).resource_mut::<ActionQueue>().enqueue_if_absent(entity, kind, av);
        world!(mut).resource_mut::<PlayerPreview>().kind = None;
        true
    } else {
        world!(mut).resource_mut::<PlayerPreview>().kind = Some(kind);
        false
    }
}

/// 处理等待键
fn handle_wait() -> bool {
    use dungeon_core::action::{Reaction, CanWait};
    if let Some(e) = player_entity() {
        let reaction_time = world!().get::<Reaction>(e).map(|r| r.time).unwrap_or(50.0);
        let duration = world!().get::<CanWait>(e).map(|w| w.duration).unwrap_or(800.0);
        handle_timed_action(e, ActionKindV3::Wait, reaction_time + duration)
    } else {
        false
    }
}

/// 处理技能键
fn handle_skill(code: KeyCode) -> bool {
    let idx = match code {
        KeyCode::Char('1') => 0,
        KeyCode::Char('2') => 1,
        KeyCode::Char('3') => 2,
        _ => 3,
    };
    if let Some(e) = player_entity() {
        use dungeon_core::action::Reaction;
        let reaction_time = world!().get::<Reaction>(e).map(|r| r.time).unwrap_or(50.0);
        handle_timed_action(e, ActionKindV3::Skill(idx), reaction_time + 600.0)
    } else {
        false
    }
}

// ══════════════════════════════════════════════════════
// 工具函数
// ══════════════════════════════════════════════════════

fn on_stairs() -> bool {
    let mut w = world!(mut);
    let pp = *w.query::<&Position>().iter(&mut *w).next().unwrap_or(&Position { x: 0, y: 0 });
    let mut q2 = w.query::<(&Stairs, &Position)>();
    q2.iter(&mut *w).any(|(_, sp)| sp.x == pp.x && sp.y == pp.y)
}

fn title_screen(
    terminal: &mut Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
) -> io::Result<Instant> {
    loop {
        terminal.draw(|frame| draw_title(frame))?;
        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Enter | KeyCode::Char('\n') | KeyCode::Char('\r') => {
                    let world = setup_world();
                    dungeon_core::global::set_world(world);
                    world!(mut).run_system_once(fov_system);
                    update_map_memory();
                    update_visible_memory();
                    return Ok(Instant::now());
                }
                KeyCode::F(9) => {
                    if let Ok(data) = std::fs::read("save.bin") {
                        if let Ok(save) = bincode::deserialize::<GameSave>(&data) {
                            save.restore();
                            world!(mut).run_system_once(fov_system);
                            update_map_memory();
                            update_visible_memory();
                            return Ok(Instant::now());
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
// 背包界面（双栏 + 详情页）
// ══════════════════════════════════════════════════════

#[derive(Clone, Copy, PartialEq, Eq)]
enum InvPanel { Left, Right }

#[derive(Clone, Copy, PartialEq, Eq)]
enum DetailSource { LeftInv, LeftEquip, Right }

fn collect_ground_items_in(w: &mut bevy_ecs::prelude::World) -> Vec<(ItemStack, Entity)> {
    let pp = {
        let mut q = w.query::<(&Player, &Position)>();
        q.iter(w).next().map(|(_, p)| (p.x, p.y)).unwrap_or((0, 0))
    };
    let mut items = Vec::new();
    let mut q = w.query::<(Entity, &ItemPickup, &Position)>();
    for (entity, pickup, pos) in q.iter(w) {
        if pos.x == pp.0 && pos.y == pp.1 {
            items.push((pickup.stack.clone(), entity));
        }
    }
    items
}

fn open_inventory(
    terminal: &mut Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
) -> io::Result<()> {
    let mut left_sel: usize = 0;
    let mut left_scroll: usize = 0;
    let mut right_sel: usize = 0;
    let mut right_scroll: usize = 0;
    let mut panel = InvPanel::Left;
    let mut detail: Option<(DetailSource, usize)> = None;

    fn left_count(eq: &Equipment, inv: &Inventory) -> usize {
        3 + inv.stacks.len()
    }

    loop {
        let (inv_stacks, inv_cap, equip, ground) = {
            let mut w = world!(mut);
            let mut q = w.query::<(&Inventory, &Equipment)>();
            let (inv, eq) = q.iter_mut(&mut *w).next()
                .map(|(i, e)| (i.clone(), e.clone())).unwrap_or_default();
            let ground = collect_ground_items_in(&mut *w);
            (inv.stacks, inv.capacity, eq, ground)
        };

        let left_total = left_count(&equip, &Inventory { stacks: inv_stacks.clone(), capacity: inv_cap });
        if panel == InvPanel::Left && left_total > 0 && left_sel >= left_total {
            left_sel = left_total - 1;
        }
        if panel == InvPanel::Right && ground.len() > 0 && right_sel >= ground.len() {
            right_sel = ground.len() - 1;
        }

        terminal.draw(|frame| {
            let area = frame.area();
            let block = Block::default()
                .title("  背包  ").title_alignment(Alignment::Center)
                .borders(Borders::ALL).border_style(Style::default().fg(Color::Cyan));
            frame.render_widget(block, area);
            let inner = Rect { x: area.x + 1, y: area.y + 1, width: area.width.saturating_sub(2), height: area.height.saturating_sub(2) };

            if let Some((dsrc, idx)) = &detail {
                let (stack, source_label, is_equip_slot) = match dsrc {
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
                        lines.push(Line::from(Span::styled(format!(" 槽位: {:?}", d.slot), Style::default().fg(Color::DarkGray))));
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
                            if item.def().map_or(false, |d| !matches!(d.slot, EquipmentSlot::Material)) {
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
                    let act = panel == InvPanel::Left;
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
                            let hk = if real < 10 { char::from_digit(real as u32, 10).unwrap() } else { '?' };
                            let cl = if stack.count > 1 { format!(" x{}", stack.count) } else { String::new() };
                            lines.push(Line::from(vec![
                                Span::styled(format!("{}{}", p, hk), Style::default().fg(Color::Yellow)),
                                Span::raw(format!(" {}{}", stack.name(), cl)),
                            ]));
                        }
                    }
                    if act { lines.push(Line::from(Span::styled(" 0-9:选中 Enter:查看 e:装备 d:丢弃", Style::default().fg(Color::DarkGray)))); }
                    frame.render_widget(Paragraph::new(lines), left_area);
                }

                // Right panel
                {
                    let mut lines = vec![];
                    let act = panel == InvPanel::Right;
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
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => { if detail.is_some() { detail = None; } else { break; } }
                KeyCode::Left => { if detail.is_none() { panel = InvPanel::Left; } }
                KeyCode::Right => { if detail.is_none() { panel = InvPanel::Right; } }
                KeyCode::Up => {
                    if detail.is_some() { continue; }
                    match panel { InvPanel::Left => { if left_sel > 0 { left_sel -= 1; } } InvPanel::Right => { if right_sel > 0 { right_sel -= 1; } } }
                }
                KeyCode::Down => {
                    if detail.is_some() { continue; }
                    match panel { InvPanel::Left => { if left_sel + 1 < left_total { left_sel += 1; } } InvPanel::Right => { if right_sel + 1 < ground.len() { right_sel += 1; } } }
                }
                KeyCode::Enter => {
                    if detail.is_some() { continue; }
                    match panel {
                        InvPanel::Left => {
                            if left_sel < 3 { if [&equip.weapon, &equip.armor, &equip.ring][left_sel].is_some() { detail = Some((DetailSource::LeftEquip, left_sel)); } }
                            else if left_sel - 3 < inv_stacks.len() { detail = Some((DetailSource::LeftInv, left_sel - 3)); }
                        }
                        InvPanel::Right => { if !ground.is_empty() { detail = Some((DetailSource::Right, right_sel)); } }
                    }
                }
                KeyCode::Char('g') => {
                    // Copy of pickup_ground inline
                    let (ppx, ppy) = { let mut w = world!(mut); let mut q = w.query::<(&Player, &Position)>(); q.iter(&mut *w).next().map(|(_, p)| (p.x, p.y)).unwrap_or((0, 0)) };
                    let mut collected = Vec::new();
                    { let mut w = world!(mut); let mut q = w.query::<(Entity, &ItemPickup, &Position)>(); for (e, pu, po) in q.iter(&mut *w) { if po.x == ppx && po.y == ppy { collected.push((e, pu.stack.clone())); } } }
                    let mut logs = Vec::new(); let mut despawn = Vec::new();
                    for (e, s) in &collected { let mut w = world!(mut); let mut q = w.query::<(&mut Inventory,)>(); if let Some((mut inv,)) = q.iter_mut(&mut *w).next() { let leftover = inv.add(s.item_id, s.count); let picked = s.count - leftover; if picked > 0 { logs.push(format!("拾取了{}x{}", s.name(), picked)); } despawn.push(*e); } }
                    for e in despawn { let mut w = world!(mut); w.entity_mut(e).despawn(); }
                    for msg in logs { let mut w = world!(mut); w.resource_mut::<EventLog>().push(msg); }
                    if let Some((DetailSource::Right, _)) = detail { detail = None; }
                }
                KeyCode::Char('e') => {
                    if let Some((DetailSource::LeftInv, idx)) = detail {
                        let item_id = { let mut w = world!(mut); let mut q = w.query::<(&Inventory,)>();
                            q.iter_mut(&mut *w).next().and_then(|(inv,)| inv.stacks.get(idx).map(|s| s.item_id)) };
                        if let Some(id) = item_id {
                            if let Some(def) = ItemStack::new(id, 1).def() {
                                let can_equip = match def.slot { EquipmentSlot::Weapon | EquipmentSlot::Armor | EquipmentSlot::Ring => true, _ => false };
                                if can_equip {
                                    let mut w = world!(mut); let mut q = w.query::<(&mut Inventory, &mut Equipment)>();
                                    if let Some((mut inv, mut eq)) = q.iter_mut(&mut *w).next() {
                                        let slot_free = match def.slot { EquipmentSlot::Weapon => eq.weapon.is_none(), EquipmentSlot::Armor => eq.armor.is_none(), EquipmentSlot::Ring => eq.ring.is_none(), _ => false };
                                        if slot_free {
                                            inv.remove(idx, 1); let es = ItemStack::new(id, 1);
                                            match def.slot { EquipmentSlot::Weapon => eq.weapon = Some(es), EquipmentSlot::Armor => eq.armor = Some(es), EquipmentSlot::Ring => eq.ring = Some(es), _ => {} }
                                            detail = None;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                KeyCode::Char('d') => {
                    if let Some((DetailSource::LeftInv, idx)) = detail {
                        let mut w = world!(mut);
                        let pp = { let mut q = w.query::<(&Player, &Position)>(); q.iter(&mut *w).next().map(|(_, p)| (p.x, p.y)).unwrap_or((0, 0)) };
                        let mut q = w.query::<(&mut Inventory,)>();
                        if let Some((mut inv,)) = q.iter_mut(&mut *w).next() {
                            if let Some(stack) = inv.stacks.get(idx) { let drop = ItemStack::new(stack.item_id, 1); inv.remove(idx, 1); w.spawn((ItemPickup { stack: drop.clone() }, Position { x: pp.0, y: pp.1 }, Renderable { glyph: drop.glyph(), color: drop.color() })); detail = None; }
                        }
                    }
                }
                KeyCode::Char('u') => {
                    if let Some((DetailSource::LeftEquip, idx)) = detail {
                        let mut w = world!(mut); let mut q = w.query::<(&mut Inventory, &mut Equipment)>();
                        if let Some((mut inv, mut eq)) = q.iter_mut(&mut *w).next() {
                            let slot = match idx { 0 => &mut eq.weapon, 1 => &mut eq.armor, 2 => &mut eq.ring, _ => unreachable!() };
                            if let Some(stack) = slot.take() {
                                let leftover = inv.add(stack.item_id, stack.count);
                                if leftover > 0 { let pp = { let mut p = w.query::<(&Player, &Position)>(); p.iter(&mut *w).next().map(|(_, p)| (p.x, p.y)).unwrap_or((0, 0)) }; w.spawn((ItemPickup { stack: ItemStack::new(stack.item_id, leftover) }, Position { x: pp.0, y: pp.1 }, Renderable { glyph: stack.glyph(), color: stack.color() })); }
                                detail = None;
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
    Ok(())
}
