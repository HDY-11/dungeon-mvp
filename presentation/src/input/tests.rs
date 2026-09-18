//! `input` 的测试：按键 → 意图 / 命令的映射。
//!
//! 键位表是**玩家可见的行为**，所以这里的用例按"键位表"组织：每个键至少一条，
//! 加上三条边界（覆盖页吃掉移动键、修饰键不参与移动、只认 Pressed）。

use render_api::{InputEvent, Key, KeyEvent, KeyState, Modifiers, PageKind};
use ecs_core::PlayerCommand;

use super::*;
use crate::ui::UiIntent;

fn ctx(page: PageKind) -> PresentationInput {
    PresentationInput::new(page, (10, 20)).with_world_size(80, 60)
}

fn key(k: Key) -> InputEvent {
    InputEvent::Key(KeyEvent::pressed(k))
}

/// 页栈在游戏页：方向键与 vi 键都映射成移动。
#[test]
fn movement_keys_map_to_move_commands() {
    let input = ctx(PageKind::Game);
    let cases = [
        (Key::Up, (0, -1)),
        (Key::Down, (0, 1)),
        (Key::Left, (-1, 0)),
        (Key::Right, (1, 0)),
        (Key::Char('k'), (0, -1)),
        (Key::Char('j'), (0, 1)),
        (Key::Char('h'), (-1, 0)),
        (Key::Char('l'), (1, 0)),
    ];
    for (k, (dx, dy)) in cases {
        assert_eq!(
            map_input_event(key(k), &input).command(),
            Some(PlayerCommand::Move { dx, dy }),
            "{k:?} 应当移动到 ({dx},{dy})"
        );
    }
}

/// 四个对角：Home/End/PgUp/PgDn，与迁移前 `main.rs` 的键位一致。
#[test]
fn diagonal_keys_map_to_diagonal_moves() {
    let input = ctx(PageKind::Game);
    let cases = [
        (Key::Home, (-1, -1)),
        (Key::End, (-1, 1)),
        (Key::PageUp, (1, -1)),
        (Key::PageDown, (1, 1)),
    ];
    for (k, (dx, dy)) in cases {
        assert_eq!(
            map_input_event(key(k), &input).command(),
            Some(PlayerCommand::Move { dx, dy }),
            "{k:?} 应当斜向移动到 ({dx},{dy})"
        );
    }
}

/// 等待键：`.` 与空格等价。
#[test]
fn wait_keys_map_to_wait() {
    let input = ctx(PageKind::Game);
    for k in [Key::Char('.'), Key::Space] {
        assert_eq!(
            map_input_event(key(k), &input).command(),
            Some(PlayerCommand::Wait),
            "{k:?} 应当是等待"
        );
    }
}

/// 退出：游戏页按 `Esc` / `q` 打开确认对话框，而**不是**直接退出。
#[test]
fn quit_keys_open_a_confirmation_dialog() {
    let input = ctx(PageKind::Game);
    for k in [Key::Esc, Key::Char('q')] {
        assert_eq!(
            map_input_event(key(k), &input).ui_intent(),
            Some(UiIntent::OpenQuitDialog),
            "{k:?} 应当先请求确认"
        );
    }
}

/// 覆盖页上按 `Esc` / `q` 是关页面，不是退出。
#[test]
fn quit_keys_close_overlays_instead_of_quitting() {
    for page in [PageKind::Look, PageKind::Dialog] {
        let input = ctx(page);
        for k in [Key::Esc, Key::Char('q')] {
            assert_eq!(
                map_input_event(key(k), &input).ui_intent(),
                Some(UiIntent::ClosePage),
                "{page:?} 上 {k:?} 应当只关页面"
            );
        }
    }
}

/// `?` 打开 Look 页，光标落在玩家位置。
#[test]
fn question_mark_opens_look_at_the_player() {
    let input = PresentationInput::new(PageKind::Game, (12, 34)).with_world_size(80, 60);
    assert_eq!(
        map_input_event(key(Key::Char('?')), &input).ui_intent(),
        Some(UiIntent::OpenLook { x: 12, y: 34 })
    );
}

/// 已经在覆盖页时 `?` 不做事（避免把 Look 换成 Look、丢掉光标）。
#[test]
fn question_mark_is_ignored_inside_overlays() {
    let input = ctx(PageKind::Look);
    assert!(map_input_event(key(Key::Char('?')), &input).is_ignored());
}

/// 覆盖页吃掉移动键：按方向键翻看地形不该把玩家走出去。
#[test]
fn overlays_swallow_movement_keys() {
    for page in [PageKind::Look, PageKind::Dialog, PageKind::Inventory] {
        let input = ctx(page);
        for k in [Key::Up, Key::Char('h'), Key::Char('.'), Key::Home] {
            assert!(
                map_input_event(key(k), &input).is_ignored(),
                "{page:?} 上 {k:?} 不得驱动世界"
            );
        }
    }
}

/// 确认键：只在退出对话框里被消费。
#[test]
fn enter_confirms_only_in_the_dialog() {
    assert_eq!(
        map_input_event(key(Key::Enter), &ctx(PageKind::Dialog)).ui_intent(),
        Some(UiIntent::ConfirmQuit)
    );
    assert!(map_input_event(key(Key::Enter), &ctx(PageKind::Game)).is_ignored());
    assert!(map_input_event(key(Key::Enter), &ctx(PageKind::Look)).is_ignored());
}

/// 只认 `Pressed`：`Repeated` / `Released` 一律忽略。
///
/// 这条防的是"按住方向键暴走"：终端的 key-repeat 在 TUI 后端去重，
/// 但 GPU 后端会产生 `Repeated`，映射层必须自己挡住。
#[test]
fn repeated_and_released_keys_are_ignored() {
    let input = ctx(PageKind::Game);
    for state in [KeyState::Repeated, KeyState::Released] {
        let event = InputEvent::Key(KeyEvent {
            key: Key::Up,
            state,
            modifiers: Modifiers::NONE,
        });
        assert!(
            map_input_event(event, &input).is_ignored(),
            "{state:?} 不得触发移动"
        );
    }
}

/// 带修饰键的组合不参与移动（`Ctrl+H` 是终端退格，不是"向左走"）。
#[test]
fn modified_keys_do_not_move_the_player() {
    let input = ctx(PageKind::Game);
    for modifiers in [
        Modifiers {
            shift: true,
            ..Modifiers::NONE
        },
        Modifiers {
            ctrl: true,
            ..Modifiers::NONE
        },
        Modifiers {
            alt: true,
            ..Modifiers::NONE
        },
    ] {
        let event = InputEvent::Key(KeyEvent {
            key: Key::Char('h'),
            state: KeyState::Pressed,
            modifiers,
        });
        assert!(map_input_event(event, &input).is_ignored(), "{modifiers:?}");
    }
}

/// 后端报告"请求退出"时必须走确认流程，不能直接退。
#[test]
fn backend_quit_request_goes_through_confirmation() {
    let input = ctx(PageKind::Game);
    assert_eq!(
        map_input_event(InputEvent::Quit, &input).ui_intent(),
        Some(UiIntent::OpenQuitDialog)
    );
}

/// 缩放 / 焦点 / 鼠标事件不产生意图（本轮）。
#[test]
fn non_key_events_are_ignored() {
    let input = ctx(PageKind::Game);
    for event in [
        InputEvent::Resize {
            width: 100,
            height: 50,
        },
        InputEvent::Focus(true),
        InputEvent::Mouse(render_api::MouseEvent {
            x: 3,
            y: 4,
            kind: render_api::MouseKind::Down,
            button: render_api::MouseButton::Left,
        }),
    ] {
        assert!(map_input_event(event, &input).is_ignored());
    }
}

/// 未知键不做事，也不会 panic。
#[test]
fn unknown_keys_are_ignored() {
    let input = ctx(PageKind::Game);
    for k in [Key::Unknown, Key::Tab, Key::Backspace, Key::F(5)] {
        assert!(map_input_event(key(k), &input).is_ignored(), "{k:?}");
    }
}

/// `apply_ui_intent` 是唯一能移动 Look 光标的地方（它带世界尺寸）。
#[test]
fn apply_ui_intent_moves_the_look_cursor() {
    let mut pages = PageStack::new();
    pages.open_look(10, 10);

    let handled = apply_ui_intent(&mut pages, UiIntent::MoveLookCursor { dx: 5, dy: -5 }, (80, 60));
    assert!(handled);
    assert_eq!(pages.look_cursor(), (15, 5));

    // 越界由世界尺寸夹住。
    apply_ui_intent(&mut pages, UiIntent::MoveLookCursor { dx: 999, dy: 999 }, (80, 60));
    assert_eq!(pages.look_cursor(), (79, 59));
}

/// 非 Look 页上移动光标返回 false，供装配层区分"被消费"与"没消费"。
#[test]
fn apply_ui_intent_reports_unhandled_cursor_moves() {
    let mut pages = PageStack::new();
    assert!(!apply_ui_intent(
        &mut pages,
        UiIntent::MoveLookCursor { dx: 1, dy: 0 },
        (80, 60)
    ));
    assert_eq!(pages.look_cursor(), (0, 0));
}

/// 其余意图转发给页栈。
#[test]
fn apply_ui_intent_forwards_other_intents() {
    let mut pages = PageStack::new();
    assert!(apply_ui_intent(&mut pages, UiIntent::OpenQuitDialog, (80, 60)));
    assert!(pages.is_quit_pending());
    assert!(apply_ui_intent(&mut pages, UiIntent::CancelQuit, (80, 60)));
    assert!(pages.is_game());
}

#[test]
fn input_outcome_accessors_are_exclusive() {
    let command = InputOutcome::Command(PlayerCommand::Wait);
    assert!(command.command().is_some());
    assert!(command.ui_intent().is_none());
    assert!(!command.is_ignored());

    let ui = InputOutcome::Ui(UiIntent::ClosePage);
    assert!(ui.ui_intent().is_some());
    assert!(ui.command().is_none());

    assert!(InputOutcome::Ignored.is_ignored());
}

#[test]
fn mouse_cell_reports_click_coordinates() {
    use render_api::{MouseButton, MouseEvent, MouseKind};
    let event = MouseEvent {
        x: 7,
        y: 9,
        kind: MouseKind::Down,
        button: MouseButton::Left,
    };
    assert_eq!(mouse_cell(&event), Some((7, 9)));

    let scroll = MouseEvent {
        kind: MouseKind::ScrollUp,
        ..event
    };
    assert_eq!(mouse_cell(&scroll), None, "滚轮没有格子坐标");
}

/// 默认世界尺寸来自 `ecs_core` 的 MAP 常量（防止有人硬编码 80×60 后漂移）。
#[test]
fn default_world_size_tracks_core_map_constants() {
    let input = PresentationInput::new(PageKind::Game, (0, 0));
    assert_eq!(
        input.world_size,
        (ecs_core::MAP_WIDTH, ecs_core::MAP_HEIGHT)
    );
}
