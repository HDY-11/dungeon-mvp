//! 输入映射：后端无关的 [`InputEvent`] → 页栈意图 / `PlayerCommand`。
//!
//! # 为什么映射住在这里
//!
//! 「哪个键做哪件事」是**游戏与 UI 的契约**，不是终端细节。TUI 只负责把
//! `crossterm::event::KeyCode` 翻译成 [`render_api::Key`]；`presentation` 负责
//! 决定 `Key::Char('h')` 意味着"向左走"。GPU 后端因此不用重写一遍按键表，
//! 也不会出现"两个后端键位不一样"（DsnX14：「平台事件 → `InputEvent` 的翻译在
//! 各自后端，页栈路由与 tap-tap 只在 `presentation` 实现一次」）。
//!
//! # 键位表（与迁移前的 `src/main.rs::key_to_command` 等价）
//!
//! | 键 | 含义 |
//! |---|---|
//! | 方向键 / `h` `j` `k` `l` | 四方向移动（相邻怪物则攻击） |
//! | `Home` `End` `PgUp` `PgDn` | 四个对角方向 |
//! | `.` / 空格 | 等待一回合 |
//! | `Esc` | 有覆盖页则关页面，否则打开退出确认 |
//! | `q` | 打开退出确认 |
//! | `?` | 打开 Look 页（当前玩家位置） |
//!
//! # 只处理 `Pressed`
//!
//! 终端的 key-repeat 由 `tui` 后端在写入队列前去重；GPU 后端可能产生
//! `Repeated`。这里**忽略 `Repeated` 与 `Released`**：一回合只该由一个按键
//! 触发一次，让 repeat 直接移动是旧实现里"按住方向键暴走"的原因。

use render_api::{InputEvent, Key, KeyState, MouseEvent, MouseKind};

use crate::ui::{PageStack, UiIntent};
use ecs_core::PlayerCommand;

/// 一次输入映射的结果。
///
/// 三条出口对应装配层的三种动作，**互斥**：一个事件不会既开页面又移动。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InputOutcome {
    /// 事件被忽略（修饰键组合、`Released`、未知键、鼠标事件……）。
    Ignored,
    /// 页栈意图：交给 [`PageStack::apply`]。
    Ui(UiIntent),
    /// 玩家命令：交给装配层的 `apply_player_command`。
    Command(PlayerCommand),
}

impl InputOutcome {
    pub const fn is_ignored(self) -> bool {
        matches!(self, Self::Ignored)
    }

    pub const fn ui_intent(self) -> Option<UiIntent> {
        match self {
            Self::Ui(intent) => Some(intent),
            _ => None,
        }
    }

    pub const fn command(self) -> Option<PlayerCommand> {
        match self {
            Self::Command(command) => Some(command),
            _ => None,
        }
    }
}

/// 输入映射所需的全部上下文。
///
/// 把上下文收成一个结构体而不是给 `map_input_event` 一长串参数：映射函数会在
/// 测试里被调用很多次，参数表越长越容易在新增上下文时漏改某个调用点。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PresentationInput {
    /// 当前页栈状态（决定按键被谁消费）。
    pub page: render_api::PageKind,
    /// 玩家当前世界坐标（`?` 打开 Look 时的初始光标）。
    pub player_position: (usize, usize),
    /// 世界尺寸（Look 光标夹取用）。
    pub world_size: (usize, usize),
}

impl PresentationInput {
    pub const fn new(page: render_api::PageKind, player_position: (usize, usize)) -> Self {
        Self {
            page,
            player_position,
            world_size: (ecs_core::MAP_WIDTH, ecs_core::MAP_HEIGHT),
        }
    }

    pub const fn with_world_size(mut self, width: usize, height: usize) -> Self {
        self.world_size = (width, height);
        self
    }
}

/// 把一个后端无关的输入事件映射成页栈意图或玩家命令。
///
/// **不改任何状态**：Look 光标的移动由调用方按 [`UiIntent`] 决定后调用
/// [`PageStack::move_look_cursor`]，这样这个函数是纯函数、可直接单测。
pub fn map_input_event(event: InputEvent, input: &PresentationInput) -> InputOutcome {
    match event {
        InputEvent::Key(key_event) => map_key(key_event, input),
        // 本轮的页面都不需要鼠标；Look 页未来要支持点击移动光标，
        // 那时在这里接 `MouseKind::Down`，而不是让后端自己处理坐标换算。
        InputEvent::Mouse(_) => InputOutcome::Ignored,
        InputEvent::Resize { .. } => InputOutcome::Ignored,
        InputEvent::Focus(_) => InputOutcome::Ignored,
        // 后端报告「窗口/终端请求退出」：等同用户按了退出，但**仍要走确认流程**
        // ——直接退出会绕过页栈，用户会以为程序崩了。
        InputEvent::Quit => InputOutcome::Ui(UiIntent::OpenQuitDialog),
    }
}

fn map_key(event: render_api::KeyEvent, input: &PresentationInput) -> InputOutcome {
    // 只认"按下"：见模块文档。
    if event.state != KeyState::Pressed {
        return InputOutcome::Ignored;
    }
    // 带修饰键的组合留给未来的快捷键（Ctrl+S 存盘之类），不参与移动，
    // 免得 `Ctrl+H` 这类终端控制键被当成"向左走"。
    if !event.modifiers.is_none() {
        return InputOutcome::Ignored;
    }

    let in_overlay = !matches!(input.page, render_api::PageKind::Game);

    match event.key {
        Key::Esc => {
            if in_overlay {
                InputOutcome::Ui(UiIntent::ClosePage)
            } else {
                InputOutcome::Ui(UiIntent::OpenQuitDialog)
            }
        }
        Key::Char('q') => {
            if in_overlay {
                InputOutcome::Ui(UiIntent::ClosePage)
            } else {
                InputOutcome::Ui(UiIntent::OpenQuitDialog)
            }
        }
        Key::Char('?') => {
            if in_overlay {
                InputOutcome::Ignored
            } else {
                InputOutcome::Ui(UiIntent::OpenLook {
                    x: input.player_position.0,
                    y: input.player_position.1,
                })
            }
        }
        Key::Enter => match input.page {
            render_api::PageKind::Dialog => InputOutcome::Ui(UiIntent::ConfirmQuit),
            _ => InputOutcome::Ignored,
        },
        other => {
            // 覆盖页打开时，移动/等待键不该同时驱动世界——否则按方向键翻看
            // 地形会把玩家走出去（旧实现没有覆盖页，所以没这个问题）。
            if in_overlay {
                return InputOutcome::Ignored;
            }
            match other {
                Key::Up | Key::Char('k') => direction(0, -1),
                Key::Down | Key::Char('j') => direction(0, 1),
                Key::Left | Key::Char('h') => direction(-1, 0),
                Key::Right | Key::Char('l') => direction(1, 0),
                Key::Home => direction(-1, -1),
                Key::End => direction(-1, 1),
                Key::PageUp => direction(1, -1),
                Key::PageDown => direction(1, 1),
                Key::Char('.') | Key::Space => InputOutcome::Command(PlayerCommand::Wait),
                _ => InputOutcome::Ignored,
            }
        }
    }
}

fn direction(dx: isize, dy: isize) -> InputOutcome {
    InputOutcome::Command(PlayerCommand::Move { dx, dy })
}

/// 鼠标事件的位置（给未来的 Look 点击用；当前只做类型收敛，不参与映射）。
pub fn mouse_cell(event: &MouseEvent) -> Option<(i32, i32)> {
    match event.kind {
        MouseKind::Down | MouseKind::Up | MouseKind::Moved => {
            Some((i32::from(event.x), i32::from(event.y)))
        }
        _ => None,
    }
}

/// 按页栈意图更新页栈状态。装配层用它把 [`InputOutcome::Ui`] 落地。
///
/// 返回 `true` 表示页栈消费了该意图。
pub fn apply_ui_intent(pages: &mut PageStack, intent: UiIntent, world: (usize, usize)) -> bool {
    match intent {
        UiIntent::MoveLookCursor { dx, dy } => {
            if pages.kind() == render_api::PageKind::Look {
                pages.move_look_cursor(dx, dy, world);
                true
            } else {
                false
            }
        }
        other => pages.apply(other),
    }
}

#[cfg(test)]
mod tests;
