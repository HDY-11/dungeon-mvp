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
    /// 下楼（站在楼梯上时才有意义）。
    ///
    /// **它不是行动**：没有 AV、不产生 action 实体——换层是世界的重组，不是某个 actor
    /// 做了一件事。所以 [`crate::world_loop::apply_player_command`] 在挂载行动之前就把它
    /// 拦下来交给 [`crate::world::floor::descend`]；生成系统里那个分支只作防御
    /// （真收到就记一条错误，而不是静默当成 `Wait`）。
    Descend,
    /// 施放法术书里的第 `slot` 格（0 起的快捷键序号，DsnE26）。
    ///
    /// 目标**不在这里指定**：`NearestEnemy` 法术由校验阶段自动选视野内最近敌人，
    /// 治疗法术固定对自己。按键层不需要知道"选谁"。
    Cast { slot: u8 },
    /// 朝玩家周边 `(dx, dy)` 偏移的格子投掷（Gm9）。
    ///
    /// 目标是**格子**不是实体：落点必中，落点格上站着谁就打谁，没人就只是落地。
    /// 取用哪件可投掷物由生成阶段从副手判定，按键层不关心。
    Throw { dx: isize, dy: isize },
}
