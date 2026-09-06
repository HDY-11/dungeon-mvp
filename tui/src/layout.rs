//! 通用 UI 布局工具。

use ratatui::layout::Rect;
use utils::{truncate_chars_owned, window_start};

pub fn inner_rect(area: Rect, border: u16) -> Rect {
    Rect {
        x: area.x.saturating_add(border),
        y: area.y.saturating_add(border),
        width: area.width.saturating_sub(border.saturating_mul(2)),
        height: area.height.saturating_sub(border.saturating_mul(2)),
    }
}

pub fn bar(current: i64, max: i64, width: usize) -> String {
    let ratio = if max <= 0 {
        0.0
    } else {
        current as f64 / max as f64
    }
    .clamp(0.0, 1.0);
    let filled = (ratio * width as f64).round() as usize;
    format!("{}{}", "█".repeat(filled.min(width)), "░".repeat(width.saturating_sub(filled)))
}

pub fn truncate_name(s: &str, max_chars: usize) -> String {
    truncate_chars_owned(s, max_chars)
}

pub fn backpack_window_start(sel: usize, len: usize, win: usize) -> usize {
    window_start(sel, len, win)
}
