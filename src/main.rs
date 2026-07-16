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
    ops, EventLog, LookCursor, TurnManager, MAP_WIDTH, MAP_HEIGHT, Position, Player,
};
use dungeon_action::{handle_player_direction, handle_wait, handle_skill, PlayerAction};
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
    world.insert_resource(dungeon_action::PageStack::default());
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
            if crossterm::event::poll(Duration::from_millis(16)).unwrap_or(false)
                && let Ok(Event::Key(key)) = crossterm::event::read() {
                    let now = Instant::now();
                    if key.code == last_code && now - last_time < Duration::from_millis(50) {
                        continue;
                    }
                    last_code = key.code;
                    last_time = now;
                    if tx.send(key.code).is_err() { break; }
                }
        }
    });

    loop {
        let frame_start = Instant::now();

        // 消费本帧所有输入
        let mut has_action = false;
        loop {
            match rx.try_recv() {
                Ok(code) => {
                    if process_key(code, terminal, &modal_flag, world, game_start)? {
                        has_action = true;
                    }
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => return Ok(()),
            }
        }

        // 推进世界
        if has_action && !world.resource::<TurnManager>().game_over {
            advance_and_settle(world);
        }

        // 渲染（每帧都画，固定帧率）
        {
            let w: &World = &*world;
            terminal.draw(|frame| render_ui(frame, game_start, w))?;
        }

        if world.resource::<TurnManager>().wants_quit {
            break Ok(());
        }

        // 帧率控制：33ms ≈ 30FPS
        let elapsed = frame_start.elapsed();
        let target = Duration::from_millis(33);
        if elapsed < target {
            std::thread::sleep(target - elapsed);
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
    game_start: Instant,
) -> io::Result<bool> {
    // 页栈分派
    let page = world.resource::<dungeon_action::PageStack>().current().clone();
    match page {
        dungeon_action::Page::Game => process_game_key(code, terminal, modal_flag, world, game_start),
        dungeon_action::Page::Look => process_look_key(code, world),
        dungeon_action::Page::Dialog(title) => process_dialog_key(code, world, &title),
    }
}

/// 光标查看页按键处理
fn process_look_key(code: KeyCode, world: &mut World) -> io::Result<bool> {
    match code {
        KeyCode::Up | KeyCode::Char('k') => {
            world.resource_mut::<LookCursor>().y = world.resource::<LookCursor>().y.saturating_sub(1);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            world.resource_mut::<LookCursor>().y = (world.resource::<LookCursor>().y + 1).min(MAP_HEIGHT - 1);
        }
        KeyCode::Left | KeyCode::Char('h') => {
            world.resource_mut::<LookCursor>().x = world.resource::<LookCursor>().x.saturating_sub(1);
        }
        KeyCode::Right | KeyCode::Char('l') => {
            world.resource_mut::<LookCursor>().x = (world.resource::<LookCursor>().x + 1).min(MAP_WIDTH - 1);
        }
        KeyCode::Char('x') | KeyCode::Esc => {
            world.resource_mut::<LookCursor>().active = false;
            world.resource_mut::<dungeon_action::PageStack>().pop();
        }
        _ => {}
    }
    Ok(false)
}

/// 游戏页按键处理
fn process_game_key(
    code: KeyCode,
    terminal: &mut Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
    modal_flag: &AtomicBool,
    world: &mut World,
    game_start: Instant,
) -> io::Result<bool> {
    // 先按大写字母处理（KeyCode::Char('E') 等）
    let code = match code {
        KeyCode::Char(c) if c.is_ascii_uppercase() => KeyCode::Char(c.to_ascii_lowercase()),
        other => other,
    };
    let Some(action) = dungeon_tui::keymap::resolve(code) else {
        return Ok(false);
    };
    match action {
        PlayerAction::Move(dx, dy) => Ok(handle_player_direction(world, *dx, *dy)),
        PlayerAction::Wait => Ok(handle_wait(world)),
        PlayerAction::Skill(i) => Ok(handle_skill(world, *i)),

        // ── 页栈弹入对话框 ──
        PlayerAction::Quit => {
            if world.resource::<TurnManager>().game_over {
                world.resource_mut::<TurnManager>().wants_quit = true;
            } else {
                world.resource_mut::<dungeon_action::PageStack>().push(
                    dungeon_action::Page::Dialog("确认退出？".into()));
            }
            Ok(false)
        }
        PlayerAction::DescendStairs => {
            if on_stairs(world) {
                world.resource_mut::<dungeon_action::PageStack>().push(
                    dungeon_action::Page::Dialog("确认下楼？".into()));
            }
            Ok(false)
        }

        // ── 模态（阻塞式 UI，需暂停输入线程） ──
        PlayerAction::Throw => {
            modal_flag.store(true, Ordering::Relaxed);
            let result = dungeon_tui::throw::open_throw_select(terminal, world, game_start)?;
            modal_flag.store(false, Ordering::Relaxed);
            Ok(result)
        }
        PlayerAction::OpenInventory => {
            modal_flag.store(true, Ordering::Relaxed);
            dungeon_tui::inventory::open_inventory(terminal, world, game_start)?;
            modal_flag.store(false, Ordering::Relaxed);
            Ok(false)
        }
        PlayerAction::OpenLook => {
            let (cx, cy) = {
                let mut q = world.try_query::<(&Player, &Position)>().expect("Player+Position registered");
                q.iter(world).next().map(|(_, p)| (p.x, p.y)).unwrap_or((MAP_WIDTH / 2, MAP_HEIGHT / 2))
            };
            world.insert_resource(LookCursor { active: true, x: cx, y: cy });
            world.resource_mut::<dungeon_action::PageStack>().push(dungeon_action::Page::Look);
            Ok(false)
        }
        PlayerAction::PickupGround => {
            pickup_ground(world);
            Ok(false)
        }
        PlayerAction::SaveGame => {
            if let Ok(data) = bincode::serialize(&GameSave::capture(world)) {
                std::fs::write("save.bin", data).ok();
                world.resource_mut::<EventLog>().push("已保存");
            }
            Ok(false)
        }
        PlayerAction::LoadGame => {
            if let Ok(data) = std::fs::read("save.bin")
                && let Ok(save) = bincode::deserialize::<GameSave>(&data) {
                    save.restore(world);
                    let _ = world.run_system_once(fov_system);
                    ops::update_map_memory(world);
                    ops::update_visible_memory(world);
                    ops::rebuild_occupancy(world);
                    world.resource_mut::<EventLog>().push("已读档");
                }
            Ok(false)
        }
    }
}

/// 对话框页按键处理
fn process_dialog_key(code: KeyCode, world: &mut World, _title: &str) -> io::Result<bool> {
    match code {
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            // 弹出前获取对话标题来决定行为
            let page = world.resource_mut::<dungeon_action::PageStack>().pop();
            match page {
                Some(dungeon_action::Page::Dialog(title)) => {
                    match title.as_str() {
                        "确认退出？" => {
                            world.resource_mut::<TurnManager>().wants_quit = true;
                        }
                        "确认下楼？" => {
                            if ops::on_stairs(world) {
                                descend(world);
                                let _ = world.run_system_once(fov_system);
                                ops::update_map_memory(world);
                                ops::update_visible_memory(world);
                                ops::rebuild_occupancy(world);
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
            Ok(false)
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            world.resource_mut::<dungeon_action::PageStack>().pop();
            Ok(false)
        }
        _ => Ok(false),
    }
}

#[allow(dead_code)]
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

#[allow(dead_code)]
fn open_look_mode(
    terminal: &mut Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
    world: &mut World,
    game_start: Instant,
) -> io::Result<()> {
    let (cx, cy) = {
        let mut q = world.try_query::<(&Player, &Position)>().expect("Player+Position registered");
        q.iter(&*world).next().map(|(_, p)| (p.x, p.y)).unwrap_or((MAP_WIDTH / 2, MAP_HEIGHT / 2))
    };
    world.insert_resource(LookCursor { active: true, x: cx, y: cy });
    loop {
        let _ = terminal.draw(|frame| render_ui(frame, game_start, &*world));
        if let Ok(Event::Key(k)) = event::read() {
            let mut cursor = world.resource_mut::<LookCursor>();
            match k.code {
                KeyCode::Up => cursor.y = cursor.y.saturating_sub(1),
                KeyCode::Down => cursor.y = (cursor.y + 1).min(MAP_HEIGHT - 1),
                KeyCode::Left => cursor.x = cursor.x.saturating_sub(1),
                KeyCode::Right => cursor.x = (cursor.x + 1).min(MAP_WIDTH - 1),
                KeyCode::Home => { cursor.x = 0; cursor.y = 0; }
                KeyCode::End => { cursor.x = MAP_WIDTH - 1; cursor.y = MAP_HEIGHT - 1; }
                KeyCode::Char('x') | KeyCode::Char('X') | KeyCode::Esc => break,
                _ => {}
            }
        }
    }
    world.resource_mut::<LookCursor>().active = false;
    Ok(())
}

fn title_screen(
    terminal: &mut Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
) -> io::Result<(World, Instant)> {
    loop {
        terminal.draw(draw_title)?;
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
                    if let Ok(data) = std::fs::read("save.bin")
                        && let Ok(save) = bincode::deserialize::<GameSave>(&data) {
                            let mut world = setup_world();
                            save.restore(&mut world);
                            let _ = world.run_system_once(fov_system);
                            ops::update_map_memory(&mut world);
                            ops::update_visible_memory(&mut world);
                            return Ok((world, Instant::now()));
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
