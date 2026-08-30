//! 游戏页按键处理：keymap 解析 → 玩家行动 / 页栈切换 / 存档读档

use std::io;

use bevy_ecs::prelude::*;
use crossterm::event::KeyCode;
use dungeon_action::{PlayerAction, handle_player_direction, handle_skill, handle_wait};
use dungeon_core::{
    EventLog, InventoryUI, LookCursor, MAP_HEIGHT, MAP_WIDTH, Player, Position, TurnManager, ops,
};
use dungeon_world::{load_game, save_game};

use crate::keymap;
use dungeon_core::OptionLogExt;

/// 游戏页按键处理
pub(super) fn process_game_key(code: KeyCode, world: &mut World) -> io::Result<bool> {
    // 先按大写字母处理（KeyCode::Char('E') 等）
    let code = match code {
        KeyCode::Char(c) if c.is_ascii_uppercase() => KeyCode::Char(c.to_ascii_lowercase()),
        other => other,
    };
    let Some(action) = keymap::resolve(code) else {
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
                    dungeon_action::Page::Dialog(dungeon_action::DialogKind::Quit),
                );
            }
            Ok(false)
        }
        PlayerAction::DescendStairs => {
            if ops::on_stairs(world) {
                world.resource_mut::<dungeon_action::PageStack>().push(
                    dungeon_action::Page::Dialog(dungeon_action::DialogKind::Descend),
                );
            }
            Ok(false)
        }

        // ── 模态（阻塞式 UI，需暂停输入线程） ──
        PlayerAction::Throw => {
            // I60/I73: 副手可投掷 → 直接进入瞄准；否则进选择页（装填或换装）
            if !crate::throw::try_enter_throw_aim(world) {
                world
                    .resource_mut::<dungeon_action::PageStack>()
                    .push(dungeon_action::Page::ThrowSelect);
            }
            Ok(false)
        }
        PlayerAction::OpenInventory => {
            world.insert_resource(InventoryUI::default());
            world
                .resource_mut::<dungeon_action::PageStack>()
                .push(dungeon_action::Page::Inventory);
            Ok(false)
        }
        PlayerAction::OpenLook => {
            let (cx, cy) = {
                let mut q = world
                    .try_query::<(&Player, &Position)>()
                    .expect_log("Player+Position registered");
                q.iter(world)
                    .next()
                    .map(|(_, p)| (p.x, p.y))
                    .unwrap_or((MAP_WIDTH / 2, MAP_HEIGHT / 2))
            };
            world.insert_resource(LookCursor {
                active: true,
                x: cx,
                y: cy,
            });
            world
                .resource_mut::<dungeon_action::PageStack>()
                .push(dungeon_action::Page::Look);
            Ok(false)
        }
        PlayerAction::PickupGround => {
            ops::pickup_ground(world);
            Ok(false)
        }
        PlayerAction::SaveGame => {
            // A37: 存档走 dungeon_world::save_game 单入口（magic + 版本化格式）
            match save_game(world, "save.bin") {
                Ok(()) => world
                    .resource_mut::<EventLog>()
                    .push(dungeon_core::EventMessage::system("已保存")),
                Err(_e) => world
                    .resource_mut::<EventLog>()
                    .push(dungeon_core::EventMessage::system("存档失败")),
            }
            Ok(false)
        }
        PlayerAction::LoadGame => {
            match load_game(world, "save.bin") {
                Ok(()) => world
                    .resource_mut::<EventLog>()
                    .push(dungeon_core::EventMessage::system("已读档")),
                Err(_e) => world
                    .resource_mut::<EventLog>()
                    .push(dungeon_core::EventMessage::system("读档失败")),
            }
            Ok(false)
        }
    }
}
