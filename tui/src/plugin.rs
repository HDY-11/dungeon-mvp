//! `TuiPlugin`：把 [`SceneFrame`] 画到终端。
//!
//! # 这不是"Bevy 插件"
//!
//! 当前工作区里**没有** `bevy_app`（REFACTOR §11.3 Phase G 的 R3 才引入，
//! 且需要联网取依赖）。所以这里的 "Plugin" 是**结构约定**而不是 `bevy_app::Plugin`
//! 实现：它把后端需要的东西收成一个类型，让装配层"选后端"这件事只有一个接缝。
//! R3 引入 `bevy_app` 时，只需给这个类型补一个 `impl Plugin`，调用点不变。
//!
//! # 依赖方向
//!
//! ```text
//! presentation ──> render-api <── tui      （tui 不认识 ecs_core）
//! ```
//!
//! `TuiPlugin` 只要求一个 [`SceneFrame`]，因此换 GPU 后端时这个文件整体被
//! `GpuPlugin` 替代，`presentation` 与 `ecs_core` 一行不改。

use ratatui::Frame;
use render_api::SceneFrame;

use crate::render::render_frame;

/// TUI 后端的绘制入口。
///
/// 之所以是"每帧 `draw` 一次"而不是持有终端：终端生命周期（raw mode、
/// alternate screen、panic-safe guard）属于装配层与 `sys`，后端不该管
/// （DsnX14：「终端生命周期」是 `TuiPlugin` 的职责之一，但 `Terminal` 实例
/// 由 `sys` 的 `TerminalSession` 提供）。
#[derive(Debug, Default, Clone, Copy)]
pub struct TuiPlugin;

impl TuiPlugin {
    pub const fn new() -> Self {
        Self
    }

    /// 在当前终端帧上绘制场景。
    ///
    /// 只吃 `SceneFrame`：调试信息已经不再渲染（DsnE25 决策），后端不需要
    /// 接触 `World`，也不需要知道 TUI 自己的日志缓冲。
    pub fn draw(&self, frame: &mut Frame, scene: &SceneFrame) {
        render_frame(frame, scene);
    }

    /// 给定终端区域，返回相机该用的视口尺寸。
    ///
    /// 装配层必须用**这个**而不是终端整体尺寸：地图只占终端的一部分，
    /// 相机拿到过大的视口就不会夹取，玩家走出中心即滚出画面。
    pub fn viewport(&self, area: ratatui::layout::Rect) -> (u16, u16) {
        crate::render::map_viewport(area)
    }
}

/// 便捷函数：装配层这样画一帧。
///
/// 保留函数形式（而不只是方法）是为了让 `src/main.rs` 的调用点最短。
pub fn draw_scene(frame: &mut Frame, scene: &SceneFrame) {
    TuiPlugin::new().draw(frame, scene);
}
