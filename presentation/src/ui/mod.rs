//! UI 页栈：页面状态机住在集成层，后端只画 [`UiView`]。
//!
//! # 为什么页栈在这里而不是后端
//!
//! 页栈是**跨后端共享的行为**：按 `Esc` 关页面、按 `?` 开 Look、退出要确认——
//! 这些规则与"用什么画"无关。放在后端意味着 TUI 和 GPU 各实现一遍，必然漂移
//! （DsnX14：「页栈路由与 tap-tap 只在 `presentation` 实现一次」）。
//!
//! # 本轮的页栈范围
//!
//! R2 计划里 `Look` / `Dialog` 先行，`Inventory` / `Throw` 依赖尚未迁移的
//! 物品/投掷规则（DsnX13 S4），因此本轮只做前两者：
//!
//! | 页面 | 本轮 | 说明 |
//! |---|---|---|
//! | `Game` | ✅ | 无覆盖 |
//! | `Dialog` | ✅ | 退出确认（`Esc` / `q` 触发），有真实交互 |
//! | `Look` | ✅ | 光标移动 + 地形详情，数据每帧从世界重算 |
//! | `Inventory` / `ThrowSelect` / `ThrowAim` | ⏳ | 需要物品/投掷规则迁移（S4） |
//!
//! # 页栈只存"看哪里"
//!
//! 页面**内容**每帧从世界重算，页栈里不缓存世界数据副本——否则就会出现
//! 「页栈里那份 HP 已经过期」的第二套状态（DsnX14：`SceneFrame` 不是第二套
//! 游戏状态，页栈同理）。

use bevy_ecs::prelude::*;
use render_api::{
    DialogView, EntityView, LookView, PageKind, Pane, TileInfo, UiTextLine, UiView, VisualKey,
};

use crate::extract::ExtractConfig;

/// 页栈可以表达的用户意图。
///
/// 装配层消费它：`ConfirmQuit` 转成 `request_quit`。
/// 页栈**不直接改游戏状态**——那是装配层的职责（后端更不许改）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiIntent {
    /// 什么都不做。
    None,
    /// 打开退出确认对话框。
    OpenQuitDialog,
    /// 关闭当前覆盖页。
    ClosePage,
    /// 打开背包页。
    OpenInventory,
    /// 背包页：4 个焦点位顺时针轮转（Tab）。
    CycleInventoryFocus,
    /// 背包页：导航栏聚焦时 `dx` 切页签，内容栏聚焦时 `dy` 移行。
    MoveInventoryCursor { dx: i32, dy: i32 },
    /// 背包页确认：身体行 = 卸下，行囊行 = 装上。
    InventoryConfirm,
    /// 背包页使用（消耗品）。
    InventoryUse,
    /// 背包页丢弃。
    InventoryDrop,
    /// 打开 Look 页，光标落在给定世界坐标。
    OpenLook { x: usize, y: usize },
    /// 移动 Look 光标。
    MoveLookCursor { dx: i32, dy: i32 },
    /// 切换游戏画面的面板焦点（Tab，顺时针：摄像机 → 信息 → 状态 → 事件）。
    CyclePaneFocus,
    /// 信息栏移动选中行（`dy` 为 ±1；焦点在信息栏时方向键改为选行）。
    MoveInfoCursor { dy: i32 },
    /// 用户确认退出。
    ConfirmQuit,
    /// 用户取消退出。
    CancelQuit,
}

/// UI 页栈状态（ECS 资源）。
#[derive(Resource, Debug, Clone, PartialEq, Default)]
pub struct PageStack {
    kind: PageKind,
    /// Look 页的光标（世界坐标）。
    look_cursor: (i32, i32),
    /// 当前 `Dialog` 页是否是退出确认（决定 `Enter` 的含义）。
    quit_pending: bool,
    /// 背包页的焦点位（顺时针 Nav1 → Nav2 → Context2 → Context1）。
    inventory_focus: render_api::InventoryFocus,
    /// 背包页 Nav1 的选中页签（身体 / 行囊）。
    inventory_nav1_selected: usize,
    /// 背包页 Nav2 的选中页签（尸体 / 地面）。
    inventory_nav2_selected: usize,
    /// 背包页 Nav2 的页签数量（0 = 脚下既没有尸体也没有地面物品）。
    ///
    /// 由应用层在分发页栈意图前从世界同步——页栈不读世界（DsnX14）。
    inventory_nav2_len: usize,
    /// Body / Pack 两个内容栏各自的选中行（页栈只存鼠标/键盘位置，不夹取）。
    inventory_context1_cursor: usize,
    inventory_context2_cursor: usize,
    /// 信息栏的选中行（只列视野内的观察候选；夹取发生在提取时）。
    info_cursor: usize,
    /// 游戏画面的面板焦点（Tab 顺时针轮转）。
    pane_focus: Pane,
}

impl PageStack {
    pub const fn new() -> Self {
        Self {
            kind: PageKind::Game,
            look_cursor: (0, 0),
            quit_pending: false,
            inventory_focus: render_api::InventoryFocus::Nav1,
            inventory_nav1_selected: 0,
            inventory_nav2_selected: 0,
            inventory_nav2_len: 0,
            inventory_context1_cursor: 0,
            inventory_context2_cursor: 0,
            info_cursor: 0,
            pane_focus: Pane::Camera,
        }
    }

    /// 信息栏的选中行（提取时按候选数量夹取）。
    pub const fn info_cursor(&self) -> usize {
        self.info_cursor
    }

    /// 当前面板焦点。
    pub const fn pane_focus(&self) -> Pane {
        self.pane_focus
    }

    /// 把焦点移到下一个面板（顺时针）。只在游戏页生效。
    pub fn cycle_pane_focus(&mut self) {
        self.pane_focus = self.pane_focus.next();
    }

    pub const fn kind(&self) -> PageKind {
        self.kind
    }

    pub const fn is_game(&self) -> bool {
        matches!(self.kind, PageKind::Game)
    }

    pub const fn look_cursor(&self) -> (i32, i32) {
        self.look_cursor
    }

    pub const fn is_quit_pending(&self) -> bool {
        self.quit_pending
    }

    /// 打开 Look 页，光标落在给定世界坐标。
    /// 打开背包页：焦点回到 Nav1/身体，所有内容栏光标归零。
    pub fn open_inventory(&mut self) {
        self.kind = PageKind::Inventory;
        self.inventory_focus = render_api::InventoryFocus::Nav1;
        self.inventory_nav1_selected = 0;
        self.inventory_nav2_selected = 0;
        self.inventory_context1_cursor = 0;
        self.inventory_context2_cursor = 0;
    }

    /// 背包页焦点位。
    pub const fn inventory_focus(&self) -> render_api::InventoryFocus {
        self.inventory_focus
    }

    /// 背包页 Nav1 的选中页签下标。
    pub const fn inventory_nav1_selected(&self) -> usize {
        self.inventory_nav1_selected
    }

    /// 背包页 Nav2 的选中页签下标（页栈不夹取，提取时按实际页签数保底）。
    pub const fn inventory_nav2_selected(&self) -> usize {
        self.inventory_nav2_selected
    }

    /// 背包页某个内容栏的选中行下标。
    pub const fn inventory_context_cursor(&self, group: render_api::InventoryNavGroup) -> usize {
        match group {
            render_api::InventoryNavGroup::Nav1 => self.inventory_context1_cursor,
            render_api::InventoryNavGroup::Nav2 => self.inventory_context2_cursor,
        }
    }

    /// 由应用层在世界变化后同步：Nav2 有几个页签（0 = 脚下什么都没有）。
    ///
    /// 焦点的合法性也跟着变：Nav2 消失时若焦点正停在 Nav2/Context2，回到 Nav1。
    pub fn set_inventory_nav2_len(&mut self, len: usize) {
        self.inventory_nav2_len = len;
        if len == 0 {
            self.inventory_nav2_selected = 0;
            if matches!(
                self.inventory_focus,
                render_api::InventoryFocus::Nav2 | render_api::InventoryFocus::Context2
            ) {
                self.inventory_focus = render_api::InventoryFocus::Nav1;
            }
        } else if self.inventory_nav2_selected >= len {
            self.inventory_nav2_selected = len - 1;
        }
    }

    /// 背包页 Tab：焦点顺时针跳过不存在的 Nav2/Context2。
    /// Tab 顺时针轮转四个焦点位；Nav2 不存在时整组跳过。
    pub fn cycle_inventory_focus(&mut self) {
        let mut next = self.inventory_focus.next();
        if self.inventory_nav2_len == 0 {
            // 跳过 Nav2 与 Context2 两个连着的焦点位，而不是只跳一个。
            while matches!(
                next,
                render_api::InventoryFocus::Nav2 | render_api::InventoryFocus::Context2
            ) {
                next = next.next();
            }
        }
        self.inventory_focus = next;
    }

    pub fn open_look(&mut self, x: usize, y: usize) {
        self.kind = PageKind::Look;
        self.look_cursor = (x as i32, y as i32);
        self.quit_pending = false;
    }

    /// 打开退出确认。
    pub fn open_quit_dialog(&mut self) {
        self.kind = PageKind::Dialog;
        self.quit_pending = true;
    }

    /// 关掉当前覆盖页，回到游戏画面。
    pub fn close(&mut self) {
        self.kind = PageKind::Game;
        self.quit_pending = false;
    }

    /// 移动 Look 光标（夹在世界范围内）。非 Look 页不响应。
    pub fn move_look_cursor(&mut self, dx: i32, dy: i32, world: (usize, usize)) {
        if self.kind != PageKind::Look {
            return;
        }
        let max_x = world.0.saturating_sub(1) as i32;
        let max_y = world.1.saturating_sub(1) as i32;
        self.look_cursor.0 = (self.look_cursor.0 + dx).clamp(0, max_x);
        self.look_cursor.1 = (self.look_cursor.1 + dy).clamp(0, max_y);
    }

    /// 处理一个页栈意图；返回 `true` 表示页栈消费了它。
    ///
    /// 返回 `false` 的两种情况：`None`（没有意图），以及"这个意图在当前页面
    /// 无意义"（例如在游戏页按 `Esc` 关闭——那里由输入层翻译成打开退出确认）。
    pub fn apply(&mut self, intent: UiIntent) -> bool {
        match intent {
            UiIntent::None => false,
            UiIntent::OpenQuitDialog => {
                self.open_quit_dialog();
                true
            }
            // 三个动作意图改的是**世界**而不是页栈：由应用层拿世界落地
            // （见 `dungeon-app` 的 `apply_ui`）。这里返回 `false`，
            // 让"忘了接地"在测试里表现为"按了没反应"，而不是偷偷改页栈状态。
            UiIntent::InventoryConfirm | UiIntent::InventoryUse | UiIntent::InventoryDrop => false,
            UiIntent::OpenLook { x, y } => {
                self.open_look(x, y);
                true
            }
            UiIntent::MoveLookCursor { dx, dy } => {
                // 光标移动需要世界尺寸来夹取，而 `apply` 拿不到世界尺寸；
                // 真正落地在 `input::apply_ui_intent`。这里**故意不处理**并返回
                // `false`——让"忘了带世界尺寸"变成调用方的编译期/测试期问题，
                // 而不是偷偷移动一个不夹取的光标。
                let _ = (dx, dy);
                false
            }
            UiIntent::OpenInventory => {
                self.open_inventory();
                true
            }
            UiIntent::MoveInventoryCursor { dx, dy } => {
                // 页栈不数内容行数（内容每帧从世界重算），所以内容栏只挪光标、
                // 不夹取；夹取发生在提取时按本帧行数做。页签数量是页栈知道的
                // （Nav1 恒 2 个，Nav2 由应用层同步），所以页签在这里夹取。
                if !matches!(self.kind, PageKind::Inventory) {
                    return false;
                }
                use render_api::InventoryFocus;
                match self.inventory_focus {
                    InventoryFocus::Nav1 if dx != 0 => {
                        let next = (self.inventory_nav1_selected as i32 + dx).clamp(0, 1);
                        self.inventory_nav1_selected = next as usize;
                        true
                    }
                    InventoryFocus::Nav2 if dx != 0 && self.inventory_nav2_len > 0 => {
                        let last = self.inventory_nav2_len.saturating_sub(1) as i32;
                        let next = (self.inventory_nav2_selected as i32 + dx).clamp(0, last);
                        self.inventory_nav2_selected = next as usize;
                        true
                    }
                    InventoryFocus::Context1 if dy != 0 => {
                        self.inventory_context1_cursor =
                            (self.inventory_context1_cursor as i32 + dy).max(0) as usize;
                        true
                    }
                    InventoryFocus::Context2 if dy != 0 => {
                        self.inventory_context2_cursor =
                            (self.inventory_context2_cursor as i32 + dy).max(0) as usize;
                        true
                    }
                    _ => false,
                }
            }
            UiIntent::CycleInventoryFocus => {
                if !matches!(self.kind, PageKind::Inventory) {
                    return false;
                }
                self.cycle_inventory_focus();
                true
            }
            UiIntent::CyclePaneFocus => {
                // 焦点只在游戏画面里有意义：页面打开时 Tab 留给页面自己的
                // 焦点模型（背包页的 Nav1/Nav2/Context1/Context2）。
                if !self.is_game() {
                    return false;
                }
                self.cycle_pane_focus();
                true
            }
            UiIntent::MoveInfoCursor { dy } => {
                // 只有游戏页、焦点在信息栏时方向键才用来选行。行数由本帧的候选
                // 决定（页栈数不出来），所以这里只挪光标；夹取在提取时对
                // `entries.len()` 做。
                if !self.is_game() || self.pane_focus != Pane::Info {
                    return false;
                }
                let next = (self.info_cursor as i32 + dy).max(0);
                self.info_cursor = next as usize;
                true
            }
            UiIntent::ClosePage => {
                if self.is_game() {
                    false
                } else {
                    self.close();
                    true
                }
            }
            UiIntent::ConfirmQuit | UiIntent::CancelQuit => {
                if self.quit_pending {
                    self.close();
                    true
                } else {
                    false
                }
            }
        }
    }

    /// 产出契约层的 [`UiView`]，并顺带算出 Look 页要显示的地形详情。
    ///
    /// **必须传 `&SceneFrame`**（而不是 `World`）：地形详情就从本帧快照里读，
    /// 这样页栈与提取器用的是同一份数据，不会出现"页栈显示的地形与地图不一致"。
    pub fn view(&self, _config: &ExtractConfig, frame: &render_api::SceneFrame) -> UiView {
        match self.kind {
            PageKind::Game => UiView::Game,
            PageKind::Dialog if self.quit_pending => {
                UiView::Dialog(DialogView::confirm("退出", "确认退出游戏？"))
            }
            PageKind::Look => UiView::Look(self.look_view(frame)),
            // R2 之前不会进入这些页；给一个空视图而不是 panic——页栈状态若被
            // 外部改成未实现的页，游戏不该直接崩掉。
            _ => UiView::Game,
        }
    }

    fn look_view(&self, frame: &render_api::SceneFrame) -> LookView {
        let (cx, cy) = self.look_cursor;
        let inside = cx >= 0 && cy >= 0;
        let (ux, uy) = (cx.max(0) as usize, cy.max(0) as usize);

        let (tile, entity) = if inside {
            (
                self.tile_info(frame, ux, uy),
                frame
                    .entities
                    .iter()
                    .find(|entity| entity.position == (cx, cy))
                    .cloned(),
            )
        } else {
            (None, None)
        };

        LookView {
            cursor: self.look_cursor,
            tile,
            entity,
            footer: "方向键移动  Esc 关闭".to_string(),
        }
    }

    /// 从快照组装光标处的地形详情。
    ///
    /// `TileInfo.walkable` / `blocks_sight` 需要 `ecs_core::Tile` 的规则，而快照里
    /// 只有 [`render_api::VisualKey`]（不含通行性）。契约层不该塞游戏规则进来，
    /// 所以这里**只报告可见性**，通行性留给未来需要时的专用字段。
    fn tile_info(&self, frame: &render_api::SceneFrame, x: usize, y: usize) -> Option<TileInfo> {
        let visual = frame.map.tile(x, y)?;
        if visual.is_unknown() {
            return None;
        }
        Some(
            TileInfo::new(visual, format!("{visual:?}")).with_visibility(
                frame.map.is_visible(x, y),
                frame.map.is_explored(x, y),
            ),
        )
    }
}

/// 实体 → Look 页要显示的文本行。
pub fn entity_lines(entity: &EntityView) -> Vec<UiTextLine> {
    let mut lines = vec![UiTextLine::plain(entity.name.clone())];
    if let Some(hp) = entity.hp {
        lines.push(UiTextLine::plain(format!("HP {:.0} / {:.0}", hp.current, hp.max)));
    }
    lines
}


/// 把 [`VisualKey`] 包成一行 UI 文本（后端排版用）。
pub fn visual_text(key: VisualKey, text: impl Into<String>) -> UiTextLine {
    UiTextLine::plain(format!("{:?} {}", key.category(), text.into()))
}

#[cfg(test)]
mod tests;
