//! Timeline view built from the ECS-native action state.

use crate::color::renderable_color;
use bevy_ecs::prelude::{Entity, Without, World};
use dungeon_action::{
    ActionTimer, Active, Attack, Chase, Flee, Move, PlayerPreview, Skill, Throw, TimedPlayerAction,
    Wait, Wander,
};
use dungeon_core::{ActiveBuffs, EntityName, Player, Position, Renderable, Stats};
use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};
use std::collections::HashSet;

pub fn build_timeline(
    player_visible: HashSet<(usize, usize)>,
    world: &World,
) -> Vec<Line<'static>> {
    let mut out: Vec<Line<'static>> = Vec::new();

    // Player preview
    let preview = world.resource::<PlayerPreview>().action.clone();
    let preview_text = match &preview {
        Some(TimedPlayerAction::Move { dx, dy }) => format!("移动({},{})", dx, dy),
        Some(TimedPlayerAction::Wait) => "等待".into(),
        Some(TimedPlayerAction::Skill(i)) => format!("技能{}", i + 1),
        Some(TimedPlayerAction::Attack { .. }) => "攻击".into(),
        _ => "等待输入".into(),
    };
    out.push(Line::from(vec![Span::styled(
        "╭────────────",
        Style::default().fg(Color::Yellow),
    )]));
    out.push(Line::from(vec![
        Span::styled("│", Style::default().fg(Color::Yellow)),
        Span::styled("@", Style::default().fg(Color::Yellow)),
        Span::raw(" "),
        Span::styled(preview_text, Style::default().fg(Color::Green)),
    ]));

    // Active actions
    for entity_ref in world.iter_entities() {
        if !entity_ref.contains::<Active>() {
            continue;
        }
        let (Some(pos), Some(timer)) = (entity_ref.get::<Position>(), entity_ref.get::<ActionTimer>()) else {
            continue;
        };
        if !player_visible.contains(&(pos.x, pos.y)) {
            continue;
        }
        let (glyph, color) = entity_ref
            .get::<Renderable>()
            .map(|r| (r.glyph, renderable_color(r.color)))
            .unwrap_or(('?', Color::White));
        let action_label = action_label(world, entity_ref.id());
        out.push(Line::from(vec![
            Span::styled(format!(" {} ", glyph), Style::default().fg(color)),
            Span::raw(action_label),
            Span::styled(
                format!(" {:>3}ms", timer.remaining_av as u32),
                Style::default().fg(Color::DarkGray),
            ),
        ]));
    }
    out.push(Line::from(vec![Span::styled(
        "╰────────────",
        Style::default().fg(Color::Yellow),
    )]));

    // Separator
    out.push(Line::from(Span::styled(
        "─────────────",
        Style::default().fg(Color::DarkGray),
    )));

    // Entity status
    let mut status_entries: Vec<(char, String, i32, i32, Color, Entity)> = Vec::new();
    if let Some(mut q) = world.try_query_filtered::<(Entity, &Position, &EntityName, &Stats, &Renderable), Without<Player>>() {
        for (e, p, n, s, r) in q.iter(world) {
            if !player_visible.contains(&(p.x, p.y)) { continue; }
            let color = Color::Rgb(r.color.0, r.color.1, r.color.2);
            status_entries.push((r.glyph, n.0.clone(), s.hp, s.max_hp, color, e));
        }
    }
    if !status_entries.is_empty() {
        for (glyph, name, hp, mhp, color, _) in &status_entries {
            let hp_color = if *hp as f32 <= *mhp as f32 * dungeon_core::LOW_HP_RATIO {
                Color::Red
            } else {
                Color::Cyan
            };
            out.push(Line::from(vec![
                Span::styled(format!(" {} ", glyph), Style::default().fg(*color)),
                Span::styled(name.clone(), Style::default().fg(*color)),
                Span::raw(" "),
                Span::styled(
                    format!("{:>3}/{:<3}", (*hp).max(0), mhp),
                    Style::default().fg(hp_color),
                ),
            ]));
        }
    }

    // Buffs
    let mut has_buffs = false;
    for (_, _, _, _, _, e) in &status_entries {
        if let Some(ab) = world.get::<ActiveBuffs>(*e)
            && !ab.0.is_empty()
        {
            if !has_buffs {
                out.push(Line::from(Span::styled(
                    "─────────────",
                    Style::default().fg(Color::DarkGray),
                )));
                has_buffs = true;
            }
            for b in &ab.0 {
                let name = match b.kind {
                    dungeon_core::BuffKind::Shield => "护盾",
                    dungeon_core::BuffKind::Berserk => "狂暴",
                };
                out.push(Line::from(vec![
                    Span::styled(
                        format!("  {} +{}", name, b.magnitude),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::styled(
                        format!(" {:>1}s", (b.remaining_av / 1000.0).ceil() as u32),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]));
            }
        }
    }

    // Player buffs
    if let Some(ab) = world
        .try_query::<(&Player, &ActiveBuffs)>()
        .and_then(|mut q| q.iter(world).next().map(|(_, ab)| ab))
        && !ab.0.is_empty()
    {
        if !has_buffs {
            out.push(Line::from(Span::styled(
                "─────────────",
                Style::default().fg(Color::DarkGray),
            )));
        }
        for b in &ab.0 {
            let name = match b.kind {
                dungeon_core::BuffKind::Shield => "护盾",
                dungeon_core::BuffKind::Berserk => "狂暴",
            };
            out.push(Line::from(vec![
                Span::styled(
                    format!("  {} +{}", name, b.magnitude),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    format!(" {:>1}s", (b.remaining_av / 1000.0).ceil() as u32),
                    Style::default().fg(Color::DarkGray),
                ),
            ]));
        }
    }

    if status_entries.is_empty() && out.len() <= 5 {
        out.push(Line::from(Span::styled(
            " (无实体)",
            Style::default().fg(Color::DarkGray),
        )));
    }

    out.push(Line::from(Span::raw("")));
    out.push(Line::from(Span::styled(
        " ↑↓←→移动 1-4技能(双击确认)",
        Style::default().fg(Color::DarkGray),
    )));
    out.push(Line::from(Span::styled(
        " .等待  e背包 x查看",
        Style::default().fg(Color::DarkGray),
    )));
    out
}

fn action_label(world: &World, entity: Entity) -> String {
    if world.get::<Chase>(entity).is_some() {
        "追击".into()
    } else if world.get::<Flee>(entity).is_some() {
        "逃跑".into()
    } else if world.get::<Wander>(entity).is_some() {
        "游荡".into()
    } else if world.get::<Wait>(entity).is_some() {
        "等待".into()
    } else if world.get::<Move>(entity).is_some() {
        "移动".into()
    } else if world.get::<Attack>(entity).is_some() {
        "攻击".into()
    } else if let Some(skill) = world.get::<Skill>(entity) {
        format!("技能{}", skill.0 + 1)
    } else if world.get::<Throw>(entity).is_some() {
        "投掷".into()
    } else {
        "行动".into()
    }
}
