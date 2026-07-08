use dungeon_core::{
    ActiveBuffs, EntityName, Player, Position, Renderable, Stats,
};
use dungeon_action::{ActionQueue, ActionKindV3, PlayerPreview};
use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};
use std::collections::HashSet;
use bevy_ecs::prelude::World;
use crate::color::renderable_color;

/// 行动表：整合 Buff 倒计时 + 视野内实体 HP
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

    // ── 队列条目（带 HP 标注） ──
    let queue = world.resource::<ActionQueue>();
    let mut queue_set: HashSet<(usize, usize)> = HashSet::new();
    for entry in &queue.entries {
        let Some(pos) = world.get::<Position>(entry.entity) else { continue };
        if !player_visible.contains(&(pos.x, pos.y)) { continue; }
        queue_set.insert((pos.x, pos.y));

        let name = world.get::<EntityName>(entry.entity)
            .map(|n| n.0.chars().take(6).collect::<String>())
            .unwrap_or("?".into());
        let color = world.get::<Renderable>(entry.entity)
            .map(|r| renderable_color(r.color))
            .unwrap_or(Color::DarkGray);
        let hp_text = world.get::<Stats>(entry.entity)
            .map(|s| format!("HP({}/{})", s.hp.max(0), s.max_hp))
            .unwrap_or_default();
        let (action_label, timer) = action_display(&entry.kind, entry.av_remaining);

        out.push(Line::from(vec![
            Span::styled(format!("{} ", name), Style::default().fg(color)),
            Span::styled(hp_text, Style::default().fg(Color::Cyan)),
            Span::raw(" "),
            Span::raw(action_label),
            Span::styled(format!(" {:>3}ms", timer as u32), Style::default().fg(Color::DarkGray)),
        ]));
    }
    out.push(Line::from(vec![Span::styled("╰────────────", Style::default().fg(Color::Yellow))]));

    // ── Buff 倒计时 ──
    if let Some(ab) = world.try_query::<(&Player, &ActiveBuffs)>()
        .expect("Player+ActiveBuffs registered")
        .iter(world).next().map(|(_, ab)| ab)
    {
        if !ab.0.is_empty() {
            out.push(Line::from(Span::styled("── Buff ──", Style::default().fg(Color::Green))));
            for b in &ab.0 {
                let name = match b.kind {
                    dungeon_core::BuffKind::Shield => "护盾",
                    dungeon_core::BuffKind::Berserk => "狂暴",
                };
                out.push(Line::from(vec![
                    Span::styled(format!(" {} +{}", name, b.magnitude), Style::default().fg(Color::Yellow)),
                    Span::styled(format!(" {:>2}s", (b.remaining_av / 1000.0).ceil() as u32), Style::default().fg(Color::DarkGray)),
                ]));
            }
        }
    }

    // ── 视野内实体（排除已在队列中出现的） ──
    let vision_ents: Vec<(String, i32, i32, Color)> = {
        let mut q = world.try_query::<(&Position, &EntityName, &Stats, &Renderable)>().expect("Pos+Name+Stats+Rend reg");
        q.iter(world)
            .filter(|(p, n, _, _)| player_visible.contains(&(p.x, p.y)) && n.0 != "冒险者")
            .filter(|(p, _, _, _)| !queue_set.contains(&(p.x, p.y)))
            .map(|(_, n, s, r)| (n.0.clone(), s.hp, s.max_hp, renderable_color(r.color)))
            .collect()
    };
    if !vision_ents.is_empty() {
        out.push(Line::from(Span::styled("── 视野 ──", Style::default().fg(Color::DarkGray))));
        for (name, hp, mhp, color) in &vision_ents {
            let hp_color = if *hp <= *mhp / 3 { Color::Red } else { Color::White };
            out.push(Line::from(vec![
                Span::styled(name.clone(), Style::default().fg(*color)),
                Span::raw(" "),
                Span::styled(format!("({}/{})", (*hp).max(0), mhp), Style::default().fg(hp_color)),
            ]));
        }
    }

    // ── 空状态提示 ──
    if queue_set.is_empty() && vision_ents.is_empty() {
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