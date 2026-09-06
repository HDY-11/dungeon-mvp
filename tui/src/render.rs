//! 最小游戏画面渲染。

use crate::state::DevLogBuffer;
use crate::scene::{
    EntityView, Scene, VIEW_HEIGHT, VIEW_WIDTH, extract_scene, tile_bg, tile_color, tile_glyph,
};
use bevy_ecs::prelude::World;
use core::{MAP_HEIGHT, MAP_WIDTH};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};
use utils::Rgb;

pub fn render_game(frame: &mut Frame, world: &mut World) {
    let scene = extract_scene(world);
    let area = frame.area();

    let [main_area, debug_area] = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(8),
    ])
    .areas(area);

    let [map_area, side_area] = Layout::horizontal([
        Constraint::Length((VIEW_WIDTH + 2) as u16),
        Constraint::Min(24),
    ])
    .areas(main_area);

    render_map(frame, map_area, &scene);
    render_side(frame, side_area, &scene);
    render_debug_panel(frame, debug_area, world);
}

fn rgb_color(rgb: Rgb) -> Color {
    Color::Rgb(rgb.0, rgb.1, rgb.2)
}

fn render_map(frame: &mut Frame, area: Rect, scene: &Scene) {
    let (px, py) = scene
        .player
        .as_ref()
        .map(|p| (p.x, p.y))
        .unwrap_or((MAP_WIDTH / 2, MAP_HEIGHT / 2));
    let cam_x = px.saturating_sub(VIEW_WIDTH / 2).min(MAP_WIDTH - VIEW_WIDTH);
    let cam_y = py.saturating_sub(VIEW_HEIGHT / 2).min(MAP_HEIGHT - VIEW_HEIGHT);

    let mut lines = Vec::with_capacity(VIEW_HEIGHT);
    for vy in 0..VIEW_HEIGHT {
        let my = cam_y + vy;
        let mut spans = Vec::with_capacity(VIEW_WIDTH);
        for vx in 0..VIEW_WIDTH {
            let mx = cam_x + vx;
            let pos = (mx, my);
            let tile = scene.tiles[my][mx];

            let entity = if scene.player.as_ref().is_some_and(|p| (p.x, p.y) == pos) {
                scene.player.as_ref()
            } else {
                scene.entities.iter().find(|e| (e.x, e.y) == pos)
            };

            if let Some(entity) = entity {
                spans.push(Span::styled(
                    entity.glyph.to_string(),
                    Style::default()
                        .fg(rgb_color(entity.color))
                        .bg(tile_bg(tile).map(rgb_color).unwrap_or(Color::Reset)),
                ));
            } else if scene.visible.contains(&pos) {
                spans.push(Span::styled(
                    tile_glyph(tile).to_string(),
                    Style::default()
                        .fg(rgb_color(tile_color(tile)))
                        .bg(tile_bg(tile).map(rgb_color).unwrap_or(Color::Reset)),
                ));
            } else if scene.explored[my][mx] {
                let fg = tile_color(tile).dim(0.55);
                let bg = tile_bg(tile).map(|c| c.dim(0.7));
                spans.push(Span::styled(
                    tile_glyph(tile).to_string(),
                    Style::default()
                        .fg(rgb_color(fg))
                        .bg(bg.map(rgb_color).unwrap_or(Color::Reset)),
                ));
            } else {
                spans.push(Span::styled(" ", Style::default().bg(Color::Reset)));
            }
        }
        lines.push(Line::from(spans));
    }

    let title = if scene.game_over {
        " 游戏结束 "
    } else {
        " Dungeon MVP (new core) "
    };
    let block = Block::default()
        .title(title)
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL);
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn render_side(frame: &mut Frame, area: Rect, scene: &Scene) {
    let mut lines = Vec::new();

    lines.push(Line::from(format!("楼层：{}", scene.floor)));
    lines.push(Line::from(format!("地图类型：{:?}", scene.map_kind)));

    if let Some(player) = &scene.player {
        lines.push(Line::from(Span::styled(
            player.name.clone(),
            Style::default().fg(Color::Yellow),
        )));
        lines.push(Line::from(format!(
            "HP：{:.0} / {:.0}",
            player.hp, player.max_hp
        )));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "可见实体",
        Style::default().fg(Color::Cyan),
    )));

    for entity in scene
        .entities
        .iter()
        .filter(|e| scene.visible.contains(&(e.x, e.y)))
    {
        let hp = if entity.max_hp > 0.0 {
            format!("{:.0}/{:.0}", entity.hp, entity.max_hp)
        } else {
            String::new()
        };
        lines.push(Line::from(format!(
            "{} {} {}",
            entity.glyph, entity.name, hp
        )));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "日志",
        Style::default().fg(Color::Cyan),
    )));
    for text in &scene.log {
        lines.push(Line::from(text.clone()));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "方向键/Home/End/PgUp/PgDn：移动或攻击",
        Style::default().fg(Color::DarkGray),
    )));
    lines.push(Line::from(Span::styled(
        ".：等待   q/Esc：退出",
        Style::default().fg(Color::DarkGray),
    )));

    if scene.game_over {
        lines.push(Line::from(Span::styled(
            "你死了。按 q 退出。",
            Style::default().fg(Color::Red),
        )));
    }

    let block = Block::default().borders(Borders::ALL).title("状态");
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn render_debug_panel(frame: &mut Frame, area: Rect, world: &World) {
    let lines = world
        .get_resource::<DevLogBuffer>()
        .map(|buffer| {
            buffer
                .lines
                .iter()
                .rev()
                .take(area.height.saturating_sub(2) as usize)
                .rev()
                .map(|entry| {
                    let level_color = match entry.level.as_str() {
                        "ERROR" => Color::Red,
                        "WARN" => Color::Yellow,
                        "INFO" => Color::Cyan,
                        _ => Color::DarkGray,
                    };
                    Line::from(vec![
                        Span::styled(
                            format!("[{}] ", entry.level),
                            Style::default().fg(level_color),
                        ),
                        Span::styled(
                            format!("{}: ", entry.target),
                            Style::default().fg(Color::DarkGray),
                        ),
                        Span::raw(entry.message.clone()),
                    ])
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let block = Block::default().borders(Borders::ALL).title("Debug");
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

#[allow(dead_code)]
fn render_entity_text(entity: &EntityView) -> String {
    format!("{} {}", entity.glyph, entity.name)
}
