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
    pub fn new(agility: u32) -> Self {
        Self { points: 0.0, speed: 50.0 + agility as f32 * 3.0 }
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
    /// 攻击
    pub attack: u32,
    /// 防御
    pub defense: u32,
    /// 法术精通
    pub magic_mastery: u32,
    /// 敏捷
    pub agility: u32,
    /// 暴击率 (默认 5%)
    pub crit_rate: f32,
    /// 暴击伤害 (默认 50%)
    pub crit_damage: f32,
}

impl Stats {
    pub fn player() -> Self {
        let level = 1;
        let def_val = 4;
        let magic_val = 8;
        Self {
            level,
            hp: crate::max_hp_for(level, def_val),
            max_hp: crate::max_hp_for(level, def_val),
            mp: crate::max_mp_for(level, magic_val),
            max_mp: crate::max_mp_for(level, magic_val),
            exp: 0,
            exp_to_next: crate::exp_to_next_level(level),
            attack: 8, defense: def_val, magic_mastery: magic_val, agility: 10,
            crit_rate: 0.05, crit_damage: 0.50,
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
                    attack: (4 + level_scale).min(18) as u32, agility: 5, magic_mastery: 1, defense: 2,
                    crit_rate: 0.05, crit_damage: 0.50,
                }
            },
            'g' => {
                let lvl = (1 + level_scale).min(20);
                let s = level_scale as i32;
                Self {
                    level: lvl as u32, hp: 18 + s * 6, max_hp: 18 + s * 6,
                    mp: 0, max_mp: 0,
                    exp: (15 + s * 6) as u64, exp_to_next: 0,
                    attack: (6 + level_scale * 2).min(25) as u32, agility: 3, magic_mastery: 3, defense: 4,
                    crit_rate: 0.05, crit_damage: 0.50,
                }
            },
            _ => {
                let s = level_scale as i32;
                Self {
                    level: (1 + level_scale).min(20) as u32,
                    hp: 10 + s * 3, max_hp: 10 + s * 3,
                    mp: 0, max_mp: 0,
                    exp: (5 + s * 3) as u64, exp_to_next: 0,
                    attack: (3 + level_scale).min(10) as u32, agility: 3, magic_mastery: 1, defense: 3,
                    crit_rate: 0.05, crit_damage: 0.50,
                }
            },
        }
    }
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

// ── 职业组件 ─────────────────────────────────────┐

#[derive(Component, Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PlayerClass {
    Warrior,   // 战士：护盾、狂暴
    Mage,      // 法师：火球
    Priest,    // 牧师：治愈、护盾
}

impl PlayerClass {
    /// 该职业是否允许施放此技能
    pub fn can_cast(&self, skill: &Skill) -> bool {
        match self {
            PlayerClass::Warrior => matches!(skill.kind, SkillKind::Shield { .. } | SkillKind::Berserk { .. }),
            PlayerClass::Mage => matches!(skill.kind, SkillKind::Firebolt { .. }),
            PlayerClass::Priest => matches!(skill.kind, SkillKind::Heal { .. } | SkillKind::Shield { .. }),
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            PlayerClass::Warrior => "战士",
            PlayerClass::Mage => "法师",
            PlayerClass::Priest => "牧师",
        }
    }

    /// 每个职业特有的技能列表
    pub fn skills(&self) -> Vec<Skill> {
        match self {
            PlayerClass::Warrior => vec![
                Skill { name: "护盾", key: '1', cost_mp: 5, description: "防御+5持续3回合", kind: SkillKind::Shield { def_boost: 5, duration: 3 } },
                Skill { name: "狂暴", key: '2', cost_mp: 5, description: "攻击+5持续3回合", kind: SkillKind::Berserk { atk_boost: 5, duration: 3 } },
            ],
            PlayerClass::Mage => vec![
                Skill { name: "火球", key: '1', cost_mp: 10, description: "对邻接敌人造成15伤害", kind: SkillKind::Firebolt { damage: 15 } },
            ],
            PlayerClass::Priest => vec![
                Skill { name: "治愈", key: '1', cost_mp: 6, description: "HP+15", kind: SkillKind::Heal { amount: 15 } },
                Skill { name: "护盾", key: '2', cost_mp: 5, description: "防御+5持续3回合", kind: SkillKind::Shield { def_boost: 5, duration: 3 } },
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

// ── 招式名组件 ──────────────────────────────────────

#[derive(Component, Clone, Debug)]
pub struct AttackName(pub String);

// ── 行动预告组件 ─────────────────────────────────────

#[derive(Component, Clone, Debug)]
pub struct ActionPreview {
    /// 上次预告的行动描述 (避免重复刷屏)
    pub last_preview: Option<String>,
}
impl ActionPreview {
    pub fn new() -> Self { Self { last_preview: None } }
}
