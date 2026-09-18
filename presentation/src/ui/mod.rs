//! UI 页栈：页面状态机住在集成层，后端只画 [`UiView`]。
//!
//! # 为什么页栈在这里而不是后端
//!
//! 页栈是**跨后端共享的行为**：按 `Esc` 关页面、按 `?` 开 Look、退出要确认——
//! 这些规则与"用什么画"无关。放在后端意味着 TUI 和 GPU 各实现一遍，必然漂移
//! （Dsn28：「页栈路由与 tap-tap 只在 `presentation` 实现一次」）。
//!
//! # 本轮的页栈范围
//!
//! R2 计划里 `Look` / `Dialog` 先行，`Inventory` / `Throw` 依赖尚未迁移的
//! 物品/投掷规则（Dsn25 S4），因此本轮只做前两者：
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
//! 「页栈里那份 HP 已经过期」的第二套状态（Dsn28：`SceneFrame` 不是第二套
//! 游戏状态，页栈同理）。

use bevy_ecs::prelude::*;
use render_api::{DialogView, EntityView, LookView, PageKind, TileInfo, UiTextLine, UiView, VisualKey};

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
    /// 打开 Look 页，光标落在给定世界坐标。
    OpenLook { x: usize, y: usize },
    /// 移动 Look 光标。
    MoveLookCursor { dx: i32, dy: i32 },
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
}

impl PageStack {
    pub const fn new() -> Self {
        Self {
            kind: PageKind::Game,
            look_cursor: (0, 0),
            quit_pending: false,
        }
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
