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
//! `TuiPlugin` 只要求一个 [`SceneFrame`] 和（可选的）开发者日志缓冲，
//! 因此换 GPU 后端时这个文件整体被 `GpuPlugin` 替代，`presentation` 与
//! `ecs_core` 一行不改。

use bevy_ecs::prelude::*;
use ratatui::Frame;
use render_api::SceneFrame;

use crate::render::render_frame;
use crate::state::DevLogBuffer;

/// TUI 后端的绘制入口。
///
/// 之所以是"每帧 `draw` 一次"而不是持有终端：终端生命周期（raw mode、
/// alternate screen、panic-safe guard）属于装配层与 `sys`，后端不该管
/// （Dsn28：「终端生命周期」是 `TuiPlugin` 的职责之一，但 `Terminal` 实例
/// 由 `sys` 的 `TerminalSession` 提供）。
#[derive(Debug, Default, Clone, Copy)]
pub struct TuiPlugin;

impl TuiPlugin {
    pub const fn new() -> Self {
        Self
    }

    /// 在当前终端帧上绘制场景。
    ///
    /// `world` 只用来读开发者日志缓冲——这是**唯一**让后端接触 `World` 的地方，
    /// 且它读的是 TUI 自己的调试资源（`DevLogBuffer`），不是游戏状态。
    /// 未来把调试面板也搬进 `SceneFrame` 之后，这个参数就可以彻底删掉。
    pub fn draw(&self, frame: &mut Frame, scene: &SceneFrame, dev_log: Option<&DevLogBuffer>) {
        render_frame(frame, scene, dev_log);
    }

    /// 从世界取开发者日志缓冲（没有就返回 `None`，不 panic）。
    pub fn dev_log<'w>(&self, world: &'w World) -> Option<&'w DevLogBuffer> {
        world.get_resource::<DevLogBuffer>()
    }
}

/// 便捷函数：装配层这样画一帧。
///
/// 保留函数形式（而不只是方法）是为了让 `src/main.rs` 的调用点最短。
pub fn draw_scene(frame: &mut Frame, scene: &SceneFrame, world: &World) {
    let plugin = TuiPlugin::new();
    plugin.draw(frame, scene, plugin.dev_log(world));
}
