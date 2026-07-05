//! Dungeon MVP — 主循环、输入处理。
//!
//! 逻辑层：dungeon-core（ECS 组件、系统、AV 引擎）
//! 渲染层：dungeon-render（UI 绘制、行动轴、状态面板）
//! 应用层：本文件（主循环、输入、模式）

use std::io::{self, stdout};
use std::time::Instant;

use bevy_ecs::prelude::Entity;
use bevy_ecs::system::RunSystemOnce;
use dungeon_core::world;
use crossterm::event::{self, Event, KeyCode};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::ExecutableCommand;
use dungeon_core::{
    check_death_system, descend,
    fov_system, rebuild_occupancy, save::GameSave,
    setup_world, update_map_memory,
    Equipment, EquipmentSlot, EventLog, Inventory, ItemInstance,
    PendingLevelUp,
    Player, Position, Skills, Stairs, Stats, TurnManager,
};
use dungeon_core::action::ActionKindV3;
use dungeon_render::{draw_level_up, draw_title, render_ui};
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Terminal;

fn main() -> io::Result<()> {
    // panic hook: 将 panic 写入文件（raw 模式下看不到 stderr）
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

// ══════════════════════════════════════════════════════
// 玩家行动函数（统一路径）
// ══════════════════════════════════════════════════════

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
    terminal.draw(|frame| render_ui(frame, game_start))?;

    loop {
        let w = world!();
        if w.resource::<TurnManager>().game_over || w.resource::<TurnManager>().wants_quit {
            break Ok(());
        }
        drop(w);

        terminal.draw(|frame| render_ui(frame, game_start))?;

        // ══════════════════════════════════════════════
        // 统一输入循环
        // ══════════════════════════════════════════════

        if let Event::Key(key) = event::read()? {
            handle_input(terminal, key)?;
            advance_and_settle();
        }
    }
}

/// 统一推进：新行动系统 + 通用后处理
fn advance_and_settle() {
    use dungeon_core::action::{tick_all_cooldowns, run_monster_decision, advance_action_queue};

    // 1. 先推进队列，得到实际推进量
    let dist = advance_action_queue();

    // 2. 用相同推进量递减冷却
    if dist > 0.0 {
        tick_all_cooldowns(dist);
    }

    // 3. 怪物决策
    run_monster_decision();

    // 4. 通用后处理
    rebuild_occupancy();
    world!(mut).run_system_once(fov_system);
    update_map_memory();
    world!(mut).run_system_once(check_death_system);
}

// ══════════════════════════════════════════════════════
// 输入处理
// ══════════════════════════════════════════════════════

fn handle_input(
    terminal: &mut Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
    key: crossterm::event::KeyEvent,
) -> io::Result<()> {
    match key.code {
        // ── 耗时行动：tap-tap 确认 ──
        KeyCode::Up => handle_player_direction(0, -1),
        KeyCode::Down => handle_player_direction(0, 1),
        KeyCode::Left => handle_player_direction(-1, 0),
        KeyCode::Right => handle_player_direction(1, 0),
        KeyCode::Char('.') => {
            use dungeon_core::action::{ActionKindV3, Reaction, CanWait};
            if let Some(e) = player_entity() {
                let reaction_time = world!().get::<Reaction>(e).map(|r| r.time).unwrap_or(50.0);
                let duration = world!().get::<CanWait>(e).map(|w| w.duration).unwrap_or(800.0);
                handle_timed_action(e, ActionKindV3::Wait, reaction_time, duration);
            }
        }
        KeyCode::Char('1') | KeyCode::Char('2') | KeyCode::Char('3') | KeyCode::Char('4') => {
            use dungeon_core::action::{ActionKindV3, Reaction};
            let (idx, _) = skill_info(key);
            if let Some(e) = player_entity() {
                let reaction_time = world!().get::<Reaction>(e).map(|r| r.time).unwrap_or(50.0);
                handle_timed_action(e, ActionKindV3::Skill(idx), reaction_time, 600.0);
            }
        }
        // ── 非耗时行动：单击执行 ──
        KeyCode::Char('q') | KeyCode::Esc => {
            if confirm_quit(terminal)? {
                world!(mut).resource_mut::<TurnManager>().wants_quit = true;
            }
        }
        KeyCode::Char('e') | KeyCode::Char('E') => {
            open_inventory(terminal)?;
        }
        KeyCode::F(5) => {
            if let Ok(data) = bincode::serialize(&GameSave::capture()) {
                std::fs::write("save.bin", data).ok();
                world!(mut).resource_mut::<EventLog>().push("已保存");
            }
        }
        KeyCode::F(9) => {
            if let Ok(data) = std::fs::read("save.bin") {
                if let Ok(save) = bincode::deserialize::<GameSave>(&data) {
                    save.restore();
                    world!(mut).resource_mut::<EventLog>().push("已读档");
                }
            }
        }
        KeyCode::Char('>') => {
            if on_stairs() && confirm_stairs(terminal)? {
                descend();
            }
        }
        _ => {}
    }
    Ok(())
}

/// 统一 tap-tap 处理：第一次 tap 设置预览，第二次相同行动确认入队
fn handle_timed_action(entity: Entity, kind: ActionKindV3, reaction_time: f32, duration: f32) {
    use dungeon_core::action::{PlayerPreview, ActionQueue};

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
        world!(mut).resource_mut::<ActionQueue>().enqueue(entity, kind, reaction_time, duration);
        world!(mut).resource_mut::<PlayerPreview>().kind = None;
    } else {
        world!(mut).resource_mut::<PlayerPreview>().kind = Some(kind);
    }
}

/// 方向键处理器：语义识别 + tap-tap 确认
/// 第一次按 → 预览（Move/Attack/无效），第二次同方向 → 入队
fn handle_player_direction(dx: isize, dy: isize) {
    use dungeon_core::action::{ActionKindV3, Reaction, CanMove};
    use dungeon_core::{Map, Tile, OccupancyMap, MAP_WIDTH, MAP_HEIGHT, Monster};
    let Some(entity) = player_entity() else { return };

    // 检查目标格
    let kind = {
        let w = world!();
        let Some(pos) = w.get::<Position>(entity) else { return };
        let nx = pos.x.wrapping_add_signed(dx);
        let ny = pos.y.wrapping_add_signed(dy);
        if nx >= MAP_WIDTH || ny >= MAP_HEIGHT { return; }
        let tile = w.resource::<Map>().tiles[ny][nx];
        let has_enemy = w.resource::<OccupancyMap>().cells[ny][nx]
            .and_then(|e| if w.get::<Monster>(e).is_some() { Some(e) } else { None });
        // 墙 → 丢弃
        if tile != Tile::Floor && has_enemy.is_none() { return; }
        if let Some(target) = has_enemy {
            ActionKindV3::Attack { target }
        } else {
            ActionKindV3::Move { dx, dy }
        }
    };

    let reaction_time = world!().get::<Reaction>(entity).map(|r| r.time).unwrap_or(50.0);
    let duration = world!().get::<CanMove>(entity).map(|m| m.duration).unwrap_or(300.0);
    handle_timed_action(entity, kind, reaction_time, duration);
}

/// 从按键获取技能索引和名称。
fn skill_info(key: crossterm::event::KeyEvent) -> (usize, Option<String>) {
    let idx = match key.code {
        KeyCode::Char('1') => 0,
        KeyCode::Char('2') => 1,
        KeyCode::Char('3') => 2,
        _ => 3,
    };
    let names: Vec<String> = {
        let mut w = world!(mut);
        let mut q = w.query::<&Skills>();
        q.iter(&mut *w)
            .next()
            .map(|sk| sk.list.iter().map(|s| s.name.to_string()).collect())
            .unwrap_or_default()
    };
    (idx, names.get(idx).cloned())
}

// ══════════════════════════════════════════════════════
// 工具函数
// ══════════════════════════════════════════════════════

fn on_stairs() -> bool {
    let mut w = world!(mut);
    let pp = {
        let mut q = w.query::<&Position>();
        *q.iter(&mut *w)
            .next()
            .unwrap_or(&Position { x: 0, y: 0 })
    };
    let mut q2 = w.query::<(&Stairs, &Position)>();
    q2.iter(&mut *w).any(|(_, sp)| sp.x == pp.x && sp.y == pp.y)
}

fn confirm_quit(
    terminal: &mut Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
) -> io::Result<bool> {
    terminal.draw(|frame| {
        let area = frame.area();
        let msg = Paragraph::new(Line::from(vec![
            Span::styled(" 确认退出？", Style::default().fg(Color::Red).bold()),
            Span::raw(" "),
            Span::styled("[Y]是", Style::default().fg(Color::Yellow)),
            Span::raw(" "),
            Span::styled("[N]否", Style::default().fg(Color::DarkGray)),
        ]))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Red)),
        );
        frame.render_widget(
            msg,
            Rect {
                x: area.width / 2 - 10,
                y: area.height / 2,
                width: 20,
                height: 3,
            },
        );
    })?;
    loop {
        if let Event::Key(k) = event::read()? {
            return Ok(matches!(k.code, KeyCode::Char('y') | KeyCode::Char('Y')));
        }
    }
}

fn confirm_stairs(
    terminal: &mut Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
) -> io::Result<bool> {
    terminal.draw(|frame| {
        let area = frame.area();
        let msg = Paragraph::new(Line::from(vec![
            Span::styled(" 确认下楼？", Style::default().fg(Color::Yellow).bold()),
            Span::raw(" "),
            Span::styled("[Y]是", Style::default().fg(Color::Yellow)),
            Span::raw(" "),
            Span::styled("[N]否", Style::default().fg(Color::DarkGray)),
        ]))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow)),
        );
        frame.render_widget(
            msg,
            Rect {
                x: area.width / 2 - 10,
                y: area.height / 2,
                width: 20,
                height: 3,
            },
        );
    })?;
    loop {
        if let Event::Key(k) = event::read()? {
            return Ok(matches!(k.code, KeyCode::Char('y') | KeyCode::Char('Y')));
        }
    }
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
                    return Ok(Instant::now());
                }
                KeyCode::F(9) => {
                    if let Ok(data) = std::fs::read("save.bin") {
                        if let Ok(save) = bincode::deserialize::<GameSave>(&data) {
                            save.restore();
                            world!(mut).run_system_once(fov_system);
                            update_map_memory();
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

#[allow(dead_code)]
fn level_up_screen(
    terminal: &mut Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
    _game_start: Instant,
) -> io::Result<()> {
    terminal.draw(|frame| draw_level_up(frame, world!().resource::<PendingLevelUp>().points))?;
    loop {
        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Char('0') => {
                    world!(mut).resource_mut::<PendingLevelUp>().points = 0;
                    break;
                }
                KeyCode::Char('1') => {
                    let mut w = world!(mut);
                    if w.resource::<PendingLevelUp>().points > 0 {
                        w.resource_mut::<PendingLevelUp>().points -= 1;
                        w.query::<&mut Stats>().single_mut(&mut *w).unwrap().attack += 1;
                    }
                }
                KeyCode::Char('2') => {
                    let mut w = world!(mut);
                    if w.resource::<PendingLevelUp>().points > 0 {
                        w.resource_mut::<PendingLevelUp>().points -= 1;
                        w.query::<&mut Stats>().single_mut(&mut *w).unwrap().defense += 1;
                    }
                }
                KeyCode::Char('3') => {
                    let mut w = world!(mut);
                    if w.resource::<PendingLevelUp>().points > 0 {
                        w.resource_mut::<PendingLevelUp>().points -= 1;
                        w.query::<&mut Stats>().single_mut(&mut *w).unwrap().magic_mastery += 1;
                    }
                }
                KeyCode::Char('4') => {
                    let mut w = world!(mut);
                    if w.resource::<PendingLevelUp>().points > 0 {
                        w.resource_mut::<PendingLevelUp>().points -= 1;
                        w.query::<&mut Stats>().single_mut(&mut *w).unwrap().agility += 1;
                    }
                }
                KeyCode::Char('5') => {
                    let mut w = world!(mut);
                    if w.resource::<PendingLevelUp>().points > 0 {
                        w.resource_mut::<PendingLevelUp>().points -= 1;
                        let mut s = w.query::<&mut Stats>().single_mut(&mut *w).unwrap();
                        s.max_hp += 5;
                        s.hp = s.hp.min(s.max_hp);
                    }
                }
                _ => {}
            }
            if world!().resource::<PendingLevelUp>().points == 0 {
                break;
            }
            terminal
                .draw(|frame| draw_level_up(frame, world!().resource::<PendingLevelUp>().points))?;
        }
    }
    Ok(())
}

fn open_inventory(
    terminal: &mut Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
) -> io::Result<()> {
    let mut selected: usize = 0;
    let mut scroll: usize = 0;

    fn get_inv() -> (Vec<ItemInstance>, usize, (Option<usize>, Option<usize>, Option<usize>)) {
        let mut w = world!(mut);
        let mut q = w.query::<(&Inventory, &Equipment)>();
        q.iter(&mut *w)
            .next()
            .map(|(inv, eq)| (inv.items.clone(), inv.capacity, (eq.weapon, eq.armor, eq.ring)))
            .unwrap_or_default()
    }

    loop {
        let inv_data = get_inv();
        terminal.draw(|frame| {
            let area = frame.area();
            let block = Block::default()
                .title("  背包  ")
                .title_alignment(Alignment::Center)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan));
            frame.render_widget(block, area);
            let inner = inner_rect(area, 1);
            let (ref items, capacity, (weapon, armor, ring)) = inv_data;
            let mut lines: Vec<Line> = Vec::new();
            lines.push(Line::from(vec![Span::styled(
                format!(
                    " 物品 {} / {}  e:装备 d:丢弃 Esc:返回",
                    items.len(),
                    capacity
                ),
                Style::default().fg(Color::DarkGray),
            )]));
            lines.push(Line::from(Span::raw("")));
            let eq_name = |idx: Option<usize>| {
                idx.and_then(|i| items.get(i))
                    .map(|it| it.name.clone())
                    .unwrap_or("(空)".into())
            };
            lines.push(Line::from(Span::raw(format!(
                " 武器: {}  防具: {}  戒指: {}",
                eq_name(weapon),
                eq_name(armor),
                eq_name(ring)
            ))));
            lines.push(Line::from(Span::raw("")));
            let page_size = (inner.height as usize).saturating_sub(5).min(20);
            if items.is_empty() {
                lines.push(Line::from(Span::styled(
                    " (背包为空)",
                    Style::default().fg(Color::DarkGray),
                )));
            } else {
                let sel = selected.min(items.len() - 1);
                let sc = scroll.min(sel);
                let end = (sc + page_size).min(items.len());
                for i in sc..end {
                    let item = &items[i];
                    let idx_char =
                        if i < 10 { char::from_digit(i as u32, 10).unwrap() } else { '?' };
                    let prefix = if i == sel { "▸" } else { " " };
                    lines.push(Line::from(vec![
                        Span::styled(
                            format!("{}{}", prefix, idx_char),
                            Style::default().fg(Color::Yellow),
                        ),
                        Span::raw(format!(" {}", item.name)),
                        Span::styled(
                            format!(" {:?}", item.slot),
                            Style::default().fg(Color::DarkGray),
                        ),
                        Span::raw(format!(" {}", item.description)),
                    ]));
                }
            }
            frame.render_widget(Paragraph::new(lines), inner);
        })?;

        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => break,
                KeyCode::Down => {
                    let len = inv_data.0.len();
                    if len > 0 {
                        selected = (selected + 1).min(len - 1);
                        if selected >= scroll + 10 { scroll += 1; }
                    }
                }
                KeyCode::Up => {
                    selected = selected.saturating_sub(1);
                    if selected < scroll { scroll = scroll.saturating_sub(1); }
                }
                KeyCode::Char('e') => {
                    let mut w = world!(mut);
                    let mut q = w.query::<(&mut Inventory, &mut Equipment)>();
                    if let Some((mut inv, mut eq)) = q.iter_mut(&mut *w).next() {
                        if inv.items.get(selected).is_some() {
                            match inv.items[selected].slot {
                                EquipmentSlot::Weapon => eq.weapon = Some(selected),
                                EquipmentSlot::Armor => eq.armor = Some(selected),
                                EquipmentSlot::Ring => eq.ring = Some(selected),
                            }
                        }
                    }
                }
                KeyCode::Char('d') => {
                    let mut w = world!(mut);
                    let mut q = w.query::<(&mut Inventory, &mut Equipment)>();
                    if let Some((mut inv, mut eq)) = q.iter_mut(&mut *w).next() {
                        if selected < inv.items.len() {
                            if eq.weapon == Some(selected) { eq.weapon = None; }
                            if eq.armor == Some(selected) { eq.armor = None; }
                            if eq.ring == Some(selected) { eq.ring = None; }
                            inv.items.remove(selected);
                            selected = selected.min(inv.items.len().saturating_sub(1));
                        }
                    }
                }
                _ => {}
            }
        }
    }
    Ok(())
}

fn inner_rect(area: Rect, border: u16) -> Rect {
    Rect {
        x: area.x + border,
        y: area.y + border,
        width: area.width.saturating_sub(border * 2),
        height: area.height.saturating_sub(border * 2),
    }
}
