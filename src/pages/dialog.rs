//! 对话框页：y 确认（退出/下楼）/ n、Esc 取消

use std::io;

use bevy_ecs::prelude::*;

use crossterm::event::KeyCode;
use dungeon_core::{TurnManager, ops};
use dungeon_world::descend;

/// 对话框页按键处理（I71: 行为按 DialogKind 分派，不再匹配标题字符串）
pub(super) fn process_dialog_key(code: KeyCode, world: &mut World) -> io::Result<bool> {
    match code {
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            // 弹出前获取对话种类来决定行为
            let page = world.resource_mut::<dungeon_action::PageStack>().pop();
            if let Some(dungeon_action::Page::Dialog(confirmed)) = page {
                match confirmed {
                    dungeon_action::DialogKind::Quit => {
                        world.resource_mut::<TurnManager>().wants_quit = true;
                    }
                    dungeon_action::DialogKind::Descend if ops::on_stairs(world) => {
                        descend(world);
                        ops::post_load_refresh(world);
                    }
                    _ => {}
                }
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
