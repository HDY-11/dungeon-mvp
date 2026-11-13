//! 行动生成侧的类型桥（玩家命令 → action 实体由 `action::entity` 负责）。
//!
//! 旧 AI 条件函数（`chase_condition` / `flee_condition` / `wants_to_flee` /
//! `player_visible_to`）在 action 实体链路落地后已无调用者，作为 C8 遗留一并删除。

pub mod player;

pub use player::*;
