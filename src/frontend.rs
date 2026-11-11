//! 前端接缝：把「主循环 + 输入来源 + 帧节奏」从游戏语义里拆出来。
//!
//! `App` 只做三件与后端无关的事：收 `InputEvent`、推进世界、产出
//! [`SceneFrame`]。至于输入从哪里来、帧画到哪里、多久一帧，由 [`Frontend`]
//! 的实现在这里之外的模块决定：
//!
//! ```text
//!   sys/终端 ──> TuiFrontend ──> App::run_with（本文件）
//!   headless 测试 ──> 脚本 Frontend ──┘
//!   将来的 GPU 前端 ──> GpuFrontend ──┘
//! ```
//!
//! 这条缝的意义是让「一局游戏怎么跑」可测：headless 前端可以脚本化喂键、
//! 捕获每一帧，不需要终端。`main.rs` 因此只剩 sys 生命周期 + 构造/运行。

use std::io;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use crossterm::event::KeyCode;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use render_api::{InputEvent, SceneFrame};
use tui::{DevLogBuffer, TuiPlugin};

use crate::{App, translate_key};

/// 目标帧间隔（≈30fps）。主循环是"输入驱动 + 定时重绘"，不做插值/动画，
/// 所以这个值只影响输入延迟与 CPU 占用。
const FRAME_INTERVAL: Duration = Duration::from_millis(33);

/// 前端：后端实现只关心「输入从哪来、帧往哪画、何时等下一帧」。
///
/// 默认实现让最小前端只需覆写 `poll_input` / `viewport` / `draw`；
/// `pump` 是给「每帧还要顺带做点事」的后端用的钩子（TUI 用它排空日志通道）。
pub trait Frontend {
    /// 取下一个后端无关输入事件。主循环会反复调用直到返回 `None`。
    fn poll_input(&mut self) -> Option<InputEvent>;

    /// 当前可用于地图的视口尺寸（**不是**整个窗口：右栏占了一部分）。
    fn viewport(&mut self) -> (u16, u16);

    /// 展示一帧。
    fn draw(&mut self, frame: &SceneFrame) -> io::Result<()>;

    /// 每帧一次的游戏侧回调（日志泵等）；默认无事。
    fn pump(&mut self, _app: &mut App) {}

    /// 一帧结束后的等待/限速。
    fn wait(&mut self) {}

    /// 主循环退出时调用一次（TUI 用它恢复光标）。
    fn finish(&mut self) -> io::Result<()> {
        Ok(())
    }
}

type TuiTerminal = Terminal<CrosstermBackend<io::Stdout>>;

/// 终端前端：真实终端、crossterm 按键来源、日志通道、33ms 帧节奏。
pub struct TuiFrontend {
    terminal: TuiTerminal,
    ui: TuiPlugin,
    keys: Receiver<KeyCode>,
    logs: Receiver<sys::LogRecord>,
    last_draw: Instant,
}

impl TuiFrontend {
    /// `logs` 由 `main.rs` 在 sys 初始化后交出；raw mode / alternate screen
    /// 仍归 `main.rs`（`sys` 的生命周期，不属于某次游戏）。
    pub fn new(logs: Receiver<sys::LogRecord>) -> io::Result<Self> {
        Ok(Self {
            terminal: Terminal::new(CrosstermBackend::new(io::stdout()))?,
            ui: TuiPlugin::new(),
            keys: sys::spawn_key_source(),
            logs,
            last_draw: Instant::now(),
        })
    }
}

impl Frontend for TuiFrontend {
    fn poll_input(&mut self) -> Option<InputEvent> {
        while let Some(code) = sys::try_recv_key(&self.keys) {
            if let Some(event) = translate_key(code) {
                return Some(event);
            }
        }
        None
    }

    fn viewport(&mut self) -> (u16, u16) {
        match self.terminal.size() {
            // `map_viewport` 收 `Rect`（布局的输入形态），终端尺寸没有原点，
            // 所以补一个 `(0, 0)` 原点——布局只看 width/height。
            Ok(size) => tui::map_viewport(ratatui::layout::Rect::new(
                0,
                0,
                size.width,
                size.height,
            )),
            // 拿不到尺寸时不 panic：`(0, 0)` 表示"未知"，相机会退回世界中心。
            Err(_) => (0, 0),
        }
    }

    fn draw(&mut self, frame: &SceneFrame) -> io::Result<()> {
        // `TuiPlugin` 是零大小 Copy，先拷出来避免闭包里再借 `self`。
        let ui = self.ui;
        self.terminal.draw(|terminal_frame| ui.draw(terminal_frame, frame))?;
        self.last_draw = Instant::now();
        Ok(())
    }

    fn pump(&mut self, app: &mut App) {
        while let Ok(record) = self.logs.try_recv() {
            app.world_mut().resource_mut::<DevLogBuffer>().push(
                record.level.to_string(),
                record.target,
                record.message,
            );
        }
    }

    fn wait(&mut self) {
        let elapsed = self.last_draw.elapsed();
        if elapsed < FRAME_INTERVAL {
            std::thread::sleep(FRAME_INTERVAL - elapsed);
        }
    }

    fn finish(&mut self) -> io::Result<()> {
        self.terminal.show_cursor()
    }
}
