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
    action_cost, apply_exp_system, apply_skill, buff_tick_system, check_death_system,
    collect_renderables, descend, effective_attack, effective_defense, fov_system,
    monster_ai_system, movement_system, pickup_system, rebuild_occupancy, save::GameSave,
    set_player_dir, setup_world, tick_action_system, update_map_memory, ActionPoints, AiBehavior,
    Buffs, Equipment, EquipmentSlot, EntityName, EventLog, FloorNumber, Inventory, ItemInstance,
    Map, MapMemory, Monster, MonsterBrain, PendingLevelUp, PendingPlayerAction, Player, PlayerClass, Position,
    skill_tick_system, Skills, Stairs, Stats, TurnManager, Viewshed, MAP_HEIGHT, MAP_WIDTH,
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

/// 根据速度估算实体的行动成本 (ms)
/// 速度越快成本越低 → 行动频率越高
fn entity_action_cost(speed: f32) -> f32 {
    // 基准速度 50（无敏捷加成），cost = MOVE * (50 / speed)
    // player(speed=80) → 300*50/80 ≈ 188ms
    // rat(speed=65)   → 300*50/65 ≈ 231ms
    // goblin(speed=59)→ 300*50/59 ≈ 254ms
    let base = action_cost::MOVE;
    let ratio = 50.0 / speed.max(1.0);
    (base * ratio).max(100.0) // 至少 100ms，防止无限快
}

fn run(
    terminal: &mut Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
    world: &mut World,
    game_start: Instant,
) -> io::Result<()> {
    loop {
        // ── 行动轴推进 (每轮只处理一个实体) ─────
        'action: loop {
            terminal.draw(|frame| ui(frame, world, game_start))?;
            let _ = world.run_system_once(tick_action_system);

            // 找出最优先行动的实体
            // 玩家用 pending_cost 判断就绪，怪物用 entity_action_cost
            let next = {
                let pending_cost = world.resource::<PendingPlayerAction>().action_cost;
                let mut q = world.query::<(Entity, &ActionPoints)>();
                q.iter(world)
                    .filter(|(e, ap)| {
                        if world.get::<Player>(*e).is_some() {
                            ap.points >= pending_cost
                        } else {
                            let cost = entity_action_cost(ap.speed);
                            ap.points >= cost
                        }
                    })
                    .min_by(|(e_a, a), (e_b, b)| {
                        let ra = if world.get::<Player>(*e_a).is_some() {
                            (pending_cost - a.points).max(0.0)
                        } else {
                            (entity_action_cost(a.speed) - a.points).max(0.0)
                        };
                        let rb = if world.get::<Player>(*e_b).is_some() {
                            (pending_cost - b.points).max(0.0)
                        } else {
                            (entity_action_cost(b.speed) - b.points).max(0.0)
                        };
                        ra.partial_cmp(&rb).unwrap()
                    })
                    .map(|(e, ap)| (e, ap.speed))
            };

            let Some((entity, speed)) = next else {
                // 无人 ready → 继续积累
                continue;
            };

            // 扣除行动成本（玩家用当前招式成本，怪物用 speed 派生）
            let cost = if world.get::<Player>(entity).is_some() {
                world.resource::<PendingPlayerAction>().action_cost
            } else {
                entity_action_cost(speed)
            };
            if let Some(mut ap) = world.get_mut::<ActionPoints>(entity) {
                ap.points -= cost;
            }

            let is_player = world.get::<Player>(entity).is_some();
            let is_monster = world.get::<Monster>(entity).is_some();

            if is_player {
                // 玩家行动 — 退出到玩家输入环节
                break 'action;
            }

            if is_monster {
                let log_before = world.resource::<EventLog>().messages.len();

                rebuild_occupancy(world);
                let _ = world.run_system_once(monster_ai_system);
                let _ = world.run_system_once(apply_exp_system);
                let _ = world.run_system_once(fov_system);
                rebuild_occupancy(world);
                let _ = world.run_system_once(check_death_system);

                // 只有战斗事件（攻击/暴击/法术等）才暂停 1s，移动/逃跑/游荡不停
                let log_after = world.resource::<EventLog>().messages.len();
                if log_after > log_before {
                    let pause = world.resource::<EventLog>().messages.last()
                        .map(|m| m.contains("攻击") || m.contains("暴击") || m.contains("火球") || m.contains("伤害"))
                        .unwrap_or(false);
                    if pause {
                        terminal.draw(|frame| ui(frame, world, game_start))?;
                        std::thread::sleep(std::time::Duration::from_secs(1));
                    }
                }

                if world.resource::<TurnManager>().game_over { break 'action; }
            }
        }

        // 死亡 → 绘制最后一帧后退出
        if world.resource::<TurnManager>().game_over {
            terminal.draw(|frame| ui(frame, world, game_start))?;
            loop {
                if let Event::Key(key) = event::read()? {
                    if matches!(key.code, KeyCode::Char('q') | KeyCode::Esc) { break; }
                }
            }
            break;
        }

        // ── 玩家回合 ────────────────────────────
        // 行动轴默认显示"移动"，移动/等待即时执行，技能需 Enter 确认
        let skill_names: Vec<(char, String)> = {
            let mut q = world.query::<&Skills>();
            q.iter(world).next()
                .map(|sk| sk.list.iter().map(|s| (s.key, s.name.to_string())).collect())
                .unwrap_or_default()
        };

        // 重置行动轴为默认
        world.resource_mut::<PendingPlayerAction>().action_name = "移动".into();
        world.resource_mut::<PendingPlayerAction>().is_pending_skill = false;

        let mut turn_done = false;
        while !turn_done {
            terminal.draw(|frame| ui(frame, world, game_start))?;

            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => {
                        terminal.draw(|frame| {
                            let area = frame.area();
                            let confirm = Paragraph::new(Line::from(vec![
                                Span::styled(" 确认退出？", Style::default().fg(Color::Red).bold()),
                                Span::raw(" "), Span::styled("[Y]是", Style::default().fg(Color::Yellow)),
                                Span::raw(" "), Span::styled("[N]否", Style::default().fg(Color::DarkGray)),
                            ])).block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::Red)));
                            frame.render_widget(confirm, Rect { x: area.width/2-10, y: area.height/2, width: 20, height: 3, });
                        })?;
                        if let Event::Key(k2) = event::read()? {
                            if matches!(k2.code, KeyCode::Char('y') | KeyCode::Char('Y')) { return Ok(()); }
                        }
                        continue;
                    }
                    // 方向键 → 即时移动
                    KeyCode::Up => {
                        set_player_dir(world, 0, -1);
                        turn_done = true;
                    }
                    KeyCode::Down => {
                        set_player_dir(world, 0, 1);
                        turn_done = true;
                    }
                    KeyCode::Left => {
                        set_player_dir(world, -1, 0);
                        turn_done = true;
                    }
                    KeyCode::Right => {
                        set_player_dir(world, 1, 0);
                        turn_done = true;
                    }
                    // 技能选择 → 显示在行动轴，等 Enter 确认
                    KeyCode::Char('1') => {
                        if let Some((_, name)) = skill_names.get(0) {
                            let pending = &mut *world.resource_mut::<PendingPlayerAction>();
                            *pending = PendingPlayerAction::new_skill(0, name);
                        }
                    }
                    KeyCode::Char('2') => {
                        if let Some((_, name)) = skill_names.get(1) {
                            let pending = &mut *world.resource_mut::<PendingPlayerAction>();
                            *pending = PendingPlayerAction::new_skill(1, name);
                        }
                    }
                    KeyCode::Char('3') => {
                        if let Some((_, name)) = skill_names.get(2) {
                            let pending = &mut *world.resource_mut::<PendingPlayerAction>();
                            *pending = PendingPlayerAction::new_skill(2, name);
                        }
                    }
                    KeyCode::Char('4') => {
                        if let Some((_, name)) = skill_names.get(3) {
                            let pending = &mut *world.resource_mut::<PendingPlayerAction>();
                            *pending = PendingPlayerAction::new_skill(3, name);
                        }
                    }
                    // Enter → 如果有待确认技能则施放
                    KeyCode::Enter => {
                        let is_pending = world.resource::<PendingPlayerAction>().is_pending_skill;
                        if is_pending {
                            let idx = world.resource::<PendingPlayerAction>().skill_idx;
                            if let Some(si) = idx {
                                apply_skill(world, si);
                                let _ = world.run_system_once(skill_tick_system);
                                turn_done = true;
                            }
                        }
                    }
                    // . → 等待
                    KeyCode::Char('.') | KeyCode::Char('5') => {
                        let mc = { let mut q = world.query::<&Monster>(); q.iter(world).count() };
                        if mc == 0 {
                            let mut q = world.query::<&mut Stats>();
                            if let Some(mut s) = q.iter_mut(world).next() {
                                s.hp = (s.hp + 5).min(s.max_hp);
                                world.resource_mut::<EventLog>().push("你休息了一回合，恢复 5 HP");
                            }
                        }
                        turn_done = true;
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
                            terminal.draw(|frame| {
                                let area = frame.area();
                                let confirm = Paragraph::new(Line::from(vec![
                                    Span::styled(" 确认下楼？", Style::default().fg(Color::Yellow).bold()),
                                    Span::raw(" "), Span::styled("[Y]是", Style::default().fg(Color::Yellow)),
                                    Span::raw(" "), Span::styled("[N]否", Style::default().fg(Color::DarkGray)),
                                ])).block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::Yellow)));
                                frame.render_widget(confirm, Rect { x: area.width/2-10, y: area.height/2, width: 20, height: 3, });
                            })?;
                            if let Event::Key(k2) = event::read()? {
                                if matches!(k2.code, KeyCode::Char('y') | KeyCode::Char('Y')) {
                                    descend(world);
                                    let save = GameSave::from_world(world);
                                    if let Ok(data) = bincode::serialize(&save) {
                                        std::fs::write("save.bin", data).ok();
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        // ── 行动后处理 ────────────────────────────
        rebuild_occupancy(world);
        let _ = world.run_system_once(movement_system);
        let _ = world.run_system_once(pickup_system);
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

    // ── 三分栏：行动轴(左) | 地图(中) | 状态面板(右) ─

    let timeline_width: u16 = 20;
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

    // ── 行动轴（左栏） ────────────────────────────
    let timeline_lines = build_timeline(world, player_visible.clone());
    frame.render_widget(
        Paragraph::new(timeline_lines)
            .style(Style::default().fg(Color::White))
            .block(Block::default()
                .title(" 行动轴 ")
                .borders(Borders::RIGHT)
                .border_style(Style::default().fg(Color::DarkGray))),
        timeline_area,
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

    // ── 读取装备+Buff 计算有效属性 ──────────
    // 有效属性 = 裸属性 + 装备加成 + Buff
    let eff_atk = {
        let mut q = world.query::<(&Inventory, &Equipment, Option<&Buffs>)>();
        q.iter(world).next()
            .map(|(inv, eq, bu)| effective_attack(s, inv, eq, bu))
            .unwrap_or(s.attack)
    };
    let eff_def = {
        let mut q = world.query::<(&Inventory, &Equipment, Option<&Buffs>)>();
        q.iter(world).next()
            .map(|(inv, eq, bu)| effective_defense(s, inv, eq, bu))
            .unwrap_or(s.defense)
    };

    // ── 属性(中文) ─────────────────────────────
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

    // ── 暴击 ────────────────────────────────────
    out.push(Line::from(vec![
        Span::styled(" 暴击率", Style::default().fg(Color::DarkGray)),
        Span::raw(format!("{:>5.1}%", s.crit_rate * 100.0)),
        Span::raw(" "),
        Span::styled("暴击伤害", Style::default().fg(Color::DarkGray)),
        Span::raw(format!("{:>4.0}%", s.crit_damage * 100.0)),
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

/// 获取实体的招式名
fn entity_attack_name(world: &World, entity: Entity) -> String {
    if world.get::<Player>(entity).is_some() {
        if let Some(class) = world.get::<PlayerClass>(entity) {
            return match class {
                PlayerClass::Warrior => "斩击",
                PlayerClass::Mage => "火球术",
                PlayerClass::Priest => "惩击",
            }.into();
        }
    }
    if let Some(name) = world.get::<EntityName>(entity) {
        return match name.0.as_str() {
            "老鼠" => "撕咬",
            "哥布林" => "重击",
            _ => "攻击",
        }.into();
    }
    "攻击".into()
}

/// 根据怪物当前状态预测其下一次行动
fn monster_action_desc(brain: &MonsterBrain, stats: &Stats, can_see_player: bool) -> &'static str {
    // 血量低于逃跑阈值 → 逃跑
    for b in &brain.behaviors {
        if let AiBehavior::FleeWhenHurt { hp_threshold } = b {
            if (stats.hp as f32) < (stats.max_hp as f32) * hp_threshold {
                return "逃跑";
            }
        }
    }
    // 能看到玩家 → 追击
    if can_see_player {
        return "追击";
    }
    // 其余 → 游荡
    "游荡"
}

/// 构造左侧行动轴
/// 从上到下: 实体 | 剩余(ms) | 招式 — 剩余从小到大（越靠上越快行动）
fn build_timeline(world: &mut World, player_visible: HashSet<(usize, usize)>) -> Vec<Line<'static>> {
    let mut out: Vec<Line<'static>> = Vec::new();

    // 玩家待执行行动 + 当前行动成本（不同招式不同成本）
    let (pending_action, pending_cost) = {
        let p = world.resource::<PendingPlayerAction>();
        (p.action_name.clone(), p.action_cost)
    };

    // (name, remaining_ms, action_desc, is_player)
    let mut raw: Vec<(String, f32, String, bool)> = Vec::new();

    // 玩家：用 pending_cost 计算剩余（不同招式不同行动值）
    if let Some((_player_e, ap)) = world.query::<(Entity, &ActionPoints)>().iter(world).find(|(e, _)| world.get::<Player>(*e).is_some()) {
        let remain = (pending_cost - ap.points).max(0.0);
        raw.push(("@".to_string(), remain, pending_action, true));
    }

    // 视野内怪物 — 判断怪物能否看见玩家（玩家位置在怪物视野范围？）
    let player_pos_opt = world.query::<(&Player, &Position)>().iter(world).next().map(|(_, p)| (p.x, p.y));
    for (pos, name, ap, brain, stats) in world.query::<(&Position, &EntityName, &ActionPoints, &MonsterBrain, &Stats)>().iter(world) {
        if !player_visible.contains(&(pos.x, pos.y)) { continue; }
        let chop: String = name.0.chars().take(5).collect();
        let cost = entity_action_cost(ap.speed);
        let remain = (cost - ap.points).max(0.0);
        let can_see_player = player_pos_opt.map(|(px, py)| {
            let dx = pos.x.abs_diff(px);
            let dy = pos.y.abs_diff(py);
            dx * dx + dy * dy <= 8 * 8 // 怪物视野范围 8
        }).unwrap_or(false);
        let action_desc = monster_action_desc(brain, stats, can_see_player);
        raw.push((chop, remain, action_desc.into(), false));
    }

    // 按剩余 ms 从小到大排序
    raw.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());

    // ── 表头 ──
    out.push(Line::from(vec![
        Span::styled(" 实体  ", Style::default().fg(Color::DarkGray)),
        Span::styled("剩余 ", Style::default().fg(Color::DarkGray)),
        Span::styled("招式      ", Style::default().fg(Color::DarkGray)),
    ]));
    out.push(Line::from(Span::styled(
        "─".repeat(19), Style::default().fg(Color::DarkGray),
    )));

    // ── 每行 ──
    for (name, remain, action_desc, is_player) in &raw {
        let remain_u32 = remain.round() as u32;
        let (fg, prefix) = if *is_player { (Color::Yellow, "▸") } else { (Color::Cyan, " ") };
        let remain_str = format!("{:>3}", remain_u32);
        // 招式名最多6个字符
        let act_trim: String = action_desc.chars().take(6).collect();

        out.push(Line::from(vec![
            Span::styled(format!("{}{:<5}", prefix, name), Style::default().fg(fg)),
            Span::raw(" "),
            Span::styled(remain_str, Style::default().fg(fg)),
            Span::raw(" "),
            Span::styled(act_trim, Style::default().fg(if *is_player { Color::Green } else { Color::DarkGray })),
        ]));
    }

    if raw.is_empty() {
        out.push(Line::from(Span::styled(" (视野无实体)", Style::default().fg(Color::DarkGray))));
    }

    out.push(Line::from(Span::raw("")));
    out.push(Line::from(Span::styled(" ↑↓←→移动", Style::default().fg(Color::DarkGray))));
    out.push(Line::from(Span::styled(" 1-4技能  .等待", Style::default().fg(Color::DarkGray))));
    out.push(Line::from(Span::styled(" e背包  >下楼", Style::default().fg(Color::DarkGray))));

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
                format!("剩余点数: {}  按 A/F/M/G 分配  Enter 确认", points),
                Style::default().fg(Color::Yellow),
            )));
            lines.push(Line::from(Span::raw("")));
            lines.push(Line::from(Span::raw(format!(
                " [A] 攻击   : {:>2}  →  {}", stats.attack, stats.attack + 1,
            ))));
            lines.push(Line::from(Span::raw(format!(
                " [F] 防御   : {:>2}  →  {}", stats.defense, stats.defense + 1,
            ))));
            lines.push(Line::from(Span::raw(format!(
                " [M] 法术精通: {:>2}  →  {}", stats.magic_mastery, stats.magic_mastery + 1,
            ))));
            lines.push(Line::from(Span::raw(format!(
                " [G] 敏捷   : {:>2}  →  {}", stats.agility, stats.agility + 1,
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
                    if points > 0 {
                        let mut q = world.query::<&mut Stats>();
                        if let Some(mut s) = q.iter_mut(world).next() {
                            s.attack += points.min(1);
                            s.defense += (points.saturating_sub(1)).min(1);
                            s.magic_mastery += points.saturating_sub(2).min(1);
                            s.agility += points.saturating_sub(3);
                            s.max_hp = dungeon_core::max_hp_for(s.level, s.defense);
                            s.max_mp = dungeon_core::max_mp_for(s.level, s.magic_mastery);
                            s.hp = s.max_hp;
                            s.mp = s.max_mp;
                        }
                    }
                    world.resource_mut::<PendingLevelUp>().points = 0;
                    break;
                }
                KeyCode::Char('a') | KeyCode::Char('A') if points > 0 => {
                    let mut q = world.query::<&mut Stats>();
                    if let Some(mut s) = q.iter_mut(world).next() {
                        s.attack += 1;
                        points -= 1;
                    }
                }
                KeyCode::Char('f') | KeyCode::Char('F') if points > 0 => {
                    let mut q = world.query::<&mut Stats>();
                    if let Some(mut s) = q.iter_mut(world).next() {
                        s.defense += 1;
                        s.max_hp = dungeon_core::max_hp_for(s.level, s.defense);
                        s.hp = s.max_hp;
                        points -= 1;
                    }
                }
                KeyCode::Char('m') | KeyCode::Char('M') if points > 0 => {
                    let mut q = world.query::<&mut Stats>();
                    if let Some(mut s) = q.iter_mut(world).next() {
                        s.magic_mastery += 1;
                        s.max_mp = dungeon_core::max_mp_for(s.level, s.magic_mastery);
                        s.mp = s.max_mp;
                        points -= 1;
                    }
                }
                KeyCode::Char('g') | KeyCode::Char('G') if points > 0 => {
                    let mut q = world.query::<&mut Stats>();
                    if let Some(mut s) = q.iter_mut(world).next() {
                        s.agility += 1;
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
                Line::from(Span::styled("  ↑↓←→移动 1-4技能 Enter确认 e背包 >下楼 q退出", Style::default().fg(Color::DarkGray))),
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
