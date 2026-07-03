//! Dungeon MVP — 主循环、输入处理、模式切换。
//!
//! 逻辑层：dungeon-core（ECS 组件、系统、AV 引擎）
//! 渲染层：dungeon-render（UI 绘制、行动轴、状态面板）
//! 应用层：本文件（主循环、输入、模式）

use std::collections::HashSet;
use std::io::{self, stdout};
use std::time::Instant;

use bevy_ecs::prelude::Entity;
use bevy_ecs::system::RunSystemOnce;
use dungeon_core::world;
use crossterm::event::{self, Event, KeyCode};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::ExecutableCommand;
use dungeon_core::{
    apply_skill, check_death_system, descend,
    fov_system, rebuild_occupancy, save::GameSave,
    setup_world, skill_tick_system, update_map_memory,
    Equipment, EquipmentSlot, EventLog, GamePacing, Inventory, ItemInstance,
    ManualOverride, Monster, PacingMode, PendingInput, PendingLevelUp, PendingPlayerAction,
    Player, Position, Skills, Stairs, Stats, TurnManager, Viewshed,
};
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
    world!(mut).insert_resource(GamePacing::default());
    world!(mut).insert_resource(PendingInput::default());
    rebuild_occupancy();
    world!(mut).run_system_once(fov_system);
    terminal.draw(|frame| render_ui(frame, game_start))?;

    loop {
        let w = world!();
        if w.resource::<TurnManager>().game_over || w.resource::<TurnManager>().wants_quit {
            break Ok(());
        }
        drop(w);

        // 闪烁相位
        let blink = !world!().resource::<GamePacing>().blink_phase;
        world!(mut).resource_mut::<GamePacing>().blink_phase = blink;

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

    // 1. 冷却递减（所有实体的 Action 组件 cooldown）
    tick_all_cooldowns(10.0);

    // 2. 怪物决策（条件满足 → 仲裁 → 入队）
    run_monster_decision();

    // 3. 推进行动队列（反应时倒计时 → 保活 → 执行 → 冷却）
    advance_action_queue();

    // 4. 通用后处理
    rebuild_occupancy();
    world!(mut).run_system_once(fov_system);
    update_map_memory();
    world!(mut).run_system_once(check_death_system);

    // 5. 退出战斗检查
    if world!().resource::<GamePacing>().combat_active {
        let mut w = world!(mut);
        let fov: HashSet<(usize, usize)> = {
            let mut q = w.query::<(&Player, &Viewshed)>();
            q.iter(&mut *w).next()
                .map(|(_, v)| v.visible_tiles.iter().copied().collect())
                .unwrap_or_default()
        };
        let any_in_fov = {
            let mut q = w.query::<(&Monster, &Position)>();
            q.iter(&mut *w).any(|(_, p)| fov.contains(&(p.x, p.y)))
        };
        if !any_in_fov {
            w.resource_mut::<GamePacing>().combat_active = false;
            w.resource_mut::<GamePacing>().mode = PacingMode::Exploration;
        }
    }
}

// ══════════════════════════════════════════════════════
// 输入处理
// ══════════════════════════════════════════════════════

fn handle_input(
    terminal: &mut Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
    key: crossterm::event::KeyEvent,
) -> io::Result<()> {
    if handle_global_toggle(key) {
        return Ok(());
    }

    // 新输入模型：tap-tap 确认方向键
    match key.code {
        KeyCode::Up => handle_player_direction(0, -1),
        KeyCode::Down => handle_player_direction(0, 1),
        KeyCode::Left => handle_player_direction(-1, 0),
        KeyCode::Right => handle_player_direction(1, 0),
        KeyCode::Char('q') | KeyCode::Esc => {
            if confirm_quit(terminal)? {
                world!(mut).resource_mut::<TurnManager>().wants_quit = true;
            }
        }
        KeyCode::Char('.') => {
            // 确认等待（直接入队）
            use dungeon_core::action::{ActionKindV3, ActionQueue, Reaction, CanWait};
            let player = crate::player_entity();
            if let Some(e) = player {
                let reaction_time = world!().get::<Reaction>(e).map(|r| r.time).unwrap_or(50.0);
                let duration = world!().get::<CanWait>(e).map(|w| w.duration).unwrap_or(800.0);
                world!(mut).resource_mut::<ActionQueue>()
                    .enqueue(e, ActionKindV3::Wait, reaction_time, duration);
            }
        }
        KeyCode::Char('1') | KeyCode::Char('2') | KeyCode::Char('3') | KeyCode::Char('4') => {
            // 技能：直接入队（暂用旧系统）
            let (idx, _name) = skill_info(key);
            use dungeon_core::action::{ActionKindV3, ActionQueue, Reaction};
            let player = crate::player_entity();
            if let Some(e) = player {
                let reaction_time = world!().get::<Reaction>(e).map(|r| r.time).unwrap_or(50.0);
                world!(mut).resource_mut::<ActionQueue>()
                    .enqueue(e, ActionKindV3::Skill(idx), reaction_time, 600.0);
            }
        }
        KeyCode::Char('5') => {
            // 等待（已在上面的 '.' 中处理）
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
                world!(mut).resource_mut::<GamePacing>().mode = PacingMode::Exploration;
            }
        }
        _ => {}
    }
    Ok(())
}

/// 新方向键处理器：tap-tap 模式
/// 第一次按 → 设预览，第二次同方向 → 入队 ActionQueue
fn handle_player_direction(dx: isize, dy: isize) {
    use dungeon_core::action::{PlayerPreview, ActionKindV3, ActionQueue, Reaction, CanMove};
    let Some(entity) = crate::player_entity() else { return };

    // 先读取需要的数据（避免在持有锁时再次锁）
    let reaction_time = world!().get::<Reaction>(entity).map(|r| r.time).unwrap_or(50.0);
    let duration = world!().get::<CanMove>(entity).map(|m| m.duration).unwrap_or(300.0);

    // 检查是否是第二次 tap
    let is_confirm = {
        let w = world!();
        let preview = w.resource::<PlayerPreview>();
        matches!(&preview.kind, Some(ActionKindV3::Move { dx: pd, dy: pd2 }) if *pd == dx && *pd2 == dy)
    };

    if is_confirm {
        // 第二次确认 → 入队
        world!(mut).resource_mut::<ActionQueue>()
            .enqueue(entity, ActionKindV3::Move { dx, dy }, reaction_time, duration);
        // 清除预览
        let mut w2 = world!(mut);
        w2.resource_mut::<PlayerPreview>().kind = None;
    } else {
        // 第一次按 → 设预览
        let mut w2 = world!(mut);
        w2.resource_mut::<PlayerPreview>().kind = Some(ActionKindV3::Move { dx, dy });
    }
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

/// 全局切换键：'m' = 强制手动, 'a' = 清除覆盖(恢复自动跟随)。
fn handle_global_toggle(key: crossterm::event::KeyEvent) -> bool {
    match key.code {
        KeyCode::Char('m') | KeyCode::Char('M') => {
            world!(mut).resource_mut::<GamePacing>().manual_override = ManualOverride::ForceManual;
            world!(mut).resource_mut::<EventLog>().push("切换为手动模式");
            true
        }
        KeyCode::Char('a') | KeyCode::Char('A') => {
            world!(mut).resource_mut::<GamePacing>().manual_override = ManualOverride::None;
            world!(mut).resource_mut::<EventLog>().push("切换为自动模式");
            true
        }
        _ => false,
    }
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
