//! 后端无关的输入事件与表面信息。
//!
//! 后端（TUI 的 crossterm 输入线程 / 未来 GPU 的 winit）负责把平台事件
//! 翻译成 [`InputEvent`] 并写入 [`InputQueue`]；`presentation` 消费队列，
//! 负责页栈路由和游戏命令映射。这样页栈行为只实现一次，TUI 和 GPU 共享。

use bevy_ecs::prelude::*;
use std::collections::VecDeque;

/// 后端无关的按键。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum Key {
    #[default]
    Unknown,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Enter,
    Esc,
    Tab,
    Backspace,
    Delete,
    Space,
    Char(char),
    F(u8),
}

/// 修饰键状态。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}

impl Modifiers {
    pub const NONE: Self = Self {
        shift: false,
        ctrl: false,
        alt: false,
    };

    pub const fn is_none(self) -> bool {
        !self.shift && !self.ctrl && !self.alt
    }
}

/// 按键事件类型。
///
/// 终端 key-repeat 通常由后端在写入队列前去重；GPU 后端可能产生 `Repeated`，
/// `presentation` 可以按需忽略或处理。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum KeyState {
    #[default]
    Pressed,
    Repeated,
    Released,
}

/// 一次按键事件。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct KeyEvent {
    pub key: Key,
    pub state: KeyState,
    pub modifiers: Modifiers,
}

impl KeyEvent {
    pub const fn pressed(key: Key) -> Self {
        Self {
            key,
            state: KeyState::Pressed,
            modifiers: Modifiers::NONE,
        }
    }

    pub const fn repeated(key: Key) -> Self {
        Self {
            key,
            state: KeyState::Repeated,
            modifiers: Modifiers::NONE,
        }
    }

    pub const fn released(key: Key) -> Self {
        Self {
            key,
            state: KeyState::Released,
            modifiers: Modifiers::NONE,
        }
    }

    pub fn with_modifiers(mut self, modifiers: Modifiers) -> Self {
        self.modifiers = modifiers;
        self
    }

    /// 是否为按下类事件（Pressed / Repeated）。
    pub const fn is_press(self) -> bool {
        matches!(self.state, KeyState::Pressed | KeyState::Repeated)
    }
}

/// 鼠标按键。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum MouseButton {
    #[default]
    None,
    Left,
    Right,
    Middle,
    Other(u8),
}

/// 鼠标事件类型。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum MouseKind {
    #[default]
    Moved,
    Down,
    Up,
    ScrollUp,
    ScrollDown,
}

/// 一次鼠标事件。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct MouseEvent {
    pub x: u16,
    pub y: u16,
    pub kind: MouseKind,
    pub button: MouseButton,
}

/// 后端无关的输入事件。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum InputEvent {
    Key(KeyEvent),
    Resize {
        width: u16,
        height: u16,
    },
    Mouse(MouseEvent),
    Focus(bool),
    /// 窗口 / 终端请求退出（例如 winit 的 CloseRequested）。
    Quit,
}

impl InputEvent {
    pub const fn key(key: Key) -> Self {
        Self::Key(KeyEvent::pressed(key))
    }

    pub const fn resize(width: u16, height: u16) -> Self {
        Self::Resize { width, height }
    }

    pub const fn is_key(self) -> bool {
        matches!(self, Self::Key(_))
    }

    pub const fn is_quit(self) -> bool {
        matches!(self, Self::Quit)
    }
}

/// 后端写入、`presentation` 消费的输入队列。
#[derive(Resource, Default, Debug)]
pub struct InputQueue {
    events: VecDeque<InputEvent>,
}

impl InputQueue {
    pub fn push(&mut self, event: InputEvent) {
        self.events.push_back(event);
    }

    pub fn extend<I: IntoIterator<Item = InputEvent>>(&mut self, events: I) {
        self.events.extend(events);
    }

    pub fn pop(&mut self) -> Option<InputEvent> {
        self.events.pop_front()
    }

    /// 取出当前所有事件，保持先进先出顺序。
    pub fn take(&mut self) -> Vec<InputEvent> {
        std::mem::take(&mut self.events).into_iter().collect()
    }

    pub fn peek(&self) -> Option<&InputEvent> {
        self.events.front()
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    pub fn clear(&mut self) {
        self.events.clear();
    }
}

/// 后端表面尺寸。
///
/// - TUI：`width` / `height` 是终端格子数，`cell_width` / `cell_height` 为 1；
/// - GPU：`width` / `height` 是像素数，`cell_width` / `cell_height` 是
///   一个世界 tile 占用的像素数。
#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug)]
pub struct SurfaceInfo {
    pub width: u16,
    pub height: u16,
    pub cell_width: u16,
    pub cell_height: u16,
}

impl Default for SurfaceInfo {
    fn default() -> Self {
        Self {
            width: 0,
            height: 0,
            cell_width: 1,
            cell_height: 1,
        }
    }
}

impl SurfaceInfo {
    pub const fn new(width: u16, height: u16, cell_width: u16, cell_height: u16) -> Self {
        Self {
            width,
            height,
            cell_width,
            cell_height,
        }
    }

    pub const fn is_valid(self) -> bool {
        self.width > 0 && self.height > 0 && self.cell_width > 0 && self.cell_height > 0
    }

    /// 可容纳的表面单位数（TUI 为格子数，GPU 为 tile 数）。
    ///
    /// 参数无效时返回 `(0, 0)`。
    pub fn cells(self) -> (u16, u16) {
        if self.is_valid() {
            (self.width / self.cell_width, self.height / self.cell_height)
        } else {
            (0, 0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_queue_preserves_fifo_order() {
        let mut queue = InputQueue::default();
        assert!(queue.is_empty());
        queue.push(InputEvent::key(Key::Up));
        queue.push(InputEvent::key(Key::Enter));
        assert_eq!(queue.len(), 2);
        assert_eq!(queue.peek(), Some(&InputEvent::key(Key::Up)));
        assert_eq!(queue.pop(), Some(InputEvent::key(Key::Up)));
        assert_eq!(queue.pop(), Some(InputEvent::key(Key::Enter)));
        assert_eq!(queue.pop(), None);
    }

    #[test]
    fn input_queue_take_drains_all_events() {
        let mut queue = InputQueue::default();
        queue.extend([
            InputEvent::key(Key::Char('a')),
            InputEvent::resize(80, 24),
            InputEvent::Quit,
        ]);
        let taken = queue.take();
        assert_eq!(taken.len(), 3);
        assert_eq!(taken[0], InputEvent::key(Key::Char('a')));
        assert_eq!(taken[1], InputEvent::resize(80, 24));
        assert_eq!(taken[2], InputEvent::Quit);
        assert!(queue.is_empty());
    }

    #[test]
    fn key_event_helpers() {
        let pressed = KeyEvent::pressed(Key::Enter);
        assert!(pressed.is_press());
        assert_eq!(pressed.state, KeyState::Pressed);

        let released = KeyEvent::released(Key::Enter);
        assert!(!released.is_press());
        assert_eq!(released.state, KeyState::Released);

        let repeated = KeyEvent::repeated(Key::Up).with_modifiers(Modifiers {
            shift: true,
            ctrl: false,
            alt: false,
        });
        assert!(repeated.is_press());
        assert!(repeated.modifiers.shift);
    }

    #[test]
    fn surface_info_cells() {
        let tui = SurfaceInfo::new(80, 24, 1, 1);
        assert!(tui.is_valid());
        assert_eq!(tui.cells(), (80, 24));

        let gpu = SurfaceInfo::new(1280, 720, 32, 32);
        assert_eq!(gpu.cells(), (40, 22));

        let invalid = SurfaceInfo::default();
        assert!(!invalid.is_valid());
        assert_eq!(invalid.cells(), (0, 0));
    }

    #[test]
    fn input_event_helpers() {
        assert!(InputEvent::key(Key::Esc).is_key());
        assert!(!InputEvent::key(Key::Esc).is_quit());
        assert!(InputEvent::Quit.is_quit());
        assert_eq!(
            InputEvent::resize(80, 24),
            InputEvent::Resize {
                width: 80,
                height: 24
            }
        );
    }
}
