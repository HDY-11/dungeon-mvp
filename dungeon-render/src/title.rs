//! 标题画面与升级画面的绘制。

use ratatui::{
    layout::{Alignment, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

/// 绘制标题画面。
pub fn draw_title(frame: &mut Frame) {
    let area = frame.area();
    let block = Block::default()
        .title("  Dungeon MVP  ")
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    frame.render_widget(block, area);
    let inner = Rect {
        x: area.x + 1,
        y: area.y + 1,
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    };
    let msg = Paragraph::new(
        Line::from(vec![
            Span::styled("按 Enter 开始 ", Style::default().fg(Color::Yellow)),
            Span::styled("[F9读档]", Style::default().fg(Color::DarkGray)),
        ])
        .centered(),
    );
    frame.render_widget(msg, inner);
}


