//! 行动执行侧的**纯规则**。
//!
//! C8 删除了旧的独占执行系统（`tick_action_timers_system` / `execute_*_system` /
//! `run_action_cycle`）——它们已被 `action::entity` 里按行动类型拆分的参数化
//! 执行器取代。本模块现在只保留执行器共用的纯函数：
//!
//! - [`movement::can_move_to`]：能不能走向某格（越界/不可走/被占用/对角穿墙）；
//! - [`movement::moved_position`]：合法移动的落点。
//!
//! 纯规则不碰 `World`、不读组件，因此可单测、可被任意执行器形态复用
//! （这也是 C2 把移动执行器从 exclusive 改成参数化系统的前提）。

pub mod movement;
