//! ECS 组件定义：玩家、怪物、物品、地图实体、渲染/战斗所需的全部组件。
//!
//! 组件只保存数据，不包含行为逻辑；行为由 dungeon-action / dungeon-world 中的 system 处理。

use bevy_ecs::prelude::*;
use rand::Rng;
use serde::{Deserialize, Serialize};

pub type RgbColor = (u8, u8, u8);

/// 渲染用视图元组别名（无 Entity）：(x, y, glyph, color)
pub type RenderableView = (usize, usize, char, RgbColor);
/// 渲染用视图元组别名（含 Entity）：(entity, x, y, glyph, color)
pub type EntityRenderable = (Entity, usize, usize, char, RgbColor);

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

// 为组件添加的快捷方法，用于优化性能和提高内聚性
impl Position {
    pub fn to_tuple(&self) -> (usize, usize) {
        (self.x, self.y)
    }

    /// 判断八个方位（相邻且不同格）
    pub fn is_near(&self, other: &Position) -> bool {
        let dx = self.x as isize - other.x as isize;
        let dy = self.y as isize - other.y as isize;
        (dx > -2 && dx < 2) && (dy > -2 && dy < 2) && (dx | dy) != 0
    }

    /// 切比雪夫距离（8 方向移动距离）
    pub fn chebyshev(&self, other: &Position) -> usize {
        self.x.abs_diff(other.x).max(self.y.abs_diff(other.y))
    }

    /// 曼哈顿距离（4 方向移动距离）
    pub fn manhattan(&self, other: &Position) -> usize {
        self.x.abs_diff(other.x) + self.y.abs_diff(other.y)
    }
}


#[derive(Component, Clone, Debug)]
pub struct Renderable {
    pub glyph: char,
    pub color: RgbColor,
}

#[derive(Component)]
pub struct Player;

#[derive(Component, Clone, Debug)]
pub struct Viewshed {
    pub range: usize,
    pub visible_tiles: Vec<(usize, usize)>,
}

impl Viewshed {
    pub fn can_see(&self, pos: (usize, usize)) -> bool {
        self.visible_tiles.contains(&pos)
    }
}


#[derive(Component)]
pub struct Monster;

/// 怪物种类标识（用于概率生成和属性查询）
/// 组件化后随实体持久化（A24），避免读档时用 glyph 反推。
/// Dsn24: 新变体追加在末尾（serde 按序编号 0-2 为旧三种，旧存档兼容）。
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum MonsterKindId {
    Rat,
    Scorpion,
    Goblin,
    // Dsn24 多类型地图怪物
    Sporeling,     // m 孢子怪（繁茂洞穴，弱怪）
    MushroomGolem, // M 蘑菇傀儡（繁茂洞穴，中怪）
    CaveFish,      // f 洞穴鱼（地海，弱怪快速）
    CaveCrab,      // c 洞穴蟹（地海，中怪高防）
    DeepEel,       // e 深鳗（地海，中怪）
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
            level,
            hp: crate::max_hp_for(level, def_val),
            max_hp: crate::max_hp_for(level, def_val),
            mp: crate::max_mp_for(level, magic_val),
            max_mp: crate::max_mp_for(level, magic_val),
            exp: 0,
            exp_to_next: crate::exp_to_next_level(level),
            attack: 8,
            defense: def_val,
            magic_mastery: magic_val,
            agility: 10,
            crit_rate: 0.05,
            crit_damage: 0.50,
        }
    }

    pub fn hp_ratio(&self) -> f32 {
        self.hp as f32 / self.max_hp as f32
    }

    pub fn is_dead(&self) -> bool {
        self.hp <= 0
    }

    pub fn is_low_hp(&self, ratio: f32) -> bool {
        self.hp_ratio() <= ratio
    }

    pub fn heal(&mut self, amount: i32) -> i32 {
        let actual = amount.min(self.max_hp - self.hp);
        self.hp += actual;
        actual
    }

    pub fn restore_mp(&mut self, amount: i32) -> i32 {
        let actual = amount.min(self.max_mp - self.mp);
        self.mp += actual;
        actual
    }

    pub fn can_afford_mp(&self, cost: i32) -> bool {
        self.mp >= cost
    }

    pub fn spend_mp(&mut self, cost: i32) {
        self.mp -= cost;
    }

    pub fn take_damage(&mut self, amount: i32) {
        self.hp -= amount;
    }
}

#[derive(Clone, Debug)]
pub enum SkillKind {
    Heal { amount: i32 },
    Shield { def_boost: i32, duration: u32 },
    Berserk { atk_boost: i32, duration: u32 },
}

#[derive(Clone, Debug)]
pub struct Skill {
    pub name: String,
    pub key: char,
    pub cost_mp: i32,
    pub description: String,
    pub kind: SkillKind,
    pub proficiency: u32,
}

#[derive(Component, Clone)]
pub struct Skills {
    pub list: Vec<Skill>,
}

impl Skills {
    pub fn index_of_key(&self, key: char) -> Option<usize> {
        self.list.iter().position(|sk| sk.key == key)
    }
}


#[derive(Component, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlayerClass {
    Warrior,
    Mage,
    Priest,
}

impl PlayerClass {
    /// Dsn13 无职业设计：初始无技能，全部由卷轴学习（I61 修复后链路畅通）
    pub fn skills(&self) -> Vec<Skill> {
        Vec::new()
    }
}

// ── Buff 系统（AV 制） ──────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuffKind {
    Shield,
    Berserk,
}

/// 堆叠标记（预留，当前不实现叠加逻辑，同种 Buff 刷新增时长）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuffStackType {
    None,
}

#[derive(Clone, Debug)]
pub struct Buff {
    pub kind: BuffKind,
    pub remaining_av: f32,
    pub magnitude: i32,
    pub stack_type: BuffStackType,
}

#[derive(Component, Clone, Debug)]
pub struct ActiveBuffs(pub Vec<Buff>);
impl ActiveBuffs {
    pub fn new() -> Self {
        Self(Vec::new())
    }

    pub fn set(&mut self, kind: BuffKind, remaining_av: f32, magnitude: i32) {
        if let Some(existing) = self.0.iter_mut().find(|b| b.kind == kind) {
            existing.remaining_av = remaining_av;
            existing.magnitude = magnitude;
        } else {
            self.0.push(Buff {
                kind,
                remaining_av,
                magnitude,
                stack_type: BuffStackType::None,
            });
        }
    }
}

impl Default for ActiveBuffs {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Component, Clone, Debug, Default)]
pub struct LastKnownPlayerPos(pub Option<(usize, usize)>);

#[derive(Component, Clone, Debug)]
pub struct AttackName(pub String);

// ── 技能卷轴组件 ────────────────────────────────────
