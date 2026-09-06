//! dungeon-app：最小可运行的新 core 装配层。
//!
//! 输入来自 sys，规则运行在 core，画面渲染在 tui。

use std::io::{self, stdout};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use bevy_ecs::prelude::World;
use crossterm::event::KeyCode;
use core::world_loop::{apply_player_command, new_game, request_quit};
use core::PlayerCommand;
use ratatui::Terminal;
use sys::{self, try_recv_key};
use tui::{DevLogBuffer, render_game};

fn main() -> io::Result<()> {
    let log_rx = sys::init_logging();
    sys::enter_raw_mode()?;
    sys::enter_alternate_screen()?;

    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    let mut world: World = new_game(seed);
    world.insert_resource(DevLogBuffer::new(80));

    let mut terminal = Terminal::new(ratatui::backend::CrosstermBackend::new(stdout()))?;
    terminal.draw(|frame| render_game(frame, &mut world))?;

    let rx = sys::spawn_key_source();

    loop {
        let frame_start = Instant::now();

        while let Some(code) = try_recv_key(&rx) {
            match key_to_command(code) {
                Some(command) => {
                    let _ = apply_player_command(&mut world, command);
                }
                None if code == KeyCode::Char('q') || code == KeyCode::Esc => {
                    request_quit(&mut world);
                }
                _ => {}
            }
        }

        while let Ok(record) = log_rx.try_recv() {
            let mut buffer = world.resource_mut::<DevLogBuffer>();
            buffer.push(
                record.level.to_string(),
                record.target,
                record.message,
            );
        }

        terminal.draw(|frame| render_game(frame, &mut world))?;

        if world.resource::<core::TurnManager>().wants_quit {
            break;
        }

        let elapsed = frame_start.elapsed();
        let target = Duration::from_millis(33);
        if elapsed < target {
            std::thread::sleep(target - elapsed);
        }
    }

    sys::shutdown_logging();
    sys::leave_raw_mode()?;
    sys::leave_alternate_screen()?;
    terminal.show_cursor()?;
    Ok(())
}

fn key_to_command(code: KeyCode) -> Option<PlayerCommand> {
    match code {
        KeyCode::Up | KeyCode::Char('k') => Some(PlayerCommand::Move { dx: 0, dy: -1 }),
        KeyCode::Down | KeyCode::Char('j') => Some(PlayerCommand::Move { dx: 0, dy: 1 }),
        KeyCode::Left | KeyCode::Char('h') => Some(PlayerCommand::Move { dx: -1, dy: 0 }),
        KeyCode::Right | KeyCode::Char('l') => Some(PlayerCommand::Move { dx: 1, dy: 0 }),
        KeyCode::Home => Some(PlayerCommand::Move { dx: -1, dy: -1 }),
        KeyCode::End => Some(PlayerCommand::Move { dx: -1, dy: 1 }),
        KeyCode::PageUp => Some(PlayerCommand::Move { dx: 1, dy: -1 }),
        KeyCode::PageDown => Some(PlayerCommand::Move { dx: 1, dy: 1 }),
        KeyCode::Char('.') => Some(PlayerCommand::Wait),
        _ => None,
    }
}