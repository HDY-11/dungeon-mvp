use bevy_ecs::prelude::Entity;
use dungeon_core::{
    ActiveBuffs, Buffs, EntityName, Equipment, EventLog, FloorNumber, Inventory, LookCursor, Map, MapMemory, Player,
    Position, Skills, Stats, Tile, TurnManager, Viewshed, VisibleMemory,
    MAP_HEIGHT, MAP_WIDTH, VIEWPORT_WIDTH, VIEWPORT_HEIGHT,
    effective_attack, effective_defense, collect_renderables,
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
use bevy_ecs::prelude::World;
use crate::color::renderable_color;
use crate::timeline::build_timeline;

pub fn render_ui(frame: &mut Frame, game_start: Instant, world: &World) {
    let area = frame.area();
    let inner = inner_rect(area, 1);

    let (game_over, player_visible, tiles, explored, px, py, visible_mem) = {
        let go = world.resource::<TurnManager>().game_over;
        let pv: HashSet<(usize, usize)> = {
            let mut q = world.try_query::<(&Player, &Viewshed)>().expect("Player+Viewshed registered at init");
            q.iter(world).next()
                .map(|(_, v)| v.visible_tiles.iter().copied().collect())
                .unwrap_or_default()
        };
        let ts = world.resource::<Map>().tiles;
        let ex = world.resource::<MapMemory>().explored;
        let pp = world.try_query::<(&Player, &Position)>().expect("Player+Position registered at init").iter(world)
            .next().map(|(_, p)| (p.x, p.y)).unwrap_or((0, 0));
        let vm: Vec<(usize, usize, char, (u8, u8, u8))> = world.resource::<VisibleMemory>().entries.values().copied().collect();
        (go, pv, ts, ex, pp.0, pp.1, vm)
    };

    let title = if game_over { "  你死了  " } else { "  Dungeon MVP " };
    let block = Block::default()
        .title(title).title_alignment(Alignment::Center)
        .borders(Borders::ALL).border_style(Style::default().fg(Color::Cyan));
    frame.render_widget(block, area);
    if game_over {
        frame.render_widget(Paragraph::new(Line::from("按 q 退出").centered())
            .style(Style::default().fg(Color::Red)), inner);
        return;
    }

    let timeline_width: u16 = 26;
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(timeline_width), Constraint::Length(1),
            Constraint::Length(VIEWPORT_WIDTH as u16), Constraint::Length(1),
            Constraint::Min(1),
        ])
        .split(inner);
    let (timeline_area, map_area, stats_area) = (chunks[0], chunks[2],
        Rect { x: chunks[4].x, y: chunks[4].y, width: chunks[4].width, height: inner.height });

    let vw = VIEWPORT_WIDTH;
    let vh = VIEWPORT_HEIGHT;
    // 摄像机偏移：以玩家为中心，钳制到地图边界
    let cam_x = (px.saturating_sub(vw / 2)).min(MAP_WIDTH.saturating_sub(vw));
    let cam_y = (py.saturating_sub(vh / 2)).min(MAP_HEIGHT.saturating_sub(vh));

    let renderables = collect_renderables(world);

    /// 将颜色降饱和+变暗（灰色滤镜）
    fn dim(c: u8, factor: f32) -> u8 {
        (c as f32 * (1.0 - factor) + 96.0 * factor) as u8
    }
    fn dim_tile(tile: dungeon_core::Tile) -> (Color, Color) {
        let (r, g, b) = tile.fg_color();
        let fg = Color::Rgb(dim(r, 0.55), dim(g, 0.55), dim(b, 0.55));
        let bg = tile.bg_color().map(|(r, g, b)|
            Color::Rgb(dim(r, 0.7), dim(g, 0.7), dim(b, 0.7))
        ).unwrap_or(Color::Reset);
        (fg, bg)
    }

    // (glyph, fg, bg)
    let mut lines: Vec<Vec<(char, Color, Color)>> = Vec::with_capacity(vh);
    for vy in 0..vh {
        let my = cam_y + vy;
        let mut row = Vec::with_capacity(vw);
        for vx in 0..vw {
            let mx = cam_x + vx;
            let pos = (mx, my);
            let tile = tiles[my][mx];
            if player_visible.contains(&pos) {
                let (r, g, b) = tile.fg_color();
                let fg = Color::Rgb(r, g, b);
                let bg = tile.bg_color().map(|(r, g, b)| Color::Rgb(r, g, b)).unwrap_or(Color::Reset);
                row.push((tile.glyph(), fg, bg));
            } else if explored[my][mx] {
                let (fg, bg) = dim_tile(tile);
                row.push((tile.glyph(), fg, bg));
            } else {
                row.push((' ', Color::DarkGray, Color::Reset));
            }
        }
        lines.push(row);
    }
    for &(ex, ey, glyph, (r, g, b)) in &renderables {
        if ey >= cam_y && ey < cam_y + vh
            && ex >= cam_x && ex < cam_x + vw
            && player_visible.contains(&(ex, ey))
        {
            let (idx, jdx) = (ey - cam_y, ex - cam_x);
            let bg = lines[idx][jdx].2;
            lines[idx][jdx] = (glyph, renderable_color((r, g, b)), bg);
        }
    }
    for &(mx, my, glyph, _) in &visible_mem {
        if !player_visible.contains(&(mx, my)) && explored[my][mx]
            && my >= cam_y && my < cam_y + vh
            && mx >= cam_x && mx < cam_x + vw
        {
            let (idx, jdx) = (my - cam_y, mx - cam_x);
            lines[idx][jdx] = (glyph, Color::Rgb(dim(160, 0.5), dim(160, 0.5), dim(160, 0.5)), lines[idx][jdx].2);
        }
    }
    let styled_lines: Vec<Line> = lines.into_iter()
        .map(|row| Line::from(row.into_iter()
            .map(|(g, fg, bg)| Span::styled(g.to_string(), Style::default().fg(fg).bg(bg)))
            .collect::<Vec<_>>()))
        .collect();
    frame.render_widget(Paragraph::new(styled_lines).style(Style::default().fg(Color::White)), map_area);

    let timeline = build_timeline(player_visible.clone(), world);
    frame.render_widget(
        Paragraph::new(timeline).style(Style::default().fg(Color::White))
            .block(Block::default().title(" 行动轴 ").borders(Borders::RIGHT)
                .border_style(Style::default().fg(Color::DarkGray))),
        timeline_area,
    );

    let stats = build_stats_panel(px, py, game_start, world);
    frame.render_widget(
        Paragraph::new(stats).style(Style::default().fg(Color::White))
            .block(Block::default().borders(Borders::LEFT)
                .border_style(Style::default().fg(Color::DarkGray))),
        stats_area,
    );
}

pub fn build_stats_panel(px: usize, py: usize, game_start: Instant, world: &World) -> Vec<Line<'static>> {
    let mut out: Vec<Line<'static>> = Vec::new();
    let stats: Option<Stats> = world.try_query::<(&Player, &Stats)>().expect("Player+Stats registered at init").iter(world).next().map(|(_, s)| s.clone());
    let Some(ref s) = stats else {
        out.push(Line::from(Span::raw("(无数据)"))); return out;
    };
    let hp_color = if s.hp as f32 <= s.max_hp as f32 * 0.3 { Color::Red } else { Color::Cyan };
    out.push(Line::from(vec![Span::styled(format!(" Lv.{}  Warrior ", s.level), Style::default().fg(hp_color).bold())]));
    out.push(Line::from(Span::raw(" ".repeat(22))));
    out.push(Line::from(vec![
        Span::styled(" HP ", Style::default().fg(Color::DarkGray)),
        Span::raw(format!("{:>3}/{:<3}", s.hp.max(0), s.max_hp)), Span::raw(" "),
        Span::styled(bar(s.hp.max(0), s.max_hp, 8), Style::default().fg(Color::Red)),
    ]));
    out.push(Line::from(vec![
        Span::styled(" MP ", Style::default().fg(Color::DarkGray)),
        Span::raw(format!("{:>3}/{:<3}", s.mp, s.max_mp)), Span::raw(" "),
        Span::styled(bar(s.mp, s.max_mp, 8), Style::default().fg(Color::Blue)),
    ]));
    out.push(Line::from(vec![
        Span::styled(" EXP", Style::default().fg(Color::DarkGray)),
        Span::raw(format!(" {:>3}/{:<3}", s.exp, s.exp_to_next)), Span::raw(" "),
        Span::styled(bar(s.exp as i32, s.exp_to_next as i32, 8), Style::default().fg(Color::Yellow)),
    ]));
    out.push(Line::from(Span::raw("")));
    let eff_atk = {
        let mut q = world.try_query::<(&Inventory, &Equipment, Option<&Buffs>, Option<&ActiveBuffs>)>().expect("Inventory+Equipment+Buffs+ActiveBuffs registered at init");
        q.iter(world).next().map(|(inv, eq, bu, ab)| effective_attack(s, inv, eq, bu, ab)).unwrap_or(s.attack)
    };
    let eff_def = {
        let mut q = world.try_query::<(&Inventory, &Equipment, Option<&Buffs>, Option<&ActiveBuffs>)>().expect("Inventory+Equipment+Buffs+ActiveBuffs registered at init");
        q.iter(world).next().map(|(inv, eq, bu, ab)| effective_defense(s, inv, eq, bu, ab)).unwrap_or(s.defense)
    };
    out.push(Line::from(vec![
        Span::styled(" 攻击", Style::default().fg(Color::DarkGray)), Span::raw(format!("{:>3}", eff_atk)), Span::raw("   "),
        Span::styled("法术精通", Style::default().fg(Color::DarkGray)), Span::raw(format!("{:>3}", s.magic_mastery)),
    ]));
    out.push(Line::from(vec![
        Span::styled(" 防御", Style::default().fg(Color::DarkGray)), Span::raw(format!("{:>3}", eff_def)), Span::raw("   "),
        Span::styled("敏捷", Style::default().fg(Color::DarkGray)), Span::raw(format!("{:>3}", s.agility)),
    ]));
    out.push(Line::from(Span::raw("")));
    out.push(Line::from(vec![
        Span::styled(" 暴击率", Style::default().fg(Color::DarkGray)), Span::raw(format!("{:>5.1}%", s.crit_rate * 100.0)), Span::raw(" "),
        Span::styled("暴击伤害", Style::default().fg(Color::DarkGray)), Span::raw(format!("{:>4.0}%", s.crit_damage * 100.0)),
    ]));
    out.push(Line::from(Span::raw("")));
    let floor = world.resource::<FloorNumber>().0;
    out.push(Line::from(Span::raw(format!(" 楼层 {}", floor))));
    out.push(Line::from(Span::raw(format!("  @ ({}, {})", px, py))));
    let elapsed = game_start.elapsed();
    out.push(Line::from(Span::styled(format!(" ⏱ {:>2}:{:02}", elapsed.as_secs() / 60, elapsed.as_secs() % 60), Style::default().fg(Color::DarkGray))));
    out.push(Line::from(Span::raw("")));
    {
        let mut q = world.try_query::<(&Skills, &Stats)>().expect("Skills+Stats registered at init");
        if let Some((sk, st)) = q.iter(world).next() {
            out.push(Line::from(Span::styled("── 技能 ──", Style::default().fg(Color::DarkGray))));
            for sk in &sk.list {
                let c = if st.mp >= sk.cost_mp { Color::White } else { Color::DarkGray };
                out.push(Line::from(vec![
                    Span::styled(format!(" {} ", sk.key), Style::default().fg(Color::Yellow)),
                    Span::styled(format!("{}({})", sk.name, sk.cost_mp), Style::default().fg(c)),
                ]));
            }
        }
    }
    out.push(Line::from(Span::raw("")));
    let log = world.resource::<EventLog>();
    if !log.messages.is_empty() {
        out.push(Line::from(Span::styled("── 事件 ──", Style::default().fg(Color::DarkGray))));
        for msg in log.messages.iter().rev().take(12) { out.push(Line::from(Span::raw(format!(" {}", msg)))); }
    }

    // ── 光标查看信息 ──
    if let Some(cursor) = world.get_resource::<LookCursor>() {
    if cursor.active {
        let (cx, cy) = (cursor.x, cursor.y);
        let explored = world.resource::<MapMemory>().explored;
        let map = world.resource::<Map>();
        let pv: std::collections::HashSet<(usize, usize)> = world.try_query::<(&Player, &Viewshed)>()
            .expect("Player+Viewshed reg").iter(world)
            .next().map(|(_, v)| v.visible_tiles.iter().copied().collect())
            .unwrap_or_default();
        out.push(Line::from(Span::raw("")));
        out.push(Line::from(Span::styled(format!(" x 光标 ({}, {})", cx, cy), Style::default().fg(Color::Yellow))));
        if cx < MAP_WIDTH && cy < MAP_HEIGHT {
            if explored[cy][cx] || pv.contains(&(cx, cy)) {
                let tile = map.tiles[cy][cx];
                let tile_name = match tile {
                    Tile::Wall => "墙壁", Tile::Floor => "地板",
                    Tile::ShallowWater => "浅水", Tile::DeepWater => "深水",
                    Tile::Stalactite => "钟乳石",
                };
                out.push(Line::from(Span::styled(format!("  {}", tile_name), Style::default().fg(Color::DarkGray))));
                // 查找光标位置的实体
                let cursor_entities: Vec<(String, Entity)> = {
                    let mut eq = world.try_query::<(Entity, &Position, &EntityName)>().expect("Entity+Pos+Name reg");
                    eq.iter(world).filter(|(_, p, _)| p.x == cx && p.y == cy).map(|(e, _, n)| (n.0.clone(), e)).collect()
                };
                for (name, e) in &cursor_entities {
                    let hp = world.get::<Stats>(*e).map(|s| format!(" ({}/{})", s.hp.max(0), s.max_hp)).unwrap_or_default();
                    out.push(Line::from(Span::styled(format!("  {}{}", name, hp), Style::default().fg(Color::White))));
                }
            } else {
                out.push(Line::from(Span::styled("  (未探索)", Style::default().fg(Color::DarkGray))));
            }
        }
    }
    }
    out
}

fn bar(current: i32, max: i32, width: usize) -> String {
    if max <= 0 { return "░".repeat(width); }
    let filled = ((current as f32 / max as f32) * width as f32).round() as usize;
    format!("{}", "█".repeat(filled.min(width)) + &"░".repeat(width - filled.min(width)))
}

fn inner_rect(area: Rect, border: u16) -> Rect {
    Rect { x: area.x + border, y: area.y + border, width: area.width.saturating_sub(border * 2), height: area.height.saturating_sub(border * 2) }
}
