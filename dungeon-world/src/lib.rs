//! 世界层：World 初始化、楼层切换、存档/读档、并行行动推进。
//!
//! 依赖 dungeon-core / dungeon-action，负责把规则组装成可运行的游戏世界。

pub mod init;
pub mod persist;
pub mod population;
pub mod tick;

pub use dungeon_core::systems::fov_system;
pub use init::{descend, setup_world};
pub use persist::{GameSave, load_game, save_game};
pub use tick::advance_and_settle_parallel;

#[cfg(test)]
mod tests;
