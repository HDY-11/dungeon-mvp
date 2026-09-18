//! 终端按键 → 后端无关的 [`InputEvent`]。
//!
//! 这是**唯一的平台相关映射**：`crossterm::KeyCode` 只在这里（以及 `sys`）出现，
//! 出了这一层全是 `render_api::Key`。
//!
//! # 为什么放在 lib 而不是 `main.rs`
//!
//! `bin` 目标无法被测试导入。放在 `main.rs` 里时，"终端按键能不能正确翻译成
//! 命令"就只能靠人工按一遍；搬到这里之后，`tests/mvp_loop_test.rs` 可以从
//! `KeyCode` 一路验到"玩家是否移动"，中间不留无人测试的缝。
//!
//! # 为什么映射是 1:1 的机械表
//!
//! 这里**不判断"这个键做什么"**，只判断"这个终端键对应契约里的哪个键"。
//! 键位语义（`h` 向左走、`.` 等待、`?` 打开查看）住在 `presentation::input`，
//! 于是换后端不用重写键位，也不会出现"两个后端键位不一样"。

use crossterm::event::KeyCode;
use render_api::{InputEvent, Key, KeyEvent, KeyState, Modifiers};

/// 翻译一个终端按键；`None` 表示契约里没有对应物（媒体键等），调用方直接忽略。
///
/// 终端按键天然是"按下"事件（没有 release），所以恒为 [`KeyState::Pressed`]、
/// 无修饰键——去重由 `sys` 的键盘线程负责（见 `sys::input`）。
pub fn translate_key(code: KeyCode) -> Option<InputEvent> {
    let key = match code {
        KeyCode::Up => Key::Up,
        KeyCode::Down => Key::Down,
        KeyCode::Left => Key::Left,
        KeyCode::Right => Key::Right,
        KeyCode::Home => Key::Home,
        KeyCode::End => Key::End,
        KeyCode::PageUp => Key::PageUp,
        KeyCode::PageDown => Key::PageDown,
        KeyCode::Enter => Key::Enter,
        KeyCode::Esc => Key::Esc,
        KeyCode::Tab => Key::Tab,
        KeyCode::Backspace => Key::Backspace,
        KeyCode::Delete => Key::Delete,
        // 空格在契约里是独立的 `Key::Space`，不是 `Key::Char(' ')`：
        // 后端（终端 / winit）都要把它归一到同一个值，否则键位表要写两份。
        KeyCode::Char(' ') => Key::Space,
        KeyCode::Char(c) => Key::Char(c),
        KeyCode::F(n) => Key::F(n),
        _ => return None,
    };
    Some(InputEvent::Key(KeyEvent {
        key,
        state: KeyState::Pressed,
        modifiers: Modifiers::NONE,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use render_api::Key;

    fn translated(code: KeyCode) -> Option<Key> {
        match translate_key(code)? {
            InputEvent::Key(event) => Some(event.key),
            _ => None,
        }
    }

    /// 方向键与翻页键是移动的主力，逐个钉住。
    #[test]
    fn navigation_keys_map_one_to_one() {
        let cases = [
            (KeyCode::Up, Key::Up),
            (KeyCode::Down, Key::Down),
            (KeyCode::Left, Key::Left),
            (KeyCode::Right, Key::Right),
            (KeyCode::Home, Key::Home),
            (KeyCode::End, Key::End),
            (KeyCode::PageUp, Key::PageUp),
            (KeyCode::PageDown, Key::PageDown),
            (KeyCode::Enter, Key::Enter),
            (KeyCode::Esc, Key::Esc),
            (KeyCode::Tab, Key::Tab),
            (KeyCode::Backspace, Key::Backspace),
            (KeyCode::Delete, Key::Delete),
        ];
        for (code, expected) in cases {
            assert_eq!(translated(code), Some(expected), "{code:?}");
        }
    }

    /// 空格归一到 `Key::Space`，不是 `Key::Char(' ')`。
    #[test]
    fn space_is_normalized_to_the_space_key() {
        assert_eq!(translated(KeyCode::Char(' ')), Some(Key::Space));
    }

    /// 普通字符原样传递（键位表在 `presentation` 决定它们的含义）。
    #[test]
    fn plain_characters_pass_through() {
        for c in ['h', 'j', 'k', 'l', 'q', '.', '?'] {
            assert_eq!(
                translated(KeyCode::Char(c)),
                Some(Key::Char(c)),
                "{c:?} 必须原样传递"
            );
        }
    }

    /// 功能键原样传递。
    #[test]
    fn function_keys_pass_through() {
        assert_eq!(translated(KeyCode::F(5)), Some(Key::F(5)));
        assert_eq!(translated(KeyCode::F(255)), Some(Key::F(255)));
    }

    /// 契约里没有对应物的按键返回 `None`，而不是硬塞一个 `Unknown`。
    ///
    /// 返回 `Key::Unknown` 会让它在键位表里"占用"一个位置，将来真有人给
    /// `Unknown` 绑了行为就会莫名其妙地生效。
    #[test]
    fn unmapped_keys_return_none() {
        for code in [
            KeyCode::Null,
            KeyCode::CapsLock,
            KeyCode::ScrollLock,
            KeyCode::Insert,
            KeyCode::Menu,
            KeyCode::Media(crossterm::event::MediaKeyCode::Play),
        ] {
            assert!(translate_key(code).is_none(), "{code:?} 不该有映射");
        }
    }

    /// 翻译出来的事件恒为"按下、无修饰键"：终端没有 release，修饰键留给未来。
    #[test]
    fn translated_events_are_plain_presses() {
        let Some(InputEvent::Key(event)) = translate_key(KeyCode::Char('h')) else {
            panic!("h 必须能翻译成按键事件");
        };
        assert_eq!(event.state, KeyState::Pressed);
        assert!(event.modifiers.is_none());
    }
}
