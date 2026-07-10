//! 声明式键盘绑定表。
//!
//! 所有按键→玩家行动的映射集中在此处。新增一个行动只需加一行绑定和一个 handler。
//! 入口通过 `resolve(code)` 查找匹配的行动。

use crossterm::event::KeyCode;
use dungeon_action::PlayerAction;

/// 单条键位绑定
pub struct KeyBinding {
    pub key: KeyCode,
    pub action: PlayerAction,
}

/// 全局键位绑定表。
/// 顺序无关——`resolve` 线性扫描返回第一个匹配。
pub const KEY_BINDINGS: &[KeyBinding] = &[
    // ── 方向键（移动/攻击） ──
    KeyBinding { key: KeyCode::Up,       action: PlayerAction::Move(0, -1) },
    KeyBinding { key: KeyCode::Down,     action: PlayerAction::Move(0, 1) },
    KeyBinding { key: KeyCode::Left,     action: PlayerAction::Move(-1, 0) },
    KeyBinding { key: KeyCode::Right,    action: PlayerAction::Move(1, 0) },
    KeyBinding { key: KeyCode::Home,     action: PlayerAction::Move(-1, -1) },
    KeyBinding { key: KeyCode::End,      action: PlayerAction::Move(-1, 1) },
    KeyBinding { key: KeyCode::PageUp,   action: PlayerAction::Move(1, -1) },
    KeyBinding { key: KeyCode::PageDown, action: PlayerAction::Move(1, 1) },
    // ── 等待 ──
    KeyBinding { key: KeyCode::Char('.'), action: PlayerAction::Wait },
    // ── 技能 ──
    KeyBinding { key: KeyCode::Char('1'), action: PlayerAction::Skill(0) },
    KeyBinding { key: KeyCode::Char('2'), action: PlayerAction::Skill(1) },
    KeyBinding { key: KeyCode::Char('3'), action: PlayerAction::Skill(2) },
    KeyBinding { key: KeyCode::Char('4'), action: PlayerAction::Skill(3) },
    // ── 模态 ──
    KeyBinding { key: KeyCode::Char('e'), action: PlayerAction::OpenInventory },
    KeyBinding { key: KeyCode::Char('x'), action: PlayerAction::OpenLook },
    KeyBinding { key: KeyCode::Char('t'), action: PlayerAction::Throw },
    KeyBinding { key: KeyCode::Char('g'), action: PlayerAction::PickupGround },
    KeyBinding { key: KeyCode::Char('>'), action: PlayerAction::DescendStairs },
    KeyBinding { key: KeyCode::F(5),      action: PlayerAction::SaveGame },
    KeyBinding { key: KeyCode::F(9),      action: PlayerAction::LoadGame },
    KeyBinding { key: KeyCode::Char('q'), action: PlayerAction::Quit },
    KeyBinding { key: KeyCode::Esc,       action: PlayerAction::Quit },
];

/// 按按键查找对应的玩家行动。
/// 返回第一个匹配，未找到返回 `None`。
pub fn resolve(code: KeyCode) -> Option<&'static PlayerAction> {
    KEY_BINDINGS.iter()
        .find(|b| b.key == code)
        .map(|b| &b.action)
}
