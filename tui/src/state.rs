//! UI 状态资源。
//!
//! 这些状态属于渲染/交互层，不应进入业务 core。

use bevy_ecs::prelude::*;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogKind {
    Quit,
    Descend,
}

impl DialogKind {
    pub const fn title(self) -> &'static str {
        match self {
            DialogKind::Quit => "确认退出？",
            DialogKind::Descend => "确认下楼？",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Page {
    Game,
    Look,
    ThrowSelect,
    ThrowAim,
    Inventory,
    Dialog(DialogKind),
}

#[derive(Resource, Default)]
pub struct PageStack(pub Vec<Page>);

impl PageStack {
    pub fn push(&mut self, page: Page) {
        self.0.push(page);
    }

    pub fn pop(&mut self) -> Option<Page> {
        self.0.pop()
    }

    pub fn current(&self) -> &Page {
        self.0.last().unwrap_or(&Page::Game)
    }
}

/// 投掷瞄准预览。
#[derive(Resource, Default)]
pub struct ThrowPreview {
    pub active: bool,
    pub cursor: (usize, usize),
    pub path: Vec<(usize, usize)>,
    pub valid_target: bool,
}

/// 查看模式光标。
#[derive(Resource)]
pub struct LookCursor {
    pub active: bool,
    pub x: usize,
    pub y: usize,
}

/// 背包页 UI 状态。
#[derive(Resource, Default)]
pub struct InventoryUI {
    pub active: bool,
    pub panel: bool,
    pub left_sel: usize,
    pub right_sel: usize,
    pub detail: bool,
    pub detail_source: usize,
    pub detail_idx: usize,
}

/// 开发者日志面板数据。由应用层从 `sys::LogRecord` 转换后写入。
#[derive(Debug, Clone)]
pub struct DevLogLine {
    pub level: String,
    pub target: String,
    pub message: String,
}

#[derive(Resource, Default)]
pub struct DevLogBuffer {
    pub lines: std::collections::VecDeque<DevLogLine>,
    pub max: usize,
}

impl DevLogBuffer {
    pub fn new(max: usize) -> Self {
        Self {
            lines: std::collections::VecDeque::new(),
            max,
        }
    }

    pub fn push(&mut self, level: String, target: String, message: String) {
        self.lines.push_back(DevLogLine {
            level,
            target,
            message,
        });
        while self.lines.len() > self.max {
            self.lines.pop_front();
        }
    }
}

/// 由渲染层记录的实体最后可见位置。
#[derive(Resource, Default)]
pub struct RenderMemory {
    pub entries: HashMap<bevy_ecs::entity::Entity, (usize, usize)>,
}
