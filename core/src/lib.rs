//! core 库
//!
//! 唯一的游戏逻辑/业务领域层，完全采用 ECS 范式。
//!
//! 模块按依赖层级组织：
//! `components/events/resources/balance -> map/spatial -> action/combat/monster -> system/world`。

pub mod action;
pub mod balance;
pub mod combat;
pub mod components;
pub mod entity_cls;
pub mod events;
pub mod map;
pub mod monster;
pub mod resources;
pub mod spatial;
pub mod system;
pub mod world;

pub use action::*;
pub use balance::*;
pub use combat::*;
pub use components::*;
pub use entity_cls::*;
pub use events::*;
pub use map::*;
pub use monster::*;
pub use resources::*;
pub use spatial::*;
pub use system::*;
pub use world::*;

pub use world::loop_ as world_loop;