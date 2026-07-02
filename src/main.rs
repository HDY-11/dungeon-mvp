//! Dungeon MVP — 主循环、输入处理、模式切换。
//!
//! 逻辑层：dungeon-core（ECS 组件、系统、AV 引擎）
//! 渲染层：dungeon-render（UI 绘制、行动轴、状态面板）
//! 应用层：本文件（主循环、输入、模式）

use std::collections::HashSet;
use std::io::{self, stdout};
use std::time::Instant;

use bevy_ecs::prelude::{Entity, World};
use bevy_ecs::system::RunSystemOnce;
use crossterm::event::{self, Event, KeyCode};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::ExecutableCommand;
use dungeon_core::{
    action_cost, advance_by, advance_to_next_decision_point, apply_skill, check_death_system, descend,
    fov_system, predict_monster_actions_system, rebuild_occupancy, save::GameSave,
    setup_world, skill_tick_system, update_map_memory, ActionKind, ActionPrediction,
    ActionValue, Equipment, EquipmentSlot, EventLog, GamePacing, Inventory, ItemInstance,
    ManualOverride, Monster, PacingMode, PendingInput, PendingLevelUp, PendingPlayerAction,
    Player, Position, Skills, Stairs, Stats, TurnManager, Viewshed,
};
use dungeon_render::{draw_level_up, draw_title, render_ui};
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Terminal;

fn main() -> io::Result<()> {
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let mut terminal = Terminal::new(ratatui::backend::CrosstermBackend::new(stdout()))?;
    let (mut world, game_start) = title_screen(&mut terminal)?;
    let result = run(&mut terminal, &mut world, game_start);
    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    result
}

// ══════════════════════════════════════════════════════
// 玩家行动函数（统一路径）
// ══════════════════════════════════════════════════════

fn player_entity(world: &mut World) -> Option<Entity> {
    let mut q = world.query::<(Entity, &Player)>();
    q.iter(world).next().map(|(e, _)| e)
}

fn player_prediction(world: &mut World) -> Option<ActionPrediction> {
    let mut q = world.query::<(&Player, &ActionPrediction)>();
    q.iter(world).next().map(|(_, p)| p.clone())
}

/// 仅填充预测（不提交），保留已有锁定状态。
fn write_player_prediction(world: &mut World, dx: isize, dy: isize) {
    let Some(entity) = player_entity(world) else { return };
    if let Some(mut pred) = world.get_mut::<ActionPrediction>(entity) {
        if pred.locked {
            return;
        }
        pred.desc = "移动".into();
        pred.kind = ActionKind::Move;
        pred.just_confirmed = false;
    }
}

/// 仅填充技能预测（不提交）。
fn write_skill_prediction(world: &mut World, skill_idx: usize, name: &str) {
    let Some(entity) = player_entity(world) else { return };
    *world.resource_mut::<PendingPlayerAction>() = PendingPlayerAction::new_skill(skill_idx, name);
    if let Some(mut pred) = world.get_mut::<ActionPrediction>(entity) {
        pred.desc = format!("技能:{}", name);
        pred.kind = ActionKind::Skill(skill_idx);
        pred.locked = false;
    }
}

/// 统一提交：写预测 + 排入 AV 队列（设为行动成本）。
/// 执行时机由 `advance_by` 在 AV 递减至 0 时触发。
///
/// 技能效果立即执行，AV 设为技能成本作为冷却。
fn commit_player_action(world: &mut World) {
    let Some(entity) = player_entity(world) else { return };
    let pending = world.resource::<PendingPlayerAction>().clone();
    let agility = world.get::<Stats>(entity).map(|s| s.agility).unwrap_or(10);

    // 已被锁定 → 解锁，推进到玩家 AV=0（其他实体同步减）
    let already_locked = world.get::<ActionPrediction>(entity)
        .map(|p| p.locked).unwrap_or(false);

    if already_locked {
        let skipped = world.get::<ActionValue>(entity).map(|av| av.current_av).unwrap_or(0.0);
        if let Some(mut pred) = world.get_mut::<ActionPrediction>(entity) {
            pred.locked = false;
        }
        // 同步推进所有实体（包括玩家）跳过玩家的剩余 AV
        advance_by(world, skipped);
    } else if pending.is_pending_skill {
        if let Some(si) = pending.skill_idx {
            apply_skill(world, si);
            let _ = world.run_system_once(skill_tick_system);
        }
        if let Some(mut av) = world.get_mut::<ActionValue>(entity) {
            *av = ActionValue::with_cost(action_cost::SKILL_CAST, agility);
        }
    } else if world.resource::<PendingInput>().direction.is_some() {
        if let Some(mut av) = world.get_mut::<ActionValue>(entity) {
            *av = ActionValue::with_cost(action_cost::MOVE, agility);
        }
    } else {
        if let Some(mut av) = world.get_mut::<ActionValue>(entity) {
            *av = ActionValue::with_cost(action_cost::WAIT, agility);
        }
        if let Some(mut pred) = world.get_mut::<ActionPrediction>(entity) {
            pred.desc = "等待".into();
            pred.kind = ActionKind::Wait;
        }
    }

    if let Some(mut pred) = world.get_mut::<ActionPrediction>(entity) {
        pred.locked = false;
        pred.just_confirmed = false;
    }

    world.resource_mut::<PendingPlayerAction>().is_pending_skill = false;
    world.resource_mut::<PendingPlayerAction>().skill_idx = None;
}

// ══════════════════════════════════════════════════════
// 主循环
// ══════════════════════════════════════════════════════

fn run(
    terminal: &mut Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
    world: &mut World,
    game_start: Instant,
) -> io::Result<()> {
    world.insert_resource(GamePacing::default());
    world.insert_resource(PendingInput::default());
    rebuild_occupancy(world);
    let _ = world.run_system_once(fov_system);
    let _ = world.run_system_once(predict_monster_actions_system);
    terminal.draw(|frame| render_ui(frame, world, game_start))?;

    loop {
        if world.resource::<TurnManager>().game_over || world.resource::<TurnManager>().wants_quit {
            break Ok(());
        }

        // 闪烁相位
        world.resource_mut::<GamePacing>().blink_phase =
            !world.resource::<GamePacing>().blink_phase;

        let is_paused = matches!(world.resource::<GamePacing>().mode, PacingMode::CombatPaused);

        terminal.draw(|frame| render_ui(frame, world, game_start))?;

        // ══════════════════════════════════════════════
        // 二态：暂停 vs 即时。
        // 逻辑路径唯一：方向键 → auto_move → 写预测+提交。
        // auto_move 内部根据 combat+locked 决定是否切暂停。
        // ══════════════════════════════════════════════

        if is_paused {
            if let Event::Key(key) = event::read()? {
                handle_combat_key(world, terminal, key)?;
            }
            if !matches!(world.resource::<GamePacing>().mode, PacingMode::CombatPaused) {
                advance_and_settle(world);
            }
        } else {
            if let Event::Key(key) = event::read()? {
                handle_input(world, terminal, key)?;
                if !matches!(world.resource::<GamePacing>().mode, PacingMode::CombatPaused) {
                    advance_and_settle(world);
                }
            }
        }
    }
}

/// 统一推进 + 模式调整。
/// 非战斗时锁定自动确认并继续推进；战斗时锁定暂停。
fn advance_and_settle(world: &mut World) {
    loop {
        advance_to_next_decision_point(world);
        post_advance(world);

        let combat = world.resource::<GamePacing>().combat_active;
        let locked = player_prediction(world).map(|p| p.locked).unwrap_or(false);

        if combat && locked {
            world.resource_mut::<GamePacing>().mode = PacingMode::CombatPaused;
            break;
        }
        if !locked {
            break;
        }
        // 非战斗锁定 → 自动确认，推进到 AV=0
        if let Some(e) = player_entity(world) {
            let skipped = world.get::<ActionValue>(e).map(|av| av.current_av).unwrap_or(0.0);
            if let Some(mut pred) = world.get_mut::<ActionPrediction>(e) {
                pred.locked = false;
            }
            advance_by(world, skipped);
        }
    }
}

fn post_advance(world: &mut World) {
    rebuild_occupancy(world);
    let _ = world.run_system_once(fov_system);
    update_map_memory(world);
    let _ = world.run_system_once(check_death_system);

    // 退出战斗：视野内无怪物
    if world.resource::<GamePacing>().combat_active {
        let fov: HashSet<(usize, usize)> = world
            .query::<(&Player, &Viewshed)>()
            .iter(world)
            .next()
            .map(|(_, v)| v.visible_tiles.iter().copied().collect())
            .unwrap_or_default();
        let any_in_fov = world
            .query::<(&Monster, &Position)>()
            .iter(world)
            .any(|(_, p)| fov.contains(&(p.x, p.y)));
        if !any_in_fov {
            let mut p = world.resource_mut::<GamePacing>();
            p.combat_active = false;
            p.mode = PacingMode::Exploration;
        }
    }
    // 进入战斗由 damage 事件触发（movement_system / execute_monster_chase）
}

// ══════════════════════════════════════════════════════
// 输入处理
// ══════════════════════════════════════════════════════

fn handle_input(
    world: &mut World,
    terminal: &mut Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
    key: crossterm::event::KeyEvent,
) -> io::Result<()> {
    if handle_global_toggle(world, key) {
        return Ok(());
    }
    match key.code {
        // 方向键 → 写预测 + 立即提交（已锁定时切暂停）
        KeyCode::Up => auto_move(world, 0, -1),
        KeyCode::Down => auto_move(world, 0, 1),
        KeyCode::Left => auto_move(world, -1, 0),
        KeyCode::Right => auto_move(world, 1, 0),
        KeyCode::Char('q') | KeyCode::Esc => {
            if confirm_quit(terminal)? {
                world.resource_mut::<TurnManager>().wants_quit = true;
            }
        }
        KeyCode::Char('1') | KeyCode::Char('2') | KeyCode::Char('3') | KeyCode::Char('4') => {
            let (idx, name) = skill_info(world, key);
            if let Some(n) = name {
                write_skill_prediction(world, idx, &n);
            }
            world.resource_mut::<GamePacing>().mode = PacingMode::CombatPaused;
        }
        KeyCode::Char('.') | KeyCode::Char('5') => {
            world.resource_mut::<PendingInput>().direction = None;
            commit_player_action(world);
        }
        KeyCode::Char('e') | KeyCode::Char('E') => {
            open_inventory(world, terminal)?;
        }
        KeyCode::F(5) => {
            if let Ok(data) = bincode::serialize(&GameSave::from_world(world)) {
                std::fs::write("save.bin", data).ok();
                world.resource_mut::<EventLog>().push("已保存");
            }
        }
        KeyCode::F(9) => {
            if let Ok(data) = std::fs::read("save.bin") {
                if let Ok(save) = bincode::deserialize::<GameSave>(&data) {
                    save.into_world(world);
                    world.resource_mut::<EventLog>().push("已读档");
                }
            }
        }
        KeyCode::Char('>') => {
            if on_stairs(world) && confirm_stairs(terminal)? {
                descend(world);
                world.resource_mut::<GamePacing>().mode = PacingMode::Exploration;
            }
        }
        _ => {}
    }
    Ok(())
}

fn handle_combat_key(
    world: &mut World,
    _terminal: &mut Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
    key: crossterm::event::KeyEvent,
) -> io::Result<()> {
    if handle_global_toggle(world, key) {
        return Ok(());
    }
    // 上次按下的方向/技能
    let last_dir = world.resource::<PendingInput>().direction;
    let last_skill = world.resource::<PendingPlayerAction>().skill_idx;

    match key.code {
        KeyCode::Up => {
            if last_dir == Some((0, -1)) {
                commit_player_action(world);
                world.resource_mut::<GamePacing>().mode = PacingMode::Exploration;
            } else {
                world.resource_mut::<PendingInput>().direction = Some((0, -1));
                write_player_prediction(world, 0, -1);
            }
        }
        KeyCode::Down => {
            if last_dir == Some((0, 1)) {
                commit_player_action(world);
                world.resource_mut::<GamePacing>().mode = PacingMode::Exploration;
            } else {
                world.resource_mut::<PendingInput>().direction = Some((0, 1));
                write_player_prediction(world, 0, 1);
            }
        }
        KeyCode::Left => {
            if last_dir == Some((-1, 0)) {
                commit_player_action(world);
                world.resource_mut::<GamePacing>().mode = PacingMode::Exploration;
            } else {
                world.resource_mut::<PendingInput>().direction = Some((-1, 0));
                write_player_prediction(world, -1, 0);
            }
        }
        KeyCode::Right => {
            if last_dir == Some((1, 0)) {
                commit_player_action(world);
                world.resource_mut::<GamePacing>().mode = PacingMode::Exploration;
            } else {
                world.resource_mut::<PendingInput>().direction = Some((1, 0));
                write_player_prediction(world, 1, 0);
            }
        }
        KeyCode::Char('1') | KeyCode::Char('2') | KeyCode::Char('3') | KeyCode::Char('4') => {
            let (idx, name) = skill_info(world, key);
            if last_skill == Some(idx) {
                commit_player_action(world);
                world.resource_mut::<GamePacing>().mode = PacingMode::Exploration;
            } else if let Some(n) = name {
                write_skill_prediction(world, idx, &n);
            }
        }
        KeyCode::Char('.') | KeyCode::Char('5') => {
            world.resource_mut::<PendingInput>().direction = None;
            world.resource_mut::<PendingPlayerAction>().is_pending_skill = false;
            commit_player_action(world);
            world.resource_mut::<GamePacing>().mode = PacingMode::Exploration;
        }
        KeyCode::Char('q') | KeyCode::Esc => {
            if confirm_quit(_terminal)? {
                world.resource_mut::<TurnManager>().game_over = true;
            }
        }
        _ => {}
    }
    Ok(())
}

/// 从按键获取技能索引和名称。
fn skill_info(world: &mut World, key: crossterm::event::KeyEvent) -> (usize, Option<String>) {
    let idx = match key.code {
        KeyCode::Char('1') => 0,
        KeyCode::Char('2') => 1,
        KeyCode::Char('3') => 2,
        _ => 3,
    };
    let names: Vec<String> = {
        let mut q = world.query::<&Skills>();
        q.iter(world)
            .next()
            .map(|sk| sk.list.iter().map(|s| s.name.to_string()).collect())
            .unwrap_or_default()
    };
    (idx, names.get(idx).cloned())
}

/// 自动模式下方向键。
/// 探索中（无战斗）：锁定自动采纳，不暂停。
/// 战斗中：锁定 → 暂停等确认；未锁定 → 即时提交。
fn auto_move(world: &mut World, dx: isize, dy: isize) {
    world.resource_mut::<PendingInput>().direction = Some((dx, dy));
    let locked = player_prediction(world).map(|p| p.locked).unwrap_or(false);
    let combat = world.resource::<GamePacing>().combat_active;

    if locked && combat {
        // 战斗中锁定 → 暂停让玩家确认
        world.resource_mut::<GamePacing>().mode = PacingMode::CombatPaused;
        return;
    }
    // 探索中 / 战斗中未锁定 → 直接提交
    write_player_prediction(world, dx, dy);
    commit_player_action(world);
}

/// 全局切换键：'m' = 强制手动, 'a' = 清除覆盖(恢复自动跟随)。
fn handle_global_toggle(world: &mut World, key: crossterm::event::KeyEvent) -> bool {
    match key.code {
        KeyCode::Char('m') | KeyCode::Char('M') => {
            world.resource_mut::<GamePacing>().manual_override = ManualOverride::ForceManual;
            world.resource_mut::<EventLog>().push("切换为手动模式");
            true
        }
        KeyCode::Char('a') | KeyCode::Char('A') => {
            world.resource_mut::<GamePacing>().manual_override = ManualOverride::None;
            world.resource_mut::<EventLog>().push("切换为自动模式");
            true
        }
        _ => false,
    }
}

// ══════════════════════════════════════════════════════
// 工具函数
// ══════════════════════════════════════════════════════

fn on_stairs(world: &mut World) -> bool {
    let pp = {
        let mut q = world.query::<&Position>();
        *q.iter(world)
            .next()
            .unwrap_or(&Position { x: 0, y: 0 })
    };
    let mut sq = world.query::<(&Stairs, &Position)>();
    sq.iter(world).any(|(_, sp)| sp.x == pp.x && sp.y == pp.y)
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
) -> io::Result<(World, Instant)> {
    loop {
        terminal.draw(|frame| draw_title(frame))?;
        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Enter => {
                    let mut world = setup_world();
                    let _ = world.run_system_once(fov_system);
                    update_map_memory(&mut world);
                    return Ok((world, Instant::now()));
                }
                KeyCode::F(9) => {
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

#[allow(dead_code)]
fn level_up_screen(
    world: &mut World,
    terminal: &mut Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
    _game_start: Instant,
) -> io::Result<()> {
    terminal.draw(|frame| draw_level_up(frame, world.resource::<PendingLevelUp>().points))?;
    loop {
        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Char('0') => {
                    world.resource_mut::<PendingLevelUp>().points = 0;
                    break;
                }
                KeyCode::Char('1') => {
                    if world.resource::<PendingLevelUp>().points > 0 {
                        world.resource_mut::<PendingLevelUp>().points -= 1;
                        world.query::<&mut Stats>().single_mut(world).unwrap().attack += 1;
                    }
                }
                KeyCode::Char('2') => {
                    if world.resource::<PendingLevelUp>().points > 0 {
                        world.resource_mut::<PendingLevelUp>().points -= 1;
                        world.query::<&mut Stats>().single_mut(world).unwrap().defense += 1;
                    }
                }
                KeyCode::Char('3') => {
                    if world.resource::<PendingLevelUp>().points > 0 {
                        world.resource_mut::<PendingLevelUp>().points -= 1;
                        world.query::<&mut Stats>().single_mut(world).unwrap().magic_mastery += 1;
                    }
                }
                KeyCode::Char('4') => {
                    if world.resource::<PendingLevelUp>().points > 0 {
                        world.resource_mut::<PendingLevelUp>().points -= 1;
                        world.query::<&mut Stats>().single_mut(world).unwrap().agility += 1;
                    }
                }
                KeyCode::Char('5') => {
                    if world.resource::<PendingLevelUp>().points > 0 {
                        world.resource_mut::<PendingLevelUp>().points -= 1;
                        let mut s = world.query::<&mut Stats>().single_mut(world).unwrap();
                        s.max_hp += 5;
                        s.hp = s.hp.min(s.max_hp);
                    }
                }
                _ => {}
            }
            if world.resource::<PendingLevelUp>().points == 0 {
                break;
            }
            terminal
                .draw(|frame| draw_level_up(frame, world.resource::<PendingLevelUp>().points))?;
        }
    }
    Ok(())
}

fn open_inventory(
    world: &mut World,
    terminal: &mut Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
) -> io::Result<()> {
    let mut selected: usize = 0;
    let mut scroll: usize = 0;

    fn get_inv(world: &mut World) -> (Vec<ItemInstance>, usize, (Option<usize>, Option<usize>, Option<usize>)) {
        let mut q = world.query::<(&Inventory, &Equipment)>();
        q.iter(world)
            .next()
            .map(|(inv, eq)| (inv.items.clone(), inv.capacity, (eq.weapon, eq.armor, eq.ring)))
            .unwrap_or_default()
    }

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
            lines.push(Line::from(vec![Span::styled(
                format!(
                    " 物品 {} / {}  e:装备 d:丢弃 Esc:返回",
                    items.len(),
                    capacity
                ),
                Style::default().fg(Color::DarkGray),
            )]));
            lines.push(Line::from(Span::raw("")));
            let eq_name = |idx: Option<usize>| {
                idx.and_then(|i| items.get(i))
                    .map(|it| it.name.clone())
                    .unwrap_or("(空)".into())
            };
            lines.push(Line::from(Span::raw(format!(
                " 武器: {}  防具: {}  戒指: {}",
                eq_name(weapon),
                eq_name(armor),
                eq_name(ring)
            ))));
            lines.push(Line::from(Span::raw("")));
            let page_size = (inner.height as usize).saturating_sub(5).min(20);
            if items.is_empty() {
                lines.push(Line::from(Span::styled(
                    " (背包为空)",
                    Style::default().fg(Color::DarkGray),
                )));
            } else {
                let sel = selected.min(items.len() - 1);
                let sc = scroll.min(sel);
                let end = (sc + page_size).min(items.len());
                for i in sc..end {
                    let item = &items[i];
                    let idx_char =
                        if i < 10 { char::from_digit(i as u32, 10).unwrap() } else { '?' };
                    let prefix = if i == sel { "▸" } else { " " };
                    lines.push(Line::from(vec![
                        Span::styled(
                            format!("{}{}", prefix, idx_char),
                            Style::default().fg(Color::Yellow),
                        ),
                        Span::raw(format!(" {}", item.name)),
                        Span::styled(
                            format!(" {:?}", item.slot),
                            Style::default().fg(Color::DarkGray),
                        ),
                        Span::raw(format!(" {}", item.description)),
                    ]));
                }
            }
            frame.render_widget(Paragraph::new(lines), inner);
        })?;

        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => break,
                KeyCode::Down => {
                    let len = inv_data.0.len();
                    if len > 0 {
                        selected = (selected + 1).min(len - 1);
                        if selected >= scroll + 10 { scroll += 1; }
                    }
                }
                KeyCode::Up => {
                    selected = selected.saturating_sub(1);
                    if selected < scroll { scroll = scroll.saturating_sub(1); }
                }
                KeyCode::Char('e') => {
                    let mut q = world.query::<(&mut Inventory, &mut Equipment)>();
                    if let Some((mut inv, mut eq)) = q.iter_mut(world).next() {
                        if inv.items.get(selected).is_some() {
                            match inv.items[selected].slot {
                                EquipmentSlot::Weapon => eq.weapon = Some(selected),
                                EquipmentSlot::Armor => eq.armor = Some(selected),
                                EquipmentSlot::Ring => eq.ring = Some(selected),
                            }
                        }
                    }
                }
                KeyCode::Char('d') => {
                    let mut q = world.query::<(&mut Inventory, &mut Equipment)>();
                    if let Some((mut inv, mut eq)) = q.iter_mut(world).next() {
                        if selected < inv.items.len() {
                            if eq.weapon == Some(selected) { eq.weapon = None; }
                            if eq.armor == Some(selected) { eq.armor = None; }
                            if eq.ring == Some(selected) { eq.ring = None; }
                            inv.items.remove(selected);
                            selected = selected.min(inv.items.len().saturating_sub(1));
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
