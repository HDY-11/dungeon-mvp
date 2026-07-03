//! 主 UI 布局与状态面板渲染。

use bevy_ecs::prelude::World;
use dungeon_core::{world, 
    collect_renderables, Buffs, EntityName, Equipment, EventLog, FloorNumber, Inventory, Map,
    MapMemory, Player, Position, Renderable, Skills, Stats, TurnManager, Viewshed,
    MAP_HEIGHT, MAP_WIDTH, effective_attack, effective_defense,
};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use std::collections::HashSet;
use std::time::Instant;

use crate::color::renderable_color;
use crate::timeline::build_timeline;

/// 渲染完整 UI（地图 + 行动轴 + 状态面板）。
pub fn render_ui(frame: &mut Frame, game_start: Instant) {
    let mut w = world!(mut);
    let area = frame.area();
    let title = if w.resource::<TurnManager>().game_over {
        "  你死了  "
    } else {
        "  Dungeon MVP "
    };
    let block = Block::default()
        .title(title)
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    frame.render_widget(block, area);
    let inner = inner_rect(area, 1);
    if w.resource::<TurnManager>().game_over {
        frame.render_widget(
            Paragraph::new(Line::from("按 q 退出").centered())
                .style(Style::default().fg(Color::Red)),
            inner,
        );
        return;
    }

    let timeline_width: u16 = 22;
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(timeline_width),
            Constraint::Length(1),
            Constraint::Length(MAP_WIDTH as u16),
            Constraint::Length(1),
            Constraint::Min(1),
        ])
        .split(inner);
    let timeline_area = chunks[0];
    let map_area = chunks[2];
    let stats_area = Rect {
        x: chunks[4].x,
        y: chunks[4].y,
        width: chunks[4].width,
        height: inner.height,
    };

    let player_visible: HashSet<(usize, usize)> = {
        let mut q = w.query::<(&Player, &Viewshed)>();
        q.iter(&mut *w)
            .next()
            .map(|(_, v)| v.visible_tiles.iter().copied().collect())
            .unwrap_or_default()
    };
    let explored = w.resource::<MapMemory>().explored;
    let tiles = w.resource::<Map>().tiles;
    let renderables = collect_renderables();
    let (px, py) = {
        let mut q = w.query::<(&Player, &Position)>();
        q.iter(&mut *w)
            .next()
            .map(|(_, p)| (p.x, p.y))
            .unwrap_or((0, 0))
    };
    let monster_count = renderables.iter().filter(|(_, _, g, _)| *g != '@').count();
    let room_count = w.resource::<Map>().rooms.len();

    // 地图
    let mut lines: Vec<Vec<(char, Color)>> = Vec::with_capacity(MAP_HEIGHT);
    for y in 0..MAP_HEIGHT {
        let mut row = Vec::with_capacity(MAP_WIDTH);
        for x in 0..MAP_WIDTH {
            let pos = (x, y);
            let tile_ch = tiles[y][x].char();
            let (glyph, fg) = if player_visible.contains(&pos) {
                (tile_ch, Color::White)
            } else if explored[y][x] {
                (tile_ch, Color::DarkGray)
            } else {
                (' ', Color::DarkGray)
            };
            row.push((glyph, fg));
        }
        lines.push(row);
    }
    for &(ex, ey, glyph, (r, g, b)) in &renderables {
        if player_visible.contains(&(ex, ey)) && ey < MAP_HEIGHT && ex < MAP_WIDTH {
            lines[ey][ex] = (glyph, renderable_color((r, g, b)));
        }
    }
    let styled_lines: Vec<Line> = lines
        .into_iter()
        .map(|row| {
            Line::from(
                row.into_iter()
                    .map(|(g, c)| Span::styled(g.to_string(), Style::default().fg(c)))
                    .collect::<Vec<_>>(),
            )
        })
        .collect();
    frame.render_widget(
        Paragraph::new(styled_lines).style(Style::default().fg(Color::White)),
        map_area,
    );

    // 行动轴
    let timeline = build_timeline(player_visible);
    frame.render_widget(
        Paragraph::new(timeline)
            .style(Style::default().fg(Color::White))
            .block(
                Block::default()
                    .title(" 行动轴 ")
                    .borders(Borders::RIGHT)
                    .border_style(Style::default().fg(Color::DarkGray)),
            ),
        timeline_area,
    );

    // 状态面板
    let stats = build_stats_panel(px, py, room_count, monster_count, game_start);
    frame.render_widget(
        Paragraph::new(stats)
            .style(Style::default().fg(Color::White))
            .block(
                Block::default()
                    .borders(Borders::LEFT)
                    .border_style(Style::default().fg(Color::DarkGray)),
            ),
        stats_area,
    );
}

/// 状态面板。
pub fn build_stats_panel(
    px: usize,
    py: usize,
    room_count: usize,
    monster_count: usize,
    game_start: Instant,
) -> Vec<Line<'static>> {
    let mut w = world!(mut);
    let mut out: Vec<Line<'static>> = Vec::new();
    let stats: Option<Stats> = w
        .query::<(&Player, &Stats)>()
        .iter(&mut *w)
        .next()
        .map(|(_, s)| s.clone());
    let Some(ref s) = stats else {
        out.push(Line::from(Span::raw("(无数据)")));
        return out;
    };

    let hp_color = if s.hp as f32 <= s.max_hp as f32 * 0.3 {
        Color::Red
    } else {
        Color::Cyan
    };
    out.push(Line::from(vec![Span::styled(
        format!(" Lv.{}  Warrior ", s.level),
        Style::default().fg(hp_color).bold(),
    )]));
    out.push(Line::from(Span::raw(" ".repeat(22))));
    let bar_color = if s.hp as f32 <= s.max_hp as f32 * 0.3 {
        Color::Red
    } else {
        Color::Red
    };
    out.push(Line::from(vec![
        Span::styled(" HP ", Style::default().fg(Color::DarkGray)),
        Span::raw(format!("{:>3}/{:<3}", s.hp.max(0), s.max_hp)),
        Span::raw(" "),
        Span::styled(bar(s.hp.max(0), s.max_hp, 8), Style::default().fg(bar_color)),
    ]));
    out.push(Line::from(vec![
        Span::styled(" MP ", Style::default().fg(Color::DarkGray)),
        Span::raw(format!("{:>3}/{:<3}", s.mp, s.max_mp)),
        Span::raw(" "),
        Span::styled(bar(s.mp, s.max_mp, 8), Style::default().fg(Color::Blue)),
    ]));
    out.push(Line::from(vec![
        Span::styled(" EXP", Style::default().fg(Color::DarkGray)),
        Span::raw(format!(" {:>3}/{:<3}", s.exp, s.exp_to_next)),
        Span::raw(" "),
        Span::styled(
            bar(s.exp as i32, s.exp_to_next as i32, 8),
            Style::default().fg(Color::Yellow),
        ),
    ]));
    out.push(Line::from(Span::raw("")));

    let eff_atk = {
        let mut q = w.query::<(&Inventory, &Equipment, Option<&Buffs>)>();
        q.iter(&mut *w)
            .next()
            .map(|(inv, eq, bu)| effective_attack(s, inv, eq, bu))
            .unwrap_or(s.attack)
    };
    let eff_def = {
        let mut q = w.query::<(&Inventory, &Equipment, Option<&Buffs>)>();
        q.iter(&mut *w)
            .next()
            .map(|(inv, eq, bu)| effective_defense(s, inv, eq, bu))
            .unwrap_or(s.defense)
    };
    out.push(Line::from(vec![
        Span::styled(" 攻击", Style::default().fg(Color::DarkGray)),
        Span::raw(format!("{:>3}", eff_atk)),
        Span::raw("   "),
        Span::styled("法术精通", Style::default().fg(Color::DarkGray)),
        Span::raw(format!("{:>3}", s.magic_mastery)),
    ]));
    out.push(Line::from(vec![
        Span::styled(" 防御", Style::default().fg(Color::DarkGray)),
        Span::raw(format!("{:>3}", eff_def)),
        Span::raw("   "),
        Span::styled("敏捷", Style::default().fg(Color::DarkGray)),
        Span::raw(format!("{:>3}", s.agility)),
    ]));
    out.push(Line::from(Span::raw("")));
    out.push(Line::from(vec![
        Span::styled(" 暴击率", Style::default().fg(Color::DarkGray)),
        Span::raw(format!("{:>5.1}%", s.crit_rate * 100.0)),
        Span::raw(" "),
        Span::styled("暴击伤害", Style::default().fg(Color::DarkGray)),
        Span::raw(format!("{:>4.0}%", s.crit_damage * 100.0)),
    ]));
    out.push(Line::from(Span::raw("")));
    let floor = w.resource::<FloorNumber>().0;
    out.push(Line::from(Span::raw(format!(" 楼层 {}", floor))));
    out.push(Line::from(Span::raw(format!(" 房间 {}", room_count))));
    out.push(Line::from(Span::raw(format!("  @ ({}, {})", px, py))));
    out.push(Line::from(Span::raw(format!(" 怪物 {}", monster_count))));
    let elapsed = game_start.elapsed();
    let secs = elapsed.as_secs();
    let min = secs / 60;
    let s = secs % 60;
    out.push(Line::from(Span::styled(
        format!(" ⏱ {:>2}:{:02}", min, s),
        Style::default().fg(Color::DarkGray),
    )));
    out.push(Line::from(Span::raw("")));
    // 技能
    {
        let mut q = w.query::<(&Skills, &Stats)>();
        if let Some((sk, st)) = q.iter(&mut *w).next() {
            out.push(Line::from(Span::styled(
                "── 技能 ──",
                Style::default().fg(Color::DarkGray),
            )));
            for sk in &sk.list {
                let c = if st.mp >= sk.cost_mp {
                    Color::White
                } else {
                    Color::DarkGray
                };
                out.push(Line::from(vec![
                    Span::styled(format!(" {} ", sk.key), Style::default().fg(Color::Yellow)),
                    Span::styled(format!("{}({})", sk.name, sk.cost_mp), Style::default().fg(c)),
                ]));
            }
        }
    }
    // Buff
    if let Some(b) = w.query::<&Buffs>().iter(&mut *w).next() {
        let parts: Vec<String> = [b.shield_turns > 0, b.berserk_turns > 0]
            .iter()
            .enumerate()
            .filter(|&(_, v)| *v)
            .map(|(i, _)| {
                if i == 0 {
                    format!("🛡{}", b.shield_turns)
                } else {
                    format!("⚔{}", b.berserk_turns)
                }
            })
            .collect();
        if !parts.is_empty() {
            out.push(Line::from(Span::styled(
                format!("── Buff ── {}", parts.join(" ")),
                Style::default().fg(Color::Green),
            )));
        }
    }
    // 事件日志
    out.push(Line::from(Span::raw("")));
    let log = w.resource::<EventLog>();
    if !log.messages.is_empty() {
        out.push(Line::from(Span::styled(
            "── 事件 ──",
            Style::default().fg(Color::DarkGray),
        )));
        for msg in log.messages.iter().rev().take(5) {
            out.push(Line::from(Span::raw(format!(" {}", msg))));
        }
    }
    // 视野实体
    {
        let pv: HashSet<(usize, usize)> = w
            .query::<(&Player, &Viewshed)>()
            .iter(&mut *w)
            .next()
            .map(|(_, v)| v.visible_tiles.iter().copied().collect())
            .unwrap_or_default();
        let ents: Vec<(String, i32, i32, usize, usize, Color)> = w
            .query::<(&Position, &EntityName, &Stats, &Renderable)>()
            .iter(&mut *w)
            .filter(|(p, _, _, _)| pv.contains(&(p.x, p.y)))
            .filter(|(_, n, _, _)| n.0 != "冒险者")
            .map(|(p, n, st, r)| (n.0.clone(), st.hp, st.max_hp, p.x, p.y, renderable_color(r.color)))
            .collect();
        if !ents.is_empty() {
            // 统计同名实体
            let mut name_counts: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
            for (name, _, _, _, _, _) in &ents {
                *name_counts.entry(name.clone()).or_insert(0) += 1;
            }
            out.push(Line::from(Span::raw("")));
            out.push(Line::from(Span::styled(
                "── 视野 ──",
                Style::default().fg(Color::DarkGray),
            )));
            for (en, hp, mhp, ex, ey, color) in &ents {
                let hp_color = if *hp <= *mhp / 3 { Color::Red } else { Color::White };
                let label = if *name_counts.get(en).unwrap_or(&1) > 1 {
                    format!("{}({},{})", en, ex, ey)
                } else {
                    en.clone()
                };
                out.push(Line::from(vec![
                    Span::styled(label, Style::default().fg(*color)),
                    Span::raw(" "),
                    Span::styled(format!("({}/{})", (*hp).max(0), mhp), Style::default().fg(hp_color)),
                ]));
            }
        }
    }
    out
}

fn bar(current: i32, max: i32, width: usize) -> String {
    if max <= 0 {
        return "░".repeat(width);
    }
    let filled = ((current as f32 / max as f32) * width as f32).round() as usize;
    let filled = filled.min(width);
    format!("{}", "█".repeat(filled) + &"░".repeat(width - filled))
}

fn inner_rect(area: Rect, border: u16) -> Rect {
    Rect {
        x: area.x + border,
        y: area.y + border,
        width: area.width.saturating_sub(border * 2),
        height: area.height.saturating_sub(border * 2),
    }
}
