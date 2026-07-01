use bevy_ecs::prelude::*;
use ratatui::style::Color;
use serde::{Deserialize, Serialize};

// ── 基础 ECS 组件 ─────────────────────────────────

#[derive(Component, Clone, Copy, Debug)]
pub struct Position {
    pub x: usize,
    pub y: usize,
}

#[derive(Component, Clone, Debug)]
pub struct Renderable {
    pub glyph: char,
    pub color: Color,
}

#[derive(Component)]
pub struct Player;

#[derive(Component, Default, Debug)]
pub struct MovingDir {
    pub dx: isize,
    pub dy: isize,
}

#[derive(Component, Clone, Debug)]
pub struct Viewshed {
    pub range: usize,
    pub visible_tiles: Vec<(usize, usize)>,
}

#[derive(Component)]
pub struct Monster;

#[derive(Component, Clone, Debug)]
pub struct EntityName(pub String);

#[derive(Component)]
pub struct Stairs;

#[derive(Component, Default)]
pub struct FleeLogState {
    pub last_turn_was_flee: bool,
}

#[derive(Component, Clone, Debug)]
pub struct ActionPoints {
    pub points: f32,
    pub speed: f32,
}

impl ActionPoints {
    pub fn new(dexterity: u32) -> Self {
        Self { points: 0.0, speed: 50.0 + dexterity as f32 * 3.0 }
    }
}

#[derive(Component, Clone, Debug, Serialize, Deserialize)]
pub struct Stats {
    pub level: u32,
    pub hp: i32,
    pub max_hp: i32,
    pub mp: i32,
    pub max_mp: i32,
    pub exp: u64,
    pub exp_to_next: u64,
    pub strength: u32,
    pub dexterity: u32,
    pub intelligence: u32,
    pub vitality: u32,
}

impl Stats {
    pub fn player() -> Self {
        let level = 1;
        let vitality = 4;
        let intelligence = 8;
        Self {
            level,
            hp: crate::max_hp_for(level, vitality),
            max_hp: crate::max_hp_for(level, vitality),
            mp: crate::max_mp_for(level, intelligence),
            max_mp: crate::max_mp_for(level, intelligence),
            exp: 0,
            exp_to_next: crate::exp_to_next_level(level),
            strength: 8, dexterity: 10, intelligence, vitality,
        }
    }

    pub fn monster(glyph: char, floor: u32) -> Self {
        let level_scale = floor.saturating_sub(1);
        match glyph {
            'r' => {
                let lvl = (1 + level_scale).min(20);
                let s = level_scale as i32;
                Self {
                    level: lvl as u32, hp: 10 + s * 4, max_hp: 10 + s * 4,
                    mp: 0, max_mp: 0,
                    exp: (6 + s * 3) as u64, exp_to_next: 0,
                    strength: (4 + level_scale).min(18) as u32, dexterity: 5, intelligence: 1, vitality: 2,
                }
            },
            'g' => {
                let lvl = (1 + level_scale).min(20);
                let s = level_scale as i32;
                Self {
                    level: lvl as u32, hp: 18 + s * 6, max_hp: 18 + s * 6,
                    mp: 0, max_mp: 0,
                    exp: (15 + s * 6) as u64, exp_to_next: 0,
                    strength: (6 + level_scale * 2).min(25) as u32, dexterity: 3, intelligence: 3, vitality: 4,
                }
            },
            _ => {
                let s = level_scale as i32;
                Self {
                    level: (1 + level_scale).min(20) as u32,
                    hp: 10 + s * 3, max_hp: 10 + s * 3,
                    mp: 0, max_mp: 0,
                    exp: (5 + s * 3) as u64, exp_to_next: 0,
                    strength: (3 + level_scale).min(10) as u32, dexterity: 3, intelligence: 1, vitality: 3,
                }
            },
        }
    }

    pub fn attack(&self) -> u32 { self.strength }
    pub fn defense(&self) -> u32 { self.vitality / 3 + self.level / 2 + crate::defense_bonus(self.level) }
}

#[derive(Clone, Debug)]
pub enum SkillKind {
    Heal { amount: i32 },
    Firebolt { damage: i32 },
    Shield { def_boost: i32, duration: i32 },
    Berserk { atk_boost: i32, duration: i32 },
}

#[derive(Clone, Debug)]
pub struct Skill {
    pub name: &'static str,
    pub key: char,
    pub cost_mp: i32,
    pub description: &'static str,
    pub kind: SkillKind,
}

#[derive(Component)]
pub struct Skills {
    pub list: Vec<Skill>,
}

impl Skills {
    pub fn default_skills() -> Self {
        Self {
            list: vec![
                Skill { name: "治愈", key: '1', cost_mp: 6, description: "HP+15", kind: SkillKind::Heal { amount: 15 } },
                Skill { name: "火球", key: '2', cost_mp: 10, description: "对邻接敌人造成15伤害", kind: SkillKind::Firebolt { damage: 15 } },
                Skill { name: "护盾", key: '3', cost_mp: 5, description: "DEF+5持续3回合", kind: SkillKind::Shield { def_boost: 5, duration: 3 } },
                Skill { name: "狂暴", key: '4', cost_mp: 5, description: "ATK+5持续3回合", kind: SkillKind::Berserk { atk_boost: 5, duration: 3 } },
            ],
        }
    }
}

#[derive(Component, Clone, Serialize, Deserialize)]
pub struct Buffs {
    pub shield_turns: i32, pub shield_def: i32,
    pub berserk_turns: i32, pub berserk_atk: i32,
}
impl Buffs { pub fn new() -> Self { Self { shield_turns: 0, shield_def: 0, berserk_turns: 0, berserk_atk: 0 } } }
