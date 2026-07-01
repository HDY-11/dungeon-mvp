use std::collections::HashSet;
use std::io::{self, stdout};
use std::time::Instant;

use bevy_ecs::prelude::World;
use bevy_ecs::system::RunSystemOnce;
use crossterm::event::{self, Event, KeyCode};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::ExecutableCommand;
use bevy_ecs::prelude::Entity;
use dungeon_core::{
    apply_exp_system, apply_skill, buff_tick_system, check_death_system, collect_renderables,
    descend, effective_attack, effective_defense, equipment_bonus, fov_system, monster_ai_system,
    movement_system, pickup_system, rebuild_occupancy, save::GameSave, set_player_dir, setup_world,
    tick_action_system, update_map_memory, ActionPoints, Buffs, Equipment, EquipmentSlot,
    EntityName, EventLog, FloorNumber, Inventory, ItemInstance, Map, MapMemory, Monster, PendingLevelUp,
    PendingPickup, PendingSkill, Player, Position, skill_tick_system, Skills, Stairs, Stats,
    TurnManager, Viewshed, MAP_HEIGHT, MAP_WIDTH,
};
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::{Frame, Terminal};

// ── 主入口 ────────────────────────────────────────────

fn main() -> io::Result<()> {
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let mut terminal = Terminal::new(ratatui::backend::CrosstermBackend::new(stdout()))?;

    // 标题画面
    let (mut world, game_start) = title_screen(&mut terminal)?;

    let result = run(&mut terminal, &mut world, game_start);

    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

fn run(
    terminal: &mut Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
    world: &mut World,
    game_start: Instant,
) -> io::Result<()> {
    loop {
        // ── 行动轴推进 ────────────────────────────
        loop {
            terminal.draw(|frame| ui(frame, world, game_start))?;
            let _ = world.run_system_once(tick_action_system);

            // 找出行动值 >= 100 的实体
            let ready = {
                let mut q = world.query::<(Entity, &ActionPoints)>();
                let mut v: Vec<(Entity, f32)> = q
                    .iter(world)
                    .filter(|(_, ap)| ap.points >= 100.0)
                    .map(|(e, ap)| (e, ap.points))
                    .collect();
                v.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap()); // 高到低
                v
            };

            if ready.is_empty() {
                // 无人到达阈值 → 继续 tick
                continue;
            }

            let mut player_turn = false;
            for (entity, _pts) in &ready {
                // 减 100 行动值
                if let Some(mut ap) = world.get_mut::<ActionPoints>(*entity) {
                    ap.points -= 100.0;
                }

                // 判断是谁的回合
                let is_player = world.get::<Player>(*entity).is_some();
                let is_monster = world.get::<Monster>(*entity).is_some();

                if is_player {
                    player_turn = true;
                } else if is_monster {
                    // 怪物回合
                    rebuild_occupancy(world);
                    let _ = world.run_system_once(monster_ai_system);
                    let _ = world.run_system_once(apply_exp_system);
                    let _ = world.run_system_once(fov_system);
                    let _ = world.run_system_once(check_death_system);

                    // 检查玩家是否死亡
                    if world.resource::<TurnManager>().game_over {
                        break;
                    }
                }
            }

            if world.resource::<TurnManager>().game_over {
                break;
            }
            if player_turn {
                break;
            }
        }

        // 死亡 → 绘制最后一帧后退出
        if world.resource::<TurnManager>().game_over {
            terminal.draw(|frame| ui(frame, world, game_start))?;
            // 等玩家按 q
            loop {
                if let Event::Key(key) = event::read()? {
                    if matches!(key.code, KeyCode::Char('q') | KeyCode::Esc) {
                        break;
                    }
                }
            }
            break;
        }

        // ── 玩家回合 ──────────────────────────────
        terminal.draw(|frame| ui(frame, world, game_start))?;

        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => break,
                KeyCode::Char('w') | KeyCode::Char('W') => set_player_dir(world, 0, -1),
                KeyCode::Char('s') | KeyCode::Char('S') => set_player_dir(world, 0, 1),
                KeyCode::Char('a') | KeyCode::Char('A') => set_player_dir(world, -1, 0),
                KeyCode::Char('d') | KeyCode::Char('D') => set_player_dir(world, 1, 0),
                KeyCode::Char('1') => apply_skill(world, 0),
                KeyCode::Char('2') => apply_skill(world, 1),
                KeyCode::Char('3') => apply_skill(world, 2),
                KeyCode::Char('4') => apply_skill(world, 3),
                KeyCode::Char('.') | KeyCode::Char('5') => {
                    let mc = { let mut q = world.query::<&Monster>(); q.iter(world).count() };
                    if mc == 0 {
                        let mut q = world.query::<&mut Stats>();
                        if let Some(mut s) = q.iter_mut(world).next() {
                            s.hp = (s.hp + 5).min(s.max_hp);
                            world.resource_mut::<EventLog>().push("你休息了一回合，恢复 5 HP");
                        }
                    }
                }
                KeyCode::Char('e') | KeyCode::Char('E') => {
                    open_inventory(world, terminal, game_start)?;
                }
                KeyCode::F(5) => {
                    let save = GameSave::from_world(world);
                    let data = bincode::serialize(&save).unwrap();
                    std::fs::write("save.bin", data).ok();
                    world.resource_mut::<EventLog>().push(String::from("已保存！"));
                }
                KeyCode::F(9) => {
                    if let Ok(data) = std::fs::read("save.bin") {
                        if let Ok(save) = bincode::deserialize::<GameSave>(&data) {
                            save.into_world(world);
                            world.resource_mut::<EventLog>().push(String::from("已读档！"));
                        }
                    }
                }
                KeyCode::Char('>') => {
                    let on_stairs = {
                        let pp = { let mut pq = world.query::<&Position>(); *pq.iter(world).next().unwrap() };
                        let mut sq = world.query::<(&Stairs, &Position)>();
                        sq.iter(world).any(|(_, sp)| sp.x == pp.x && sp.y == pp.y)
                    };
                    if on_stairs {
                        descend(world);
                        // 下楼后自动存档
                        let save = GameSave::from_world(world);
                        if let Ok(data) = bincode::serialize(&save) {
                            std::fs::write("save.bin", data).ok();
                        }
                    }
                }
                _ => continue,
            }

            // 执行玩家行动 + 后续处理
            rebuild_occupancy(world);
            let _ = world.run_system_once(movement_system);
            let _ = world.run_system_once(pickup_system);
            let _ = world.run_system_once(skill_tick_system);
            let _ = world.run_system_once(buff_tick_system);
            let _ = world.run_system_once(apply_exp_system);
            let _ = world.run_system_once(fov_system);
            update_map_memory(world);
            let _ = world.run_system_once(check_death_system);

            // 升级加点
            if world.resource::<PendingLevelUp>().points > 0 {
                level_up_screen(world, terminal, game_start)?;
            }
        }
    }
    Ok(())
}

// ═══════════════════════════════════════════════════════
// 渲染
// ═══════════════════════════════════════════════════════

fn ui(frame: &mut Frame, world: &mut World, game_start: Instant) {
    let area = frame.area();

    let title = if world.resource::<TurnManager>().game_over {
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

    if world.resource::<TurnManager>().game_over {
        // 死亡画面
        let msg = Paragraph::new(Line::from("按 q 退出").centered())
            .style(Style::default().fg(Color::Red));
        frame.render_widget(msg, inner);
        return;
    }

    // ── 左右分栏：地图 | 状态面板 ──────────────────

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(MAP_WIDTH as u16),
            Constraint::Length(1),
            Constraint::Min(1),
        ])
        .split(inner);
    let map_area = chunks[0];
    let stats_area = Rect {
        x: chunks[2].x,
        y: chunks[2].y,
        width: chunks[2].width,
        height: inner.height,
    };

    // ── 收集渲染数据 ────────────────────────────────

    let player_visible: HashSet<(usize, usize)> = {
        let mut q = world.query::<(&Player, &Viewshed)>();
        q.iter(world)
            .next()
            .map(|(_, v)| v.visible_tiles.iter().copied().collect())
            .unwrap_or_default()
    };

    let explored = { world.resource::<MapMemory>().explored };
    let tiles = { world.resource::<Map>().tiles };
    let renderables = collect_renderables(world);

    let (px, py) = {
        let mut q = world.query::<(&Player, &Position)>();
        q.iter(world)
            .next()
            .map(|(_, p)| (p.x, p.y))
            .unwrap_or((0, 0))
    };

    let monster_count = renderables.iter().filter(|(_, _, g, _)| *g != '@').count();
    let room_count = { world.resource::<Map>().rooms.len() };

    // ── 地图（左栏） ────────────────────────────────

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

    for &(ex, ey, glyph, color) in &renderables {
        if player_visible.contains(&(ex, ey)) && ey < MAP_HEIGHT && ex < MAP_WIDTH {
            lines[ey][ex] = (glyph, color);
        }
    }

    let styled_lines: Vec<Line> = lines
        .into_iter()
        .map(|row| {
            let spans: Vec<Span> = row
                .into_iter()
                .map(|(g, c)| Span::styled(g.to_string(), Style::default().fg(c)))
                .collect();
            Line::from(spans)
        })
        .collect();

    frame.render_widget(
        Paragraph::new(styled_lines).style(Style::default().fg(Color::White)),
        map_area,
    );

    // ── 状态面板（右栏） ────────────────────────────
    let stats_lines = build_stats_panel(world, px, py, room_count, monster_count, game_start);

    frame.render_widget(
        Paragraph::new(stats_lines)
            .style(Style::default().fg(Color::White))
            .block(
                Block::default()
                    .borders(Borders::LEFT)
                    .border_style(Style::default().fg(Color::DarkGray)),
            ),
        stats_area,
    );
}

/// 构造右侧状态面板的内容行
fn build_stats_panel(
    world: &mut World,
    px: usize,
    py: usize,
    room_count: usize,
    monster_count: usize,
    game_start: Instant,
) -> Vec<Line<'static>> {
    let mut out: Vec<Line<'static>> = Vec::new();

    let stats: Option<Stats> = {
        let mut q = world.query::<(&Player, &Stats)>();
        q.iter(world).next().map(|(_, s)| s.clone())
    };

    let s = match stats {
        Some(ref s) => s,
        None => {
            out.push(Line::from(Span::raw("(无数据)")));
            return out;
        }
    };

    // ── 标题 ──────────────────────────────────────
    let hp_color = if s.hp as f32 <= s.max_hp as f32 * 0.3 {
        Color::Red
    } else {
        Color::Cyan
    };
    out.push(Line::from(vec![
        Span::styled(
            format!(" Lv.{}  Warrior ", s.level),
            Style::default().fg(hp_color).bold(),
        ),
    ]));
    out.push(Line::from(Span::raw(" ".repeat(22))));

    // ── HP ────────────────────────────────────────
    let bar_color = if s.hp as f32 <= s.max_hp as f32 * 0.3 {
        Color::Red
    } else {
        Color::Red
    };
    let hp_display = s.hp.max(0);
    out.push(Line::from(vec![
        Span::styled(" HP ", Style::default().fg(Color::DarkGray)),
        Span::raw(format!("{:>3}/{:<3}", hp_display, s.max_hp)),
        Span::raw(" "),
        Span::styled(bar(hp_display, s.max_hp, 8), Style::default().fg(bar_color)),
    ]));

    // ── MP ────────────────────────────────────────
    out.push(Line::from(vec![
        Span::styled(" MP ", Style::default().fg(Color::DarkGray)),
        Span::raw(format!("{:>3}/{:<3}", s.mp, s.max_mp)),
        Span::raw(" "),
        Span::styled(
            bar(s.mp, s.max_mp, 8),
            Style::default().fg(Color::Blue),
        ),
    ]));

    // ── EXP ──────────────────────────────────────
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

    // ── 属性 ────────────────────────────────────
    out.push(Line::from(vec![
        Span::styled(" STR", Style::default().fg(Color::DarkGray)),
        Span::raw(format!("{:>3}", s.strength)),
        Span::raw("   "),
        Span::styled("INT", Style::default().fg(Color::DarkGray)),
        Span::raw(format!("{:>3}", s.intelligence)),
    ]));
    out.push(Line::from(vec![
        Span::styled(" DEX", Style::default().fg(Color::DarkGray)),
        Span::raw(format!("{:>3}", s.dexterity)),
        Span::raw("   "),
        Span::styled("VIT", Style::default().fg(Color::DarkGray)),
        Span::raw(format!("{:>3}", s.vitality)),
    ]));

    out.push(Line::from(Span::raw("")));

    // ── 派生 ────────────────────────────────────
    out.push(Line::from(vec![
        Span::styled(" ATK", Style::default().fg(Color::DarkGray)),
        Span::raw(format!("{:>3}", s.attack())),
        Span::raw("   "),
        Span::styled("DEF", Style::default().fg(Color::DarkGray)),
        Span::raw(format!("{:>3}", s.defense())),
    ]));

    out.push(Line::from(Span::raw("")));
    out.push(Line::from(Span::raw(" ".repeat(22))));

    // ── 游戏信息 ────────────────────────────────
    let floor = world.resource::<FloorNumber>().0;
    out.push(Line::from(Span::raw(format!(" 楼层 {}", floor))));
    out.push(Line::from(Span::raw(format!(" 房间 {}", room_count))));
    out.push(Line::from(Span::raw(format!("  @ ({}, {})", px, py))));
    out.push(Line::from(Span::raw(format!(" 怪物 {}", monster_count))));

    // ── 游戏时间 ────────────────────────────────
    let elapsed = game_start.elapsed();
    let secs = elapsed.as_secs();
    let min = secs / 60;
    let s = secs % 60;
    out.push(Line::from(Span::styled(
        format!(" ⏱ {:>2}:{:02}", min, s),
        Style::default().fg(Color::DarkGray),
    )));

    // ── 技能栏 ────────────────────────────────
    out.push(Line::from(Span::raw("")));
    {
        let mut q = world.query::<(&Skills, &Stats)>();
        if let Some((skills, stats)) = q.iter(world).next() {
            out.push(Line::from(Span::styled("── 技能 ──", Style::default().fg(Color::DarkGray))));
            for sk in &skills.list {
                let can_cast = stats.mp >= sk.cost_mp;
                let color = if can_cast { Color::White } else { Color::DarkGray };
                out.push(Line::from(vec![
                    Span::styled(format!(" {} ", sk.key), Style::default().fg(Color::Yellow)),
                    Span::styled(format!("{}({})", sk.name, sk.cost_mp), Style::default().fg(color)),
                ]));
            }
        }
    }

    // ── Buff 栏 ────────────────────────────────
    {
        let mut q = world.query::<&Buffs>();
        if let Some(b) = q.iter(world).next() {
            let mut parts = Vec::new();
            if b.shield_turns > 0 { parts.push(format!("🛡{}", b.shield_turns)); }
            if b.berserk_turns > 0 { parts.push(format!("⚔{}", b.berserk_turns)); }
            if !parts.is_empty() {
                out.push(Line::from(Span::styled(
                    format!("── Buff ── {}", parts.join(" ")),
                    Style::default().fg(Color::Green),
                )));
            }
        }
    }

    // ── 事件日志 ────────────────────────────────
    out.push(Line::from(Span::raw("")));
    {
        let log = world.resource::<EventLog>();
        if !log.messages.is_empty() {
            out.push(Line::from(
                Span::styled("── 事件 ──", Style::default().fg(Color::DarkGray)),
            ));
            for msg in log.messages.iter().rev().take(5) {
                out.push(Line::from(Span::raw(format!(" {}", msg))));
            }
        }
    }

    // ── 视野内实体描述 ────────────────────────────
    {
        let player_visible: HashSet<(usize, usize)> = {
            let mut q = world.query::<(&Player, &Viewshed)>();
            q.iter(world).next()
                .map(|(_, v)| v.visible_tiles.iter().copied().collect())
                .unwrap_or_default()
        };
        let mut entities: Vec<(String, i32, i32)> = {
            let mut q = world.query::<(&Position, &EntityName, &Stats)>();
            q.iter(world)
                .filter(|(pos, _, _)| player_visible.contains(&(pos.x, pos.y)))
                .filter(|(_, name, _)| name.0 != "冒险者")
                .map(|(_, name, stats)| (name.0.clone(), stats.hp, stats.max_hp))
                .collect()
        };
        if !entities.is_empty() {
            entities.sort_by(|a, b| a.0.cmp(&b.0));
            out.push(Line::from(Span::raw("")));
            out.push(Line::from(
                Span::styled("── 视野 ──", Style::default().fg(Color::DarkGray)),
            ));
            for (ename, hp, max_hp) in &entities {
                let hp_color = if *hp <= *max_hp / 3 {
                    Color::Red
                } else {
                    Color::White
                };
                out.push(Line::from(vec![
                    Span::raw(format!(" {} ", ename)),
                    Span::styled(
                        format!("({}/{})", (*hp).max(0), max_hp),
                        Style::default().fg(hp_color),
                    ),
                ]));
            }
        }
    }

    out
}

fn bar(current: i32, max: i32, width: usize) -> String {
    let ratio = if max > 0 {
        (current as f32 / max as f32).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let filled = (ratio * width as f32).round() as usize;
    let filled = filled.min(width);
    let empty = width.saturating_sub(filled);
    "█".repeat(filled) + "░".repeat(empty).as_str()
}

// ═══════════════════════════════════════════════════════
// 背包界面
// ═══════════════════════════════════════════════════════

fn get_inv(world: &mut World) -> (Vec<ItemInstance>, usize, (Option<usize>, Option<usize>, Option<usize>)) {
    let mut q = world.query::<(&Inventory, &Equipment)>();
    if let Some((inv, eq)) = q.iter(world).next() {
        return (inv.items.clone(), inv.capacity, (eq.weapon, eq.armor, eq.ring));
    }
    (Vec::new(), 36, (None, None, None))
}

fn get_inv_mut(world: &mut World) -> Option<(bevy_ecs::change_detection::Mut<'_, Inventory>, bevy_ecs::change_detection::Mut<'_, Equipment>)> {
    let mut q = world.query::<(&mut Inventory, &mut Equipment)>();
    q.iter_mut(world).next()
}

fn open_inventory(
    world: &mut World,
    terminal: &mut Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
    game_start: Instant,
) -> io::Result<()> {
    let mut selected: usize = 0;
    let mut scroll: usize = 0;

    loop {
        let inv_data = get_inv(world);

        terminal.draw(|frame| {
            let area = frame.area();
            let block = Block::default()
                .title("  背包  ")
                .title_alignment(Alignment::Center)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan));
            frame.render_widget(block, area);
            let inner = inner_rect(area, 1);

            let (ref items, capacity, (weapon, armor, ring)) = inv_data;

            let mut lines: Vec<Line> = Vec::new();
            lines.push(Line::from(vec![
                Span::styled(
                    format!(" 物品 {} / {}  e:装备 d:丢弃 Esc:返回", items.len(), capacity),
                    Style::default().fg(Color::DarkGray),
                ),
            ]));
            lines.push(Line::from(Span::raw("")));

            let eq_name = |idx: Option<usize>| -> String {
                idx.and_then(|i| items.get(i)).map(|it| it.name.clone()).unwrap_or("(空)".into())
            };
            lines.push(Line::from(Span::raw(format!(
                " 武器: {}  防具: {}  戒指: {}",
                eq_name(weapon), eq_name(armor), eq_name(ring),
            ))));
            lines.push(Line::from(Span::raw("")));

            let page_size = (inner.height as usize).saturating_sub(5).min(20);
            if items.is_empty() {
                lines.push(Line::from(Span::styled(" (背包为空)", Style::default().fg(Color::DarkGray))));
            } else {
                let sel = selected.min(items.len() - 1);
                let sc = scroll.min(sel);
                let end = (sc + page_size).min(items.len());

                for i in sc..end {
                    let item = &items[i];
                    let idx_char = if i < 10 {
                        char::from_digit(i as u32, 10).unwrap()
                    } else {
                        char::from_u32(i as u32 - 10 + b'a' as u32).unwrap_or('?')
                    };
                    let prefix = if i == sel { "▸" } else { " " };
                    let is_equipped = weapon == Some(i) || armor == Some(i) || ring == Some(i);
                    let eq_mark = if is_equipped { " [E]" } else { "" };
                    let slot_name = match item.slot { EquipmentSlot::Weapon => "武", EquipmentSlot::Armor => "防", EquipmentSlot::Ring => "戒" };
                    lines.push(Line::from(vec![
                        Span::styled(format!("{}{}", prefix, idx_char), Style::default().fg(Color::Yellow)),
                        Span::raw(format!(" {}{} ", item.glyph, eq_mark)),
                        Span::styled(&item.name, Style::default().fg(item.color)),
                        Span::raw(format!(" ({})", slot_name)),
                    ]));
                }
            }

            if !items.is_empty() {
                let sel = selected.min(items.len() - 1);
                lines.push(Line::from(Span::raw("")));
                lines.push(Line::from(Span::styled(
                    format!(" ── {} ──", items[sel].name), Style::default().fg(Color::Cyan),
                )));
                lines.push(Line::from(Span::raw(format!(" {}", items[sel].description))));
            }

            frame.render_widget(Paragraph::new(lines).style(Style::default().fg(Color::White)), inner);
        })?;

        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Esc => break,
                KeyCode::Char('e') | KeyCode::Char('E') => {
                    if let Some((mut inv, mut eq)) = get_inv_mut(world) {
                        let sel = selected.min(inv.items.len().saturating_sub(1));
                        if sel < inv.items.len() {
                            let slot = inv.items[sel].slot;
                            let target = match slot { EquipmentSlot::Weapon => &mut eq.weapon, EquipmentSlot::Armor => &mut eq.armor, EquipmentSlot::Ring => &mut eq.ring };
                            if *target == Some(sel) { *target = None; }
                            else { *target = Some(sel); }
                        }
                    }
                }
                KeyCode::Char('d') | KeyCode::Char('D') => {
                    if let Some((mut inv, mut eq)) = get_inv_mut(world) {
                        let sel = selected.min(inv.items.len().saturating_sub(1));
                        if sel < inv.items.len() {
                            if eq.weapon == Some(sel) { eq.weapon = None; }
                            if eq.armor  == Some(sel) { eq.armor  = None; }
                            if eq.ring   == Some(sel) { eq.ring   = None; }
                            inv.items.remove(sel);
                            if selected > 0 { selected -= 1; }
                        }
                    }
                }
                KeyCode::Char(ch) if ch.is_ascii_digit() => {
                    selected = ch as usize - '0' as usize;
                }
                KeyCode::Char(ch) if ('a'..='z').contains(&ch) => {
                    selected = ch as usize - 'a' as usize + 10;
                }
                _ => {}
            }
        }
    }
    Ok(())
}

// ═══════════════════════════════════════════════════════
// 升级加点界面
// ═══════════════════════════════════════════════════════

fn level_up_screen(
    world: &mut World,
    terminal: &mut Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
    game_start: Instant,
) -> io::Result<()> {
    let mut points = { world.resource::<PendingLevelUp>().points };

    loop {
        // 读取当前属性用于显示
        let stats = {
            let mut q = world.query::<(&Stats)>();
            q.iter(world).next().cloned().unwrap()
        };

        terminal.draw(|frame| {
            let area = frame.area();
            let block = Block::default()
                .title("  升级加点  ")
                .title_alignment(Alignment::Center)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan));
            frame.render_widget(block, area);
            let inner = inner_rect(area, 1);

            let mut lines = Vec::new();
            lines.push(Line::from(Span::styled(
                format!("剩余点数: {}  按 S/D/I/V 分配  Enter 确认", points),
                Style::default().fg(Color::Yellow),
            )));
            lines.push(Line::from(Span::raw("")));
            lines.push(Line::from(Span::raw(format!(
                " [S] 力量 STR: {:>2}  →  {}", stats.strength, stats.strength + 1,
            ))));
            lines.push(Line::from(Span::raw(format!(
                " [D] 敏捷 DEX: {:>2}  →  {}", stats.dexterity, stats.dexterity + 1,
            ))));
            lines.push(Line::from(Span::raw(format!(
                " [I] 智力 INT: {:>2}  →  {}", stats.intelligence, stats.intelligence + 1,
            ))));
            lines.push(Line::from(Span::raw(format!(
                " [V] 体质 VIT: {:>2}  →  {}", stats.vitality, stats.vitality + 1,
            ))));
            lines.push(Line::from(Span::raw("")));
            lines.push(Line::from(Span::styled(
                " Enter 确认  Esc 跳过",
                Style::default().fg(Color::DarkGray),
            )));

            frame.render_widget(
                Paragraph::new(lines).style(Style::default().fg(Color::White)),
                inner,
            );
        })?;

        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Esc => { world.resource_mut::<PendingLevelUp>().points = points; break; }
                KeyCode::Enter => {
                    // 确认加点
                    if points > 0 {
                        // 把剩余点数自动分配掉
                        let mut q = world.query::<&mut Stats>();
                        if let Some(mut s) = q.iter_mut(world).next() {
                            s.strength += points.min(1);
                            s.dexterity += (points.saturating_sub(1)).min(1);
                            s.intelligence += points.saturating_sub(2).min(1);
                            s.vitality += points.saturating_sub(3);
                            s.max_hp = dungeon_core::max_hp_for(s.level, s.vitality);
                            s.max_mp = dungeon_core::max_mp_for(s.level, s.intelligence);
                            s.hp = s.max_hp;
                            s.mp = s.max_mp;
                        }
                    }
                    world.resource_mut::<PendingLevelUp>().points = 0;
                    break;
                }
                KeyCode::Char('s') | KeyCode::Char('S') if points > 0 => {
                    let mut q = world.query::<&mut Stats>();
                    if let Some(mut s) = q.iter_mut(world).next() {
                        s.strength += 1;
                        s.max_hp = dungeon_core::max_hp_for(s.level, s.vitality);
                        s.hp = s.max_hp;
                        points -= 1;
                    }
                }
                KeyCode::Char('d') | KeyCode::Char('D') if points > 0 => {
                    let mut q = world.query::<&mut Stats>();
                    if let Some(mut s) = q.iter_mut(world).next() {
                        s.dexterity += 1;
                        points -= 1;
                    }
                }
                KeyCode::Char('i') | KeyCode::Char('I') if points > 0 => {
                    let mut q = world.query::<&mut Stats>();
                    if let Some(mut s) = q.iter_mut(world).next() {
                        s.intelligence += 1;
                        s.max_mp = dungeon_core::max_mp_for(s.level, s.intelligence);
                        s.mp = s.max_mp;
                        points -= 1;
                    }
                }
                KeyCode::Char('v') | KeyCode::Char('V') if points > 0 => {
                    let mut q = world.query::<&mut Stats>();
                    if let Some(mut s) = q.iter_mut(world).next() {
                        s.vitality += 1;
                        s.max_hp = dungeon_core::max_hp_for(s.level, s.vitality);
                        s.hp = s.max_hp;
                        points -= 1;
                    }
                }
                _ => {}
            }
        }
    }
    Ok(())
}

// ═══════════════════════════════════════════════════════
// 标题画面
// ═══════════════════════════════════════════════════════

fn title_screen(
    terminal: &mut Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
) -> io::Result<(World, Instant)> {
    loop {
        terminal.draw(|frame| {
            let area = frame.area();
            let block = Block::default()
                .title("  Dungeon MVP ")
                .title_alignment(Alignment::Center)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan));
            frame.render_widget(block, area);
            let inner = inner_rect(area, 1);

            let lines = vec![
                Line::from(Span::styled("  Dungeon MVP", Style::default().fg(Color::Yellow).bold())),
                Line::from(Span::raw("")),
                Line::from(Span::styled("  [N] 新游戏", Style::default().fg(Color::White))),
                Line::from(Span::styled("  [C] 继续", Style::default().fg(Color::White))),
                Line::from(Span::raw("")),
                Line::from(Span::styled("  WASD 移动  1-4 技能  e 背包  > 下楼  q 退出", Style::default().fg(Color::DarkGray))),
            ];
            frame.render_widget(Paragraph::new(lines).style(Style::default().fg(Color::White)), inner);
        })?;

        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Char('n') | KeyCode::Char('N') => {
                    let mut world = setup_world();
                    let _ = world.run_system_once(fov_system);
                    update_map_memory(&mut world);
                    return Ok((world, Instant::now()));
                }
                KeyCode::Char('c') | KeyCode::Char('C') => {
                    if let Ok(data) = std::fs::read("save.bin") {
                        if let Ok(save) = bincode::deserialize::<GameSave>(&data) {
                            let mut world = World::new();
                            save.into_world(&mut world);
                            let _ = world.run_system_once(fov_system);
                            update_map_memory(&mut world);
                            return Ok((world, Instant::now()));
                        }
                    }
                }
                KeyCode::Char('q') | KeyCode::Esc => {
                    disable_raw_mode()?;
                    stdout().execute(LeaveAlternateScreen)?;
                    std::process::exit(0);
                }
                _ => {}
            }
        }
    }
}

fn inner_rect(area: Rect, border: u16) -> Rect {
    Rect {
        x: area.x + border,
        y: area.y + border,
        width: area.width.saturating_sub(border * 2),
        height: area.height.saturating_sub(border * 2),
    }
}
