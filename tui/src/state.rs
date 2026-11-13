//! 后端自己的 UI 资源：**只剩开发者日志缓冲**。
//!
//! 页栈/查看光标/背包状态/投掷预览等都已经迁到 `presentation::ui::PageStack`
//! （契约见 `render-api`）；这里曾经重复定义过一套，前端接缝（DsnX19）之后
//! 全部删除，避免出现两份 UI 状态。

use bevy_ecs::prelude::Resource;

/// 开发者日志面板的一行。
#[derive(Debug, Clone)]
pub struct DevLogLine {
    pub level: String,
    pub target: String,
    pub message: String,
}

/// 后端自己的调试日志缓冲（由装配层从 `sys` 的日志通道灌入）。
///
/// 派生 `Clone` 是为了让装配层每帧交给后端**一份快照**而不是借用：
/// `App::refresh(&mut self)` 与取日志的 `&self` 借用无法共存，而这点数据
/// （最多几十行字符串）的拷贝代价远小于为此拆分状态。
#[derive(Resource, Default, Clone)]
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
