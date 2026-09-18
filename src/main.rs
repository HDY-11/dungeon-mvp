//! `dungeon-app`：装配层。
//!
//! 它只做三件事，任何一条游戏规则都不在这里：
//!
//! 1. **造世界**：`ecs_core::world_loop::new_game`；
//! 2. **接输入**：`sys` 的终端按键 → `render-api` 的 `InputEvent` → `presentation`
//!    的页栈意图 / `PlayerCommand` → `apply_player_command`；
//! 3. **选后端**：每帧 `presentation::extract_scene_frame` → `tui::draw_scene`。
//!
//! ```text
//! sys（终端/输入）   ecs_core（规则）   presentation（提取/输入映射）   tui（绘制）
//!        └──────────────────────┬──────────────────────────┘
//!                          本文件：装配 + 主循环
//! ```
//!
//! # 为什么输入要绕一圈 `InputEvent`
//!
//! `crossterm::event::KeyCode` 是**平台细节**：它只该出现在这里和 `sys`。
//! 键位表（哪个键做什么）住在 `presentation`，于是：
//!
//! - 换 GPU 后端不用重写键位；
//! - 键位可以被单测（不需要终端）；
//! - 这个文件退化成一张"两个 `match` 的翻译表"，没有规则可漂移。
//!
//! # 为什么不在渲染循环里直接查询 ECS
//!
//! 那正是 Dsn28 要消除的耦合（旧 `render_game(frame, &mut World)` 把后端与
//! World 焊死）。现在 `tui` 的依赖表里没有 `ecs_core`，从编译期就无法这么做。

use std::io::{self, stdout};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use bevy_ecs::prelude::World;
use crossterm::event::KeyCode;
use ecs_core::world_loop::{apply_player_command, new_game, request_quit};
use presentation::{
    ExtractConfig, InputOutcome, PageStack, PresentationInput, SceneFrameSource, apply_ui_intent,
    map_input_event,
};
use ratatui::Terminal;
use render_api::{InputEvent, Key, KeyEvent, Modifiers};
use sys::{self, try_recv_key};
use tui::{DevLogBuffer, TuiPlugin};

/// 目标帧间隔（≈30fps）。与旧实现一致：主循环是"输入驱动 + 定时重绘"，
/// 不做插值/动画，所以这个值只影响输入延迟与 CPU 占用。
const FRAME_INTERVAL: Duration = Duration::from_millis(33);

fn main() -> io::Result<()> {
    let log_rx = sys::init_logging();
    sys::enter_raw_mode()?;
    sys::enter_alternate_screen()?;

    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    let mut world: World = new_game(seed);

    // 集成层状态：页栈与帧号。两者都**不属于** `ecs_core`——它们是渲染/交互状态。
    world.insert_resource(PageStack::new());
    world.insert_resource(DevLogBuffer::new(80));

    let config = ExtractConfig::default();
    let mut source = SceneFrameSource::new();
    let ui = TuiPlugin::new();

    let mut terminal = Terminal::new(ratatui::backend::CrosstermBackend::new(stdout()))?;

    // 首帧：终端尺寸此时已知，用它算相机视口。
    let (view_w, view_h) = viewport(&terminal);
    let config = config.with_viewport(view_w, view_h);
    draw(&mut terminal, &mut world, &mut source, &config, &ui)?;

    let rx = sys::spawn_key_source();

    loop {
        let frame_start = Instant::now();

        while let Some(code) = try_recv_key(&rx) {
            handle_input(&mut world, code);
        }

        while let Ok(record) = log_rx.try_recv() {
            world.resource_mut::<DevLogBuffer>().push(
                record.level.to_string(),
                record.target,
                record.message,
            );
        }

        let config = config.with_viewport(view_w, view_h);
        draw(&mut terminal, &mut world, &mut source, &config, &ui)?;

        if world.resource::<ecs_core::TurnManager>().wants_quit {
            break;
        }

        let elapsed = frame_start.elapsed();
        if elapsed < FRAME_INTERVAL {
            std::thread::sleep(FRAME_INTERVAL - elapsed);
        }
    }

    sys::shutdown_logging();
    sys::leave_raw_mode()?;
    sys::leave_alternate_screen()?;
    terminal.show_cursor()?;
    Ok(())
}

/// 终端当前可用的整帧尺寸（供相机视口使用）。
fn viewport(terminal: &Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>) -> (u16, u16) {
    terminal
        .size()
        .map(|area| (area.width, area.height))
        // 拿不到尺寸时不 panic：`(0, 0)` 表示"未知"，相机会退回世界中心。
        .unwrap_or((0, 0))
}

fn draw(
    terminal: &mut Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
    world: &mut World,
    source: &mut SceneFrameSource,
    config: &ExtractConfig,
    ui: &TuiPlugin,
) -> io::Result<()> {
    // 页栈按值取一份，避免在 `next_frame(world, ..)` 的可变借用期间还持着
    // world 的不可变借用。
    let pages: PageStack = world.resource::<PageStack>().clone();
    let frame = source.next_frame(world, config, &pages);
    // `dev_log` 借用 world（不可变），先取出来再进闭包，避免与 `next_frame` 的

    // 可变借用在同一表达式里打架。

    let dev_log = ui.dev_log(world);

    terminal.draw(|terminal_frame| ui.draw(terminal_frame, &frame, dev_log))?;
    Ok(())
}

/// 按键 → 意图：翻译、落地、推进世界。
///
/// 顺序要紧：**先处理页栈意图，再处理玩家命令**，两者互斥（一次按键只产生一种
/// `InputOutcome`），所以顺序其实不影响结果；但页栈意图可能改状态（例如打开
/// 确认框），让它先执行能让"同一帧内状态一致"更容易推理。
fn handle_input(world: &mut World, code: KeyCode) {
    let Some(event) = translate_key(code) else {
        return;
    };

    let input = PresentationInput::new(
        world.resource::<PageStack>().kind(),
        presentation::player_position(world).unwrap_or((0, 0)),
    );

    match map_input_event(event, &input) {
        InputOutcome::Ignored => {}
        InputOutcome::Ui(intent) => {
            let world_size = (ecs_core::MAP_WIDTH, ecs_core::MAP_HEIGHT);
            let mut pages = world.resource::<PageStack>().clone();
            let handled = apply_ui_intent(&mut pages, intent, world_size);
            *world.resource_mut::<PageStack>() = pages;
            if handled {
                match intent {
                    presentation::UiIntent::ConfirmQuit => request_quit(world),
                    // 打开 Look 页时把相机钉住由提取层处理（相机意图在页栈里）；
                    // 这里只需要把世界推进一次，让画面立刻反映新页面。
                    _ => {}
                }
            }
        }
        InputOutcome::Command(command) => {
            let _ = apply_player_command(world, command);
        }
    }
}

/// 终端按键 → 后端无关的 [`InputEvent`]。**这是唯一的平台相关映射**。
///
/// 返回 `None` 表示这个按键在契约里没有对应物（例如媒体键、鼠标捕获键）；
/// 调用方直接忽略。
fn translate_key(code: KeyCode) -> Option<InputEvent> {
    let key = match code {
        KeyCode::Up => Key::Up,
        KeyCode::Down => Key::Down,
        KeyCode::Left => Key::Left,
        KeyCode::Right => Key::Right,
        KeyCode::Home => Key::Home,
        KeyCode::End => Key::End,
        KeyCode::PageUp => Key::PageUp,
        KeyCode::PageDown => Key::PageDown,
        KeyCode::Enter => Key::Enter,
        KeyCode::Esc => Key::Esc,
        KeyCode::Tab => Key::Tab,
        KeyCode::Backspace => Key::Backspace,
        KeyCode::Delete => Key::Delete,
        KeyCode::Char(' ') => Key::Space,
        KeyCode::Char(c) => Key::Char(c),
        KeyCode::F(n) => Key::F(n),
        _ => return None,
    };
    Some(InputEvent::Key(KeyEvent {
        key,
        state: render_api::KeyState::Pressed,
        modifiers: Modifiers::NONE,
    }))
}

