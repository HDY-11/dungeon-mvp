//! 页栈各页面按键处理器（按页拆分，main.rs 只保留分派入口）
//!
//! 每个页面一个文件，与 `dungeon_action::Page` 枚举一一对应：
//! Game / Look / ThrowSelect / ThrowAim / Inventory / Dialog。

mod dialog;
mod game;
mod inventory;
mod look;
mod throw_aim;
mod throw_select;

use bevy_ecs::prelude::World;
use crossterm::event::KeyCode;
use dungeon_core::LookCursor;
use std::io;

/// 游标移动（I73 收敛：look/throw_aim 的方向键移动共用，边界钳制）
pub(crate) fn move_cursor(world: &mut World, dx: isize, dy: isize) {
    let mut c = world.resource_mut::<LookCursor>();
    c.x = (c.x as isize + dx).clamp(0, dungeon_core::MAP_WIDTH as isize - 1) as usize;
    c.y = (c.y as isize + dy).clamp(0, dungeon_core::MAP_HEIGHT as isize - 1) as usize;
}

/// 页栈分派：当前页 → 对应页面处理器
pub fn process_key(code: KeyCode, world: &mut World) -> io::Result<bool> {
    let page = world
        .resource::<dungeon_action::PageStack>()
        .current()
        .clone();
    match page {
        dungeon_action::Page::Game => game::process_game_key(code, world),
        dungeon_action::Page::Look => look::process_look_key(code, world),
        dungeon_action::Page::ThrowSelect => throw_select::process_throw_select_key(code, world),
        dungeon_action::Page::ThrowAim => throw_aim::process_throw_aim_key(code, world),
        dungeon_action::Page::Inventory => inventory::process_inventory_key(code, world),
        dungeon_action::Page::Dialog(_) => dialog::process_dialog_key(code, world),
    }
}
