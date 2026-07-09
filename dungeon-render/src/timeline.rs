use dungeon_core::{
    ActiveBuffs, EntityName, Player, Position, Renderable, Stats,
};
use dungeon_action::{ActionQueue, ActionKindV3, PlayerPreview};
use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};
use std::collections::HashSet;
use bevy_ecs::prelude::{Entity, World};
use crate::color::{entity_color, renderable_color};

/// 行动表：符号 行动 耗时（无 HP），下方分割后显示符号 怪物名 血量 + Buff
pub fn build_timeline(player_visible: HashSet<(usize, usize)>, world: &World) -> Vec<Line<'static>> {
    let mut out: Vec<Line<'static>> = Vec::new();

    // ── 玩家预览 ──
    let preview = world.resource::<PlayerPreview>().kind.clone();
    let preview_text = match &preview {
        Some(ActionKindV3::Move { dx, dy }) => format!("移动({},{})", dx, dy),
        Some(ActionKindV3::Wait) => "等待".into(),
        Some(ActionKindV3::Skill(i)) => format!("技能{}", i + 1),
        Some(ActionKindV3::Attack { .. }) => "攻击".into(),
        _ => "等待输入".into(),
    };
    out.push(Line::from(vec![Span::styled("╭────────────", Style::default().fg(Color::Yellow))]));
    out.push(Line::from(vec![
        Span::styled("│ ", Style::default().fg(Color::Yellow)),
        Span::styled("@", Style::default().fg(Color::Yellow)),
        Span::raw(" "),
        Span::styled(preview_text, Style::default().fg(Color::Green)),
    ]));

    // ── 行动队列：符号 行动 耗时（无 HP）──
    let queue = world.resource::<ActionQueue>();
    for entry in &queue.entries {
        let Some(pos) = world.get::<Position>(entry.entity) else { continue };
        if !player_visible.contains(&(pos.x, pos.y)) { continue; }
        let glyph = world.get::<Renderable>(entry.entity)
            .map(|r| r.glyph).unwrap_or('?');
        let color = renderable_color(entity_color(entry.entity.to_bits(), 0));
        let (action_label, timer) = action_display(&entry.kind, entry.av_remaining);
        out.push(Line::from(vec![
            Span::styled(format!(" {} ", glyph), Style::default().fg(color)),
            Span::raw(action_label),
            Span::styled(format!(" {:>3}ms", timer as u32), Style::default().fg(Color::DarkGray)),
        ]));
    }
    out.push(Line::from(vec![Span::styled("╰────────────", Style::default().fg(Color::Yellow))]));

    // ── 分割线 ──
    out.push(Line::from(Span::styled("─────────────────", Style::default().fg(Color::DarkGray))));

    // ── 实体状态：符号 怪物名 血量（去重，仅可见实体）──
    let queue_positions: HashSet<(usize, usize)> = queue.entries.iter()
        .filter_map(|e| world.get::<Position>(e.entity))
        .map(|p| (p.x, p.y))
        .collect();

    let mut status_entries: Vec<(char, String, i32, i32, Color, Entity)> = Vec::new();
    if let Some(mut q) = world.try_query::<(Entity, &Position, &EntityName, &Stats, &Renderable)>() {
        for (e, p, n, s, r) in q.iter(world) {
            if n.0 == "冒险者" || !player_visible.contains(&(p.x, p.y)) { continue; }
            let color = Color::Rgb(r.color.0, r.color.1, r.color.2);
            status_entries.push((r.glyph, n.0.clone(), s.hp, s.max_hp, color, e));
        }
    }
    if !status_entries.is_empty() {
        for (glyph, name, hp, mhp, color, _) in &status_entries {
            let hp_color = if *hp <= *mhp / 3 { Color::Red } else { Color::Cyan };
            out.push(Line::from(vec![
                Span::styled(format!(" {} ", glyph), Style::default().fg(*color)),
                Span::styled(name.clone(), Style::default().fg(*color)),
                Span::raw(" "),
                Span::styled(format!("{:>3}/{:<3}", (*hp).max(0), mhp), Style::default().fg(hp_color)),
            ]));
        }
    }

    // ── 次级标注：实体身上的 Buff（小字号 = dim 样式）──
    let mut has_buffs = false;
    for (_, _, _, _, _, e) in &status_entries {
        if let Some(ab) = world.get::<ActiveBuffs>(*e) {
            if !ab.0.is_empty() {
                if !has_buffs {
                    out.push(Line::from(Span::styled("─────────────────", Style::default().fg(Color::DarkGray))));
                    has_buffs = true;
                }
                for b in &ab.0 {
                    let name = match b.kind {
                        dungeon_core::BuffKind::Shield => "护盾",
                        dungeon_core::BuffKind::Berserk => "狂暴",
                    };
                    out.push(Line::from(vec![
                        Span::styled(format!("  {} +{}", name, b.magnitude), Style::default().fg(Color::DarkGray)),
                        Span::styled(format!(" {:>1}s", (b.remaining_av / 1000.0).ceil() as u32), Style::default().fg(Color::DarkGray)),
                    ]));
                }
            }
        }
    }

    // 玩家自己的 Buff 也显示
    if let Some(ab) = world.try_query::<(&Player, &ActiveBuffs)>()
        .and_then(|mut q| q.iter(world).next().map(|(_, ab)| ab))
    {
        if !ab.0.is_empty() {
            if !has_buffs {
                out.push(Line::from(Span::styled("─────────────────", Style::default().fg(Color::DarkGray))));
            }
            for b in &ab.0 {
                let name = match b.kind {
                    dungeon_core::BuffKind::Shield => "护盾",
                    dungeon_core::BuffKind::Berserk => "狂暴",
                };
                out.push(Line::from(vec![
                    Span::styled(format!("  {} +{}", name, b.magnitude), Style::default().fg(Color::DarkGray)),
                    Span::styled(format!(" {:>1}s", (b.remaining_av / 1000.0).ceil() as u32), Style::default().fg(Color::DarkGray)),
                ]));
            }
        }
    }

    // ── 空状态提示 ──
    if status_entries.is_empty() && out.len() <= 5 {
        out.push(Line::from(Span::styled(" (无实体)", Style::default().fg(Color::DarkGray))));
    }

    out.push(Line::from(Span::raw("")));
    out.push(Line::from(Span::styled(" ↑↓←→移动 1-4技能 (双击确认)", Style::default().fg(Color::DarkGray))));
    out.push(Line::from(Span::styled(" .等待  e背包 x查看", Style::default().fg(Color::DarkGray))));
    out
}

fn action_display(kind: &ActionKindV3, av: f32) -> (String, f32) {
    match kind {
        ActionKindV3::Move { .. } => ("移动".into(), av),
        ActionKindV3::Chase => ("追击".into(), av),
        ActionKindV3::Flee => ("逃跑".into(), av),
        ActionKindV3::Wander => ("游荡".into(), av),
        ActionKindV3::Wait => ("等待".into(), av),
        ActionKindV3::Attack { .. } => ("攻击".into(), av),
        ActionKindV3::Skill(i) => (format!("技能{}", i + 1), av),
    }
}