//! 终端入口：终端生命周期 + 平台按键翻译 + 主循环。
//!
//! **这里没有任何游戏逻辑**。装配与输入映射都在 `dungeon_app::App`
//! （`src/lib.rs`），因为那部分需要能被 headless 测试驱动。
//!
//! ```text
//! sys（终端/键盘）   本文件（翻译 + 循环）   dungeon_app::App（装配）
//!                                     └──> ecs_core / presentation / tui
//! ```
//!
//! 本文件的两条职责边界：
//!
//! - `crossterm::KeyCode` → `render_api::Key`：唯一的平台相关映射在
//!   `dungeon_app::keys::translate_key`（放 lib 里才能被测试覆盖）；
//! - 终端 raw mode / alternate screen 的进入与退出（含 panic 前的恢复）。

use std::io::{self, stdout};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use dungeon_app::{App, dev_log, translate_key, tui_plugin};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use sys::{self, try_recv_key};

/// 目标帧间隔（≈30fps）。主循环是"输入驱动 + 定时重绘"，不做插值/动画，
/// 所以这个值只影响输入延迟与 CPU 占用。
const FRAME_INTERVAL: Duration = Duration::from_millis(33);

type TuiTerminal = Terminal<CrosstermBackend<io::Stdout>>;

fn main() -> io::Result<()> {
    let log_rx = sys::init_logging();
    sys::enter_raw_mode()?;
    sys::enter_alternate_screen()?;

    let result = run(&log_rx);

    // 无论正常退出还是出错，都要恢复终端——否则用户会拿到一个 raw mode 的终端。
    sys::shutdown_logging();
    sys::leave_raw_mode()?;
    sys::leave_alternate_screen()?;
    result
}

fn run(log_rx: &std::sync::mpsc::Receiver<sys::LogRecord>) -> io::Result<()> {
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;

    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;
    let ui = tui_plugin();

    // 首帧：终端尺寸此时已知，用它算相机视口。
    let mut app = App::new(seed, terminal_size(&terminal));
    draw(&mut terminal, &mut app, &ui)?;

    let rx = sys::spawn_key_source();

    loop {
        let frame_start = Instant::now();

        while let Some(code) = try_recv_key(&rx) {
            if let Some(event) = translate_key(code) {
                // `App::handle` 返回"是否推进了世界"；被拒绝的命令（撞墙）
                // 不是错误，无需处理。
                let _ = app.handle(event);
            }
        }

        while let Ok(record) = log_rx.try_recv() {
            app.world_mut().resource_mut::<tui::DevLogBuffer>().push(
                record.level.to_string(),
                record.target,
                record.message,
            );
        }

        // 终端可能被 resize：每帧重取尺寸，相机随之更新。
        app.set_viewport(terminal_size(&terminal));
        draw(&mut terminal, &mut app, &ui)?;

        if app.wants_quit() {
            break;
        }

        let elapsed = frame_start.elapsed();
        if elapsed < FRAME_INTERVAL {
            std::thread::sleep(FRAME_INTERVAL - elapsed);
        }
    }

    terminal.show_cursor()?;
    Ok(())
}

/// 画一帧：提取 → 交给后端。
///
/// 提取与绘制是两个 crate 的两件事，这里只是把它们按顺序串起来，
/// 不做任何加工（加工会变成"后端偷偷改视图数据"）。
fn draw(terminal: &mut TuiTerminal, app: &mut App, ui: &tui::TuiPlugin) -> io::Result<()> {
    // `refresh` 要 `&mut App`；先取出日志快照（`dev_log` 返回 owned），
    // 再提取帧，两个步骤之间没有借用重叠。
    let log = dev_log(app.world());
    let frame = app.refresh();
    terminal.draw(|terminal_frame| ui.draw(terminal_frame, frame, log.as_ref()))?;
    Ok(())
}

/// 终端当前可用的**地图区**尺寸（供相机视口使用）。
///
/// 注意**不是**终端整体尺寸：地图只占终端的一部分（右边有侧栏、下面有调试面板）。
/// 给相机过大的视口会让它以为"视口比世界还大"从而放弃夹取，玩家一走出中心
/// 就滚出画面。尺寸由 `tui::map_viewport` 给出——那是布局的唯一权威。
fn terminal_size(terminal: &TuiTerminal) -> (u16, u16) {
    match terminal.size() {
        // `map_viewport` 收 `Rect`（布局的输入形态），终端尺寸没有原点，
        // 所以补一个 `(0, 0)` 原点——布局只看 width/height。
        Ok(size) => tui::map_viewport(ratatui::layout::Rect::new(0, 0, size.width, size.height)),
        // 拿不到尺寸时不 panic：`(0, 0)` 表示"未知"，相机会退回世界中心。
        Err(_) => (0, 0),
    }
}
