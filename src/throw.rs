//! 投掷模式：投掷物选择 + 瞄准 + 弹道可视化

use std::io;
use std::time::Instant;

use bevy_ecs::prelude::*;
use crossterm::event::{self, Event, KeyCode};
use dungeon_core::{
    ops, EventLog, Monster, Position, Stats,
    ThrowPreview, GameRng, Inventory, Equipment, EntityName,
    Player, FloorNumber, PendingExp, ItemPickup, LootTable,
    ItemStack,
    MAP_HEIGHT, MAP_WIDTH,
};
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
) -> io::Result<()> {
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
        return Ok(());
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
                    if selected > 0 { selected -= 1; }
                }
                KeyCode::Char('y') => {
                    if selected + 1 < throwable_ids.len() { selected += 1; }
                }
                KeyCode::Enter => {
                    let (item_id, _name, _count) = &throwable_ids[selected];
                    let item_id = *item_id;
                    // 将选中的投掷物装备到副手（分步操作，避免 get_mut 嵌套）
                    let player = ops::player_entity(world);
                    if let Some(p) = player {
                        // Phase 1: 查询当前副手状态（只读）
                        let already_equipped = world.get::<Equipment>(p)
                            .map(|eq| eq.off_hand.as_ref().map(|s| s.item_id) == Some(item_id))
                            .unwrap_or(false);
                        if !already_equipped {
                            // Phase 2: 卸载旧副手→背包（Equipment 写）
                            let old_item = {
                                let mut eq = world.get_mut::<Equipment>(p);
                                eq.as_mut().and_then(|eq| eq.off_hand.take())
                            };
                            if let Some(old) = old_item {
                                let mut inv = world.get_mut::<Inventory>(p);
                                if let Some(ref mut inv) = inv {
                                    inv.add(old.item_id, old.count);
                                }
                            }
                            // Phase 3: 从背包取投掷物→装副手（Inventory 写 → Equipment 写）
                            let taken = {
                                let mut inv = world.get_mut::<Inventory>(p);
                                inv.as_mut().and_then(|inv| {
                                    let idx = inv.stacks.iter().position(|s| s.item_id == item_id)?;
                                    Some(inv.stacks.remove(idx))
                                })
                            };
                            if let Some(stack) = taken {
                                let mut eq = world.get_mut::<Equipment>(p);
                                if let Some(ref mut eq) = eq {
                                    eq.off_hand = Some(stack);
                                }
                            }
                        }
                    }
                    // 进入瞄准模式
                    let consumed = open_throw_aim(terminal, world, game_start)?;
                    if consumed {
                        world.resource_mut::<EventLog>().push("投掷了石子".to_string());
                    }
                    return Ok(());
                }
                KeyCode::Char('x') | KeyCode::Char('X') | KeyCode::Esc => {
                    return Ok(());
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
        let mut q = world.try_query::<(&Player, &Position)>().expect("Player+Position registered");
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
                    drop(tp);
                    update_throw_path(world);
                }
                KeyCode::Enter => {
                    let consumed = execute_throw(world);
                    world.resource_mut::<ThrowPreview>().active = false;
                    return Ok(consumed);
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
    let player_pos = world.try_query::<(&Player, &Position)>()
        .expect("Player+Position registered")
        .iter(world).next().map(|(_, p)| (p.x, p.y))
        .unwrap_or((0, 0));

    let map;
    let path;
    let in_range;
    let los_clear;
    let cursor;
    {
        let tp = world.resource::<ThrowPreview>();
        cursor = tp.cursor;
    }
    let (cx, cy) = cursor;

    let dist_cheb = (cx as isize - player_pos.0 as isize).unsigned_abs()
        .max((cy as isize - player_pos.1 as isize).unsigned_abs());

    path = ops::line_bresenham(player_pos.0, player_pos.1, cx, cy);
    map = world.resource::<dungeon_core::Map>();
    in_range = dist_cheb <= 5;
    los_clear = path.iter().all(|&(px, py)| {
        (px == cx && py == cy) || !map.tiles[py][px].blocks_vision()
    });

    let mut tp = world.resource_mut::<ThrowPreview>();
    tp.path = path;
    tp.valid_target = in_range && los_clear;
}

fn execute_throw(world: &mut World) -> bool {
    let valid;
    let cursor_pos;
    {
        let tp = world.resource::<ThrowPreview>();
        valid = tp.valid_target;
        cursor_pos = tp.cursor;
    }
    if !valid {
        world.resource_mut::<EventLog>().push("目标无效".to_string());
        return false;
    }
    let (tx, ty) = cursor_pos;

    // 检查副手是否有投掷物
    let player = ops::player_entity(world);
    let off_hand_empty = match player {
        None => true,
        Some(p) => world.get::<Equipment>(p)
            .map(|eq| eq.off_hand.is_none())
            .unwrap_or(true),
    };

    if off_hand_empty {
        world.resource_mut::<EventLog>().push("副手没有投掷物".to_string());
        return false;
    }

    // 查找光标格上的怪物
    let target = {
        let mut q = world.try_query::<(Entity, &Position, &Monster)>()
            .expect("Entity+Position+Monster registered");
        q.iter(world)
            .find(|(_, pos, _)| pos.x == tx && pos.y == ty)
            .map(|(e, _, _)| e)
    };

    if let Some(target_entity) = target {
        // 计算伤害（投掷时不使用主手武器加成）
        let floor = world.resource::<FloorNumber>().0;
        let base_dmg = 3 + floor / 2;
        let extra = world.resource_mut::<GameRng>().random_range(0, 2) as u32;
        let target_def = world.get::<Stats>(target_entity)
            .map(|s| s.defense as i32)
            .unwrap_or(0);
        let raw_dmg = ((base_dmg as i32 + extra as i32 - target_def).max(1)) as u32;

        // 暴击检查（不含主手武器加成）
        let crit_mult = if let Some(p) = player {
            let p_stats = world.get::<Stats>(p).cloned();
            let _inv = world.get::<Inventory>(p).cloned();
            let eq = world.get::<Equipment>(p).cloned();
            // 仅从防具+戒指计算暴击率加成（主手/副手不参与）
            let bonus_crit = eq.as_ref()
                .map(|eq| {
                    let mut total = 0.0f32;
                    if let Some(ref a) = eq.armor { if let Some(d) = ItemStack::new(a.item_id, 1).def() { total += d.bonus.crit_rate; } }
                    if let Some(ref r) = eq.ring { if let Some(d) = ItemStack::new(r.item_id, 1).def() { total += d.bonus.crit_rate; } }
                    total
                })
                .unwrap_or(0.0);
            let total_crit_rate = p_stats.as_ref()
                .map(|s| (s.crit_rate + bonus_crit).min(1.0))
                .unwrap_or(0.05);

            let roll = world.resource_mut::<GameRng>().random_f32();
            if roll < total_crit_rate {
                p_stats.map(|s| 1.0 + s.crit_damage).unwrap_or(1.5)
            } else {
                1.0
            }
        } else {
            1.0
        };

        let final_dmg = (raw_dmg as f32 * crit_mult).round() as i32;

        let target_name = world.get::<EntityName>(target_entity)
            .map(|n| n.0.clone())
            .unwrap_or("怪物".into());
        {
            if let Some(mut s) = world.get_mut::<Stats>(target_entity) {
                s.hp -= final_dmg;
            }
        }

        if world.get::<Stats>(target_entity).map(|s| s.hp <= 0).unwrap_or(false) {
            let exp = world.get::<Stats>(target_entity).map(|s| s.exp).unwrap_or(0);
            world.resource_mut::<PendingExp>().amount += exp;
            world.resource_mut::<EventLog>()
                .push(format!("石子击杀了{}！获得{}经验", target_name, exp));

            let loot = world.get::<LootTable>(target_entity).cloned();
            if let Some(lt) = loot {
                let mut rng2 = world.resource_mut::<GameRng>();
                let stacks = lt.roll(&mut rng2.rng);
                let pos = world.get::<Position>(target_entity).map(|p| (p.x, p.y));
                if let Some((px, py)) = pos {
                    for stack in &stacks {
                        let sname = stack.name();
                        world.resource_mut::<EventLog>()
                            .push(format!("{}掉落{}x{}", target_name, sname, stack.count));
                        world.spawn((
                            ItemPickup { stack: stack.clone() },
                            Position { x: px, y: py },
                            dungeon_core::Renderable { glyph: stack.glyph(), color: stack.color() },
                        ));
                    }
                }
            }
            world.entity_mut(target_entity).despawn();
        } else {
            let crit_text = if crit_mult > 1.0 { "暴击" } else { "" };
            world.resource_mut::<EventLog>()
                .push(format!("石子命中{}！造成{}伤害{}", target_name, final_dmg, crit_text));
        }

        // 消耗副手 1 颗石子
        if let Some(p) = player {
            if let Some(mut eq) = world.get_mut::<Equipment>(p) {
                if let Some(ref mut stack) = eq.off_hand {
                    stack.count = stack.count.saturating_sub(1);
                    if stack.count == 0 {
                        eq.off_hand = None;
                    }
                }
            }
        }

        true
    } else {
        world.resource_mut::<EventLog>().push("石子落在地上".to_string());
        // 仍然消耗石子
        if let Some(p) = player {
            if let Some(mut eq) = world.get_mut::<Equipment>(p) {
                if let Some(ref mut stack) = eq.off_hand {
                    stack.count = stack.count.saturating_sub(1);
                    if stack.count == 0 {
                        eq.off_hand = None;
                    }
                }
            }
        }
        true
    }
}
