//! 应用层入口：标题画面 → 独立输入线程 + 主循环。
//!
//! 各页面按键处理器已按页拆分到 `src/pages/`（Game/Look/ThrowSelect/ThrowAim/Inventory/Dialog），
//! 本文件只保留进程入口、主循环编排与标题画面。

use std::io::{self, stdout};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use bevy_ecs::prelude::*;
use bevy_ecs::system::RunSystemOnce;
use crossterm::ExecutableCommand;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use dungeon_core::{TurnManager, ops};
use dungeon_render::{draw_title, render_ui};
use dungeon_world::{
    advance_and_settle_parallel as advance_and_settle, fov_system, load_game, setup_world,
};
use ratatui::Terminal;

mod keymap;
mod pages;
mod throw;

fn main() -> io::Result<()> {
    dungeon_core::init_logging();

    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let mut terminal = Terminal::new(ratatui::backend::CrosstermBackend::new(stdout()))?;
    let (mut world, game_start) = title_screen(&mut terminal)?;
    let result = run(&mut terminal, game_start, &mut world);
    dungeon_core::shutdown_logging();
    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    result
}

fn run(
    terminal: &mut Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
    game_start: Instant,
    world: &mut World,
) -> io::Result<()> {
    // ========== 进入游戏主循环前的初始化 ==========
    // 页栈是 UI 导航的核心：当前栈顶决定按键由哪个页面处理器消费。
    // 初始只有 Game 页，后续 Look/Inventory/ThrowAim 等通过 push 进入。
    world.insert_resource(dungeon_action::PageStack::default());

    // 重建占用表：根据所有 Position 实体生成 OccupancyMap，
    // 供移动、寻路、攻击距离等逻辑查询“某格是否被占用”。
    ops::rebuild_occupancy(world);

    // 先执行一次视野计算，保证首帧渲染前玩家视野已正确。
    let _ = world.run_system_once(fov_system);

    // 将当前可见信息写入 VisibleMemory，用于渲染“已探索/当前可见”区域。
    ops::update_visible_memory(world);

    // 首帧立即绘制一次，避免进入循环前屏幕空白。
    {
        let w: &World = &*world;
        terminal.draw(|frame| render_ui(frame, game_start, w))?;
    }

    // ========== 启动后台输入线程 ==========
    // 使用 mpsc channel 将 crossterm 的按键事件从输入线程发送到主线程。
    // 主线程每帧用 try_recv 非阻塞消费，不会因为等待输入而卡住渲染。
    let (tx, rx) = mpsc::channel::<KeyCode>();

    // modal_flag 是保留的“暂停输入”开关；当前没有页面写 true，
    // 若未来需要阻塞式对话框/模态输入，可置 true 让输入线程暂时休眠。
    let modal_flag = Arc::new(AtomicBool::new(false));
    let thread_flag = modal_flag.clone();

    thread::spawn(move || {
        // 去重状态：记录上一个按键和触发时间，用于过滤 33ms 内的重复按键。
        let mut last_code: KeyCode = KeyCode::Null;
        let mut last_time = Instant::now();
        loop {
            // 如果未来某处把 thread_flag 置为 true，则暂停读取终端输入。
            if thread_flag.load(Ordering::Relaxed) {
                thread::sleep(Duration::from_millis(16));
                continue;
            }

            // 非阻塞轮询终端事件；没有事件就继续循环，避免忙等。
            if crossterm::event::poll(Duration::from_millis(16)).unwrap_or(false)
                && let Ok(Event::Key(key)) = crossterm::event::read()
            {
                // 现代终端会区分 Press/Repeat/Release；
                // 这里只处理 Press，避免长按产生大量重复行动。
                if key.kind != KeyEventKind::Press {
                    continue;
                }

                // 33ms 去重：同一按键在极短时间内再次触发则丢弃，
                // 与主循环 30FPS 的帧节奏保持一致。
                let now = Instant::now();
                if key.code == last_code && now - last_time < Duration::from_millis(33) {
                    continue;
                }
                last_code = key.code;
                last_time = now;

                // 把按键发送给主线程；如果主线程已退出（channel 关闭），则结束输入线程。
                if tx.send(key.code).is_err() {
                    break;
                }
            }
        }
    });

    // ========== 主循环：输入 -> 行动 -> 世界推进 -> 渲染 ==========
    loop {
        // 记录本帧开始时间，用于末尾的帧率控制。
        let frame_start = Instant::now();

        // 1. 消费本帧所有输入
        // has_action 表示是否有按键真正产生了游戏行动（移动/攻击/使用等）。
        // 像打开背包、查看地图这类 UI 操作不会推进世界时间。
        let mut has_action = false;
        loop {
            match rx.try_recv() {
                Ok(code) => {
                    // 将按键交给当前页面的处理器；返回 true 表示该键触发了行动。
                    if pages::process_key(code, world)? {
                        has_action = true;
                    }
                }
                // 当前没有更多按键时退出内层循环，进入本帧世界推进。
                Err(mpsc::TryRecvError::Empty) => break,
                // 输入线程已结束（例如 channel 发送端被丢弃），游戏正常退出。
                Err(mpsc::TryRecvError::Disconnected) => return Ok(()),
            }
        }

        // 2. 推进世界
        // 只有“产生行动”且游戏未结束时才推进行动队列；
        // 否则本帧只做渲染，保持游戏暂停/等待状态。
        if has_action && !world.resource::<TurnManager>().game_over {
            advance_and_settle(world);
        }

        // 3. 渲染
        // 无论是否有输入都重绘一帧，保证 UI 状态（光标、日志、时间轴）及时刷新。
        {
            let w: &World = &*world;
            terminal.draw(|frame| render_ui(frame, game_start, w))?;
        }

        // 4. 退出检查
        // 页面处理器（如 Dialog 确认退出）会设置 wants_quit，
        // 主循环在此统一退出，避免散落在各处直接 return。
        if world.resource::<TurnManager>().wants_quit {
            break Ok(());
        }

        // 5. 帧率控制：目标 33ms/帧，约 30FPS。
        // 如果本帧处理耗时不足 33ms，则睡眠补齐，避免 CPU 空转。
        let elapsed = frame_start.elapsed();
        let target = Duration::from_millis(33);
        if elapsed < target {
            std::thread::sleep(target - elapsed);
        }
    }
}

fn title_screen(
    terminal: &mut Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
) -> io::Result<(World, Instant)> {
    loop {
        terminal.draw(draw_title)?;
        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Enter | KeyCode::Char('\n') | KeyCode::Char('\r') => {
                    let mut world = setup_world();
                    let _ = world.run_system_once(fov_system);
                    ops::update_map_memory(&mut world);
                    ops::update_visible_memory(&mut world);
                    return Ok((world, Instant::now()));
                }
                KeyCode::F(9) => {
                    // A37: 读档走 dungeon_world::load_game 单入口（双格式兼容 + post_load_refresh）
                    let mut world = setup_world();
                    if load_game(&mut world, "save.bin").is_ok() {
                        return Ok((world, Instant::now()));
                    }
                }
                KeyCode::Char('q') | KeyCode::Esc => {
                    disable_raw_mode()?;
                    stdout().execute(LeaveAlternateScreen)?;
                    std::process::exit(0);
                }
                _ => {}
            }
        }
    }
}
