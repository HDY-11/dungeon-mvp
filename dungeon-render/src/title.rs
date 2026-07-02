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

/// 绘制升级画面。
pub fn draw_level_up(frame: &mut Frame, remaining_points: u32) {
    let area = frame.area();
    let block = Block::default()
        .title("  升级！  ")
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));
    frame.render_widget(block, area);
    let inner = Rect {
        x: area.x + 1,
        y: area.y + 1,
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    };
    let lines = vec![
        Line::from(Span::styled(
            format!("剩余属性点: {}", remaining_points),
            Style::default().fg(Color::Cyan),
        )),
        Line::from(Span::styled(" 1: 攻击+1", Style::default().fg(Color::White))),
        Line::from(Span::styled(" 2: 防御+1", Style::default().fg(Color::White))),
        Line::from(Span::styled(" 3: 法术精通+1", Style::default().fg(Color::White))),
        Line::from(Span::styled(" 4: 敏捷+1", Style::default().fg(Color::White))),
        Line::from(Span::styled(" 5: HP+5", Style::default().fg(Color::White))),
        Line::from(Span::styled(" 0: 完成", Style::default().fg(Color::DarkGray))),
    ];
    frame.render_widget(Paragraph::new(lines), inner);
}

