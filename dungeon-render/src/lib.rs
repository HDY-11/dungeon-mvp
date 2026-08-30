//! 渲染层：从 ECS 提取快照并绘制 TUI。
//!
//! 依赖 dungeon-core / dungeon-action 的类型，但不直接修改游戏世界。

pub mod color;
pub mod pipeline;
pub mod timeline;
pub mod title;
pub mod ui;

pub use color::{entity_color, renderable_color};
pub use timeline::build_timeline;
pub use title::draw_title;
pub use ui::{build_stats_panel, render_ui};
