//! TUI 主渲染入口：组装地图、状态面板、背包、事件日志等 UI 区域。

use crate::pipeline;
use crate::timeline::build_timeline;
use bevy_ecs::prelude::Entity;
use bevy_ecs::prelude::World;
use dungeon_action::PageStack;
use dungeon_core::OptionLogExt;
use dungeon_core::{
    EntityName, Equipment, EventLog, Inventory, InventoryUI, LookCursor, MAP_HEIGHT, MAP_WIDTH,
    Map, MapMemory, Player, Position, Stats, Tile, VIEWPORT_HEIGHT, VIEWPORT_WIDTH, Viewshed,
    effective_attack, effective_defense,
};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};
use std::time::Instant;

pub fn render_ui(frame: &mut Frame, game_start: Instant, world: &World) {
    let area = frame.area();
    let inner = inner_rect(area, 1);
    let scene = pipeline::extract_scene(world);

    // 页栈感知：非 Game 页面跳过游戏 UI 渲染
    let current_page = world
        .get_resource::<PageStack>()
        .map(|ps| ps.current().clone())
        .unwrap_or(dungeon_action::Page::Game);

    let is_fullscreen = matches!(current_page, dungeon_action::Page::Inventory);

    let title = if scene.game_over {
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
    if scene.game_over {
        frame.render_widget(
            Paragraph::new(Line::from("按 q 退出").centered())
                .style(Style::default().fg(Color::Red)),
            inner,
        );
        return;
    }

    if !is_fullscreen {
        render_game_ui(frame, inner, &scene, world, game_start);
    }

    // 页栈：对话框叠加层（在所有 UI 之上）
    render_dialog_overlay(frame, world);
    // 页栈：投掷选择页
    render_throw_select_overlay(frame, world);
    // 页栈：背包页面（全屏）
    if is_fullscreen {
        render_inventory_overlay(frame, world);
    }
}

/// 游戏主 UI（地图 + 面板），仅 Game/Look/ThrowAim 等非全屏页面时渲染
fn render_game_ui(
    frame: &mut Frame,
    inner: Rect,
    scene: &pipeline::RenderScene,
    world: &World,
    game_start: Instant,
) {
    let timeline_width: u16 = 26;
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(timeline_width),
            Constraint::Length(1),
            Constraint::Length(VIEWPORT_WIDTH as u16),
            Constraint::Length(1),
            Constraint::Min(1),
        ])
        .split(inner);
    let (timeline_area, map_events_area) = (chunks[0], chunks[2]);
    let stats_area = Rect {
        x: chunks[4].x,
        y: chunks[4].y,
        width: chunks[4].width,
        height: inner.height,
    };

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
    let (grid, _cam_x, _cam_y) = pipeline::render_map_grid(scene, world);

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
        event_lines.push(Line::from(Span::styled(
            "── 事件 ──",
            Style::default().fg(Color::DarkGray),
        )));
        for msg in log.messages.iter().rev().take(12) {
            let color = match msg.level {
                dungeon_core::EventLevel::Combat => Color::Red,
                dungeon_core::EventLevel::Item => Color::Yellow,
                dungeon_core::EventLevel::Skill => Color::Cyan,
                dungeon_core::EventLevel::System => Color::DarkGray,
                dungeon_core::EventLevel::Danger => Color::LightRed,
            };
            event_lines.push(Line::from(Span::styled(
                format!(" {}", msg.text),
                Style::default().fg(color),
            )));
        }
        frame.render_widget(
            Paragraph::new(event_lines).style(Style::default().fg(Color::White)),
            events_area,
        );
    }

    // UI 层：行动轴
    let timeline = build_timeline(scene.player_visible.clone(), world);
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

    // UI 层：状态面板
    let stats = build_stats_panel(scene.px, scene.py, game_start, scene, world);
    frame.render_widget(
        Paragraph::new(stats)
            .style(Style::default().fg(Color::White))
            .block(
                Block::default()
                    .title(" 状态 ")
                    .borders(Borders::LEFT)
                    .border_style(Style::default().fg(Color::DarkGray)),
            ),
        stats_area,
    );
}

/// 对话框叠加层
fn render_dialog_overlay(frame: &mut Frame, world: &World) {
    if let Some(dungeon_action::Page::Dialog(kind)) =
        world.get_resource::<PageStack>().and_then(|ps| ps.0.last())
    {
        let dialog = Paragraph::new(vec![
            Line::from(Span::styled(
                kind.title(),
                Style::default().fg(Color::Yellow).bold(),
            )),
            Line::from(Span::styled(
                " Y)是  N)否",
                Style::default().fg(Color::DarkGray),
            )),
        ])
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow)),
        )
        .alignment(Alignment::Center);
        let area = frame.area();
        frame.render_widget(
            dialog,
            Rect {
                x: area.width / 2 - 12,
                y: area.height / 2,
                width: 24,
                height: 5,
            },
        );
    }
}

/// 投掷选择页叠加层
fn render_throw_select_overlay(frame: &mut Frame, world: &World) {
    if world
        .get_resource::<PageStack>()
        .map(|ps| ps.0.last() == Some(&dungeon_action::Page::ThrowSelect))
        .unwrap_or(false)
    {
        let name = world
            .try_query::<(&Player, &Equipment)>()
            .and_then(|mut q| {
                q.iter(world)
                    .next()
                    .and_then(|(_, eq)| eq.off_hand.as_ref().map(|s| s.name()))
            })
            .unwrap_or("(无投掷物)".into());
        let msg = Paragraph::new(vec![
            Line::from(Span::styled(
                "选择投掷物",
                Style::default().fg(Color::Yellow).bold(),
            )),
            Line::from(Span::raw(format!(" 当前: {}", name))),
            Line::from(Span::styled(
                " Enter 确认  Esc 取消",
                Style::default().fg(Color::DarkGray),
            )),
        ])
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .alignment(Alignment::Center);
        let area = frame.area();
        frame.render_widget(
            msg,
            Rect {
                x: area.width / 2 - 16,
                y: area.height / 2,
                width: 32,
                height: 5,
            },
        );
    }
}

/// 背包页全屏渲染
fn render_inventory_overlay(frame: &mut Frame, world: &World) {
    let inv_state = world.resource::<InventoryUI>();

    // 收集数据：装备、背包、地面
    let (equip, inv_stacks, inv_cap) = {
        let mut q = world
            .try_query::<(&Equipment, &Inventory)>()
            .expect_log("Equipment+Inventory registered at init");
        q.iter(world)
            .next()
            .map(|(eq, inv)| (eq.clone(), inv.stacks.clone(), inv.capacity))
            .unwrap_or_default()
    };
    let ground_items: Vec<(dungeon_core::ItemStack, Entity)> = {
        let mut q = world
            .try_query::<(Entity, &Position, &dungeon_core::ItemPickup)>()
            .expect_log("ItemPickup+Pos reg");
        let px = world
            .try_query::<(&Player, &Position)>()
            .expect_log("Player+Pos reg")
            .iter(world)
            .next()
            .map(|(_, p)| (p.x, p.y));
        px.map(|(px, py)| {
            q.iter(world)
                .filter(|(_, p, _)| p.x == px && p.y == py)
                .map(|(e, _, ip)| (ip.stack.clone(), e))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
    };

    let mut lines: Vec<Line> = Vec::new();

    if inv_state.detail {
        // ═══════════════ 详情模式 ═══════════════
        let (stack, source_label, _is_equip) = match inv_state.detail_source {
            1 => {
                let slots = [&equip.main_hand, &equip.off_hand, &equip.armor, &equip.ring];
                (
                    slots.get(inv_state.detail_idx).and_then(|s| s.as_ref()),
                    "装备",
                    true,
                )
            }
            0 => (inv_stacks.get(inv_state.detail_idx), "背包", false),
            _ => (
                ground_items.get(inv_state.detail_idx).map(|(s, _)| s),
                "地面",
                false,
            ),
        };
        if let Some(item) = stack {
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
            if let Some(d) = item.def() {
                let class_str = d.class.display_name();
                let slot_str = d.slot.map(|s| format!("{:?}", s)).unwrap_or_default();
                lines.push(Line::from(vec![
                    Span::styled(
                        format!(" 类别: {}", class_str),
                        Style::default().fg(Color::DarkGray),
                    ),
                    if !slot_str.is_empty() {
                        Span::styled(
                            format!(" [{}]", slot_str),
                            Style::default().fg(Color::DarkGray),
                        )
                    } else {
                        Span::raw("")
                    },
                ]));
                let b = &d.bonus;
                let mut parts = Vec::new();
                if b.attack != 0 {
                    parts.push(format!("攻击{:+}", b.attack));
                }
                if b.defense != 0 {
                    parts.push(format!("防御{:+}", b.defense));
                }
                if b.magic_mastery != 0 {
                    parts.push(format!("法术精通{:+}", b.magic_mastery));
                }
                if b.agility != 0 {
                    parts.push(format!("敏捷{:+}", b.agility));
                }
                if b.hp != 0 {
                    parts.push(format!("HP{:+}", b.hp));
                }
                if b.crit_rate != 0.0 {
                    parts.push(format!("暴击率{:.0}%", b.crit_rate * 100.0));
                }
                if !parts.is_empty() {
                    lines.push(Line::from(Span::styled(
                        format!(" {}", parts.join(" ")),
                        Style::default().fg(Color::Green),
                    )));
                }
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
            // I66/L47: 操作提示由 detail_item_actions 生成（与 main.rs 处理器共用判定）
            let hint_text: String =
                dungeon_core::detail_item_actions(Some(item), inv_state.detail_source)
                    .iter()
                    .map(|a| match a {
                        dungeon_core::ItemAction::Equip => "e:装备",
                        dungeon_core::ItemAction::Use => "r:使用/学习",
                        dungeon_core::ItemAction::Drop => "d:丢弃",
                        dungeon_core::ItemAction::Unequip => "u:卸载",
                        dungeon_core::ItemAction::Pickup => "g:拾取",
                    })
                    .collect::<Vec<_>>()
                    .join("  ");
            if !hint_text.is_empty() {
                lines.push(Line::from(Span::styled(
                    format!(" {}", hint_text),
                    Style::default().fg(Color::DarkGray),
                )));
            }
            lines.push(Line::from(Span::styled(
                " Esc:返回",
                Style::default().fg(Color::DarkGray),
            )));
        }
    } else {
        // ═══════════════ 列表模式 ═══════════════
        let slot_labels = ["[主]", "[副]", "[防]", "[戒]"];
        let slot_items = [&equip.main_hand, &equip.off_hand, &equip.armor, &equip.ring];

        // 装备段
        for i in 0..4 {
            let name = slot_items[i]
                .as_ref()
                .map(|s| s.name())
                .unwrap_or("(空)".into());
            let marker = if !inv_state.panel && inv_state.left_sel == i {
                "▸"
            } else {
                " "
            };
            let right = if i < ground_items.len() {
                let (stack, _) = &ground_items[i];
                format!("{} x{}", stack.name(), stack.count)
            } else {
                String::new()
            };
            lines.push(Line::from(Span::raw(format!(
                " {}{} {:<15}  {}",
                marker, slot_labels[i], name, right
            ))));
        }

        // 背包分隔 + 物品（G27：以选中项为中心的滚动窗口——>14 物品时光标不再移出屏幕）
        lines.push(Line::from(Span::styled(
            format!(" ── 背包 ({}/{})", inv_stacks.len(), inv_cap),
            Style::default().fg(Color::DarkGray),
        )));
        const WIN: usize = 14;
        let start =
            backpack_window_start(inv_state.left_sel.saturating_sub(4), inv_stacks.len(), WIN);
        for (i, s) in inv_stacks.iter().enumerate().skip(start).take(WIN) {
            let real = i + 4;
            let marker = if !inv_state.panel && inv_state.left_sel == real {
                "▸"
            } else {
                " "
            };
            // 热键按背包绝对索引（0-9/a-z ↔ 第 0-35 个），滚动窗口不影响热键编号
            let hk = if i < 10 {
                char::from_digit(i as u32, 10).expect_log("index 0-9 maps to digit")
            } else {
                char::from(b'a' + (i - 10) as u8)
            };
            let right = if real < ground_items.len() {
                let (stack, _) = &ground_items[real];
                format!("{} x{}", stack.name(), stack.count)
            } else {
                String::new()
            };
            lines.push(Line::from(Span::raw(format!(
                " {}{} {:<15}  {}",
                marker,
                hk,
                s.name(),
                right
            ))));
        }

        // 地面独立列表（右栏未选中时也显示）
        if inv_state.panel && ground_items.len() > inv_stacks.len().min(14) + 4 {
            lines.push(Line::from(Span::styled(
                " ── 地面（续）──".to_string(),
                Style::default().fg(Color::DarkGray),
            )));
            let start = inv_stacks.len().min(14) + 4;
            let end = ground_items.len().min(18);
            for (i, (stack, _)) in ground_items
                .iter()
                .enumerate()
                .skip(start)
                .take(end.saturating_sub(start))
            {
                let marker = if inv_state.panel && inv_state.right_sel == i {
                    "▸"
                } else {
                    " "
                };
                lines.push(Line::from(Span::raw(format!(
                    "  {} {} x{}",
                    marker,
                    stack.name(),
                    stack.count
                ))));
            }
        }

        // 底部帮助
        lines.push(Line::from(Span::raw("")));
        lines.push(Line::from(Span::styled(
            " ← → 切换栏  ↑ ↓ 选择  Enter 查看详情  g 拾取  Esc 关闭",
            Style::default().fg(Color::DarkGray),
        )));
    }

    let inv_paragraph = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" 背包 ")
            .border_style(Style::default().fg(Color::Cyan)),
    );
    frame.render_widget(inv_paragraph, frame.area());
}

/// 状态面板头部（I76 快照化）：仅依赖快照数据，可脱离 ECS 单测
fn panel_header(scene: &crate::pipeline::RenderScene) -> Vec<Line<'static>> {
    let mut out: Vec<Line<'static>> = Vec::new();
    let Some(ref s) = scene.stats else {
        out.push(Line::from(Span::raw("(无数据)")));
        return out;
    };
    let hp_color = if s.hp as f32 <= s.max_hp as f32 * dungeon_core::LOW_HP_RATIO {
        Color::Red
    } else {
        Color::Cyan
    };
    out.push(Line::from(vec![Span::styled(
        format!(" Lv.{}  Warrior ", s.level),
        Style::default().fg(hp_color).bold(),
    )]));
    out.push(Line::from(Span::raw(" ".repeat(22))));
    out.push(Line::from(vec![
        Span::styled(" HP ", Style::default().fg(Color::DarkGray)),
        Span::raw(format!("{:>3}/{:<3}", s.hp.max(0), s.max_hp)),
        Span::raw(" "),
        Span::styled(
            bar(s.hp.max(0), s.max_hp, 8),
            Style::default().fg(Color::Red),
        ),
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
    // I76: buff 加成直接由快照 buffs 计算（不再构造 ActiveBuffs 查询）
    let bonus_ab = dungeon_core::ActiveBuffs(scene.buffs.clone());
    let eff_atk = scene
        .equip
        .as_ref()
        .map(|eq| effective_attack(s, eq, Some(&bonus_ab)))
        .unwrap_or(s.attack);
    let eff_def = scene
        .equip
        .as_ref()
        .map(|eq| effective_defense(s, eq, Some(&bonus_ab)))
        .unwrap_or(s.defense);
    let display_crit_rate = scene
        .equip
        .as_ref()
        .map(|eq| {
            let bonus = dungeon_core::equipment_bonus(eq);
            (s.crit_rate + bonus.crit_rate).min(1.0) * 100.0
        })
        .unwrap_or(s.crit_rate * 100.0);
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
        Span::raw(format!("{:>5.1}%", display_crit_rate)),
        Span::raw(" "),
        Span::styled(" 暴击伤害", Style::default().fg(Color::DarkGray)),
        Span::raw(format!("{:>4.0}%", s.crit_damage * 100.0)),
    ]));
    out.push(Line::from(Span::raw("")));
    if let Some(equip) = &scene.equip {
        let mh = equip
            .main_hand
            .as_ref()
            .map(|st| st.name())
            .unwrap_or("(空)".into());
        let oh = equip
            .off_hand
            .as_ref()
            .map(|st| st.name())
            .unwrap_or("(空)".into());
        let ar = equip
            .armor
            .as_ref()
            .map(|st| st.name())
            .unwrap_or("(空)".into());
        let rg = equip
            .ring
            .as_ref()
            .map(|st| st.name())
            .unwrap_or("(空)".into());
        out.push(Line::from(Span::styled(
            "── 装备 ──",
            Style::default().fg(Color::DarkGray),
        )));
        out.push(Line::from(vec![
            Span::styled("主手:", Style::default().fg(Color::DarkGray)),
            Span::raw(truncate_name(&mh, 5)),
        ]));
        out.push(Line::from(vec![
            Span::styled("副手:", Style::default().fg(Color::DarkGray)),
            Span::raw(truncate_name(&oh, 5)),
        ]));
        out.push(Line::from(vec![
            Span::styled("防具:", Style::default().fg(Color::DarkGray)),
            Span::raw(truncate_name(&ar, 5)),
            Span::raw("   "),
            Span::styled("戒指:", Style::default().fg(Color::DarkGray)),
            Span::raw(truncate_name(&rg, 5)),
        ]));
    }
    out.push(Line::from(Span::raw("")));
    out.push(Line::from(Span::raw(format!(" 楼层 {}", scene.floor))));
    out
}

pub fn build_stats_panel(
    px: usize,
    py: usize,
    game_start: Instant,
    scene: &crate::pipeline::RenderScene,
    world: &World,
) -> Vec<Line<'static>> {
    let mut out = panel_header(scene);
    out.push(Line::from(Span::raw(format!("  @ ({}, {})", px, py))));
    let elapsed = game_start.elapsed();
    out.push(Line::from(Span::styled(
        format!(
            " ⏱ {:>2}:{:02}",
            elapsed.as_secs() / 60,
            elapsed.as_secs() % 60
        ),
        Style::default().fg(Color::DarkGray),
    )));
    out.push(Line::from(Span::raw("")));
    if let (Some(sk), Some(st)) = (&scene.skills, &scene.stats) {
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
                Span::styled(
                    format!("{}({})", sk.name, sk.cost_mp),
                    Style::default().fg(c),
                ),
            ]));
        }
    }
    // ── 光标查看信息 ──
    if let Some(cursor) = world.get_resource::<LookCursor>()
        && cursor.active
    {
        let (cx, cy) = (cursor.x, cursor.y);
        let explored = world.resource::<MapMemory>().explored;
        let map = world.resource::<Map>();
        let pv: std::collections::HashSet<(usize, usize)> = world
            .try_query::<(&Player, &Viewshed)>()
            .expect_log("Player+Viewshed reg")
            .iter(world)
            .next()
            .map(|(_, v)| v.visible_tiles.iter().copied().collect())
            .unwrap_or_default();
        out.push(Line::from(Span::raw("")));
        out.push(Line::from(Span::styled(
            format!(" x 光标 ({}, {})", cx, cy),
            Style::default().fg(Color::Yellow),
        )));
        if cx < MAP_WIDTH && cy < MAP_HEIGHT {
            if pv.contains(&(cx, cy)) {
                // 可见格：显示完整信息（地形+实体+HP）
                let tile = map.tiles[cy][cx];
                let tile_name = match tile {
                    Tile::Wall => "墙壁",
                    Tile::Floor => "地板",
                    Tile::ShallowWater => "浅水",
                    Tile::DeepWater => "深水",
                    Tile::Stalactite => "钟乳石",
                    Tile::Mycelium => "菌丝",
                    Tile::FungalPatch => "蘑菇丛",
                    Tile::HangingVine => "垂藤",
                    Tile::Sand => "沙岸",
                    Tile::Seagrass => "海草",
                    Tile::CoralReef => "珊瑚礁",
                };
                out.push(Line::from(Span::styled(
                    format!("  {}", tile_name),
                    Style::default().fg(Color::DarkGray),
                )));
                let cursor_entities: Vec<(String, Entity)> = {
                    let mut eq = world
                        .try_query::<(Entity, &Position, &EntityName)>()
                        .expect_log("Entity+Pos+Name reg");
                    eq.iter(world)
                        .filter(|(_, p, _)| p.x == cx && p.y == cy)
                        .map(|(e, _, n)| (n.0.clone(), e))
                        .collect()
                };
                for (name, e) in &cursor_entities {
                    let hp = world
                        .get::<Stats>(*e)
                        .map(|s| format!(" ({}/{})", s.hp.max(0), s.max_hp))
                        .unwrap_or_default();
                    out.push(Line::from(Span::styled(
                        format!("  {}{}", name, hp),
                        Style::default().fg(Color::White),
                    )));
                }
            } else if explored[cy][cx] {
                // 已探索但当前不可见：只显示地形名，不显示实体
                let tile = map.tiles[cy][cx];
                let tile_name = match tile {
                    Tile::Wall => "墙壁",
                    Tile::Floor => "地板",
                    Tile::ShallowWater => "浅水",
                    Tile::DeepWater => "深水",
                    Tile::Stalactite => "钟乳石",
                    Tile::Mycelium => "菌丝",
                    Tile::FungalPatch => "蘑菇丛",
                    Tile::HangingVine => "垂藤",
                    Tile::Sand => "沙岸",
                    Tile::Seagrass => "海草",
                    Tile::CoralReef => "珊瑚礁",
                };
                out.push(Line::from(Span::styled(
                    format!("  {} (已探索)", tile_name),
                    Style::default().fg(Color::DarkGray),
                )));
            } else {
                out.push(Line::from(Span::styled(
                    "  (未探索)",
                    Style::default().fg(Color::DarkGray),
                )));
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
    "█".repeat(filled.min(width)) + &"░".repeat(width - filled.min(width))
}

/// 装备名安全截断（I78：按字符而非 UTF-8 字节截断，中文名不再 panic）。
/// max_chars 为字符数（CJK 占 2 列，5 字符 ≈ 原 10 字节的显示宽度意图）。
fn truncate_name(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect()
}

/// 背包列表滚动窗口起点（G27）：以选中项为中心（偏上），列表不足一屏时从 0 开始。
/// `sel` 为背包段选中索引（0 基），`len` 为背包物品数，`win` 为窗口大小。
fn backpack_window_start(sel: usize, len: usize, win: usize) -> usize {
    if len <= win {
        return 0;
    }
    sel.saturating_sub(win / 2).min(len - win)
}

fn inner_rect(area: Rect, border: u16) -> Rect {
    Rect {
        x: area.x + border,
        y: area.y + border,
        width: area.width.saturating_sub(border * 2),
        height: area.height.saturating_sub(border * 2),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::RenderScene;
    use dungeon_core::{Equipment, ItemStack, MAP_HEIGHT, MAP_WIDTH, Tile};

    fn line_text(l: &Line) -> String {
        l.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    fn scene_with(stats: Option<dungeon_core::Stats>) -> RenderScene {
        RenderScene {
            game_over: false,
            player_visible: Default::default(),
            tiles: [[Tile::Wall; MAP_WIDTH]; MAP_HEIGHT],
            explored: [[false; MAP_WIDTH]; MAP_HEIGHT],
            px: 10,
            py: 10,
            visible_mem: Vec::new(),
            renderables: Vec::new(),
            stats,
            equip: Some(Equipment::new()),
            buffs: Vec::new(),
            skills: Some(dungeon_core::Skills { list: Vec::new() }),
            floor: 3,
        }
    }

    /// I76: 状态面板头部由快照数据生成（dungeon-render 首个单测，脱离 ECS）
    #[test]
    fn test_panel_header_from_scene() {
        dungeon_core::ItemRegistry::load();
        let mut scene = scene_with(Some(dungeon_core::Stats::player()));
        // 给主手一把剑验证装备栏
        if let Some(eq) = &mut scene.equip {
            eq.main_hand = Some(ItemStack::new(dungeon_core::ITEM_RUSTY_SWORD, 1));
        }
        let lines = panel_header(&scene);
        let joined: String = lines.iter().map(line_text).collect::<Vec<_>>().join("|");
        assert!(joined.contains("Lv.1"), "应显示等级: {}", joined);
        assert!(joined.contains("HP"), "应显示 HP 条");
        assert!(joined.contains("攻击"), "应显示攻击");
        assert!(joined.contains("楼层 3"), "应显示楼层: {}", joined);
        assert!(
            joined.contains("主手:") && joined.contains("锈铁剑"),
            "应显示装备栏: {}",
            joined
        );
    }

    /// I78 回归：4 字中文名装备（12 字节 > 10）不 panic，且按字符截断显示
    #[test]
    fn test_panel_header_chinese_long_name_no_panic() {
        dungeon_core::ItemRegistry::load();
        let mut scene = scene_with(Some(dungeon_core::Stats::player()));
        if let Some(eq) = &mut scene.equip {
            // 攻击戒指 = 4 个中文字（12 字节）；旧实现 mh[..10] 非字符边界直接 panic
            eq.main_hand = Some(ItemStack::new(dungeon_core::ITEM_ATTACK_RING, 1));
            eq.ring = Some(ItemStack::new(dungeon_core::ITEM_ATTACK_RING, 1));
        }
        let lines = panel_header(&scene);
        let joined: String = lines.iter().map(line_text).collect::<Vec<_>>().join("|");
        assert!(
            joined.contains("攻击戒指"),
            "长中文名应完整或截断显示而不崩溃: {}",
            joined
        );
        // 主手 + 戒指槽都显示攻击戒指（截断后仍含关键字符）
        assert!(
            joined.contains("主手:") && joined.contains("戒指:"),
            "装备槽应渲染: {}",
            joined
        );
    }

    /// truncate_name：按字符截断，任何 UTF-8 输入不 panic
    #[test]
    fn test_truncate_name_char_safe() {
        assert_eq!(truncate_name("攻击戒指", 5), "攻击戒指");
        assert_eq!(truncate_name("染血兽牙指环模板", 5), "染血兽牙指");
        assert_eq!(truncate_name("abc", 5), "abc");
        assert_eq!(truncate_name("abcdef", 3), "abc");
    }

    /// backpack_window_start：滚动窗口边界（G27）
    #[test]
    fn test_backpack_window_start() {
        // 不足一屏：窗口从 0 开始
        assert_eq!(backpack_window_start(0, 5, 14), 0);
        assert_eq!(backpack_window_start(4, 5, 14), 0);
        // 超过一屏：选中项居中（偏上）
        assert_eq!(backpack_window_start(7, 36, 14), 0); // 前 7 个仍在窗口前半
        assert_eq!(backpack_window_start(20, 36, 14), 13); // 窗口底部触底
        assert_eq!(backpack_window_start(35, 36, 14), 22); // 末尾：窗口对齐底部
        // 窗口恰好等于列表长度
        assert_eq!(backpack_window_start(10, 14, 14), 0);
    }

    /// 无玩家数据时降级显示且不 panic
    #[test]
    fn test_panel_header_no_stats() {
        let lines = panel_header(&scene_with(None));
        let joined: String = lines.iter().map(line_text).collect::<Vec<_>>().join("|");
        assert!(joined.contains("(无数据)"), "应降级显示: {}", joined);
    }

    /// 低血量阈值（I72 常量单源）驱动红色标记
    #[test]
    fn test_panel_header_low_hp_red() {
        dungeon_core::ItemRegistry::load();
        let mut scene = scene_with(Some(dungeon_core::Stats::player()));
        let s = scene.stats.as_mut().unwrap();
        s.hp = (s.max_hp as f32 * 0.2) as i32; // 20% < 30% 阈值
        let lines = panel_header(&scene);
        // 第一行 Lv 行应使用红色（低血）
        let styled = &lines[0].spans[0];
        assert!(styled.content.contains("Lv.1"));
        assert_eq!(
            styled.style.fg,
            Some(ratatui::style::Color::Red),
            "低血时等级行应标红"
        );
    }
}
