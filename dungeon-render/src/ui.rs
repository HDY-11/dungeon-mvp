use bevy_ecs::prelude::Entity;
use dungeon_core::{
    ActiveBuffs, EntityName, Equipment, EventLog, FloorNumber, Inventory, InventoryUI, LookCursor, Map, MapMemory, Player,
    Position, Skills, Stats, Tile, Viewshed,
    MAP_HEIGHT, MAP_WIDTH, VIEWPORT_WIDTH, VIEWPORT_HEIGHT,
    effective_attack, effective_defense,
};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use std::time::Instant;
use bevy_ecs::prelude::World;
use crate::pipeline;
use crate::timeline::build_timeline;

pub fn render_ui(frame: &mut Frame, game_start: Instant, world: &World) {
    let area = frame.area();
    let inner = inner_rect(area, 1);
    let scene = pipeline::extract_scene(world);

    let title = if scene.game_over { "  你死了  " } else { "  Dungeon MVP " };
    let block = Block::default()
        .title(title).title_alignment(Alignment::Center)
        .borders(Borders::ALL).border_style(Style::default().fg(Color::Cyan));
    frame.render_widget(block, area);
    if scene.game_over {
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
    let (timeline_area, map_events_area) = (chunks[0], chunks[2]);
    let stats_area = Rect { x: chunks[4].x, y: chunks[4].y, width: chunks[4].width, height: inner.height };

    let map_events_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(VIEWPORT_HEIGHT as u16),
            Constraint::Min(1),
        ])
        .split(map_events_area);
    let map_area = map_events_chunks[0];
    let events_area = map_events_chunks[1];

    // 管道 1：渲染地图格栅
    let (grid, _cam_x, _cam_y) = pipeline::render_map_grid(&scene, world);

    // 管道 2：写地图到 frame Buffer
    let buf = frame.buffer_mut();
    let map_x = map_area.x as usize;
    let map_y = map_area.y as usize;
    for (vy, row) in grid.iter().enumerate() {
        for (vx, &(g, fg, bg)) in row.iter().enumerate() {
            let cell = &mut buf[((map_x + vx) as u16, (map_y + vy) as u16)];
            cell.set_symbol(g.encode_utf8(&mut [0u8; 4]));
            cell.set_style(Style::default().fg(fg).bg(bg));
        }
    }

    // UI 层：事件日志
    let log = world.resource::<EventLog>();
    {
        let mut event_lines: Vec<Line> = Vec::new();
        event_lines.push(Line::from(Span::styled("── 事件 ──", Style::default().fg(Color::DarkGray))));
        for msg in log.messages.iter().rev().take(12) {
            event_lines.push(Line::from(Span::raw(format!(" {}", msg))));
        }
        frame.render_widget(
            Paragraph::new(event_lines).style(Style::default().fg(Color::White)),
            events_area,
        );
    }

    // UI 层：行动轴
    let timeline = build_timeline(scene.player_visible.clone(), world);
    frame.render_widget(
        Paragraph::new(timeline).style(Style::default().fg(Color::White))
            .block(Block::default().title(" 行动轴 ").borders(Borders::RIGHT)
                .border_style(Style::default().fg(Color::DarkGray))),
        timeline_area,
    );

    // UI 层：状态面板
    let stats = build_stats_panel(scene.px, scene.py, game_start, world);
    frame.render_widget(
        Paragraph::new(stats).style(Style::default().fg(Color::White))
            .block(Block::default().title(" 状态 ").borders(Borders::LEFT)
                .border_style(Style::default().fg(Color::DarkGray))),
        stats_area,
    );

    // 页栈：对话框叠加层
    if let Some(dungeon_action::Page::Dialog(title)) = world.get_resource::<dungeon_action::PageStack>()
        .and_then(|ps| ps.0.last())
    {
        let dialog = Paragraph::new(vec![
            Line::from(Span::styled(title.clone(), Style::default().fg(Color::Yellow).bold())),
            Line::from(Span::styled(" Y)是  N)否", Style::default().fg(Color::DarkGray))),
        ])
        .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::Yellow)))
        .alignment(Alignment::Center);
        let area = frame.area();
        frame.render_widget(dialog, Rect {
            x: area.width / 2 - 12, y: area.height / 2,
            width: 24, height: 5,
        });
    }
    // 页栈：投掷选择页
    if world.get_resource::<dungeon_action::PageStack>()
        .map(|ps| ps.0.last() == Some(&dungeon_action::Page::ThrowSelect))
        .unwrap_or(false)
    {
        let name = world.try_query::<(&Player, &Equipment)>()
            .and_then(|mut q| q.iter(world).next()
                .and_then(|(_, eq)| eq.off_hand.as_ref().map(|s| s.name())))
            .unwrap_or("(无投掷物)".into());
        let msg = Paragraph::new(vec![
            Line::from(Span::styled("选择投掷物", Style::default().fg(Color::Yellow).bold())),
            Line::from(Span::raw(format!(" 当前: {}", name))),
            Line::from(Span::styled(" Enter 确认  Esc 取消", Style::default().fg(Color::DarkGray))),
        ])
        .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::Cyan)))
        .alignment(Alignment::Center);
        let area = frame.area();
        frame.render_widget(msg, Rect {
            x: area.width / 2 - 16, y: area.height / 2,
            width: 32, height: 5,
        });
    }
    // 页栈：背包页面
    if world.get_resource::<dungeon_action::PageStack>()
        .map(|ps| ps.0.last() == Some(&dungeon_action::Page::Inventory))
        .unwrap_or(false)
    {
        let inv_state = world.resource::<InventoryUI>();
        let left_total = world.try_query::<(&Player, &Inventory)>()
            .map(|mut q| q.iter(world).next().map(|(_, inv)| inv.stacks.len()).unwrap_or(0))
            .unwrap_or(0);
        let ground_items = {
            let mut q = world.try_query::<(Entity, &Position, &dungeon_core::ItemPickup)>()
                .expect("ItemPickup+Pos reg");
            let px = world.try_query::<(&Player, &Position)>().expect("Player+Pos reg").iter(world)
                .next().map(|(_, p)| (p.x, p.y));
            px.map(|(px, py)| q.iter(world)
                .filter(|(_, p, _)| p.x == px && p.y == py)
                .map(|(e, _, ip)| (ip.stack.clone(), e))
                .collect::<Vec<_>>())
                .unwrap_or_default()
        };
        let mut lines: Vec<Line> = Vec::new();
        lines.push(Line::from(Span::styled(" 背包                             地面", Style::default().fg(Color::Yellow))));
        let max_rows = 18;
        for i in 0..max_rows {
            let left = if i < left_total {
                let inv = world.try_query::<(&Player, &Inventory)>()
                    .and_then(|mut q| q.iter(world).next().map(|(_, inv)| &inv.stacks));
                inv.map(|stacks| {
                    let s = &stacks[i];
                    let marker = if i == inv_state.left_sel && !inv_state.detail { "▸" } else { " " };
                    let equipped = if i >= 4 { " (装备)" } else { "" };
                    format!("{}{}{}", marker, s.name(), equipped)
                }).unwrap_or_default()
            } else { "".into() };
            let right = if i < ground_items.len() {
                let (stack, _) = &ground_items[i];
                let marker = if i == inv_state.right_sel && !inv_state.detail { "▸" } else { " " };
                format!("{}{} x{}", marker, stack.name(), stack.count)
            } else { "".into() };
            lines.push(Line::from(Span::raw(format!(" {:<20}  {:<20}", left, right))));
        }
        if inv_state.detail {
            lines.push(Line::from(Span::raw("")));
            lines.push(Line::from(Span::styled(" [e]装备  [d]丢弃  [Esc]返回", Style::default().fg(Color::DarkGray))));
        } else {
            lines.push(Line::from(Span::raw("")));
            lines.push(Line::from(Span::styled(" ← → 切换栏  ↑ ↓ 选择  Enter 查看详情  g 拾取  Esc 关闭", Style::default().fg(Color::DarkGray))));
        }
        let inv_paragraph = Paragraph::new(lines.clone())
            .block(Block::default().borders(Borders::ALL).title(" 背包 ").border_style(Style::default().fg(Color::Cyan)));
        frame.render_widget(inv_paragraph, frame.area());
    }
}






pub fn build_stats_panel
(px: usize, py: usize, game_start: Instant, world: &World) -> Vec<Line<'static>> {
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
        let mut q = world.try_query::<(&Inventory, &Equipment, Option<&ActiveBuffs>)>().expect("Inventory+Equipment+ActiveBuffs registered at init");
        q.iter(world).next().map(|(inv, eq, ab)| effective_attack(s, inv, eq, ab)).unwrap_or(s.attack)
    };
    let eff_def = {
        let mut q = world.try_query::<(&Inventory, &Equipment, Option<&ActiveBuffs>)>().expect("Inventory+Equipment+ActiveBuffs registered at init");
        q.iter(world).next().map(|(inv, eq, ab)| effective_defense(s, inv, eq, ab)).unwrap_or(s.defense)
    };
    let display_crit_rate = {
        let mut q = world.try_query::<(&Inventory, &Equipment)>().expect("Inventory+Equipment registered at init");
        q.iter(world).next().map(|(inv, eq)| {
            let bonus = dungeon_core::equipment_bonus(inv, eq);
            (s.crit_rate + bonus.crit_rate).min(1.0) * 100.0
        }).unwrap_or(s.crit_rate * 100.0)
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
        Span::styled(" 暴击率", Style::default().fg(Color::DarkGray)), Span::raw(format!("{:>5.1}%", display_crit_rate)), Span::raw(" "),
        Span::styled(" 暴击伤害", Style::default().fg(Color::DarkGray)), Span::raw(format!("{:>4.0}%", s.crit_damage * 100.0)),
    ]));
    out.push(Line::from(Span::raw("")));
    // 装备栏显示
    if let Some(equip) = world.try_query::<(&Player, &Equipment)>()
        .and_then(|mut q| q.iter(world).next().map(|(_, eq)| eq))
    {
        let mh = equip.main_hand.as_ref().map(|s| s.name()).unwrap_or("(空)".into());
        let oh = equip.off_hand.as_ref().map(|s| s.name()).unwrap_or("(空)".into());
        let ar = equip.armor.as_ref().map(|s| s.name()).unwrap_or("(空)".into());
        let rg = equip.ring.as_ref().map(|s| s.name()).unwrap_or("(空)".into());
        out.push(Line::from(Span::styled("── 装备 ──", Style::default().fg(Color::DarkGray))));
        out.push(Line::from(vec![
            Span::styled("主手:", Style::default().fg(Color::DarkGray)),
            Span::raw(mh[..mh.len().min(10)].to_string()),
        ]));
        out.push(Line::from(vec![
            Span::styled("副手:", Style::default().fg(Color::DarkGray)),
            Span::raw(oh[..oh.len().min(10)].to_string()),
        ]));
        out.push(Line::from(vec![
            Span::styled("防具:", Style::default().fg(Color::DarkGray)),
            Span::raw(ar[..ar.len().min(10)].to_string()),
            Span::raw("   "),
            Span::styled("戒指:", Style::default().fg(Color::DarkGray)),
            Span::raw(rg[..rg.len().min(10)].to_string()),
        ]));
    }
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
    // ── 光标查看信息 ──
    if let Some(cursor) = world.get_resource::<LookCursor>()
    && cursor.active {
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
            if pv.contains(&(cx, cy)) {
                // 可见格：显示完整信息（地形+实体+HP）
                let tile = map.tiles[cy][cx];
                let tile_name = match tile {
                    Tile::Wall => "墙壁", Tile::Floor => "地板",
                    Tile::ShallowWater => "浅水", Tile::DeepWater => "深水",
                    Tile::Stalactite => "钟乳石",
                };
                out.push(Line::from(Span::styled(format!("  {}", tile_name), Style::default().fg(Color::DarkGray))));
                let cursor_entities: Vec<(String, Entity)> = {
                    let mut eq = world.try_query::<(Entity, &Position, &EntityName)>().expect("Entity+Pos+Name reg");
                    eq.iter(world).filter(|(_, p, _)| p.x == cx && p.y == cy).map(|(e, _, n)| (n.0.clone(), e)).collect()
                };
                for (name, e) in &cursor_entities {
                    let hp = world.get::<Stats>(*e).map(|s| format!(" ({}/{})", s.hp.max(0), s.max_hp)).unwrap_or_default();
                    out.push(Line::from(Span::styled(format!("  {}{}", name, hp), Style::default().fg(Color::White))));
                }
            } else if explored[cy][cx] {
                // 已探索但当前不可见：只显示地形名，不显示实体
                let tile = map.tiles[cy][cx];
                let tile_name = match tile {
                    Tile::Wall => "墙壁", Tile::Floor => "地板",
                    Tile::ShallowWater => "浅水", Tile::DeepWater => "深水",
                    Tile::Stalactite => "钟乳石",
                };
                out.push(Line::from(Span::styled(format!("  {} (已探索)", tile_name), Style::default().fg(Color::DarkGray))));
            } else {
                out.push(Line::from(Span::styled("  (未探索)", Style::default().fg(Color::DarkGray))));
            }
        }
    }
    out
}

fn bar(current: i32, max: i32, width: usize) -> String {
    if max <= 0 { return "░".repeat(width); }
    let filled = ((current as f32 / max as f32) * width as f32).round() as usize;
    "█".repeat(filled.min(width)) + &"░".repeat(width - filled.min(width))
}

fn inner_rect(area: Rect, border: u16) -> Rect {
    Rect { x: area.x + border, y: area.y + border, width: area.width.saturating_sub(border * 2), height: area.height.saturating_sub(border * 2) }
}
