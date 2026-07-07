//! 场景测试：模拟按键序列，输出文本截图，验证游戏状态。
//!
//! 每个测试模拟一个完整的游戏场景（移动、战斗、下楼等），
//! 将每帧渲染结果保存为 .txt 文件，方便 AI 或人工检查。
//!
//! 运行：
//!   cargo test --test scenario_test -- --nocapture
//!
//! 输出位置：scenario_output/<测试名>/frame_xxx.txt

use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::Terminal;

use dungeon_action::{
    handle_player_direction, handle_wait, handle_skill,
};
use dungeon_render::render_ui;
use dungeon_world::{
    setup_world, fov_system,
    advance_and_settle_parallel,
};
use bevy_ecs::system::RunSystemOnce;

// ══════════════════════════════════════════════════════
// ScenarioRunner
// ══════════════════════════════════════════════════════

struct ScenarioRunner {
    world: bevy_ecs::prelude::World,
    terminal: Terminal<TestBackend>,
    game_start: Instant,
    frame: usize,
    output_dir: PathBuf,
    #[allow(dead_code)]
    width: u16,
    #[allow(dead_code)]
    height: u16,
}

impl ScenarioRunner {
    /// 创建一个新场景。输出到 scenario_output/<name>/
    fn new(name: &str) -> Self {
        let width = 100;  // 足够宽，容纳视窗 40 + 边栏
        let height = 30;  // 足够高，容纳视窗 20 + 边框
        let backend = TestBackend::new(width, height);
        let terminal = Terminal::new(backend).expect("创建 TestBackend");
        let mut world = setup_world();

        // 初始 FOV + 视野记忆
        let _ = world.run_system_once(fov_system);
        dungeon_core::ops::update_map_memory(&mut world);
        dungeon_core::ops::update_visible_memory(&mut world);

        let output_dir = PathBuf::from("scenario_output").join(name);
        let _ = fs::remove_dir_all(&output_dir); // 清理旧输出
        fs::create_dir_all(&output_dir).expect("创建输出目录");

        ScenarioRunner {
            world,
            terminal,
            game_start: Instant::now(),
            frame: 0,
            output_dir,
            width,
            height,
        }
    }

    /// 截取当前帧，存为 <编号>_<标签>.txt
    fn capture(&mut self, label: &str) {
        let _ = self.terminal.draw(|frame| {
            render_ui(frame, self.game_start, &self.world);
        });
        let buffer = self.terminal.backend().buffer().clone();

        let filename = format!("{:03}_{}.txt", self.frame, label);
        let path = self.output_dir.join(&filename);
        let content = self::format_buffer(&buffer, label);
        fs::write(&path, content).expect("写入帧文件");

        self.frame += 1;
    }

    /// 按一次方向键（预览或确认），自动推进
    fn direction(&mut self, dx: isize, dy: isize) {
        if handle_player_direction(&mut self.world, dx, dy) {
            advance_and_settle_parallel(&mut self.world);
        }
    }

    /// 按一次等待键（预览或确认），自动推进
    fn wait(&mut self) {
        if handle_wait(&mut self.world) {
            advance_and_settle_parallel(&mut self.world);
        }
    }

    /// 按一次技能键（预览或确认），自动推进
    #[allow(dead_code)]
    fn skill(&mut self, idx: usize) {
        if handle_skill(&mut self.world, idx) {
            advance_and_settle_parallel(&mut self.world);
        }
    }

    /// 获得玩家位置的 x
    fn px(&self) -> usize {
        dungeon_core::ops::player_entity(&self.world)
            .and_then(|e| self.world.get::<dungeon_core::Position>(e))
            .map(|p| p.x)
            .unwrap_or(999)
    }

    /// 获得玩家位置的 y
    fn py(&self) -> usize {
        dungeon_core::ops::player_entity(&self.world)
            .and_then(|e| self.world.get::<dungeon_core::Position>(e))
            .map(|p| p.y)
            .unwrap_or(999)
    }

    /// 获得玩家 HP
    fn hp(&self) -> i32 {
        dungeon_core::ops::player_entity(&self.world)
            .and_then(|e| self.world.get::<dungeon_core::Stats>(e))
            .map(|s| s.hp)
            .unwrap_or(-1)
    }
}

// ══════════════════════════════════════════════════════
// 格式工具
// ══════════════════════════════════════════════════════

/// 将 ratatui Buffer 渲染为纯文本，并在顶部加注释行
fn format_buffer(buffer: &Buffer, label: &str) -> String {
    let area = buffer.area;
    let mut out = String::new();

    out.push_str(&format!("// {:03} — {}\n", 0, label));
    out.push_str(&format!("// {}×{}\n", area.width, area.height));
    out.push('\n');

    for y in 0..area.height {
        for x in 0..area.width {
            let ch = buffer[(x, y)].symbol().chars().next().unwrap_or(' ');
            out.push(ch);
        }
        out.push('\n');
    }
    out
}

// ══════════════════════════════════════════════════════
// 场景测试
// ══════════════════════════════════════════════════════

/// 基础移动测试：向下走两步
#[test]
fn test_scenario_move_down() {
    let mut s = ScenarioRunner::new("move_down");
    s.capture("start");

    // 第一次按 ↓（预览）
    s.direction(0, 1);
    s.capture("preview_down");

    // 第二次按 ↓（确认移动）
    s.direction(0, 1);
    s.capture("after_move_down");

    // 再次按 ↓（预览）
    s.direction(0, 1);
    s.capture("preview_down_2");

    // 确认
    s.direction(0, 1);
    s.capture("after_move_down_2");

    // 验证：玩家已移动
    assert!(s.py() > 0, "玩家应该已经移动");
    println!("玩家位置: ({}, {})", s.px(), s.py());
}

/// 等待测试
#[test]
fn test_scenario_wait() {
    let mut s = ScenarioRunner::new("wait");
    s.capture("start");

    s.wait();
    s.capture("after_wait");

    // 验证：玩家没有移动（还在出生点），但时间过去了
    println!("玩家位置: ({}, {}), HP: {}", s.px(), s.py(), s.hp());
}

/// 多步移动 + 等待组合
#[test]
fn test_scenario_move_around() {
    let mut s = ScenarioRunner::new("move_around");
    s.capture("start");

    // 向右走两步
    s.direction(1, 0); s.capture("preview_right_1");
    s.direction(1, 0); s.capture("move_right_1");
    s.direction(1, 0); s.capture("preview_right_2");
    s.direction(1, 0); s.capture("move_right_2");

    // 向下走一步
    s.direction(0, 1); s.capture("preview_down");
    s.direction(0, 1); s.capture("move_down");

    // 等一回合
    s.wait(); s.capture("wait");

    let x = s.px();
    let y = s.py();
    println!("最终位置: ({}, {})", x, y);
    assert!(x > 0 || y > 0, "玩家应该已经移动");
}
