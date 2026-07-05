use std::io::{self, stdout};
use std::time::Instant;

use bevy_ecs::prelude::Entity;
use bevy_ecs::system::RunSystemOnce;
use dungeon_core::world;
use crossterm::event::{self, Event, KeyCode};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::ExecutableCommand;
use dungeon_core::{
    check_death_system, descend,
    fov_system, rebuild_occupancy, save::GameSave,
    setup_world, update_map_memory,
    Equipment, EquipmentSlot, EventLog, Inventory, ItemPickup, ItemStack,
    Player, Position, Renderable, Skills, Stairs, TurnManager,
};
use dungeon_core::action::ActionKindV3;
use dungeon_render::{draw_title, render_ui};
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Terminal;

fn main() -> io::Result<()> {
    // panic hook: 将 panic 写入文件（raw 模式下看不到 stderr）
    std::panic::set_hook(Box::new(|info| {
        let msg = format!("PANIC: {}\n", info);
        std::fs::write("panic.log", msg).ok();
    }));

    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let mut terminal = Terminal::new(ratatui::backend::CrosstermBackend::new(stdout()))?;
    let game_start = title_screen(&mut terminal)?;
    let result = run(&mut terminal, game_start);
    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    result
}

fn player_entity() -> Option<Entity> {
    let mut w = world!(mut);
    let mut q = w.query::<(Entity, &Player)>();
    q.iter(&mut *w).next().map(|(e, _)| e)
}

// ══════════════════════════════════════════════════════
// 主循环
// ══════════════════════════════════════════════════════

fn run(
    terminal: &mut Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
    game_start: Instant,
) -> io::Result<()> {
    rebuild_occupancy();
    world!(mut).run_system_once(fov_system);
    terminal.draw(|frame| render_ui(frame, game_start))?;

    loop {
        let w = world!();
        if w.resource::<TurnManager>().game_over || w.resource::<TurnManager>().wants_quit {
            break Ok(());
        }
        drop(w);

        terminal.draw(|frame| render_ui(frame, game_start))?;

        if let Event::Key(key) = event::read()? {
            handle_input(terminal, key)?;
            advance_and_settle();
        }
    }
}

fn advance_and_settle() {
    use dungeon_core::action::{tick_all_cooldowns, run_monster_decision, advance_action_queue};

    let dist = advance_action_queue();

    if dist > 0.0 {
        tick_all_cooldowns(dist);
    }

    run_monster_decision();

    rebuild_occupancy();
    world!(mut).run_system_once(fov_system);
    update_map_memory();
    world!(mut).run_system_once(check_death_system);
}

// ══════════════════════════════════════════════════════
// 拾取地面物品（g 键）
// ══════════════════════════════════════════════════════

fn pickup_ground() {
    let (ppx, ppy) = {
        let mut w = world!(mut);
        let mut q = w.query::<(&Player, &Position)>();
        q.iter(&mut *w).next().map(|(_, p)| (p.x, p.y)).unwrap_or((0, 0))
    };
    // 收集地面物品
    let ground: Vec<(Entity, ItemStack)> = {
        let mut w = world!(mut);
        let mut q = w.query::<(Entity, &ItemPickup, &Position)>();
        q.iter(&mut *w)
            .filter(|(_, _, pos)| pos.x == ppx && pos.y == ppy)
            .map(|(e, p, _)| (e, p.stack.clone()))
            .collect()
    };
    if ground.is_empty() { return; }
    // 加入背包
    let mut logs = Vec::new();
    let mut despawn = Vec::new();
    for (entity, stack) in &ground {
        let mut w = world!(mut);
        let mut q = w.query::<(&mut Inventory,)>();
        if let Some((mut inv,)) = q.iter_mut(&mut *w).next() {
            let leftover = inv.add(stack.item_id, stack.count);
            let picked = stack.count - leftover;
            if picked > 0 {
                logs.push(format!("拾取了{}x{}", stack.name(), picked));
            }
            despawn.push(*entity);
        }
    }
    for e in despawn {
        let mut w = world!(mut);
        w.entity_mut(e).despawn();
    }
    for msg in logs {
        let mut w = world!(mut);
        w.resource_mut::<EventLog>().push(msg);
    }
}

// ══════════════════════════════════════════════════════
// 输入处理
// ══════════════════════════════════════════════════════

fn handle_input(
    terminal: &mut Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
    key: crossterm::event::KeyEvent,
) -> io::Result<()> {
    match key.code {
        KeyCode::Up => handle_player_direction(0, -1),
        KeyCode::Down => handle_player_direction(0, 1),
        KeyCode::Left => handle_player_direction(-1, 0),
        KeyCode::Right => handle_player_direction(1, 0),
        KeyCode::Char('.') => {
            use dungeon_core::action::{ActionKindV3, Reaction, CanWait};
            if let Some(e) = player_entity() {
                let reaction_time = world!().get::<Reaction>(e).map(|r| r.time).unwrap_or(50.0);
                let duration = world!().get::<CanWait>(e).map(|w| w.duration).unwrap_or(800.0);
                handle_timed_action(e, ActionKindV3::Wait, reaction_time, duration);
            }
        }
        KeyCode::Char('1') | KeyCode::Char('2') | KeyCode::Char('3') | KeyCode::Char('4') => {
            use dungeon_core::action::{ActionKindV3, Reaction};
            let (idx, _) = skill_info(key);
            if let Some(e) = player_entity() {
                let reaction_time = world!().get::<Reaction>(e).map(|r| r.time).unwrap_or(50.0);
                handle_timed_action(e, ActionKindV3::Skill(idx), reaction_time, 600.0);
            }
        }
        KeyCode::Char('q') | KeyCode::Esc => {
            if confirm_quit(terminal)? {
                world!(mut).resource_mut::<TurnManager>().wants_quit = true;
            }
        }
        KeyCode::Char('e') | KeyCode::Char('E') => {
            open_inventory(terminal)?;
        }
        KeyCode::Char('g') | KeyCode::Char('G') => {
            pickup_ground();
        }
        KeyCode::F(5) => {
            if let Ok(data) = bincode::serialize(&GameSave::capture()) {
                std::fs::write("save.bin", data).ok();
                world!(mut).resource_mut::<EventLog>().push("已保存");
            }
        }
        KeyCode::F(9) => {
            if let Ok(data) = std::fs::read("save.bin") {
                if let Ok(save) = bincode::deserialize::<GameSave>(&data) {
                    save.restore();
                    world!(mut).resource_mut::<EventLog>().push("已读档");
                }
            }
        }
        KeyCode::Char('>') => {
            if on_stairs() && confirm_stairs(terminal)? {
                descend();
            }
        }
        _ => {}
    }
    Ok(())
}

fn handle_timed_action(entity: Entity, kind: ActionKindV3, reaction_time: f32, duration: f32) {
    use dungeon_core::action::{PlayerPreview, ActionQueue};

    let is_confirm = {
        let w = world!();
        let preview = w.resource::<PlayerPreview>();
        match (&preview.kind, &kind) {
            (Some(ActionKindV3::Move { dx: pd, dy: pd2 }), ActionKindV3::Move { dx, dy })
                if *pd == *dx && *pd2 == *dy => true,
            (Some(ActionKindV3::Wait), ActionKindV3::Wait) => true,
            (Some(ActionKindV3::Skill(a)), ActionKindV3::Skill(b)) if *a == *b => true,
            (Some(ActionKindV3::Attack { .. }), ActionKindV3::Attack { .. }) => true,
            _ => false,
        }
    };

    if is_confirm {
        world!(mut).resource_mut::<ActionQueue>().enqueue(entity, kind, reaction_time, duration);
        world!(mut).resource_mut::<PlayerPreview>().kind = None;
    } else {
        world!(mut).resource_mut::<PlayerPreview>().kind = Some(kind);
    }
}

fn handle_player_direction(dx: isize, dy: isize) {
    use dungeon_core::action::{ActionKindV3, Reaction, CanMove};
    use dungeon_core::{Map, Tile, OccupancyMap, MAP_WIDTH, MAP_HEIGHT, Monster};
    let Some(entity) = player_entity() else { return };

    let kind = {
        let w = world!();
        let Some(pos) = w.get::<Position>(entity) else { return };
        let nx = pos.x.wrapping_add_signed(dx);
        let ny = pos.y.wrapping_add_signed(dy);
        if nx >= MAP_WIDTH || ny >= MAP_HEIGHT { return; }
        let tile = w.resource::<Map>().tiles[ny][nx];
        let has_enemy = w.resource::<OccupancyMap>().cells[ny][nx]
            .and_then(|e| if w.get::<Monster>(e).is_some() { Some(e) } else { None });
        if tile != Tile::Floor && has_enemy.is_none() { return; }
        if let Some(target) = has_enemy {
            ActionKindV3::Attack { target }
        } else {
            ActionKindV3::Move { dx, dy }
        }
    };

    let reaction_time = world!().get::<Reaction>(entity).map(|r| r.time).unwrap_or(50.0);
    let duration = world!().get::<CanMove>(entity).map(|m| m.duration).unwrap_or(300.0);
    handle_timed_action(entity, kind, reaction_time, duration);
}

fn skill_info(key: crossterm::event::KeyEvent) -> (usize, Option<String>) {
    let idx = match key.code {
        KeyCode::Char('1') => 0,
        KeyCode::Char('2') => 1,
        KeyCode::Char('3') => 2,
        _ => 3,
    };
    let names: Vec<String> = {
        let mut w = world!(mut);
        let mut q = w.query::<&Skills>();
        q.iter(&mut *w)
            .next()
            .map(|sk| sk.list.iter().map(|s| s.name.to_string()).collect())
            .unwrap_or_default()
    };
    (idx, names.get(idx).cloned())
}

// ══════════════════════════════════════════════════════
// 工具函数
// ══════════════════════════════════════════════════════

fn on_stairs() -> bool {
    let mut w = world!(mut);
    let pp = {
        let mut q = w.query::<&Position>();
        *q.iter(&mut *w)
            .next()
            .unwrap_or(&Position { x: 0, y: 0 })
    };
    let mut q2 = w.query::<(&Stairs, &Position)>();
    q2.iter(&mut *w).any(|(_, sp)| sp.x == pp.x && sp.y == pp.y)
}

fn confirm_quit(
    terminal: &mut Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
) -> io::Result<bool> {
    terminal.draw(|frame| {
        let area = frame.area();
        let msg = Paragraph::new(Line::from(vec![
            Span::styled(" 确认退出？", Style::default().fg(Color::Red).bold()),
            Span::raw(" "),
            Span::styled("[Y]是", Style::default().fg(Color::Yellow)),
            Span::raw(" "),
            Span::styled("[N]否", Style::default().fg(Color::DarkGray)),
        ]))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Red)),
        );
        frame.render_widget(
            msg,
            Rect {
                x: area.width / 2 - 10,
                y: area.height / 2,
                width: 20,
                height: 3,
            },
        );
    })?;
    loop {
        if let Event::Key(k) = event::read()? {
            return Ok(matches!(k.code, KeyCode::Char('y') | KeyCode::Char('Y')));
        }
    }
}

fn confirm_stairs(
    terminal: &mut Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
) -> io::Result<bool> {
    terminal.draw(|frame| {
        let area = frame.area();
        let msg = Paragraph::new(Line::from(vec![
            Span::styled(" 确认下楼？", Style::default().fg(Color::Yellow).bold()),
            Span::raw(" "),
            Span::styled("[Y]是", Style::default().fg(Color::Yellow)),
            Span::raw(" "),
            Span::styled("[N]否", Style::default().fg(Color::DarkGray)),
        ]))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow)),
        );
        frame.render_widget(
            msg,
            Rect {
                x: area.width / 2 - 10,
                y: area.height / 2,
                width: 20,
                height: 3,
            },
        );
    })?;
    loop {
        if let Event::Key(k) = event::read()? {
            return Ok(matches!(k.code, KeyCode::Char('y') | KeyCode::Char('Y')));
        }
    }
}

fn title_screen(
    terminal: &mut Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
) -> io::Result<Instant> {
    loop {
        terminal.draw(|frame| draw_title(frame))?;
        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Enter | KeyCode::Char('\n') | KeyCode::Char('\r') => {
                    let world = setup_world();
                    dungeon_core::global::set_world(world);
                    world!(mut).run_system_once(fov_system);
                    update_map_memory();
                    return Ok(Instant::now());
                }
                KeyCode::F(9) => {
                    if let Ok(data) = std::fs::read("save.bin") {
                        if let Ok(save) = bincode::deserialize::<GameSave>(&data) {
                            save.restore();
                            world!(mut).run_system_once(fov_system);
                            update_map_memory();
                            return Ok(Instant::now());
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

// ══════════════════════════════════════════════════════
// 背包界面（双栏 + 详情页）
// ══════════════════════════════════════════════════════

#[derive(Clone, Copy, PartialEq, Eq)]
enum InvPanel { Left, Right }

struct InvState {
    panel: InvPanel,
    left_sel: usize,
    left_scroll: usize,
    right_sel: usize,
    right_scroll: usize,
    detail: Option<(DetailSource, usize)>, // (source, stack index)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DetailSource { LeftInv, LeftEquip, Right }

fn collect_ground_items_in(world: &mut bevy_ecs::prelude::World) -> Vec<(ItemStack, Entity)> {
    let pp = {
        let mut q = world.query::<(&Player, &Position)>();
        q.iter(world).next().map(|(_, p)| (p.x, p.y)).unwrap_or((0, 0))
    };
    let mut items = Vec::new();
    let mut q = world.query::<(Entity, &ItemPickup, &Position)>();
    for (entity, pickup, pos) in q.iter(world) {
        if pos.x == pp.0 && pos.y == pp.1 {
            items.push((pickup.stack.clone(), entity));
        }
    }
    items
}

fn open_inventory(
    terminal: &mut Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
) -> io::Result<()> {
    let mut st = InvState {
        panel: InvPanel::Left,
        left_sel: 0, left_scroll: 0,
        right_sel: 0, right_scroll: 0,
        detail: None,
    };

    loop {
        // ── 读取数据 ──
        let (inv_stacks, inv_cap, equip, ground): (Vec<ItemStack>, usize, Equipment, Vec<(ItemStack, Entity)>) = {
            let mut w = world!(mut);
            let mut q = w.query::<(&Inventory, &Equipment)>();
            let (inv, eq) = q.iter_mut(&mut *w).next().map(|(i, e)| (i.clone(), e.clone())).unwrap_or_default();
            let ground = collect_ground_items_in(&mut *w);
            (inv.stacks, inv.capacity, eq, ground)
        };

        // ── 构建 UI ──
        terminal.draw(|frame| {
            let area = frame.area();
            let block = Block::default()
                .title("  背包  ")
                .title_alignment(Alignment::Center)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan));
            frame.render_widget(block, area);
            let inner = inner_rect(area, 1);

            if let Some((dsrc, idx)) = &st.detail {
                // ── 详情页 ──
                let (stack, source_label) = match dsrc {
                    DetailSource::LeftInv => (inv_stacks.get(*idx), "背包"),
                    DetailSource::LeftEquip => {
                        let all_equip = equip.equipped_stacks();
                        let stacks: Vec<&ItemStack> = all_equip.into_iter().collect();
                        (stacks.get(*idx).copied(), "装备")
                    }
                    DetailSource::Right => (ground.get(*idx).map(|(s, _)| s), "地面"),
                };
                if let Some(item) = stack {
                    let def = item.def();
                    let mut lines = Vec::new();
                    lines.push(Line::from(Span::styled(
                        format!(" ── {} ──", source_label),
                        Style::default().fg(Color::DarkGray),
                    )));
                    lines.push(Line::from(Span::raw("")));
                    lines.push(Line::from(Span::styled(
                        format!(" {}", item.name()),
                        Style::default().fg(Color::Yellow).bold(),
                    )));
                    if item.count > 1 {
                        lines.push(Line::from(Span::styled(
                            format!(" 数量: {}", item.count),
                            Style::default().fg(Color::White),
                        )));
                    }
                    if let Some(d) = def {
                        lines.push(Line::from(Span::styled(
                            format!(" 槽位: {:?}", d.slot),
                            Style::default().fg(Color::DarkGray),
                        )));
                    }
                    lines.push(Line::from(Span::raw("")));
                    let desc = item.description();
                    if !desc.is_empty() {
                        lines.push(Line::from(Span::styled(
                            format!(" {}", desc),
                            Style::default().fg(Color::DarkGray),
                        )));
                    }
                    lines.push(Line::from(Span::raw("")));
                    lines.push(Line::from(Span::raw("")));
                    // 操作提示
                    match dsrc {
                        DetailSource::LeftInv => {
                            if let Some(d) = def {
                                match d.slot {
                                    EquipmentSlot::Material => {
                                        lines.push(Line::from(Span::styled(" d:丢弃", Style::default().fg(Color::DarkGray))));
                                    }
                                    _ => {
                                        lines.push(Line::from(Span::styled(" e:装备  d:丢弃", Style::default().fg(Color::DarkGray))));
                                    }
                                }
                            }
                        }
                        DetailSource::LeftEquip => {
                            lines.push(Line::from(Span::styled(" u:卸载装备", Style::default().fg(Color::DarkGray))));
                        }
                        DetailSource::Right => {
                            lines.push(Line::from(Span::styled(" g:拾取", Style::default().fg(Color::DarkGray))));
                        }
                    }
                    lines.push(Line::from(Span::styled(" Esc:返回", Style::default().fg(Color::DarkGray))));
                    frame.render_widget(Paragraph::new(lines), inner);
                }
            } else {
                // ── 双栏布局 ──
                let half = inner.width / 2;
                let left_area = Rect { x: inner.x, y: inner.y, width: half, height: inner.height };
                let right_area = Rect { x: inner.x + half, y: inner.y, width: inner.width - half, height: inner.height };

                // ── 左栏：背包 + 装备 ──
                {
                    let mut lines: Vec<Line> = Vec::new();
                    let active = st.panel == InvPanel::Left;
                    let title_style = if active { Style::default().fg(Color::Cyan).bold() } else { Style::default().fg(Color::DarkGray) };
                    lines.push(Line::from(Span::styled(" ── 装备 ──", title_style)));

                    // 装备槽
                    let equip_items: Vec<(&str, Option<&ItemStack>)> = vec![
                        ("武器", equip.weapon.as_ref()),
                        ("防具", equip.armor.as_ref()),
                        ("戒指", equip.ring.as_ref()),
                    ];
                    for (i, (label, item)) in equip_items.iter().enumerate() {
                        let name = item.map(|s| s.name()).unwrap_or("(空)".into());
                        let prefix = if active && st.panel == InvPanel::Left && st.left_sel == usize::MAX - i {
                            "▸ "
                        } else { "  " };
                        lines.push(Line::from(vec![
                            Span::styled(format!("{}[{}]", prefix, label.chars().next().unwrap()), Style::default().fg(Color::Yellow)),
                            Span::raw(format!(" {}", name)),
                        ]));
                    }

                    lines.push(Line::from(Span::styled(
                        format!(" ── 背包 ({}/{}) ──", inv_stacks.len(), inv_cap),
                        title_style,
                    )));

                    if inv_stacks.is_empty() {
                        lines.push(Line::from(Span::styled(" (空)", Style::default().fg(Color::DarkGray))));
                    } else {
                        let sel = if active { st.left_sel } else { st.left_sel };
                        let page_size = (left_area.height as usize).saturating_sub(8).min(15);
                        let sc = st.left_scroll.min(sel.min(inv_stacks.len().saturating_sub(1)));
                        let end = (sc + page_size).min(inv_stacks.len());
                        for i in sc..end {
                            let stack = &inv_stacks[i];
                            let idx_char = if i < 36 { char::from_digit((i as u32) % 10, 10).unwrap_or('?') } else { '?' };
                            let prefix = if active && i == st.left_sel { "▸" } else { " " };
                            let count_label = if stack.count > 1 { format!(" x{}", stack.count) } else { String::new() };
                            let slot_label = stack.def().map(|d| format!("{:?}", d.slot)).unwrap_or_default();
                            lines.push(Line::from(vec![
                                Span::styled(format!("{}{}", prefix, idx_char), Style::default().fg(Color::Yellow)),
                                Span::raw(format!(" {}{}", stack.name(), count_label)),
                                Span::styled(format!(" {}", slot_label), Style::default().fg(Color::DarkGray)),
                            ]));
                        }
                    }
                    frame.render_widget(Paragraph::new(lines), left_area);
                }

                // ── 右栏：地面物品 ──
                {
                    let mut lines: Vec<Line> = Vec::new();
                    let active = st.panel == InvPanel::Right;
                    let title_style = if active { Style::default().fg(Color::Cyan).bold() } else { Style::default().fg(Color::DarkGray) };
                    lines.push(Line::from(Span::styled(" ── 地面 ──", title_style)));

                    if ground.is_empty() {
                        lines.push(Line::from(Span::styled(" (无物品)", Style::default().fg(Color::DarkGray))));
                    } else {
                        let sel = st.right_sel;
                        let page_size = (right_area.height as usize).saturating_sub(4).min(15);
                        let sc = st.right_scroll.min(sel.min(ground.len().saturating_sub(1)));
                        let end = (sc + page_size).min(ground.len());
                        for i in sc..end {
                            let (stack, _) = &ground[i];
                            let prefix = if active && i == st.right_sel { "▸" } else { " " };
                            let count_label = if stack.count > 1 { format!(" x{}", stack.count) } else { String::new() };
                            lines.push(Line::from(vec![
                                Span::styled(format!("{}", prefix), Style::default().fg(Color::Yellow)),
                                Span::raw(format!(" {}{}", stack.name(), count_label)),
                            ]));
                        }
                    }
                    lines.push(Line::from(Span::raw("")));
                    if active {
                        if !ground.is_empty() {
                            lines.push(Line::from(Span::styled(" Enter:查看  g:拾取全部", Style::default().fg(Color::DarkGray))));
                        }
                    }
                    frame.render_widget(Paragraph::new(lines), right_area);
                }
            }
        })?;

        // ── 输入处理 ──
        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => {
                    if st.detail.is_some() {
                        st.detail = None;
                    } else {
                        break;
                    }
                }
                KeyCode::Left => {
                    if st.detail.is_none() {
                        st.panel = InvPanel::Left;
                    }
                }
                KeyCode::Right => {
                    if st.detail.is_none() {
                        st.panel = InvPanel::Right;
                    }
                }
                KeyCode::Up => {
                    if st.detail.is_some() { continue; }
                    match st.panel {
                        InvPanel::Left => {
                            if st.left_sel > 0 { st.left_sel -= 1; }
                            if st.left_sel < st.left_scroll { st.left_scroll = st.left_sel; }
                        }
                        InvPanel::Right => {
                            if st.right_sel > 0 { st.right_sel -= 1; }
                            if st.right_sel < st.right_scroll { st.right_scroll = st.right_sel; }
                        }
                    }
                }
                KeyCode::Down => {
                    if st.detail.is_some() { continue; }
                    match st.panel {
                        InvPanel::Left => {
                            if inv_stacks.len() > 0 && st.left_sel < inv_stacks.len() - 1 {
                                st.left_sel += 1;
                                let page = 10;
                                if st.left_sel >= st.left_scroll + page { st.left_scroll += 1; }
                            }
                        }
                        InvPanel::Right => {
                            if ground.len() > 0 && st.right_sel < ground.len() - 1 {
                                st.right_sel += 1;
                                let page = 10;
                                if st.right_sel >= st.right_scroll + page { st.right_scroll += 1; }
                            }
                        }
                    }
                }
                KeyCode::Enter => {
                    if st.detail.is_some() { continue; }
                    match st.panel {
                        InvPanel::Left => {
                            if !inv_stacks.is_empty() {
                                st.detail = Some((DetailSource::LeftInv, st.left_sel));
                            }
                        }
                        InvPanel::Right => {
                            if !ground.is_empty() {
                                st.detail = Some((DetailSource::Right, st.right_sel));
                            }
                        }
                    }
                }
                KeyCode::Char('g') => {
                    // 拾取全部地面物品（主界面g或详情页g）
                    let (ppx, ppy) = {
                        let mut w = world!(mut);
                        let mut q = w.query::<(&Player, &Position)>();
                        q.iter(&mut *w).next().map(|(_, p)| (p.x, p.y)).unwrap_or((0, 0))
                    };
                    // 收集该格地面物品
                    let ground_items: Vec<(Entity, ItemStack)> = {
                        let mut w = world!(mut);
                        let mut q = w.query::<(Entity, &ItemPickup, &Position)>();
                        q.iter(&mut *w)
                            .filter(|(_, _, pos)| pos.x == ppx && pos.y == ppy)
                            .map(|(e, pickup, _)| (e, pickup.stack.clone()))
                            .collect()
                    };
                    // 拾取
                    let mut despawn_list = Vec::new();
                    let mut log_entries: Vec<String> = Vec::new();
                    {
                        let mut w = world!(mut);
                        let mut inv_q = w.query::<(&mut Inventory,)>();
                        if let Some((mut inv,)) = inv_q.iter_mut(&mut *w).next() {
                            for (entity, stack) in &ground_items {
                                let leftover = inv.add(stack.item_id, stack.count);
                                let picked = stack.count - leftover;
                                if picked > 0 {
                                    log_entries.push(format!("拾取了{}x{}", stack.name(), picked));
                                }
                                despawn_list.push(*entity);
                            }
                        }
                    }
                    // 写日志（释放了 inv_q 之后再拿 w）
                    if !log_entries.is_empty() {
                        let mut w = world!(mut);
                        for msg in &log_entries {
                            w.resource_mut::<EventLog>().push(msg.clone());
                        }
                    }
                    // 销毁已拾取的实体
                    {
                        let mut w = world!(mut);
                        for e in despawn_list { w.entity_mut(e).despawn(); }
                    }
                    if let Some((DetailSource::Right, _)) = st.detail { st.detail = None; }
                }
                KeyCode::Char('e') => {
                    // 装备：从背包详情页触发
                    if let Some((DetailSource::LeftInv, idx)) = st.detail {
                        let mut w = world!(mut);
                        let mut q = w.query::<(&mut Inventory, &mut Equipment)>();
                        if let Some((mut inv, mut eq)) = q.iter_mut(&mut *w).next() {
                            if let Some(stack) = inv.stacks.get(idx) {
                                let item_id = stack.item_id;
                                let count = stack.count;
                                if let Some(def) = stack.def() {
                                    let can_equip = match def.slot {
                                        EquipmentSlot::Weapon => eq.weapon.is_none(),
                                        EquipmentSlot::Armor => eq.armor.is_none(),
                                        EquipmentSlot::Ring => eq.ring.is_none(),
                                        EquipmentSlot::Material => false,
                                    };
                                    if can_equip {
                                        // 从背包移除 1 个
                                        inv.remove(idx, 1);
                                        let equip_stack = ItemStack::new(item_id, 1);
                                        match def.slot {
                                            EquipmentSlot::Weapon => eq.weapon = Some(equip_stack),
                                            EquipmentSlot::Armor => eq.armor = Some(equip_stack),
                                            EquipmentSlot::Ring => eq.ring = Some(equip_stack),
                                            _ => {}
                                        }
                                        st.detail = None;
                                    }
                                }
                            }
                        }
                    }
                }
                KeyCode::Char('d') => {
                    // 丢弃：从背包详情页触发
                    if let Some((DetailSource::LeftInv, idx)) = st.detail {
                        let mut w = world!(mut);
                        let pp = {
                            let mut q = w.query::<(&Player, &Position)>();
                            q.iter(&mut *w).next().map(|(_, p)| (p.x, p.y)).unwrap_or((0, 0))
                        };
                        let mut q = w.query::<(&mut Inventory,)>();
                        if let Some((mut inv,)) = q.iter_mut(&mut *w).next() {
                            if let Some(stack) = inv.stacks.get(idx) {
                                let drop_stack = ItemStack::new(stack.item_id, 1);
                                inv.remove(idx, 1);
                                w.spawn((
                                    ItemPickup { stack: drop_stack.clone() },
                                    Position { x: pp.0, y: pp.1 },
                                    Renderable { glyph: drop_stack.glyph(), color: drop_stack.color() },
                                ));
                                st.detail = None;
                            }
                        }
                    }
                }
                KeyCode::Char('u') => {
                    // 卸载装备
                    if let Some((DetailSource::LeftEquip, idx)) = st.detail {
                        let mut w = world!(mut);
                        let mut q = w.query::<(&mut Inventory, &mut Equipment)>();
                        if let Some((mut inv, mut eq)) = q.iter_mut(&mut *w).next() {
                            let slot = match idx {
                                0 => &mut eq.weapon,
                                1 => &mut eq.armor,
                                2 => &mut eq.ring,
                                _ => unreachable!(),
                            };
                            if let Some(stack) = slot.take() {
                                let leftover = inv.add(stack.item_id, stack.count);
                                if leftover > 0 {
                                    // 背包满了，掉地上
                                    let pp = {
                                        let mut p = w.query::<(&Player, &Position)>();
                                        p.iter(&mut *w).next().map(|(_, p)| (p.x, p.y)).unwrap_or((0, 0))
                                    };
                                    w.spawn((
                                        ItemPickup { stack: ItemStack::new(stack.item_id, leftover) },
                                        Position { x: pp.0, y: pp.1 },
                                        Renderable { glyph: stack.glyph(), color: stack.color() },
                                    ));
                                }
                                st.detail = None;
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
    Ok(())
}

fn inner_rect(area: Rect, border: u16) -> Rect {
    Rect {
        x: area.x + border,
        y: area.y + border,
        width: area.width.saturating_sub(border * 2),
        height: area.height.saturating_sub(border * 2),
    }
}
