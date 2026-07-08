use bevy_ecs::prelude::*;
use rand::Rng;
use serde::{Deserialize, Serialize};

pub type RgbColor = (u8, u8, u8);

// ── 掉落表组件 ─────────────────────────────────────

#[derive(Component, Clone, Debug)]
pub struct LootTable {
    pub entries: Vec<LootEntry>,
}

#[derive(Clone, Debug)]
pub struct LootEntry {
    pub item_id: usize,
    /// 独立掉落概率 0.0 ~ 1.0
    pub chance: f32,
    pub min_count: u32,
    pub max_count: u32,
}

impl LootTable {
    /// 掷骰决定掉落物，返回掉落的 ItemStack 列表
    pub fn roll(&self, rng: &mut impl Rng) -> Vec<crate::items::ItemStack> {
        use rand::RngExt;
        let mut results = Vec::new();
        for entry in &self.entries {
            if rng.random_range(0.0..1.0) < entry.chance {
                let count = if entry.min_count == entry.max_count {
                    entry.min_count
                } else {
                    let range = entry.max_count - entry.min_count + 1;
                    entry.min_count + (rng.random_range(0u32..range))
                };
                if count > 0 {
                    results.push(crate::items::ItemStack::new(entry.item_id, count));
                }
            }
        }
        results
    }
}

// ── 基础 ECS 组件 ─────────────────────────────────

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Position {
    pub x: usize,
    pub y: usize,
}

#[derive(Component, Clone, Debug)]
pub struct Renderable {
    pub glyph: char,
    pub color: RgbColor,
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

/// 怪物种类标识（用于概率生成和属性查询）
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MonsterKindId {
    Rat,
    Scorpion,
    Goblin,
}

#[derive(Component, Clone, Debug)]
pub struct EntityName(pub String);

#[derive(Component)]
pub struct Stairs;

#[derive(Component, Clone, Debug, Serialize, Deserialize)]
pub struct Stats {
    pub level: u32,
    pub hp: i32,
    pub max_hp: i32,
    pub mp: i32,
    pub max_mp: i32,
    pub exp: u64,
    pub exp_to_next: u64,
    pub attack: u32,
    pub defense: u32,
    pub magic_mastery: u32,
    pub agility: u32,
    pub crit_rate: f32,
    pub crit_damage: f32,
}

impl Stats {
    pub fn player() -> Self {
        let level = 1;
        let def_val = 4;
        let magic_val = 8;
        Self {
            level, hp: crate::max_hp_for(level, def_val), max_hp: crate::max_hp_for(level, def_val),
            mp: crate::max_mp_for(level, magic_val), max_mp: crate::max_mp_for(level, magic_val),
            exp: 0, exp_to_next: crate::exp_to_next_level(level),
            attack: 8, defense: def_val, magic_mastery: magic_val, agility: 10,
            crit_rate: 0.05, crit_damage: 0.50,
        }
    }

    pub fn monster(glyph: char, floor: u32) -> Self {
        let level_scale = floor.saturating_sub(1);
        match glyph {
            'r' => {
                let s = level_scale as f64;
                Self {
                    level: (1 + level_scale as u32).min(20), hp: 10 + (s * 4.0) as i32, max_hp: 10 + (s * 4.0) as i32,
                    mp: 0, max_mp: 0,
                    exp: (6.0 + s * 6.0 * 0.5).round() as u64, exp_to_next: 0,
                    attack: (4 + level_scale).min(18) as u32, agility: 5, magic_mastery: 1, defense: 2,
                    crit_rate: 0.05, crit_damage: 0.50,
                }
            },
            'g' => {
                let s = level_scale as f64;
                Self {
                    level: (1 + level_scale as u32).min(20), hp: 18 + (s * 6.0) as i32, max_hp: 18 + (s * 6.0) as i32,
                    mp: 0, max_mp: 0,
                    exp: (15.0 + s * 15.0 * 0.5).round() as u64, exp_to_next: 0,
                    attack: (6 + level_scale * 2).min(25) as u32, agility: 3, magic_mastery: 3, defense: 4,
                    crit_rate: 0.05, crit_damage: 0.50,
                }
            },
            _ => {
                let s = level_scale as f64;
                Self {
                    level: (1 + level_scale as u32).min(20),
                    hp: 10 + (s * 3.0) as i32, max_hp: 10 + (s * 3.0) as i32,
                    mp: 0, max_mp: 0, exp: (5.0 + s * 5.0 * 0.5).round() as u64, exp_to_next: 0,
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

#[derive(Component, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlayerClass {
    Warrior, Mage, Priest,
}

impl PlayerClass {
    pub fn can_cast(&self, skill: &Skill) -> bool {
        match self {
            PlayerClass::Warrior => matches!(skill.kind, SkillKind::Shield { .. } | SkillKind::Berserk { .. }),
            PlayerClass::Mage => matches!(skill.kind, SkillKind::Shield { .. } | SkillKind::Berserk { .. }),
            PlayerClass::Priest => matches!(skill.kind, SkillKind::Heal { .. } | SkillKind::Shield { .. }),
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self { PlayerClass::Warrior => "战士", PlayerClass::Mage => "法师", PlayerClass::Priest => "牧师" }
    }

    pub fn skills(&self) -> Vec<Skill> {
        match self {
            PlayerClass::Warrior => vec![
                Skill { name: "护盾", key: '1', cost_mp: 5, description: "防御+5持续3回合", kind: SkillKind::Shield { def_boost: 5, duration: 3 } },
                Skill { name: "狂暴", key: '2', cost_mp: 5, description: "攻击+5持续3回合", kind: SkillKind::Berserk { atk_boost: 5, duration: 3 } },
            ],
            PlayerClass::Mage => vec![
                Skill { name: "护盾", key: '1', cost_mp: 5, description: "防御+5持续3回合", kind: SkillKind::Shield { def_boost: 5, duration: 3 } },
                Skill { name: "狂暴", key: '2', cost_mp: 5, description: "攻击+5持续3回合", kind: SkillKind::Berserk { atk_boost: 5, duration: 3 } },
            ],
            PlayerClass::Priest => vec![
                Skill { name: "治愈", key: '1', cost_mp: 6, description: "HP+15", kind: SkillKind::Heal { amount: 15 } },
                Skill { name: "护盾", key: '2', cost_mp: 5, description: "防御+5持续3回合", kind: SkillKind::Shield { def_boost: 5, duration: 3 } },
            ],
        }
    }
}

// ── Buff 系统（AV 制） ──────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuffKind { Shield, Berserk }

/// 堆叠标记（预留，当前不实现叠加逻辑，同种 Buff 刷新增时长）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuffStackType { None }

#[derive(Clone, Debug)]
pub struct Buff {
    pub kind: BuffKind,
    pub remaining_av: f32,
    pub magnitude: i32,
    pub stack_type: BuffStackType,
}

#[derive(Component, Clone, Debug)]
pub struct ActiveBuffs(pub Vec<Buff>);
impl ActiveBuffs { pub fn new() -> Self { Self(Vec::new()) } }

/// 技能冷却（AV 制，与 ActiveBuffs 共享受同一推进机制）
#[derive(Clone, Debug)]
pub struct Cooldown { pub skill_id: usize, pub remaining_av: f32 }

#[derive(Component, Clone, Debug, Default)]
pub struct ActiveCooldowns(pub Vec<Cooldown>);

#[derive(Component, Clone, Debug, Default)]
pub struct LastKnownPlayerPos(pub Option<(usize, usize)>);

#[derive(Component, Clone, Serialize, Deserialize)]
pub struct Buffs {
    pub shield_turns: i32, pub shield_def: i32,
    pub berserk_turns: i32, pub berserk_atk: i32,
}
impl Buffs { pub fn new() -> Self { Self { shield_turns: 0, shield_def: 0, berserk_turns: 0, berserk_atk: 0 } } }

#[derive(Component, Clone, Debug)]
pub struct AttackName(pub String);
