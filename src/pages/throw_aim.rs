//! 投掷瞄准页：方向键移动光标 → Enter 投掷 / x、Esc 取消

use std::io;

use bevy_ecs::prelude::*;
use crossterm::event::KeyCode;
use dungeon_core::ThrowPreview;

/// 瞄准游标移动：更新 ThrowPreview 并同步渲染光标（LookCursor），弹道随动
fn move_throw_cursor(world: &mut World, dx: isize, dy: isize) {
    let (nx, ny) = {
        let mut tp = world.resource_mut::<ThrowPreview>();
        tp.cursor.0 =
            (tp.cursor.0 as isize + dx).clamp(0, dungeon_core::MAP_WIDTH as isize - 1) as usize;
        tp.cursor.1 =
            (tp.cursor.1 as isize + dy).clamp(0, dungeon_core::MAP_HEIGHT as isize - 1) as usize;
        tp.cursor
    };
    world.resource_mut::<dungeon_core::LookCursor>().x = nx;
    world.resource_mut::<dungeon_core::LookCursor>().y = ny;
    crate::throw::update_throw_path(world);
}

/// 投掷瞄准页
pub(super) fn process_throw_aim_key(code: KeyCode, world: &mut World) -> io::Result<bool> {
    match code {
        // I73: 瞄准游标移动（ThrowPreview 为主，LookCursor 同步——修复光标高亮不跟手的既有瑕疵）
        KeyCode::Up | KeyCode::Char('k') => move_throw_cursor(world, 0, -1),
        KeyCode::Down | KeyCode::Char('j') => move_throw_cursor(world, 0, 1),
        KeyCode::Left | KeyCode::Char('h') => move_throw_cursor(world, -1, 0),
        KeyCode::Right | KeyCode::Char('l') => move_throw_cursor(world, 1, 0),
        // I77/D20: Enter 一次确认（直接入队，不再走 tap-tap 双确认——瞄准页本身已是确认页）
        KeyCode::Enter => return Ok(dungeon_action::confirm_throw(world)),
        KeyCode::Esc | KeyCode::Char('x') => {
            world.resource_mut::<ThrowPreview>().active = false;
            world.resource_mut::<dungeon_core::LookCursor>().active = false; // I63: 清除光标残留
            world.resource_mut::<dungeon_action::PageStack>().pop();
        }
        _ => {}
    }
    Ok(false)
}
