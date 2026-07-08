use dungeon_core::{EntityName, Position, Renderable};
use dungeon_action::{ActionQueue, ActionKindV3, PlayerPreview};
use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};
use std::collections::HashSet;
use bevy_ecs::prelude::World;
use crate::color::renderable_color;

/// 行动表：显示视野内已确认待执行的行动
pub fn build_timeline(player_visible: HashSet<(usize, usize)>, world: &World) -> Vec<Line<'static>> {

    let mut out: Vec<Line<'static>> = Vec::new();

    let preview = {
        world.resource::<PlayerPreview>().kind.clone()
    };

    let preview_text = match &preview {
        Some(ActionKindV3::Move { dx, dy }) => format!("移动({},{})", dx, dy),
        Some(ActionKindV3::Wait) => "等待".into(),
        Some(ActionKindV3::Skill(i)) => format!("技能{}", i + 1),
        Some(ActionKindV3::Attack { .. }) => "攻击".into(),
        _ => "等待输入".into(),
    };
    out.push(Line::from(vec![
        Span::styled("╭──────────", Style::default().fg(Color::Yellow)),
    ]));
    out.push(Line::from(vec![
        Span::styled("│ ", Style::default().fg(Color::Yellow)),
        Span::styled("@", Style::default().fg(Color::Yellow)),
        Span::raw(" "),
        Span::styled(preview_text, Style::default().fg(Color::Green)),
    ]));
    out.push(Line::from(vec![
        Span::styled("╰──────────", Style::default().fg(Color::Yellow)),
    ]));

    {
        let queue = world.resource::<ActionQueue>();
        for entry in &queue.entries {
            let in_view = world.get::<Position>(entry.entity)
                .map(|p| player_visible.contains(&(p.x, p.y)))
                .unwrap_or(false);
            if !in_view { continue; }

            let name = world.get::<EntityName>(entry.entity)
                .map(|n| n.0.chars().take(5).collect::<String>())
                .unwrap_or("?".into());

            let color = world.get::<Renderable>(entry.entity)
                .map(|r| renderable_color(r.color))
                .unwrap_or(Color::DarkGray);

            let (action_label, timer) = action_display(&entry.kind, entry.av_remaining);

            out.push(Line::from(vec![
                Span::styled(format!("{} ", name), Style::default().fg(color)),
                Span::raw(action_label),
                Span::styled(format!(" {:>3}ms", timer as u32), Style::default().fg(Color::DarkGray)),
            ]));
        }
    }

    if out.len() <= 3 {
        out.push(Line::from(Span::styled(" (视野无行动)", Style::default().fg(Color::DarkGray))));
    }

    out.push(Line::from(Span::raw("")));
    out.push(Line::from(Span::styled(" ↑↓←→移动 1-4技能 (双击确认)", Style::default().fg(Color::DarkGray))));
    out.push(Line::from(Span::styled(" .等待  e背包", Style::default().fg(Color::DarkGray))));
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
