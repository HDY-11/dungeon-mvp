//! 投掷选择页：自动装填投掷物到副手（不可投掷的旧副手会被换下）

use std::io;

use bevy_ecs::prelude::*;
use crossterm::event::KeyCode;
use dungeon_core::EventLog;

/// 投掷选择页
pub(super) fn process_throw_select_key(code: KeyCode, world: &mut World) -> io::Result<bool> {
    match code {
        KeyCode::Enter | KeyCode::Char('r') | KeyCode::Char('y') => {
            // I73: 自动装填后复用共享的进入瞄准逻辑（不可投掷的旧副手会被换下）
            crate::throw::auto_equip_throwable(world);
            world.resource_mut::<dungeon_action::PageStack>().pop(); // 离开选择页
            if !crate::throw::try_enter_throw_aim(world) {
                world
                    .resource_mut::<EventLog>()
                    .push(dungeon_core::EventMessage::system(
                        "没有可投掷的物品".to_string(),
                    ));
            }
        }
        KeyCode::Esc => {
            world.resource_mut::<dungeon_action::PageStack>().pop();
        }
        _ => {}
    }
    Ok(false)
}
