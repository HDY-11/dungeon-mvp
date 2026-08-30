//! 查看模式页（x 键进入）：方向键移动光标，Home/End 跳角落

use std::io;

use bevy_ecs::prelude::World;
use crossterm::event::KeyCode;
use dungeon_core::{LookCursor, MAP_HEIGHT, MAP_WIDTH};

/// 光标查看页按键处理
pub(super) fn process_look_key(code: KeyCode, world: &mut World) -> io::Result<bool> {
    match code {
        KeyCode::Up | KeyCode::Char('k') => super::move_cursor(world, 0, -1),
        KeyCode::Down | KeyCode::Char('j') => super::move_cursor(world, 0, 1),
        KeyCode::Left | KeyCode::Char('h') => super::move_cursor(world, -1, 0),
        KeyCode::Right | KeyCode::Char('l') => super::move_cursor(world, 1, 0),
        KeyCode::Home => {
            let mut c = world.resource_mut::<LookCursor>();
            c.x = 0;
            c.y = 0;
        }
        KeyCode::End => {
            let mut c = world.resource_mut::<LookCursor>();
            c.x = MAP_WIDTH - 1;
            c.y = MAP_HEIGHT - 1;
        }
        KeyCode::Char('x') | KeyCode::Esc => {
            world.resource_mut::<LookCursor>().active = false;
            world.resource_mut::<dungeon_action::PageStack>().pop();
        }
        _ => {}
    }
    Ok(false)
}
