//! 把真实 TUI 渲染成文本，并按给定的按键序列推进——用来"用眼睛玩一局"。
//!
//! 与 `playthrough.rs` 的区别：那个看的是**世界状态**（位置/HP/日志），这个看的是**玩家看到的屏幕**。
//! 两者能发现的问题不重叠：状态脚本看不出"泥潭和靴子都是星号""日志被挤掉""镜头裁掉了楼梯"。
//!
//! 用法：
//!
//! ```text
//! cargo run --example play -- --seed 2026                      # 只看开局画面
//! cargo run --example play -- --seed 2026 --keys "jjl" --frames all
//! cargo run --example play -- --seed 2026 --keys "jjl" --size 100x36
//! ```
//!
//! 按键按 `dungeon_app::translate_key` 翻译（h/j/k/l、y/u/b/n、`.`、`?`、`q`…），
//! 与 `main.rs` 走完全同一条路径：终端按键 → `App::handle` → 世界 → `SceneFrame` → 真实绘制。

use std::io::Write;

use dungeon_app::App;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut seed: u64 = 2026;
    let mut keys = String::new();
    let mut frames_all = false;
    let mut size = (80u16, 30u16);

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--seed" => {
                i += 1;
                seed = args[i].parse().unwrap_or(2026);
            }
            "--keys" => {
                i += 1;
                keys = args[i].clone();
            }
            "--frames" => {
                i += 1;
                frames_all = args[i] == "all";
            }
            "--size" => {
                i += 1;
                if let Some((w, h)) = args[i].split_once('x') {
                    size = (
                        w.parse().unwrap_or(80),
                        h.parse().unwrap_or(30),
                    );
                }
            }
            other => eprintln!("未知参数 {other}"),
        }
        i += 1;
    }

    let viewport = tui::map_viewport(ratatui::layout::Rect::new(0, 0, size.0, size.1));
    let mut app = App::new(seed, viewport);

    println!("seed={seed} 终端={}x{} 相机视口={viewport:?}", size.0, size.1);
    print_frame(&mut app, size, "开局");

    for (n, ch) in keys.chars().enumerate() {
        let code = match ch {
            '.' => crossterm::event::KeyCode::Char('.'),
            'h' | 'j' | 'k' | 'l' | 'y' | 'u' | 'b' | 'n' | 'q' | '?' | 'g' | 'r' => {
                crossterm::event::KeyCode::Char(ch)
            }
            'H' => crossterm::event::KeyCode::Left,
            'J' => crossterm::event::KeyCode::Down,
            'K' => crossterm::event::KeyCode::Up,
            'L' => crossterm::event::KeyCode::Right,
            'E' => crossterm::event::KeyCode::Esc,
            'R' => crossterm::event::KeyCode::Enter,
            other => {
                eprintln!("跳过无法映射的键 {other:?}");
                continue;
            }
        };
        let Some(event) = dungeon_app::translate_key(code) else {
            eprintln!("按键 {code:?} 没有翻译");
            continue;
        };
        let accepted = app.handle(event);
        if frames_all || n + 1 == keys.chars().count() {
            print_frame(&mut app, size, &format!("第 {} 键 {ch:?} 接受={accepted}", n + 1));
        }
        if app.wants_quit() {
            println!("（游戏请求退出）");
            break;
        }
    }
}

/// 是否宽字符（CJK / 全角）。只为"逐行拼接时跳过被吃掉的第二格"。
fn is_wide_char(c: char) -> bool {
    matches!(c as u32,
        0x1100..=0x115F | 0x2E80..=0xA4CF | 0xAC00..=0xD7A3 |
        0xF900..=0xFAFF | 0xFE30..=0xFE6F | 0xFF00..=0xFF60 | 0xFFE0..=0xFFE6)
}

/// 用真实 `TuiPlugin` 画一帧，逐行打印（不做拍平：CJK 占两格）。
fn print_frame(app: &mut App, size: (u16, u16), title: &str) {
    let ui = tui::TuiPlugin::new();
    let log = dungeon_app::dev_log(app.world());
    let frame = app.refresh();

    let mut terminal = Terminal::new(TestBackend::new(size.0, size.1)).expect("测试终端");
    terminal
        .draw(|f| ui.draw(f, frame, log.as_ref()))
        .expect("绘制不得失败");
    let buffer = terminal.backend().buffer();

    let mut out = String::new();
    out.push_str(&format!("┌─ {title} ─ 帧号 {}\n", frame.revision));
    for y in 0..buffer.area.height {
        let mut row = String::new();
        let mut x = 0;
        while x < buffer.area.width {
            let sym = buffer[(x, y)].symbol();
            row.push_str(sym);
            // 宽字符（CJK）占两格：TestBackend 会把它后面那格填成空格，
            // 直接拼出来会变成"楼 层"。这里跳过被吃掉的那一格。
            let wide = sym.chars().next().is_some_and(is_wide_char);
            x += if wide { 2 } else { 1 };
        }
        out.push_str(&format!("{row}\n"));
    }
    out.push_str(&format!(
        "└─ 玩家 {:?} HP {:.0}/{:.0} 楼层 {} 死亡={} 输入被页栈捕获={}",
        frame.player.as_ref().map(|p| (p.position.0, p.position.1)),
        frame.hud.hp.current,
        frame.hud.hp.max,
        frame.hud.floor,
        frame.game_over,
        frame.ui.captures_input(),
    ));
    println!("{out}");
    let _ = std::io::stdout().flush();
}
