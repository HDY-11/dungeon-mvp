//! 玩家输入 → 行动生成的类型桥。
//!
//! C8 起旧链路（`ActionKind` + `mount_action` + actor 上的行动 ZST）已删除：
//! 玩家命令由 `action::entity::PlayerActionRequest` 直接翻译成 **action 子实体**
//! （`action::entity::player_action_generation_system`），本模块只剩输入边界类型。

/// 应用层确认后的玩家命令。
///
/// 这是**输入边界类型**：应用层把按键翻译成它，`core` 把它翻译成 action 实体。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerCommand {
    /// 向 `(dx, dy)` 移动一步；目标格是怪物时表示声明攻击（C3）。
    Move { dx: isize, dy: isize },
    /// 原地等待一回合。
    Wait,
}
