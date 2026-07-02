use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};

/// ASCII 渲染颜色（不依赖 ratatui）。渲染层负责转换为目标颜色类型。
pub type RgbColor = (u8, u8, u8);

// ── 速度 → 有效 AV 成本 ────────────────────────────

/// 基础 cost 受速度缩放后的有效 AV 消耗
pub fn effective_cost(base_cost: f32, speed: f32) -> f32 {
    (base_cost * 50.0 / speed.max(1.0)).max(10.0)
}

// ── 基础 ECS 组件 ─────────────────────────────────

#[derive(Component, Clone, Copy, Debug)]
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

#[derive(Component, Clone, Debug)]
pub struct EntityName(pub String);

#[derive(Component)]
pub struct Stairs;

#[derive(Component, Default)]
pub struct FleeLogState {
    pub last_turn_was_flee: bool,
}

// ── 敏捷 → 速度 ──────────────────────────────────

/// 从敏捷推算速度（对数曲线，防止速度过快）
/// 基线 60，对数增长：敏捷×4 → 速度约翻倍
pub fn agility_to_speed(agility: u32) -> f32 {
    let a = agility as f32;
    60.0 + 25.0 * (1.0 + a).ln()
}

// ── 行动值（AV）组件 ──────────────────────────────

/// 行动值：描述实体在跑道上的位置与参数。
///
/// ## 跑道隐喻
/// - `current_av` = 距离终点线的剩余距离。递减至 0 时行动执行。
/// - `base_av` = 本轮行动的总长度（跑道全长）。用于跑道切换时
///   按比例保留已消耗进度。
/// - `speed` = 当前速度。影响行动成本：`base_av = 基础成本 × 50 / speed`。
/// - `reaction_time` = 反应时（锁定窗口）。当 `current_av ≤ reaction_time`
///   时行动不可再修改（Phase 2 将改为 per-entity）。
#[derive(Component, Clone, Debug)]
pub struct ActionValue {
    /// 剩余 AV（距行动还有多远），递减到 0 时执行
    pub current_av: f32,
    /// 本轮行动的总 AV 长度（跑道全长）
    pub base_av: f32,
    /// 当前速度（从敏捷派生，受加减速效果影响）
    pub speed: f32,
    /// 反应时：AV 低于此值后行动锁定，不可修改
    pub reaction_time: f32,
}

impl ActionValue {
    /// 以默认动作（MOVE）初始化，根据敏捷计算速度与 AV。
    pub fn new(agility: u32) -> Self {
        let speed = agility_to_speed(agility);
        let base_av = effective_cost(crate::action_cost::MOVE, speed);
        Self {
            current_av: base_av,
            base_av,
            speed,
            reaction_time: 100.0, // Phase 2 改为 per‑entity
        }
    }

    /// 以指定动作类型初始化（用于非默认动作的 AV 重置）。
    pub fn with_cost(base_cost: f32, agility: u32) -> Self {
        let speed = agility_to_speed(agility);
        let base_av = effective_cost(base_cost, speed);
        Self {
            current_av: base_av,
            base_av,
            speed,
            reaction_time: 100.0,
        }
    }
}

/// 实体初始 AV（默认动作为 MOVE）
pub fn self_av_initial(agility: u32) -> f32 {
    ActionValue::new(agility).current_av
}

// ── 加减速效果组件 ──────────────────────────────

/// 附加在实体上的速度修正效果（持续指定次数行动后自动移除）。
#[derive(Component, Clone, Debug)]
pub struct SpeedModifier {
    /// 速度倍率：1.0 = 正常，>1.0 = 加速（更短 AV），<1.0 = 减速（更长 AV）
    pub factor: f32,
    /// 剩余生效次数（每次实体行动后 -1，归零后移除）
    pub remaining_ticks: u32,
}

// ── 行动预测组件 ──────────────────────────────────

#[derive(Component, Clone, Debug)]
pub struct ActionPrediction {
    /// 本轮预测描述
    pub desc: String,
    /// 本轮动作类型
    pub kind: ActionKind,
    /// 是否已锁定（AV ≤ reaction_time 后不可反悔）
    pub locked: bool,
    /// 本轮是否已确认（防止重复锁定）
    pub just_confirmed: bool,
    /// 下轮预测描述（比当前更远的一步）
    pub next_desc: String,
    /// 下轮动作类型
    pub next_kind: ActionKind,
}

impl ActionPrediction {
    pub fn new(desc: &str, kind: ActionKind) -> Self {
        Self {
            desc: desc.into(), kind: kind.clone(), locked: false,
            just_confirmed: false,
            next_desc: desc.into(), next_kind: kind,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ActionKind {
    Move,
    BumpAttack,
    Skill(usize),
    UseItem(usize),
    Wait,
    // 怪物行动
    Chase,
    Attack,
    Flee,
    Wander,
    // 无行动（待预测）
    None,
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
