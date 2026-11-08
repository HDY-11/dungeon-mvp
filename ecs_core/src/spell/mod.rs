//! 法术（DsnE26）：**数据文件 + 固定词汇的即时效果**。
//!
//! # 为什么是数据
//!
//! 物品轮（DsnE22）已经拍板"数值是内容、代码只描述解释方式"。法术同理：
//! `assets/spells.json` 写名称/耗蓝/吟唱时间/射程/效果，`ecs_core` 只认识
//! [`SpellEffect`] 这个**动词表**（`damage` / `heal`）。加一个"火花术"
//! 或把它改成范围伤害，改数据即可；加一种全新效果（例如持续 buff）才动代码。
//!
//! 这与 DsnE11 的技能三层并不冲突：这里落的是第 ① 层（激活数据）与第 ③ 层
//! （效果实体的薄版本——即时效果不值得建实体）；第 ② 层"每个技能一处代码"
//! 留给带有专属逻辑的技能（护盾、召唤、位移）。
//!
//! # 施法是一个行动
//!
//! 施法走与攻击同一条行动链路：生成 → 仲裁 → 计时 → 校验 → 落地。
//! **定案**在校验阶段（选目标、检查射程/法力），**落地**在 settle 里只发
//! [`SpellIntentEvent`]；真正扣蓝与结算效果在结算链路的 [`resolve_spell_system`]。
//! 这样"施法动作"与"法术数值"保持分离，和攻击/伤害的关系完全同构。
//!
//! # 不进存档
//!
//! 法术书目前完全由初始数据派生（[`SpellBook::starting`]），没有玩家行为能改变它；
//! 按 DsnE19 的投影原则，不需要写盘。学卷轴（Gm6）落地时才需要升存档版本。

use std::sync::LazyLock;

use bevy_ecs::prelude::*;
use serde::Deserialize;

use crate::components::{EntityName, Health, MagicMastery, Mana, Position, Stats};
use crate::effect::EffectContext;
use crate::entity_cls::Player;
use crate::events::SpellIntentEvent;
use crate::resources::{EventLog, EventMessage};
use crate::rules::{StatId, apply_modifiers, clamp_resistance};

/// 法术数据文件的 schema 版本（与物品数据同一套"两层版本"口径）。
pub const SPELLS_SCHEMA_VERSION: u32 = 1;

/// 内嵌的法术数据（构建期进二进制，无运行期文件 I/O）。
static SPELLS_JSON: &str = include_str!("../../assets/spells.json");

/// **法术注册表**：首次访问时从内嵌 JSON 加载并逐行校验；失败直接 panic。
///
/// 数据文件是编译进二进制的，启动期都过不去的表不该带病运行。
pub static SPELLS: LazyLock<&'static [SpellDef]> = LazyLock::new(|| {
    let loaded = load_spells(SPELLS_JSON).unwrap_or_else(|error| {
        panic!("法术数据加载失败（ecs_core/assets/spells.json）：{error}")
    });
    let leaked: &'static mut [SpellDef] = Box::leak(loaded.into_boxed_slice());
    &*leaked
});

/// 法术表（只读）。数据来自内嵌的 `assets/spells.json`。
pub fn spells() -> &'static [SpellDef] {
    *SPELLS
}

/// 法术编号。数据里编号必须从 0 开始连续递增，所以编号就是下标。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SpellId(pub u16);

impl SpellId {
    /// 查注册表。未知编号会 panic——调用方必须先 [`Self::exists`]（数据/存档校验点）。
    pub fn def(self) -> &'static SpellDef {
        spells()
            .get(self.0 as usize)
            .filter(|def| def.id == self)
            .unwrap_or_else(|| panic!("未知法术编号: {}", self.0))
    }

    /// 编号是否存在（加载期/存档校验用）。
    pub fn exists(self) -> bool {
        spells().get(self.0 as usize).is_some_and(|def| def.id == self)
    }
}

/// 目标口径：自己，还是视野内最近的敌人。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpellTargeting {
    /// 只能对自己施放（治疗、增益）。
    SelfOnly,
    /// 自动选视野内、射程内最近的敌人。
    NearestEnemy,
}

/// 法术效果词表（数据里的 `effect.kind`）。
///
/// 只放**即时效果**：伤害与治疗都在落地那一瞬结算。持续效果（护盾/加速）
/// 将来走"效果实体 + Modifier"（DsnE12），不塞进这个枚举。
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SpellEffect {
    /// 魔法伤害：被 [`StatId::MAGIC_RESIST`] 减伤，不吃护甲。
    Damage { amount: f64 },
    /// 回复生命（受上限钳制）。
    Heal { amount: f64 },
}

/// 一条法术蓝图（GAME.md Gm15 表格的一行）。
#[derive(Debug, Clone, Copy)]
pub struct SpellDef {
    pub id: SpellId,
    pub name: &'static str,
    pub desc: &'static str,
    /// 施放的 MP 消耗。
    pub mp_cost: u32,
    /// 吟唱耗时（AV，固定耗时，不受速度影响）。
    pub cast_av: f64,
    /// 射程（格，欧氏距离）；`SelfOnly` 恒为 0。
    pub range: u32,
    pub targeting: SpellTargeting,
    pub effect: SpellEffect,
}

/// 玩家已知的法术（DsnE26）。**不是存档数据**：目前完全由初始表派生。
#[derive(Component, Debug, Clone, Default, PartialEq, Eq)]
pub struct SpellBook {
    known: Vec<SpellId>,
}

/// 施法行动实体上的载荷：这一发是哪个法术。
///
/// 校验器按它查射程/目标，落地器按它发 [`SpellIntentEvent`]；
/// **法术编号在生成期就固定**，吟唱期间不会改（换目标由 `CastTarget` 表达）。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct CastSpell {
    pub spell: SpellId,
}

impl SpellBook {
    pub fn new(known: Vec<SpellId>) -> Self {
        Self { known }
    }

    /// 初始法术：火花术（0）、治愈术（1）。编号由内容表固定（GAME.md Gm15）。
    pub fn starting() -> Self {
        Self::new(vec![SpellId(0), SpellId(1)])
    }

    pub fn known(&self) -> &[SpellId] {
        &self.known
    }

    /// 第 `slot` 格的法术（0 起的快捷键序号）。
    pub fn get(&self, slot: usize) -> Option<SpellId> {
        self.known.get(slot).copied()
    }
}

/// 施法者能看见并射程内的最近敌人。
///
/// 排序口径与信息栏一致：距离近者优先，同距按实体编号升序（确定性）。
/// 视野用施法者的 [`crate::components::Viewshed`]；射程用欧氏距离。
pub fn nearest_enemy_in_range(
    caster_position: Position,
    viewshed: &crate::components::Viewshed,
    range: u32,
    candidates: impl Iterator<Item = (Entity, Position)>,
) -> Option<Entity> {
    let range = f64::from(range);
    let distance = |position: Position| {
        let dx = position.x.abs_diff(caster_position.x) as f64;
        let dy = position.y.abs_diff(caster_position.y) as f64;
        dx.hypot(dy)
    };
    candidates
        .filter(|(_, position)| viewshed.can_see(position.to_tuple()) && distance(*position) <= range)
        .map(|(entity, position)| (distance(position), entity))
        .min_by(|(left_distance, left), (right_distance, right)| {
            left_distance
                .total_cmp(right_distance)
                .then_with(|| left.to_bits().cmp(&right.to_bits()))
        })
        .map(|(_, entity)| entity)
}

/// 法术伤害：`(威力 + 法术精通) × (1 - 魔法抗性)`，下限 1。
///
/// 与物理伤害（DsnE24 五域）刻意不同：法术**不吃护甲**，只吃抗性；
/// "法术精通"既是威力也是命中后的固定加成，避免出现"低威力法术被抗性归零"。
pub fn spell_damage(amount: f64, mastery: f64, magic_resist: f64) -> f64 {
    let resist = clamp_resistance(magic_resist);
    ((amount + mastery) * (1.0 - resist)).max(1.0)
}

/// 法术治疗：`威力 + 法术精通`（与 Gm2 的治愈公式同口径，熟练度项留到卷轴阶段）。
pub fn spell_heal(amount: f64, mastery: f64) -> f64 {
    (amount + mastery).max(0.0)
}

/// 受害者查询：`Health` 可写，位置/属性/名字可选（玩家单独走 `players`）。
///
/// 抽成别名是为了把"谁会吃法术伤害"的口径只写一次，并避免
/// `clippy::type_complexity` 在函数签名上报警。
type SpellVictims<'w, 's> = Query<
    'w,
    's,
    (
        &'static mut Health,
        &'static Position,
        Option<&'static Stats>,
        Option<&'static EntityName>,
    ),
    Without<Player>,
>;

/// 结算一次施法意图：扣蓝、算效果、写日志（DsnE26）。
///
/// 读 [`SpellIntentEvent`] 而不是在 settle 里直接改血：与
/// [`crate::system::combat`] 的 `AttackIntentEvent → resolve_attack → apply_damage`
/// 保持同构；施法落地器因此仍然只做"发意图"这一件事。
///
/// 目标在吟唱期间死掉时法术落空（法力照扣）；这是"锁定目标"口径的一部分。
pub fn resolve_spell_system(
    mut intents: EventReader<SpellIntentEvent>,
    mut casters: Query<(&mut Mana, &MagicMastery, Option<&EntityName>, Has<Player>)>,
    mut players: Query<(&mut Health, &Position), With<Player>>,
    mut bodies: SpellVictims,
    effects: EffectContext,
    mut event_log: ResMut<EventLog>,
) {
    for intent in intents.read() {
        let Ok((mut mana, mastery, caster_name, is_player)) = casters.get_mut(intent.caster)
        else {
            continue;
        };
        let spell = intent.spell.def();
        if let Err(reason) = mana.try_cost(f64::from(spell.mp_cost)) {
            if is_player {
                event_log.push(EventMessage::skill(format!(
                    "{}没有施放出来：{reason}",
                    spell.name
                )));
            }
            log::debug!("施法失败（法力不足）: caster={:?} spell={}", intent.caster, spell.name);
            continue;
        }
        let caster_label = if is_player {
            "你"
        } else {
            caster_name.map(|name| name.0.as_str()).unwrap_or("对方")
        };

        match spell.effect {
            SpellEffect::Damage { amount } => {
                // 目标必须还活着（`Health` 缺失 = 已经倒下/被回收）。
                let Ok((mut health, position, stats, target_name)) =
                    bodies.get_mut(intent.target)
                else {
                    if is_player {
                        event_log.push(EventMessage::skill("目标已经倒下，法术落空"));
                    }
                    continue;
                };
                let base = stats
                    .map(|stats| stats.get(StatId::MAGIC_RESIST))
                    .unwrap_or(0.0);
                let collected = effects.collect(intent.target, position.to_tuple());
                let resist = apply_modifiers(
                    StatId::MAGIC_RESIST,
                    base,
                    &collected.actor,
                    &collected.ignored,
                );
                let damage = spell_damage(amount, mastery.0, resist);
                *health = health.damage(damage);
                let name = target_name.map(|name| name.0.as_str()).unwrap_or("目标");
                event_log.push(EventMessage::skill(format!(
                    "{caster_label}施放{}，对{name}造成 {damage:.0} 点魔法伤害",
                    spell.name
                )));
            }
            SpellEffect::Heal { amount } => {
                // 治疗只对自己：先试玩家查询，再试非玩家（将来的怪物自愈）。
                let restored = if let Ok((mut health, _)) = players.get_mut(intent.caster) {
                    health.restore(spell_heal(amount, mastery.0))
                } else if let Ok((mut health, _, _, _)) = bodies.get_mut(intent.caster) {
                    health.restore(spell_heal(amount, mastery.0))
                } else {
                    0.0
                };
                event_log.push(EventMessage::skill(format!(
                    "{caster_label}施放{}，恢复了 {restored:.0} 点生命",
                    spell.name
                )));
            }
        }
    }
}

// ── 数据加载与校验 ───────────────────────────────────

/// JSON 的磁盘形状：`{ "schema": 1, "spells": [ ... ] }`。
#[derive(Deserialize)]
struct SpellsFile {
    schema: u32,
    spells: Vec<RawSpell>,
}

/// 数据里一行法术的原始形状（`&'static` 化之前）。
#[derive(Deserialize)]
struct RawSpell {
    id: u16,
    name: String,
    desc: String,
    mp_cost: u32,
    cast_av: f64,
    range: u32,
    targeting: SpellTargeting,
    effect: SpellEffect,
}

/// 加载并逐行校验内嵌数据；错误信息要能直接指出是哪一行哪一项。
fn load_spells(json: &str) -> Result<Vec<SpellDef>, String> {
    let file: SpellsFile = serde_json::from_str(json).map_err(|error| format!("JSON 解析失败：{error}"))?;
    if file.schema != SPELLS_SCHEMA_VERSION {
        return Err(format!(
            "数据 schema 是 {}，当前代码只认识 {}",
            file.schema, SPELLS_SCHEMA_VERSION
        ));
    }
    if file.spells.is_empty() {
        return Err("法术表不能为空".to_string());
    }

    let mut spells = Vec::with_capacity(file.spells.len());
    let mut seen_names = Vec::new();
    for (index, raw) in file.spells.into_iter().enumerate() {
        let label = format!("第 {index} 行（{}）", raw.name);
        // 编号从 0 连续：`SpellId.0` 就是下标，查表 O(1) 且没有空洞歧义。
        if usize::from(raw.id) != index {
            return Err(format!(
                "{label}：编号必须从 0 开始连续递增，这一行写的是 {}",
                raw.id
            ));
        }
        if raw.name.trim().is_empty() || raw.desc.trim().is_empty() {
            return Err(format!("{label}：name / desc 不能为空"));
        }
        if seen_names.iter().any(|name| name == &raw.name) {
            return Err(format!("{label}：法术名重复"));
        }
        seen_names.push(raw.name.clone());
        if raw.mp_cost == 0 {
            return Err(format!("{label}：mp_cost 必须大于 0"));
        }
        if !raw.cast_av.is_finite() || raw.cast_av <= 0.0 {
            return Err(format!("{label}：cast_av 必须是正的有限数"));
        }
        match raw.targeting {
            SpellTargeting::SelfOnly if raw.range != 0 => {
                return Err(format!("{label}：self_only 法术的 range 必须为 0"));
            }
            SpellTargeting::NearestEnemy if raw.range == 0 => {
                return Err(format!("{label}：nearest_enemy 法术的 range 必须大于 0"));
            }
            _ => {}
        }
        match raw.effect {
            SpellEffect::Damage { amount } | SpellEffect::Heal { amount } => {
                if !amount.is_finite() || amount <= 0.0 {
                    return Err(format!("{label}：效果 amount 必须是正数"));
                }
            }
        }

        spells.push(SpellDef {
            id: SpellId(raw.id),
            name: Box::leak(raw.name.into_boxed_str()),
            desc: Box::leak(raw.desc.into_boxed_str()),
            mp_cost: raw.mp_cost,
            cast_av: raw.cast_av,
            range: raw.range,
            targeting: raw.targeting,
            effect: raw.effect,
        });
    }
    Ok(spells)
}

#[cfg(test)]
#[path = "spell_tests.rs"]
mod tests;
