//! `dungeon-app`：装配层——把四个 crate 接成一个能跑的游戏。
//!
//! 它只做三件事，任何一条游戏规则都不在这里：
//!
//! 1. **造世界**：`ecs_core::world_loop::new_game`；
//! 2. **接输入**：后端无关的 `render_api::InputEvent` → `presentation` 的页栈意图 /
//!    `PlayerCommand` → `ecs_core::world_loop::apply_player_command`；
//! 3. **出画面**：`presentation::extract_scene_frame` → `tui::TuiPlugin`。
//!
//! ```text
//!                       ┌──────────────┐
//!   InputEvent ────────>│              │────> PlayerCommand ──> ecs_core
//!                       │ Presentation │
//!   SceneFrame <────────│              │<──── apply_ui_intent
//!                       └──────────────┘
//! ```
//!
//! # 为什么装配逻辑在 lib 而不是 `main.rs`
//!
//! `main.rs` 里放不了测试（bin 目标无法被集成测试导入），于是"主循环接线是否正确"
//! 就只能靠人工玩一遍。把 [`App`] 放在这里之后，`tests/mvp_loop_test.rs` 可以
//! **headless 地**驱动真实链路：喂事件 → 世界推进 → 提取帧 → 断言画面。
//!
//! `main.rs` 因此退化成"终端生命周期 + 两个 match 的翻译表"，它自己不承载逻辑。
//!
//! # 平台细节的边界
//!
//! `crossterm::KeyCode` 只允许出现在 `main.rs` 与 `sys`；本模块只认
//! `render_api::InputEvent`。这样键位表可以被单测（`presentation::input`），
//! 换后端也不用重写。

#![forbid(unsafe_code)]

pub mod keys;

pub use keys::translate_key;

use bevy_ecs::prelude::World;
use ecs_core::world_loop::{apply_player_command, new_game, request_quit};
use presentation::{
    ExtractConfig, InputOutcome, PageStack, PresentationInput, SceneFrameSource, UiIntent,
    apply_ui_intent, map_input_event,
};
use render_api::{InputEvent, SceneFrame};
use tui::{DevLogBuffer, TuiPlugin};

/// 装配好的游戏实例：世界 + 集成层状态 + 后端接缝。
///
/// 三者都是"跑一局游戏"必需的、生命周期一致，所以打包成一个类型——散在
/// `main.rs` 的局部变量里时，"哪一层拥有什么状态"只能靠读代码。
pub struct App {
    world: World,
    config: ExtractConfig,
    source: SceneFrameSource,
    /// 最后一帧。渲染与断言都读它，避免重复提取。
    frame: SceneFrame,
}

impl App {
    /// 开一局新游戏。
    ///
    /// `viewport` 是后端表面尺寸（TUI 为终端格子数）；`(0, 0)` 表示"未知"，
    /// 相机会退回世界中心——**不要**因为拿不到尺寸就 panic，那会让 resize
    /// 边界变成崩溃点。
    pub fn new(seed: u64, viewport: (u16, u16)) -> Self {
        let mut world: World = new_game(seed);

        // 集成层状态：页栈与帧号。两者都**不属于** `ecs_core`——
        // 它们是渲染/交互状态，和"世界是什么样"无关。
        world.insert_resource(PageStack::new());
        // 后端调试面板的数据源（由 `main.rs` 从 `sys` 的日志通道灌入）。
        world.insert_resource(DevLogBuffer::new(80));

        let mut app = Self {
            world,
            config: ExtractConfig::default().with_viewport(viewport.0, viewport.1),
            source: SceneFrameSource::new(),
            frame: SceneFrame::empty(),
        };
        app.refresh();
        app
    }

    /// 更新视口尺寸（终端 resize 时调用）。
    pub fn set_viewport(&mut self, viewport: (u16, u16)) {
        self.config = self.config.with_viewport(viewport.0, viewport.1);
    }

    /// 世界的只读访问（测试与调试用；改游戏状态仍只能经 `ecs_core` 的公共 API）。
    pub fn world(&self) -> &World {
        &self.world
    }

    /// 世界的可变访问（仅装配/测试需要，例如灌入调试日志）。
    pub fn world_mut(&mut self) -> &mut World {
        &mut self.world
    }

    /// 当前帧。
    pub fn frame(&self) -> &SceneFrame {
        &self.frame
    }

    /// 重新提取一帧。
    ///
    /// 页栈按值取一份：`next_frame(world, ..)` 要可变借用 world，而
    /// `world.resource::<PageStack>()` 是不可变借用，直接传引用会打架。
    pub fn refresh(&mut self) -> &SceneFrame {
        let pages: PageStack = self.world.resource::<PageStack>().clone();
        self.frame = self.source.next_frame(&mut self.world, &self.config, &pages);
        &self.frame
    }

    /// 处理一个后端无关的输入事件，返回是否推进了世界。
    ///
    /// 返回值只为测试与日志：命令被**拒绝**时（撞墙、越界）
    /// `apply_player_command` 返回 `false`，这在本层不是错误。
    pub fn handle(&mut self, event: InputEvent) -> bool {
        let page = self.world.resource::<PageStack>().kind();
        let player = presentation::player_position(&mut self.world).unwrap_or((0, 0));
        let input = PresentationInput::new(page, player);

        match map_input_event(event, &input) {
            InputOutcome::Ignored => false,
            InputOutcome::Ui(intent) => self.apply_ui(intent),
            InputOutcome::Command(command) => apply_player_command(&mut self.world, command),
        }
    }

    /// 落地一个页栈意图。
    fn apply_ui(&mut self, intent: UiIntent) -> bool {
        let world_size = (ecs_core::MAP_WIDTH, ecs_core::MAP_HEIGHT);
        let mut pages: PageStack = self.world.resource::<PageStack>().clone();
        let handled = apply_ui_intent(&mut pages, intent, world_size);
        *self.world.resource_mut::<PageStack>() = pages;

        // 退出必须**确认之后**才生效：`OpenQuitDialog` 只开页面，
        // `ConfirmQuit` 才真的请求退出。这条区分是"按 q 直接退出"（用户会以为
        // 程序崩了）与"按 q 弹出确认"的分界。
        if handled && matches!(intent, UiIntent::ConfirmQuit) {
            request_quit(&mut self.world);
        }
        handled
    }

    /// 游戏是否已请求退出。
    pub fn wants_quit(&self) -> bool {
        self.world.resource::<ecs_core::TurnManager>().wants_quit
    }

    /// 玩家是否已死（`ecs_core` 会在玩家死亡时置 `game_over`）。
    pub fn is_game_over(&self) -> bool {
        self.world.resource::<ecs_core::TurnManager>().game_over
    }
}

/// 便捷：把世界里的开发者日志缓冲**拷贝一份**交给后端。
///
/// 为什么是拷贝而不是引用：`App::refresh(&mut self)` 与 `App::world(&self)`
/// 的借用无法在同一表达式里共存（一个要可变、一个要不可变），而日志面板每帧
/// 最多几十行字符串——拷贝的代价远小于为了让借用通过而把 `App` 拆成两块状态。
///
/// 这也是渲染唯一一次接触 `World`，读的还是 TUI 自己的调试资源而非游戏状态；
/// 等调试面板搬进 `SceneFrame` 之后，这个函数就该消失。
pub fn dev_log(world: &World) -> Option<DevLogBuffer> {
    world.get_resource::<DevLogBuffer>().cloned()
}

/// 后端插件（当前只有 TUI；GPU 后端将来扮演同一角色）。
pub fn tui_plugin() -> TuiPlugin {
    TuiPlugin::new()
}
