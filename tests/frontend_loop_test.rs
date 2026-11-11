//! 前端接缝的 headless 测试：`Frontend` 负责输入/绘制/节奏，`App::run_with`
//! 只串「收输入 → 推进世界 → 出画面」。
//!
//! 这组测试替代了以前只能靠人工在终端里跑一遍的"主循环接线"检查：
//! 脚本化喂键 + 捕获每一帧，就能断言循环确实转了、帧确实画了、退出确实收口。

use std::collections::VecDeque;
use std::io;

use dungeon_app::{App, Frontend};
use render_api::{InputEvent, Key, KeyEvent, SceneFrame};

fn key(k: Key) -> InputEvent {
    InputEvent::Key(KeyEvent::pressed(k))
}

/// 脚本化前端：按键来自队列，每帧被拷贝进 `frames`，不做任何真实 I/O。
///
/// `allow_input` 让每次主循环迭代**只放行一个输入**（真实终端也一样是逐帧
/// 轮询；测试用它是为了让"等待一回合"与"确认退出"落在不同帧里）。
struct HeadlessFrontend {
    inputs: VecDeque<InputEvent>,
    frames: Vec<SceneFrame>,
    viewport: (u16, u16),
    finished: bool,
    allow_input: bool,
}

impl HeadlessFrontend {
    fn new(inputs: impl IntoIterator<Item = InputEvent>, viewport: (u16, u16)) -> Self {
        Self {
            inputs: inputs.into_iter().collect(),
            frames: Vec::new(),
            viewport,
            finished: false,
            allow_input: true,
        }
    }
}

impl Frontend for HeadlessFrontend {
    fn poll_input(&mut self) -> Option<InputEvent> {
        if !self.allow_input {
            return None;
        }
        self.allow_input = false;
        self.inputs.pop_front()
    }

    fn viewport(&mut self) -> (u16, u16) {
        self.viewport
    }

    fn draw(&mut self, frame: &SceneFrame) -> io::Result<()> {
        self.frames.push(frame.clone());
        self.allow_input = true;
        Ok(())
    }

    fn finish(&mut self) -> io::Result<()> {
        self.finished = true;
        Ok(())
    }
}

#[test]
fn headless_frontend_drives_the_loop_until_confirmed_quit() {
    let mut app = App::new(7, (80, 24));
    let mut frontend = HeadlessFrontend::new(
        [
            // 先等待一回合，确保循环至少画过一帧世界推进后的画面。
            key(Key::Char('.')),
            // 然后在页内开/确认退出弹窗：单按 q 只开页，Enter 才真的退出。
            key(Key::Char('q')),
            key(Key::Enter),
        ],
        (60, 20),
    );

    app.run_with(&mut frontend).expect("headless 主循环不应报错");

    assert!(app.wants_quit(), "确认退出后主循环必须结束");
    assert!(frontend.finished, "退出时前端必须收到 finish");
    assert!(
        frontend.frames.len() >= 2,
        "等待一回合 + 退出弹窗应各画一帧，实际 {} 帧",
        frontend.frames.len()
    );
    assert_eq!(
        app.viewport(),
        (60, 20),
        "主循环须把前端视口同步给 App（相机视口以后端为准）"
    );
    assert!(
        !frontend.frames[0].hud.player_name.is_empty(),
        "第一帧必须是提取好的世界画面，而不是空帧"
    );

    // 等待命令确实推进了世界时钟（“收输入 → 推进世界”这一半的接线）。
    let clock = app.world().resource::<ecs_core::WorldClock>();
    assert!(clock.total_av > 0.0, "等待一回合后世界时钟必须前进");
}

#[test]
fn a_draw_error_stops_the_loop_and_bubbles_up() {
    struct FailingFrontend;

    impl Frontend for FailingFrontend {
        fn poll_input(&mut self) -> Option<InputEvent> {
            None
        }

        fn viewport(&mut self) -> (u16, u16) {
            (80, 24)
        }

        fn draw(&mut self, _frame: &SceneFrame) -> io::Result<()> {
            Err(io::Error::other("绘制失败"))
        }
    }

    let mut app = App::new(1, (80, 24));
    let error = app
        .run_with(&mut FailingFrontend)
        .expect_err("绘制错误必须冒泡，而不是被主循环吞掉");
    assert_eq!(error.kind(), io::ErrorKind::Other);
}
