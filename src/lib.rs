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

pub mod save;

pub use keys::translate_key;

use std::path::Path;

use bevy_ecs::prelude::World;
use ecs_core::world_loop::{apply_player_command, new_game, request_quit};
use presentation::{
    ExtractConfig, InputOutcome, PageStack, PresentationInput, SceneFrameSource, UiIntent,
    apply_ui_intent, map_input_event,
};
use render_api::{InputEvent, Key, SceneFrame};
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
        Self::assemble(new_game(seed), viewport)
    }

    /// 把已建好的世界装配成 `App`（新游戏与读档共用）。
    fn assemble(mut world: World, viewport: (u16, u16)) -> Self {
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

    /// 当前视口尺寸（读档重建 `App` 时沿用）。
    pub fn viewport(&self) -> (u16, u16) {
        self.config.viewport
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
        // 存档是集成层能力（DsnE19）：文件 I/O 不属于世界规则，也不进 `presentation`
        // 的键位表。F5 存、F9 读；失败写日志让玩家看见（静默失败 = 玩家以为存上了）。
        if let InputEvent::Key(key) = &event {
            if key.is_press() {
                match key.key {
                    Key::F(5) => {
                        let _ = self.save_to(&save::default_path());
                        return true;
                    }
                    Key::F(9) => {
                        let path = save::default_path();
                        match self.load_from(&path) {
                            Ok(()) => {}
                            Err(error) => save::push_log(&mut self.world, error.describe()),
                        }
                        self.refresh();
                        return true;
                    }
                    _ => {}
                }
            }
        }

        let page = self.world.resource::<PageStack>().kind();
        let pane_focus = self.world.resource::<PageStack>().pane_focus();
        let player = presentation::player_position(&mut self.world).unwrap_or((0, 0));
        let input = PresentationInput::new(page, player).with_pane_focus(pane_focus);

        match map_input_event(event, &input) {
            InputOutcome::Ignored => false,
            InputOutcome::Ui(intent) => self.apply_ui(intent),
            InputOutcome::Command(command) => apply_player_command(&mut self.world, command),
        }
    }

    /// 存档到 `path`（F5）。成功/失败都会写一条世界日志。
    pub fn save_to(&mut self, path: &Path) -> Result<(), save::SaveError> {
        let result = if self.is_game_over() {
            Err(save::SaveError::GameOver)
        } else {
            save::write(&self.world, path)
        };
        let message = match &result {
            Ok(()) => {
                let floor = self.world.resource::<ecs_core::FloorNumber>().0;
                format!("已存档（第 {floor} 层）")
            }
            Err(error) => error.describe(),
        };
        save::push_log(&mut self.world, message);
        self.refresh();
        result
    }

    /// 从存档文件读一局新游戏（F9 的构造侧；测试也直接用它）。
    pub fn load(path: &Path, viewport: (u16, u16)) -> Result<Self, save::SaveError> {
        let save = save::read(path)?;
        let world = save::restore_to_world(&save)?;
        let floor = world.resource::<ecs_core::FloorNumber>().0;
        let mut app = Self::assemble(world, viewport);
        save::push_log(&mut app.world, format!("读档：第 {floor} 层"));
        app.refresh();
        Ok(app)
    }

    /// 用存档替换当前这局（F9）：成功则整局换成读出来的世界，失败世界不动。
    pub fn load_from(&mut self, path: &Path) -> Result<(), save::SaveError> {
        let loaded = Self::load(path, self.viewport())?;
        *self = loaded;
        Ok(())
    }

    /// 落地一个页栈意图。
    fn apply_ui(&mut self, intent: UiIntent) -> bool {
        // 投掷页要读世界（自动装填、自动瞄准），也不改页栈之外的状态，先在这里接地。
        if matches!(intent, UiIntent::OpenThrow { .. }) {
            return self.open_throw_page();
        }
        if matches!(intent, UiIntent::ConfirmThrow) {
            return self.confirm_throw();
        }
        // 背包页的动作意图改的是**世界**（不是页栈）：先在这里接地。
        if matches!(
            intent,
            UiIntent::InventoryConfirm | UiIntent::InventoryUse | UiIntent::InventoryDrop
        ) {
            return self.apply_inventory_action(intent);
        }
        let world_size = (ecs_core::MAP_WIDTH, ecs_core::MAP_HEIGHT);
        let mut pages: PageStack = self.world.resource::<PageStack>().clone();
        // 页栈不读世界：Nav2 有几个页签（脚下有没有尸体/地面物品）由这里同步，
        // 否则 Tab 会转到"看不见的焦点位"上。提取层用同一份推导建导航栏。
        pages.set_inventory_nav2_len(presentation::inventory_nav2_tabs(&self.world).len());
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

    /// 背包页的动作：把"选中的行"换算成槽位，再调核心动作。
    ///
    /// 失败转成一条日志（玩家必须知道"为什么没动"）；返回 `true`
    /// ——按键确实被处理了，只是没成功。视图在下一帧 `refresh` 时按容器重建，
    /// 所以这里不需要手动失效任何缓存。
    fn apply_inventory_action(&mut self, intent: UiIntent) -> bool {
        let pages = self.world.resource::<PageStack>().clone();
        let Some(target) = presentation::inventory_target(&self.world, &pages) else {
            return false;
        };
        use presentation::InventoryTarget;
        // 脚下尸体/地面只认"拿取"：进/用/丢都说不通。这是把自动拾取改成显式动作的入口。
        if let (UiIntent::InventoryConfirm, InventoryTarget::Corpse { corpse, slot }) = (intent, target) {
            match ecs_core::take_from_corpse(&mut self.world, corpse, slot) {
                Ok(id) => log::info!("从尸体上拿起了{}", id.def().name),
                Err(error) => log::info!("背包动作失败：{}", error.describe()),
            }
            return true;
        }
        if let (UiIntent::InventoryConfirm, InventoryTarget::Ground { index }) = (intent, target) {
            match ecs_core::take_from_ground(&mut self.world, index) {
                Ok(id) => log::info!("捡起了{}", id.def().name),
                Err(error) => log::info!("背包动作失败：{}", error.describe()),
            }
            return true;
        }
        let result = match (intent, target) {
            (UiIntent::InventoryConfirm, InventoryTarget::Body { slot }) => {
                ecs_core::unequip_to_pack(&mut self.world, slot)
            }
            (UiIntent::InventoryConfirm, InventoryTarget::Pack { slot }) => {
                ecs_core::equip_from_pack(&mut self.world, slot)
            }
            (UiIntent::InventoryUse, InventoryTarget::Pack { slot }) => {
                ecs_core::use_from_pack(&mut self.world, slot)
            }
            (UiIntent::InventoryDrop, InventoryTarget::Pack { slot }) => {
                ecs_core::drop_from_pack(&mut self.world, slot)
            }
            // 身体行没有"用"与"丢"：装备要先卸下（避免"把剑吃了"这种误操作）。
            (UiIntent::InventoryUse | UiIntent::InventoryDrop, InventoryTarget::Body { .. }) => {
                Err(ecs_core::ItemActionError::NotUsable)
            }
            (UiIntent::InventoryUse | UiIntent::InventoryDrop, InventoryTarget::Corpse { .. }) => {
                Err(ecs_core::ItemActionError::NotUsable)
            }
            (UiIntent::InventoryUse | UiIntent::InventoryDrop, InventoryTarget::Ground { .. }) => {
                Err(ecs_core::ItemActionError::NotUsable)
            }
            _ => return false,
        };
        if let Err(error) = result {
            log::info!("背包动作失败：{}", error.describe());
        }
        true
    }

    /// 打开投掷瞄准页：副手没有可投掷物时先从行囊装填，再按信息栏选中目标自动瞄准。
    ///
    /// 自动瞄准是"先给一个合理落点"；手动方向键移动光标仍是备用方案（m10559 问答）。
    /// 开页失败（行囊里也没有可投掷物）只写日志、不开页——让玩家能看见原因。
    fn open_throw_page(&mut self) -> bool {
        if ecs_core::off_hand_throwable(&self.world).is_none() {
            match ecs_core::load_throwable_from_pack(&mut self.world) {
                Ok(id) => log::info!("装填了{}", id.def().name),
                Err(error) => {
                    log::info!("无法投掷：{}", error.describe());
                    self.refresh();
                    return true;
                }
            }
        }
        let player = presentation::player_position(&mut self.world).unwrap_or((0, 0));
        let cursor = self.auto_throw_target(player).unwrap_or(player);
        let mut pages = self.world.resource::<PageStack>().clone();
        pages.open_throw(cursor.0, cursor.1);
        *self.world.resource_mut::<PageStack>() = pages;
        self.refresh();
        true
    }

    /// 信息栏里当前选中的候选若在投掷射程/视线内，就是自动瞄准的落点。
    fn auto_throw_target(&self, player: (usize, usize)) -> Option<(usize, usize)> {
        let id = self.frame.info.selected_id()?;
        let entity = self.frame.entities.iter().find(|entity| entity.id == id)?;
        let cursor = (entity.position.0.max(0) as usize, entity.position.1.max(0) as usize);
        let map = self.world.resource::<ecs_core::Map>();
        ecs_core::throw_reachable(map, player, cursor).then_some(cursor)
    }

    /// 确认投掷：把瞄准光标换算成相对玩家的方向命令，交给行动流水线。
    ///
    /// 无论行动是否被接受都关掉瞄准页：被拒绝的原因已经由规则层写进日志，
    /// 把玩家留在瞄准页里只会让"按了 Enter 没反应"更困惑。
    fn confirm_throw(&mut self) -> bool {
        let pages = self.world.resource::<PageStack>().clone();
        let player = presentation::player_position(&mut self.world).unwrap_or((0, 0));
        let cursor = pages.throw_cursor();
        let dx = cursor.0 - player.0 as i32;
        let dy = cursor.1 - player.1 as i32;
        let accepted = apply_player_command(
            &mut self.world,
            ecs_core::PlayerCommand::Throw {
                dx: dx as isize,
                dy: dy as isize,
            },
        );
        let mut pages = self.world.resource::<PageStack>().clone();
        pages.close();
        *self.world.resource_mut::<PageStack>() = pages;
        self.refresh();
        accepted
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
