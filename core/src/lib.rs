//! core 库
//!
//! 唯一的游戏逻辑/业务领域层，完全采用 ECS 范式。
//!
//! 组件只保存数据；规则由 `system` / `action` / `ai` / `combat` 中的系统实现。
//! 数值统一使用 `f64`。

pub mod action;
pub mod ai;
pub mod balance;
pub mod combat;
pub mod components;
pub mod entity_cls;
pub mod events;
pub mod fov;
pub mod map;
pub mod map_gen;
pub mod monster;
pub mod movement;
pub mod pathfinding;
pub mod query;
pub mod resources;
pub mod spatial;
pub mod system;

pub use action::*;
pub use ai::*;
pub use balance::*;
pub use combat::*;
pub use components::*;
pub use entity_cls::*;
pub use events::*;
pub use map::*;
pub use monster::*;
pub use movement::*;
pub use pathfinding::*;
pub use resources::*;
pub use spatial::*;
pub use system::*;
