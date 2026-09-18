//! TUI 渲染后端。
//!
//! 只做「[`render_api::SceneFrame`] → 终端」的绘制，**不认识 `ecs_core`**：
//! 这条边界写在 `Cargo.toml` 的依赖表里，因此"顺手查一个 core 组件"在编译期
//! 就不可能（DsnX14 的可替换性保证）。
//!
//! ```text
//! ecs_core ──> presentation ──> render-api <── tui
//! ```
//!
//! # 模块
//!
//! | 模块 | 职责 |
//! |---|---|
//! | [`catalog`] | [`VisualKey`] → glyph / 颜色（**唯一**的外观定义处） |
//! | [`render`] | 画地图、状态栏、覆盖页、调试面板 |
//! | [`plugin`] | [`TuiPlugin`]：后端的接缝（换 GPU 时整体替换） |
//! | [`state`] | 后端自己的 UI 资源（开发者日志缓冲） |
//! | [`canvas`] | 持久画布（dirty tracking 用，尚未接入渲染管线） |
//! | [`color`] / [`layout`] / [`title`] | 颜色、布局、标题页的小工具 |
//!
//! [`VisualKey`]: render_api::VisualKey

pub mod canvas;
pub mod catalog;
pub mod color;
pub mod layout;
pub mod plugin;
pub mod render;
pub mod state;
pub mod title;

pub use canvas::*;
pub use catalog::{TuiCatalog, log_color};
pub use color::*;
pub use layout::*;
pub use plugin::{TuiPlugin, draw_scene};
pub use render::{FrameAreas, build_map_lines, frame_areas, map_viewport, render_frame};
pub use state::*;
pub use title::*;
