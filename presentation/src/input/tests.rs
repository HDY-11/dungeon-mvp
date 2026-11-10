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

/// 下楼键：`>` 翻译成 `Descend`，**输入层不检查脚下有没有楼梯**（那是规则的事）。
#[test]
fn the_greater_than_key_means_descend() {
    let input = ctx(PageKind::Game);
    assert_eq!(
        map_input_event(key(Key::Char('>')), &input).command(),
        Some(PlayerCommand::Descend),
        "> 应当是下楼"
    );
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
///
/// 背包页的 ↑/↓ 不是"被吞掉"，而是翻译成**光标移动意图**（`MoveInventoryCursor`）——
/// 关键性质相同：它们不驱动世界。
#[test]
fn overlays_swallow_movement_keys() {
    for page in [PageKind::Look, PageKind::Dialog] {
        let input = ctx(page);
        for k in [Key::Up, Key::Char('h'), Key::Char('.'), Key::Home] {
            assert!(
                map_input_event(key(k), &input).is_ignored(),
                "{page:?} 上 {k:?} 不得驱动世界"
            );
        }
    }
    // 背包页：方向键变成光标意图，仍然不是移动命令。
    let input = ctx(PageKind::Inventory);
    let up = map_input_event(key(Key::Up), &input);
    assert!(up.command().is_none(), "背包页 ↑ 不得驱动世界");
    let enter = map_input_event(key(Key::Enter), &input);
    assert!(enter.command().is_none(), "背包页 Enter 不得驱动世界");
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
    for k in [Key::Unknown, Key::Backspace, Key::F(5)] {
        assert!(map_input_event(key(k), &input).is_ignored(), "{k:?}");
    }
}

/// Tab 在游戏页轮转四栏焦点；背包页轮转网格焦点；查看/对话框页不处理。
#[test]
fn tab_cycles_the_pane_focus_only_on_the_game_page() {
    let input = ctx(PageKind::Game);
    assert_eq!(
        map_input_event(key(Key::Tab), &input).ui_intent(),
        Some(UiIntent::CyclePaneFocus)
    );

    let input = ctx(PageKind::Inventory);
    assert_eq!(
        map_input_event(key(Key::Tab), &input).ui_intent(),
        Some(UiIntent::CycleInventoryFocus),
        "背包页的 Tab 轮转 Nav/Context 焦点位"
    );

    for page in [PageKind::Look, PageKind::Dialog] {
        let input = ctx(page);
        assert!(
            map_input_event(key(Key::Tab), &input).is_ignored(),
            "{page:?} 页的 Tab 不轮转焦点"
        );
    }
}

/// 焦点在信息栏时，↑/↓ 变成选行意图而不是玩家移动；切回摄像机后方向键恢复移动。
#[test]
fn arrow_keys_select_info_rows_only_when_the_info_pane_is_focused() {
    let input = ctx(PageKind::Game).with_pane_focus(render_api::Pane::Info);
    let up = map_input_event(key(Key::Up), &input);
    assert!(up.command().is_none(), "信息栏焦点时 ↑ 不得驱动世界");
    assert_eq!(up.ui_intent(), Some(UiIntent::MoveInfoCursor { dy: -1 }));
    let down = map_input_event(key(Key::Down), &input);
    assert!(down.command().is_none(), "信息栏焦点时 ↓ 不得驱动世界");
    assert_eq!(down.ui_intent(), Some(UiIntent::MoveInfoCursor { dy: 1 }));

    let input = ctx(PageKind::Game).with_pane_focus(render_api::Pane::Camera);
    let up = map_input_event(key(Key::Up), &input);
    assert!(up.command().is_some(), "摄像机焦点时 ↑ 仍是移动");
    assert!(up.ui_intent().is_none());
}

/// 焦点在事件栏时，↑/↓ 变成"向上/向下翻历史"的意图；不影响玩家移动，
/// 也不和信息栏的选行混淆（DsnE21 决策 4 / DsnA6）。
#[test]
fn arrow_keys_scroll_events_only_when_the_events_pane_is_focused() {
    let input = ctx(PageKind::Game).with_pane_focus(render_api::Pane::Events);
    let up = map_input_event(key(Key::Up), &input);
    assert!(up.command().is_none(), "事件栏焦点时 ↑ 不得驱动世界");
    assert_eq!(up.ui_intent(), Some(UiIntent::ScrollEvents { dy: 1 }));
    let down = map_input_event(key(Key::Down), &input);
    assert!(down.command().is_none(), "事件栏焦点时 ↓ 不得驱动世界");
    assert_eq!(down.ui_intent(), Some(UiIntent::ScrollEvents { dy: -1 }));

    // `j/k` 仍留给移动：焦点只是"看"，别把走路键也接管掉。
    let walking = map_input_event(key(Key::Char('k')), &input);
    assert!(walking.command().is_some(), "事件栏焦点时 k 仍是移动");

    // 信息栏焦点时 ↑ 是选行，不是滚事件。
    let info = ctx(PageKind::Game).with_pane_focus(render_api::Pane::Info);
    assert_eq!(
        map_input_event(key(Key::Up), &info).ui_intent(),
        Some(UiIntent::MoveInfoCursor { dy: -1 })
    );
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

/// 投掷键位：游戏页 `t` 开瞄准页；瞄准页方向键移光标、Enter 确认、x 取消。
#[test]
fn throw_keys_open_aim_move_and_confirm() {
    let game = ctx(PageKind::Game);
    assert_eq!(
        map_input_event(key(Key::Char('t')), &game),
        InputOutcome::Ui(UiIntent::OpenThrow { x: 10, y: 20 }),
        "t 带着玩家当前位置开瞄准页（初始落点由应用层算）"
    );

    let aim = ctx(PageKind::ThrowAim);
    let cases = [
        (Key::Up, (0, -1)),
        (Key::Char('k'), (0, -1)),
        (Key::Down, (0, 1)),
        (Key::Char('j'), (0, 1)),
        (Key::Left, (-1, 0)),
        (Key::Char('h'), (-1, 0)),
        (Key::Right, (1, 0)),
        (Key::Char('l'), (1, 0)),
    ];
    for (k, (dx, dy)) in cases {
        assert_eq!(
            map_input_event(key(k), &aim),
            InputOutcome::Ui(UiIntent::MoveThrowCursor { dx, dy }),
            "{k:?} 应当移动瞄准光标"
        );
    }
    assert_eq!(
        map_input_event(key(Key::Enter), &aim),
        InputOutcome::Ui(UiIntent::ConfirmThrow)
    );
    assert_eq!(
        map_input_event(key(Key::Char('x')), &aim),
        InputOutcome::Ui(UiIntent::ClosePage)
    );
    // 瞄准页不驱动玩家移动：方向键是 UI 意图，不是 Move 命令。
    assert!(
        map_input_event(key(Key::Up), &aim).command().is_none(),
        "瞄准时方向键不能同时走路"
    );
}
