//! `presentation`：唯一知道 `ecs_core` 的集成层。
//!
//! 它把 ECS 世界提取成 [`render_api::SceneFrame`]，并在同一个地方集中
//! 「相机」「UI 页栈」「输入映射」三件事。后端（`tui` / 未来 `gpu`）只读
//! `SceneFrame`，因此**不认识 `ecs_core`**。
//!
//! ```text
//! ecs_core ──> presentation ──> render-api <── tui / gpu
//! ```
//!
//! # 依赖边界（DESIGN Dsn28）
//!
//! | 允许依赖 | 禁止依赖 |
//! |---|---|
//! | `ecs_core`、`render-api`、`bevy_ecs` | ratatui、crossterm、wgpu、`bevy_app` |
//!
//! 这一条是靠 Cargo 强制执行的：本 crate 的依赖表里没有终端库，所以「把终端
//! 细节漏进集成层」在编译期就不可能。
//!
//! # 分层职责
//!
//! | 模块 | 职责 | 关键约束 |
//! |---|---|---|
//! | [`extract`] | 世界 → [`SceneFrame`] | 只读；每帧重建；不写游戏状态 |
//! | [`visual`] | `core` 的枚举/ID → [`VisualKey`] | 只给语义，不给 glyph/颜色 |
//! | [`camera`] | 视口尺寸 → [`Camera2D`] | 纯计算，可用 `TestBackend` 之外的输入测 |
//! | [`ui`] | 页栈状态机 → [`UiView`] | 页栈状态住在这里，后端不持有 |
//! | [`input`] | [`InputEvent`] → 页栈动作 / `PlayerCommand` | tap-tap 只实现一次 |
//!
//! [`SceneFrame`]: render_api::SceneFrame
//! [`VisualKey`]: render_api::VisualKey

#![forbid(unsafe_code)]

pub mod catalog;
pub mod camera;
pub mod extract;
pub mod input;
pub mod ui;

pub use catalog::{
    TileCatalog, map_monster_kind, map_tile, map_visual_category, monster_kind_id, tile_id,
};
pub use camera::{CameraFollow, camera_for, center_on_player};
pub use extract::{
    ExtractConfig, ExtractedScene, SceneFrameSource, extract_scene_frame, player_position,
};
pub use input::{
    InputOutcome, PresentationInput, apply_ui_intent, map_input_event, mouse_cell,
};
pub use ui::{PageStack, UiIntent, entity_lines, visual_text};

/// 常用类型预导出，省得每个后端写一长串 `use`。
pub mod prelude {
    pub use crate::camera::{CameraFollow, camera_for, center_on_player};
    pub use crate::extract::{ExtractConfig, SceneFrameSource, extract_scene_frame};
    pub use crate::input::{InputOutcome, PresentationInput, apply_ui_intent, map_input_event};
    pub use crate::ui::{PageStack, UiIntent};
    pub use render_api::prelude::*;
}
