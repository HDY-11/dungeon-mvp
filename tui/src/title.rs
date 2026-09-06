//! 标题画面。

use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

pub fn draw_title(frame: &mut Frame, title: &str, prompt: &str) {
    let area = frame.area();
    let block = Block::default()
        .title(title)
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
            Span::styled(prompt, Style::default().fg(Color::Yellow)),
            Span::styled("[F9读档]", Style::default().fg(Color::DarkGray)),
        ])
        .centered(),
    );
    frame.render_widget(msg, inner);
}
