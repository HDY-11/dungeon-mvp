//! 终端入口：终端生命周期 + 构造前端与 App。
//!
//! **这里没有任何游戏逻辑，也没有主循环**。主循环在
//! [`dungeon_app::App::run_with`]（`src/lib.rs`），终端前端实现在
//! `dungeon_app::frontend`，两者都能被 headless 测试覆盖。
//!
//! ```text
//! sys（终端/键盘）   本文件（生命周期）   dungeon_app（主循环 + 装配）
//!                                     └──> ecs_core / presentation / tui
//! ```
//!
//! 本文件的两条职责边界：
//!
//! - `crossterm::KeyCode` → `render_api::Key`：唯一的平台相关映射在
//!   `dungeon_app::keys::translate_key`（放 lib 里才能被测试覆盖）；
//! - 终端 raw mode / alternate screen 的进入与退出（含 panic 前的恢复）。

use std::io;
use std::time::{SystemTime, UNIX_EPOCH};

use dungeon_app::{App, Frontend, TuiFrontend};

fn main() -> io::Result<()> {
    let log_rx = sys::init_logging();
    sys::enter_raw_mode()?;
    sys::enter_alternate_screen()?;

    let result = run(log_rx);

    // 无论正常退出还是出错，都要恢复终端——否则用户会拿到一个 raw mode 的终端。
    sys::shutdown_logging();
    sys::leave_raw_mode()?;
    sys::leave_alternate_screen()?;
    result
}

fn run(log_rx: std::sync::mpsc::Receiver<sys::LogRecord>) -> io::Result<()> {
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;

    let mut frontend = TuiFrontend::new(log_rx)?;

    // 首帧尺寸在构造终端后、造 App 前取：相机视口必须用地图区尺寸。
    let mut app = App::new(seed, frontend.viewport());
    app.run_with(&mut frontend)
}
