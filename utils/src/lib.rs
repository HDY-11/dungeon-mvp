//! 无状态、无业务的通用工具与数据结构。
//!
//! 约束：不依赖 bevy_ecs / ratatui / crossterm，也不依赖任何业务 crate。

pub mod color;
pub mod geometry;
pub mod grid;
pub mod text;

pub use color::*;
pub use geometry::*;
pub use grid::*;
pub use text::*;
