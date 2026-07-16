//! 投掷模式：投掷物选择 + 瞄准 + 弹道可视化

use std::io;
use std::time::Instant;

use bevy_ecs::prelude::*;
use crossterm::event::{self, Event, KeyCode};
use dungeon_core::{
    ops, EventLog, Position, Stats,
    ThrowPreview, Inventory,
    Player,
    MAP_HEIGHT, MAP_WIDTH,
};
use dungeon_action::{ActionQueue, ActionKindV3, agility_speed_factor};
use dungeon_render::render_ui;
use ratatui::{
    layout::{Alignment, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Terminal,
};

/// 投掷物选择弹窗。遍历背包中所有 tag 含 "throwable" 的物品，
/// 列出供玩家选择，然后进入瞄准模式。
pub fn open_throw_select(
    terminal: &mut Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
    world: &mut World,
    game_start: Instant,
) -> io::Result<bool> {
    // 收集背包中所有可投掷物
    let throwable_ids: Vec<(usize, String, u32)> = {
        let mut q = world.query::<(&Inventory,)>();
        let inv = q.iter(world).next().map(|(inv,)| inv);
        match inv {
            Some(inv) => inv.stacks.iter()
                .filter(|s| {
                    s.def().map(|d| d.has_tag("throwable")).unwrap_or(false)
                })
                .map(|s| (s.item_id, s.name(), s.count))
                .collect(),
            None => Vec::new(),
        }
    };

    if throwable_ids.is_empty() {
        world.resource_mut::<EventLog>().push("无可投掷物".to_string());
        return Ok(false);
    }

    let mut selected: usize = 0;
    loop {
        // 渲染选择弹窗
        terminal.draw(|frame| {
            let area = frame.area();

            // 先渲染正常画面作为背景
            render_ui(frame, game_start, &*world);

            // 再覆盖弹窗
            let popup_w = 30u16.min(area.width.saturating_sub(4));
            let popup_h = (throwable_ids.len() as u16 + 4).min(area.height.saturating_sub(4));
            let popup_rect = Rect {
                x: (area.width.saturating_sub(popup_w)) / 2,
                y: (area.height.saturating_sub(popup_h)) / 2,
                width: popup_w,
                height: popup_h,
            };

            let mut lines = vec![
                Line::from(Span::styled(" 选择投掷物 ", Style::default().fg(Color::Cyan).bold())),
                Line::from(Span::raw("")),
            ];
            for (i, (_id, name, count)) in throwable_ids.iter().enumerate() {
                let prefix = if i == selected { " ▸" } else { "  " };
                let style = if i == selected {
                    Style::default().fg(Color::Yellow).bold()
                } else {
                    Style::default().fg(Color::White)
                };
                lines.push(Line::from(vec![
                    Span::styled(prefix, Style::default().fg(Color::Yellow)),
                    Span::styled(format!(" {} x{}", name, count), style),
                ]));
            }
            lines.push(Line::from(Span::raw("")));
            lines.push(Line::from(Span::styled(" r/y:切换  Enter:投掷  Esc:取消", Style::default().fg(Color::DarkGray))));

            let block = Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan));
            frame.render_widget(
                Paragraph::new(lines).block(block).alignment(Alignment::Left),
                popup_rect,
            );
        })?;

        if let Ok(Event::Key(key)) = event::read() {
            match key.code {
                KeyCode::Char('r') => {
                    selected = selected.saturating_sub(1);
                }
                KeyCode::Char('y')
                    if selected + 1 < throwable_ids.len() => { selected += 1; }
                KeyCode::Enter => {
                    let (item_id, _name, _count) = &throwable_ids[selected];
                    let item_id = *item_id;
                    // 将选中的投掷物装备到副手（共享函数，消除重复）
                    if let Some(p) = ops::player_entity(world) {
                        ops::equip_throwable_to_off_hand(world, p, item_id);
                    }
                    // 进入瞄准模式，透传是否入队（true=有行动入队）
                    let consumed = open_throw_aim(terminal, world, game_start)?;
                    return Ok(consumed);
                }
                KeyCode::Char('x') | KeyCode::Char('X') | KeyCode::Esc => {
                    return Ok(false);
                }
                _ => {}
            }
        }
    }
}

/// 投掷瞄准模式。
pub fn open_throw_aim(
    terminal: &mut Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
    world: &mut World,
    game_start: Instant,
) -> io::Result<bool> {
    let (cx, cy) = {
        let Some(mut q) = world.try_query::<(&Player, &Position)>() else {
            return Ok(false);
        };
        q.iter(&*world).next().map(|(_, p)| (p.x, p.y)).unwrap_or((MAP_WIDTH / 2, MAP_HEIGHT / 2))
    };

    // 先设置光标位置，drop 后再计算 path（避免 resource_mut 双重借用）
    {
        let mut tp = world.resource_mut::<ThrowPreview>();
        tp.active = true;
        tp.cursor = (cx, cy);
    }
    update_throw_path(world);

    loop {
        let _ = terminal.draw(|frame| render_ui(frame, game_start, &*world));
        if let Ok(Event::Key(k)) = event::read() {
            match k.code {
                KeyCode::Up | KeyCode::Down | KeyCode::Left | KeyCode::Right
                | KeyCode::Home | KeyCode::End | KeyCode::PageUp | KeyCode::PageDown => {
                    let mut tp = world.resource_mut::<ThrowPreview>();
                    match k.code {
                        KeyCode::Up => tp.cursor.1 = tp.cursor.1.saturating_sub(1),
                        KeyCode::Down => tp.cursor.1 = (tp.cursor.1 + 1).min(MAP_HEIGHT - 1),
                        KeyCode::Left => tp.cursor.0 = tp.cursor.0.saturating_sub(1),
                        KeyCode::Right => tp.cursor.0 = (tp.cursor.0 + 1).min(MAP_WIDTH - 1),
                        KeyCode::Home => { tp.cursor.0 = 0; tp.cursor.1 = 0; }
                        KeyCode::End => { tp.cursor.0 = MAP_WIDTH - 1; tp.cursor.1 = MAP_HEIGHT - 1; }
                        KeyCode::PageUp => { tp.cursor.1 = tp.cursor.1.saturating_sub(5); }
                        KeyCode::PageDown => { tp.cursor.1 = (tp.cursor.1 + 5).min(MAP_HEIGHT - 1); }
                        _ => {}
                    }
                    #[allow(clippy::drop_non_drop)]
                    drop(tp);
                    update_throw_path(world);
                }
                KeyCode::Enter => {
                    let (valid, tx, ty) = {
                        let tp = world.resource::<ThrowPreview>();
                        (tp.valid_target, tp.cursor.0, tp.cursor.1)
                    };
                    world.resource_mut::<ThrowPreview>().active = false;
                    if !valid {
                        world.resource_mut::<EventLog>().push("目标无效".to_string());
                        return Ok(false);
                    }
                    // 入队 AV 行动（投掷耗时 190ms，快于近战移动，确保先于怪物追击执行）
                    if let Some(p) = ops::player_entity(world) {
                        let reaction = world.get::<dungeon_action::Reaction>(p)
                            .map(|r| r.time).unwrap_or(70.0);
                        let speed = agility_speed_factor(
                            world.get::<Stats>(p).map(|s| s.agility).unwrap_or(10)
                        );
                        let av = reaction + 190.0 * speed;
                        world.resource_mut::<ActionQueue>()
                            .enqueue(p, ActionKindV3::Throw { tx, ty }, av);
                    }
                    return Ok(true);
                }
                KeyCode::Char('x') | KeyCode::Char('X') | KeyCode::Esc => {
                    world.resource_mut::<ThrowPreview>().active = false;
                    return Ok(false);
                }
                _ => {}
            }
        }
    }
}

fn update_throw_path(world: &mut World) {
    let player_pos = match world.try_query::<(&Player, &Position)>() {
        Some(mut q) => q.iter(world).next().map(|(_, p)| (p.x, p.y)).unwrap_or((0, 0)),
        None => (0, 0),
    };

    // Phase 1: 只读收集（所有 &World 借用在此完成）
    let cursor = world.resource::<ThrowPreview>().cursor;
    let (cx, cy) = cursor;
    let dist_cheb = (cx as isize - player_pos.0 as isize).unsigned_abs()
        .max((cy as isize - player_pos.1 as isize).unsigned_abs());
    let path = ops::line_bresenham(player_pos.0, player_pos.1, cx, cy);
    let in_range = dist_cheb <= 5;
    // 视线检查：在闭包内完成 Map 借用，不跨 Phase 边界
    let los_clear = {
        let map = world.resource::<dungeon_core::Map>();
        path.iter().all(|&(px, py)| {
            (px == cx && py == cy) || !map.tiles[py][px].blocks_vision()
        })
    };  // &Map 借用在此结束

    // Phase 2: 写入（&mut World）
    let mut tp = world.resource_mut::<ThrowPreview>();
    tp.path = path;
    tp.valid_target = in_range && los_clear;
}


