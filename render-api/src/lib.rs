//! 渲染后端与游戏逻辑之间的只读数据契约。
//!
//! 本 crate 不依赖 `core`、ratatui 或任何 GPU API。职责边界：
//!
//! ```text
//! core  ──>  presentation  ──>  render-api  <──  tui
//!   ^                                              ^
//!   └────────── 未来 gpu 也依赖 render-api ────────┘
//! ```
//!
//! - `presentation` 负责把 ECS 世界提取成这里的视图模型；
//! - `tui` / 未来的 `gpu` 只消费这些数据，不直接查询 `core` 组件；
//! - 具体外观（glyph、颜色、纹理、材质）由各后端的 catalog 决定，
//!   因此同一份 [`SceneFrame`] 可以同时喂给 TUI 和 GPU。
//!
//! # 设计边界
//!
//! - **只描述“要显示什么”**：不包含“怎么显示”，没有 glyph / 颜色 / 纹理句柄。
//! - **只读快照**：所有数据每帧从 ECS 提取；后端不得反向修改游戏状态。
//! - **不包含游戏规则**：没有伤害公式、行动优先级、AI 条件。
//! - **不持久化**：契约数据是派生产物，存档仍然是 `core` 的职责。
//!
//! # 版本
//!
//! 契约结构发生影响后端语义的变更时，必须递增 [`CONTRACT_VERSION`]，
//! 并在后端 golden 测试中校验。已有字段的语义变更同样视为破坏性变更。

#![forbid(unsafe_code)]

pub mod input;
pub mod scene;
pub mod ui;
pub mod visual;

pub use input::*;
pub use scene::*;
pub use ui::*;
pub use visual::*;

/// 契约版本。
///
/// 任何会影响后端渲染语义的结构变更都必须递增此值。
pub const CONTRACT_VERSION: u32 = 1;

/// 常用类型预导出。
pub mod prelude {
    pub use crate::CONTRACT_VERSION;
    pub use crate::input::{
        InputEvent, InputQueue, Key, KeyEvent, KeyState, Modifiers, MouseButton, MouseEvent,
        MouseKind, SurfaceInfo,
    };
    pub use crate::scene::{
        Camera2D, EntityId, EntityView, HudView, LogLevel, LogLine, MapView, Meter, SceneFrame,
        TileInfo, WorldRect,
    };
    pub use crate::ui::{
        DetailView, DialogView, InventoryFocus, InventoryPanel, InventoryPanelView, InventoryView,
        ListItemView, ListView, LookView, PageKind, ThrowAimView, UiSpan, UiTextLine, UiTextStyle,
        UiView,
    };
    pub use crate::visual::{UiIcon, VisualCategory, VisualKey, VisualLayer};
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_ecs::prelude::Resource;

    fn assert_resource<T: Resource>() {}

    #[test]
    fn contract_types_are_ecs_resources() {
        assert_resource::<SceneFrame>();
        assert_resource::<InputQueue>();
        assert_resource::<SurfaceInfo>();
    }

    #[test]
    fn contract_version_is_pinned() {
        assert_eq!(CONTRACT_VERSION, 1);
    }
}
