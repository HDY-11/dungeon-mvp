//! 行动轴渲染。
//!
//! 从 `World` 读取所有实体的 AV 状态与预测，产出行序列的 ASCII 布局。

use bevy_ecs::prelude::{Entity, World};
use dungeon_core::{
    ActionPrediction, ActionValue, EntityName, GamePacing, PacingMode, Player, Position,
    Renderable,
};
use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};
use std::collections::HashSet;

use crate::color::renderable_color;

/// 行动轴：每实体显示当前 AV、动作(锁定/预测/下轮)。
///
/// 排序按 AV 升序（距终点最近者排最前）。
/// 锁定条目与未锁定条目之间画一条锁定边界线。
/// 玩家未锁定条目闪烁提醒"可修改"。
pub fn build_timeline(
    world: &mut World,
    player_visible: HashSet<(usize, usize)>,
) -> Vec<Line<'static>> {
    let mut out: Vec<Line<'static>> = Vec::new();

    struct Card {
        label: String,
        av: f32,
        reaction_time: f32,
        action: String,
        locked: bool,
        next_action: String,
        is_player: bool,
        color: Color,
    }

    let mut cards: Vec<Card> = Vec::new();

    // 玩家卡片
    if let Some((av, pred)) = world
        .query::<(&Player, &ActionValue, &ActionPrediction)>()
        .iter(world)
        .next()
        .map(|(_, av, p)| (av, p))
    {
        cards.push(Card {
            label: "@".into(),
            av: av.current_av,
            reaction_time: av.reaction_time,
            action: pred.desc.clone(),
            locked: pred.locked,
            next_action: pred.next_desc.clone(),
            is_player: true,
            color: Color::Yellow,
        });
    }

    // 怪物卡片（仅可见 + 非玩家）
    let mut name_counts: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    for (e, pos, name, rend, av, pred) in world
        .query::<(Entity, &Position, &EntityName, &Renderable, &ActionValue, &ActionPrediction)>()
        .iter(world)
    {
        if world.get::<Player>(e).is_some() {
            continue;
        }
        if !player_visible.contains(&(pos.x, pos.y)) {
            continue;
        }
        let key = name.0.clone();
        let count = name_counts.get(&key).copied().unwrap_or(0);
        name_counts.insert(key.clone(), count + 1);
        let label = if count > 0 {
            format!(
                "{}({},{})",
                name.0.chars().take(4).collect::<String>(),
                pos.x,
                pos.y
            )
        } else {
            name.0.chars().take(5).collect()
        };
        cards.push(Card {
            label,
            av: av.current_av,
            reaction_time: av.reaction_time,
            action: pred.desc.clone(),
            locked: pred.locked,
            next_action: pred.next_desc.clone(),
            is_player: false,
            color: renderable_color(rend.color),
        });
    }

    // 按 AV 升序
    cards.sort_by(|a, b| a.av.partial_cmp(&b.av).unwrap());

    // 锁定边界索引
    let boundary_idx = cards.iter().position(|c| !c.locked);

    // 渲染
    let blink = world.resource::<GamePacing>().blink_phase;

    for (i, card) in cards.iter().enumerate() {
        if Some(i) == boundary_idx {
            let rt_min = cards
                .iter()
                .take_while(|c| c.locked)
                .map(|c| c.reaction_time as u32)
                .min()
                .unwrap_or(100);
            let rt_max = cards
                .iter()
                .take_while(|c| c.locked)
                .map(|c| c.reaction_time as u32)
                .max()
                .unwrap_or(100);
            let rt_label = if rt_min == rt_max {
                format!(" ──── 锁定边界 (RT={}) ────", rt_min)
            } else {
                format!(" ──── 锁定边界 (RT={}~{}) ────", rt_min, rt_max)
            };
            out.push(Line::from(Span::styled(
                rt_label,
                Style::default().fg(Color::Yellow),
            )));
        }

        let locked = card.locked;
        let (top, side, bottom, fg) = if locked {
            ("╭", "│", "╰", Color::Red)
        } else {
            ("┌", "│", "└", Color::DarkGray)
        };

        let name_fg = if card.is_player && !locked && blink {
            Color::DarkGray
        } else if card.is_player {
            Color::Yellow
        } else {
            card.color
        };

        let av_str = format!("{:>3}", card.av as u32);
        let act_trim: String = card.action.chars().take(5).collect();
        let next_trim: String = card.next_action.chars().take(5).collect();

        out.push(Line::from(Span::styled(
            format!(" {} {}", top, "────────────"),
            Style::default().fg(fg),
        )));
        out.push(Line::from(vec![
            Span::styled(format!(" {} ", side), Style::default().fg(fg)),
            Span::styled(format!("{:<6}", card.label), Style::default().fg(name_fg)),
            Span::styled(format!("AV{:>3}", av_str), Style::default().fg(name_fg)),
            Span::raw(" "),
            Span::styled(
                format!("{:<4}", act_trim),
                Style::default().fg(if card.is_player {
                    Color::Green
                } else {
                    Color::DarkGray
                }),
            ),
            Span::styled("→", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{:<4}", next_trim), Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{}", side), Style::default().fg(fg)),
        ]));
        out.push(Line::from(Span::styled(
            format!(" {} {}", bottom, "────────────"),
            Style::default().fg(fg),
        )));
    }

    if cards.is_empty() {
        out.push(Line::from(Span::styled(
            " (视野无实体)",
            Style::default().fg(Color::DarkGray),
        )));
        out.push(Line::from(Span::raw("")));
    }

    // 底部操作提示
    let pacing = world.resource::<GamePacing>();
    let combat_active = pacing.combat_active;
    let paused = matches!(pacing.mode, PacingMode::CombatPaused);

    if paused {
        out.push(Line::from(Span::styled(
            " [锁定中]",
            Style::default().fg(Color::Yellow),
        )));
        out.push(Line::from(Span::styled(
            " Enter确认  .等待",
            Style::default().fg(Color::DarkGray),
        )));
    } else if combat_active {
        out.push(Line::from(Span::styled(
            " [战斗中]",
            Style::default().fg(Color::Yellow),
        )));
        out.push(Line::from(Span::styled(
            " ↑↓←→移动  1-4技能",
            Style::default().fg(Color::DarkGray),
        )));
        out.push(Line::from(Span::styled(
            " .等待  e背包",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        out.push(Line::from(Span::styled(
            " ↑↓←→移动",
            Style::default().fg(Color::DarkGray),
        )));
        out.push(Line::from(Span::styled(
            " 1-4技能  .等待",
            Style::default().fg(Color::DarkGray),
        )));
        out.push(Line::from(Span::styled(
            " e背包  >下楼",
            Style::default().fg(Color::DarkGray),
        )));
    }

    out
}
