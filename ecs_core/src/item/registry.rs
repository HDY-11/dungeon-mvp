//! 物品注册表：**一份数据文件 + 一个 ID**（GAME.md Gm7，DESIGN DsnE22 / DsnE27）。
//!
//! # 数据在文件，加载在启动期
//!
//! 物品的数值、效果、重量、可装位标签全部写在 `ecs_core/assets/items.json` 里；构建时
//! `include_str!` 内嵌进二进制，第一次访问 [`ITEMS`] 时解析并**逐行校验**。运行时没有
//! "读文件失败"这条路径，但"数据写错了"会在启动/测试期直接报错，而不是变成某个玩家手里
//! "捡不起来"的装备。
//!
//! # 核心字段 + 部件（DsnE27）
//!
//! 数据行的形状是：
//!
//! ```text
//! { id, name, desc, quality, weight, max_stack, tags, parts: [ ... ] }
//! ```
//!
//! 核心字段是**每件物品都有、且物品之间只取值不同**的东西；"它是不是装备/武器/容器、
//! 带来什么修正、有什么行为"都写进 `parts`，每种部件有自己的一组字段：
//!
//! | part `type` | 字段 | 装配后变成 |
//! |---|---|---|
//! | `equippable` | — | [`Equippable`](crate::item::Equippable) marker |
//! | `consumable` | — | [`Consumable`](crate::item::Consumable) marker |
//! | `weapon` | `speed_ms` / `strength_cap` / `growth` | [`Weapon`](crate::item::Weapon) |
//! | `container` | `layout` | [`Container`](crate::item::Container) |
//! | `modifiers` | `list` | [`Modifiers`](crate::item::Modifiers) |
//! | `behaviors` | `list` | [`Behaviors`](crate::item::Behaviors) |
//!
//! 加一件"新语义"的物品时，只加它实际有的部件；材料写空数组。加一种**全新语义**
//! （例如施法工具）仍然要加一个 part 变体 + 一个组件 + 一个装配分支——数据化省掉的是
//! "每个物品结构体都多一个 Option 字段"，不是"新概念零代码"。
//!
//! # 形状 vs 数据（分阶段落地）
//!
//! - DsnE22 阶段 ①–④：物品表搬到数据文件、属性词表、稠密组件、行为数据化、实例组件。
//! - DsnE27：`kind` 枚举与装备/武器的散装 `Option` 字段合并成 `parts`；耐久按用户要求
//!   删除；装配后组件是运行期真相。
//!
//! 关键仍然是没有**中央分派**：代码里没有 `match item_id { .. }`，物品的行为由数据上的
//! 部件/标签/效果决定。
//!
//! # 编号
//!
//! [`ItemId`] 的数值与 GAME.md Gm7 的表格**一一对应**（0/1/2/3 装备、10–14 材料、
//! 18–32 消耗品与模板）。留空号是有意的：GAME.md 的表格是权威，代码跟着它走，
//! 而不是反过来重排号段让"看起来紧凑"。
//!
//! # 加一件物品要改几处（LECS22）
//!
//! 1. `assets/items.json` 的 `items` 数组加一行；
//! 2. 想让它掉出来 → 在 `monster/template.rs` 对应物种的 `loot` 里加一条。
//!
//! 第 2 步是可选的（很多物品只由模板/配方产出）。**不需要**改任何系统。

use std::sync::LazyLock;

use serde::Deserialize;
use serde_json::Value;

use crate::item::container::{ContainerDef, ItemTag, TRAVELER_PACK};
use crate::item::quality::Quality;
use crate::rules::{EffectSource, Modifier, ModifierOp};

/// 旅者行囊的编号（初始化时给玩家挂一件，DsnE18）。
pub const TRAVELER_PACK_ITEM: ItemId = ItemId(41);

/// 物品数据文件的 schema 版本（DsnE22 的两层版本之一：数据版本）。
///
/// 改数据的**形状**（不是加一行）时升这里；加载器遇到不认识的版本直接失败，
/// 不做兼容推断。schema 8 = DsnE27 的 parts 形状（并删除耐久字段）。
pub const ITEMS_SCHEMA_VERSION: u32 = 8;

/// 单件物品的重量上限（DsnE20：正常物品不超过 10）。
const MAX_ITEM_WEIGHT: f32 = 10.0;

/// 内嵌的物品数据（构建期进二进制，无运行期文件 I/O）。
static ITEMS_JSON: &str = include_str!("../../assets/items.json");

/// **物品注册表**（GAME.md Gm7）：首次访问时从内嵌 JSON 加载并校验。
///
/// 加载失败直接 panic：数据文件是编译进二进制的，启动期都过不去的表不该带病运行。
pub static ITEMS: LazyLock<&'static [ItemDef]> = LazyLock::new(|| {
    let loaded = load_items(ITEMS_JSON).unwrap_or_else(|error| {
        panic!("物品数据加载失败（ecs_core/assets/items.json）：{error}")
    });
    let leaked: &'static mut [ItemDef] = Box::leak(loaded.into_boxed_slice());
    &*leaked
});

/// 物品表（只读）。数据来自内嵌的 `assets/items.json`，首次访问时解析并校验。
pub fn items() -> &'static [ItemDef] {
    *ITEMS
}

/// 行为的**触发时机**（数据里写 `"trigger": "use"` / `"equipped"`）。
///
/// 触发时机是解释器唯一认识的入口词表：加新触发时机 = 加一个变体 + 一个解释它的系统。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BehaviorTrigger {
    /// 玩家主动使用这一件（消耗品路径；资源已满时不消耗）。
    Use,
    /// 装备在身上时持续生效（被动路径）。
    Equipped,
}

/// 行为产生的一条**效果**（数据里用 `"type"` 区分）。
///
/// 效果词表是解释器的"动词"；加新效果 = 加一个变体 + 一个应用它的函数。
/// 不支持的效果写法在加载期被拒绝（见 `load_items` 的触发/效果矩阵校验），
/// 所以运行期不需要 `match` 兜底分支。
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BehaviorEffect {
    /// 回复生命（受上限钳制；满血时不生效）。
    Heal { amount: f64 },
    /// 回复法力（受上限钳制；满蓝时不生效）。
    RestoreMagic { amount: f64 },
    /// 让持有者求值时丢掉指定来源桶（"无视地形减速"）。
    IgnoreSource { source: EffectSource },
}

impl BehaviorEffect {
    pub fn describe(self) -> String {
        match self {
            BehaviorEffect::Heal { amount } => format!("回复 {amount:.0} 点生命"),
            BehaviorEffect::RestoreMagic { amount } => format!("回复 {amount:.0} 点法力"),
            BehaviorEffect::IgnoreSource { source } => format!("无视{}来源", source.label()),
        }
    }
}

/// 一个行为 = **触发时机 + 效果列表**（数据里是一段数组）。
///
/// 这是"配置只描述、逻辑在系统"的分界：注册表只存这张表，
/// 什么时候、按什么顺序应用效果由消费它的系统决定。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Behavior {
    pub trigger: BehaviorTrigger,
    pub effects: &'static [BehaviorEffect],
}

/// 一件物品的**部件**（数据里 `parts` 数组的一项，DsnE27）。
///
/// 部件按"名词/形容词"三分（DsnE27 决策 4）：
/// - 形容词（能力）= 零大小 marker：[`ItemPart::Equippable`] / [`ItemPart::Consumable`]；
/// - 名词（参数/数据）= 带字段的部件：weapon / container / modifiers / behaviors；
/// - 类别 = `tags` 词汇，**不建部件**。
///
/// 装配时每个部件映射成一个 ECS 组件（见 [`crate::item::spawn_item_assembled`]）；
/// 运行期规则只读组件，不再回头查这张表。
#[derive(Debug, Clone, Copy)]
pub enum ItemPart {
    /// 可装备。能进哪个部位由 `tags` 决定（DsnE18）。
    Equippable,
    /// 消耗品：主动使用会应用 `behavior` 里 `use` 触发的效果。
    Consumable,
    /// 主手武器：攻速、力量上限、成长曲线（DsnE24）。
    Weapon(WeaponSpec),
    /// 自己是一个容器（行囊、背包…）。
    Container(&'static ContainerDef),
    /// 数值修正清单（DsnE10 的词表）。
    Modifiers(&'static [Modifier]),
    /// 行为表（DsnE22 阶段 ③）。
    Behaviors(&'static [Behavior]),
}

/// 武器的蓝图参数（数据里 `weapon` part 的字段，装配成 [`crate::item::Weapon`]）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WeaponSpec {
    /// 每一击的基础耗时（毫秒）；`AV = 它 ÷ 攻击速度`。
    pub speed_ms: f64,
    /// 力量加成上限 S_m（DsnE24）：基础伤害里的力量项按 `min(力量, 它)` 封顶。
    pub strength_cap: f32,
    /// 等级成长曲线（DsnE24 阶段 ②）。
    pub growth: WeaponGrowth,
}

/// 一件物品的定义（GAME.md Gm7 表格的一行）。
#[derive(Debug, Clone, Copy)]
pub struct ItemDef {
    /// 编号（与 GAME.md 表格一致）。
    pub id: ItemId,
    /// 显示名。
    pub name: &'static str,
    /// 详情页的一句话描述。
    pub desc: &'static str,
    /// 物品品质（DsnE23）：蓝图上的分类，实例不改变它。
    pub quality: Quality,
    /// 每单位重量（DsnE20）。行囊载重上限就是靠它算出来的；
    /// 重量是**派生数据**，不进存档（读档后用注册表重算）。
    pub weight: f32,
    /// 堆叠上限（装备恒为 1）。
    pub max_stack: u32,
    /// **部件**（DsnE27）：装备/武器/容器/修正/行为。
    ///
    /// 空 = 纯材料（只有核心字段）。
    pub parts: &'static [ItemPart],
    /// **可装位标签**（DsnE18 的过滤词汇）：这件东西能进哪些部位的槽位。
    ///
    /// 空 = 任何有过滤的槽位都不收（材料、消耗品、模板）。身体各部位的 `SlotAccept::Only`
    /// 与这里求交，所以"这件物品能不能装"由两边的数据决定，规则里没有 `match`。
    pub tags: &'static [ItemTag],
}

impl ItemDef {
    /// 能不能堆叠（`max_stack > 1`）。装备恒 1，所以这条等价于"不是装备"。
    pub const fn is_stackable(&self) -> bool {
        self.max_stack > 1
    }

    /// 是不是可装备物品（"能进哪个部位"看 [`ItemDef::tags`] 与 `BODY` 的槽位）。
    pub fn is_equipment(&self) -> bool {
        self.parts
            .iter()
            .any(|part| matches!(part, ItemPart::Equippable))
    }

    /// 是不是消耗品（主动使用会应用 [`ItemDef::use_effects`]）。
    pub fn is_consumable(&self) -> bool {
        self.parts
            .iter()
            .any(|part| matches!(part, ItemPart::Consumable))
    }

    /// 这件物品自带容器布局（行囊类）；否则 `None`。
    pub fn container_def(&self) -> Option<&'static ContainerDef> {
        self.parts.iter().find_map(|part| match part {
            ItemPart::Container(def) => Some(*def),
            _ => None,
        })
    }

    /// 这件装备带来的修正（非装备为空）。
    pub fn modifiers(&self) -> &'static [Modifier] {
        self.parts
            .iter()
            .find_map(|part| match part {
                ItemPart::Modifiers(list) => Some(*list),
                _ => None,
            })
            .unwrap_or(&[])
    }

    /// 武器的蓝图参数；非武器为 `None`。
    pub fn weapon(&self) -> Option<WeaponSpec> {
        self.parts.iter().find_map(|part| match part {
            ItemPart::Weapon(spec) => Some(*spec),
            _ => None,
        })
    }

    /// 行为表（触发时机 + 效果列表）。
    pub fn behaviors(&self) -> &'static [Behavior] {
        self.parts
            .iter()
            .find_map(|part| match part {
                ItemPart::Behaviors(list) => Some(*list),
                _ => None,
            })
            .unwrap_or(&[])
    }

    /// 这件装备声明"无视"的来源桶（非装备/无声明为空）。
    ///
    /// 数据来源是行为表里 `Equipped` 行的 `IgnoreSource` 效果；这层迭代器只是把
    /// 静态表投影成求值侧要的形状（DsnE22 阶段 ③）。
    pub fn ignored_sources(&self) -> impl Iterator<Item = EffectSource> + 'static {
        self.behaviors()
            .iter()
            .filter(|behavior| behavior.trigger == BehaviorTrigger::Equipped)
            .flat_map(|behavior| behavior.effects.iter())
            .filter_map(|effect| match effect {
                BehaviorEffect::IgnoreSource { source } => Some(*source),
                _ => None,
            })
    }

    /// 主动使用时应用的效果（按数据顺序）。
    ///
    /// 加载期已保证非消耗品没有 `Use` 行为、消耗品至少有一条。
    pub fn use_effects(&self) -> impl Iterator<Item = BehaviorEffect> + 'static {
        self.behaviors()
            .iter()
            .filter(|behavior| behavior.trigger == BehaviorTrigger::Use)
            .flat_map(|behavior| behavior.effects.iter().copied())
    }

    /// 是否有指定触发时机的行为（空表 = 没有）。
    pub fn has_behavior(&self, trigger: BehaviorTrigger) -> bool {
        self.behaviors()
            .iter()
            .any(|behavior| behavior.trigger == trigger && !behavior.effects.is_empty())
    }
}

/// 物品编号。用 `u16` 是因为 GAME.md 的号段留空很多（0/1/2/3、10–14、18–32），
/// 未来加号段不必改类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ItemId(pub u16);

impl ItemId {
    /// 查表拿到定义。**编号必须是表里存在的行**——不存在就是编码错误。
    ///
    /// 返回 `&'static ItemDef` 而不是 `Option`：`ItemId` 只由数据与常量产生
    /// （表外编号要靠 `ItemId(999)` 手写），所以这里 panic 是"你写了一个不存在的号"，
    /// 而不是"玩家数据坏了"。`every_item_id_resolves` 枚举全表，确认没有漏行。
    pub fn def(self) -> &'static ItemDef {
        items()
            .iter()
            .find(|item| item.id == self)
            .unwrap_or_else(|| panic!("物品编号 {} 不在注册表里", self.0))
    }

    /// 编号是否存在（给 `TryFrom`/反序列化这类外部输入用）。
    pub fn exists(self) -> bool {
        items().iter().any(|item| item.id == self)
    }
}

// ── 数据文件 → 运行期静态表 ─────────────────────────
//
// 两段式：先按 DTO 解析成"拥有所有权的数据"，全部校验通过后再 leak 成 `&'static`。
// 校验失败时不会留下半张表，也不会把未通过检查的行 leak 出去。

/// 数据文件顶层（`assets/items.json`）。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTable {
    schema_version: u32,
    items: Vec<RawItem>,
}

/// 数据文件里的一行：核心字段 + 部件数组。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawItem {
    id: u16,
    name: String,
    desc: String,
    quality: Quality,
    weight: f32,
    max_stack: u32,
    #[serde(default)]
    tags: Vec<ItemTag>,
    #[serde(default)]
    parts: Vec<Value>,
}

/// 数据文件里的一条行为（DsnE22 阶段 ③）。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawBehavior {
    trigger: BehaviorTrigger,
    effects: Vec<BehaviorEffect>,
}

/// `weapon` part 的字段。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawWeapon {
    speed_ms: f64,
    strength_cap: f32,
    growth: WeaponGrowth,
}

/// `container` part 的字段。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawContainer {
    layout: String,
}

/// `modifiers` part 的字段。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawModifiers {
    list: Vec<Modifier>,
}

/// `behaviors` part 的字段。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawBehaviors {
    list: Vec<RawBehavior>,
}

/// 解析后的一个部件：字段仍是"拥有所有权"的 DTO，校验通过后才 leak 成静态表。
enum ParsedPart {
    Equippable,
    Consumable,
    Weapon(RawWeapon),
    Container(&'static ContainerDef),
    Modifiers(Vec<Modifier>),
    Behaviors(Vec<RawBehavior>),
}

/// 武器攻速与成长曲线（GAME.md Gm7「武器攻速」）。
///
/// **武器成长曲线**（DsnE24 阶段 ②）：蓝图数据，实例只存当前等级与累积经验。
/// 曲线本身不包含"当前值"——`WeaponProgress` 是物品实体上的稀疏实例组件；升级时
/// 攻击加成与力量上限由这里按等级算出（见 [`crate::item::WeaponProgress`]），
/// 存档只存等级与经验这两个玩家造成的差异。
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct WeaponGrowth {
    /// 等级上限（所有武器共用同一上限口径，Gm7）。
    pub max_level: u32,
    /// 每升一级增加的基础攻击（直接进 `attack` 属性）。
    pub attack_per_level: f32,
    /// 每升一级增加的力量上限 S_m。
    pub strength_cap_per_level: f32,
    /// 升到 2 级所需经验（`exp_to_next(1)`）；更高等级按 `level^1.5` 放大。
    pub exp_base: f32,
}

impl WeaponGrowth {
    /// 从当前等级升到下一级所需经验；已满级时返回 `f64::INFINITY`。
    pub fn exp_to_next(&self, level: u32) -> f64 {
        if level >= self.max_level {
            return f64::INFINITY;
        }
        let level = f64::from(level.max(1));
        (f64::from(self.exp_base) * level.powf(1.5)).max(1.0)
    }

    /// 等级带来的额外攻击（1 级为 0）。
    pub fn attack_bonus(&self, level: u32) -> f64 {
        f64::from(self.attack_per_level) * f64::from(level.saturating_sub(1))
    }

    /// 等级带来的额外力量上限（1 级为 0）。
    pub fn strength_cap_bonus(&self, level: u32) -> f64 {
        f64::from(self.strength_cap_per_level) * f64::from(level.saturating_sub(1))
    }
}

/// 解析并校验一份物品数据。
///
/// 校验是**加载期**的：核心字段（编号顺序、空名、重量范围、堆叠上限）、每个部件的字段
/// （武器数值、容器布局存在、修正有限、行为矩阵），以及**跨部件约束**（武器必须可装备、
/// 修正只属于可装备、装备/容器不可堆叠、消耗品必须至少一条 `use` 行为）。
/// 每条失败都带编号与名字，报错直接指到数据行。
fn load_items(json: &str) -> Result<Vec<ItemDef>, String> {
    let table: RawTable =
        serde_json::from_str(json).map_err(|error| format!("JSON 解析失败：{error}"))?;
    if table.schema_version != ITEMS_SCHEMA_VERSION {
        return Err(format!(
            "数据 schema 版本 {} 不被支持（本程序只认 {ITEMS_SCHEMA_VERSION}）",
            table.schema_version
        ));
    }
    if table.items.is_empty() {
        return Err("物品表为空".to_string());
    }

    let mut defs = Vec::with_capacity(table.items.len());
    let mut previous_id: Option<u16> = None;
    for raw in table.items {
        let label = format!("编号 {}（{}）", raw.id, raw.name);
        if let Some(previous) = previous_id
            && raw.id <= previous
        {
            return Err(format!(
                "物品编号必须严格递增：{} 排在 {} 之后（重复或乱序）",
                raw.id, previous
            ));
        }
        previous_id = Some(raw.id);
        defs.push(load_item(raw).map_err(|why| format!("{label}：{why}"))?);
    }
    Ok(defs)
}

/// 校验并构造一行物品。
fn load_item(raw: RawItem) -> Result<ItemDef, String> {
    let RawItem {
        id,
        name,
        desc,
        quality,
        weight,
        max_stack,
        tags,
        parts,
    } = raw;
    let label = format!("编号 {id}（{name}）");
    if name.trim().is_empty() {
        return Err(format!("编号 {id} 的名字为空"));
    }
    if desc.trim().is_empty() {
        return Err(format!("{label} 的描述为空"));
    }
    if !weight.is_finite() || weight <= 0.0 {
        return Err(format!("{label} 的重量必须是正的有限数，实得 {weight}"));
    }
    if weight > MAX_ITEM_WEIGHT {
        return Err(format!(
            "{label} 的重量 {weight} 超过了单件上限 {MAX_ITEM_WEIGHT}"
        ));
    }
    if max_stack == 0 {
        return Err(format!("{label} 的堆叠上限必须至少为 1"));
    }

    let mut equippable = false;
    let mut consumable = false;
    let mut weapon: Option<RawWeapon> = None;
    let mut container: Option<&'static ContainerDef> = None;
    let mut modifiers: Option<Vec<Modifier>> = None;
    let mut behaviors: Option<Vec<RawBehavior>> = None;
    for (index, value) in parts.into_iter().enumerate() {
        match parse_part(&label, index, value)? {
            ParsedPart::Equippable => {
                if equippable {
                    return Err(format!("{label} 声明了多个 equippable 部件"));
                }
                equippable = true;
            }
            ParsedPart::Consumable => {
                if consumable {
                    return Err(format!("{label} 声明了多个 consumable 部件"));
                }
                consumable = true;
            }
            ParsedPart::Weapon(spec) => {
                if weapon.is_some() {
                    return Err(format!("{label} 声明了多个 weapon 部件"));
                }
                weapon = Some(spec);
            }
            ParsedPart::Container(def) => {
                if container.is_some() {
                    return Err(format!("{label} 声明了多个 container 部件"));
                }
                container = Some(def);
            }
            ParsedPart::Modifiers(list) => {
                if modifiers.is_some() {
                    return Err(format!("{label} 声明了多个 modifiers 部件"));
                }
                if list.is_empty() {
                    return Err(format!("{label} 的 modifiers 部件是空清单"));
                }
                modifiers = Some(list);
            }
            ParsedPart::Behaviors(list) => {
                if behaviors.is_some() {
                    return Err(format!("{label} 声明了多个 behaviors 部件"));
                }
                if list.is_empty() {
                    return Err(format!("{label} 的 behaviors 部件是空清单"));
                }
                behaviors = Some(list);
            }
        }
    }

    // 跨部件约束：装备/容器不可堆叠（一个槽位放一件）。
    if (equippable || container.is_some()) && max_stack != 1 {
        return Err(format!("{label} 是装备或容器，堆叠上限必须是 1"));
    }

    // 武器：必须可装备、必须是主手（带 weapon 标签）、数值与成长曲线合法。
    if let Some(spec) = &weapon {
        if !equippable {
            return Err(format!("{label} 是武器，必须同时声明 equippable 部件"));
        }
        if !tags.contains(&ItemTag::Weapon) {
            return Err(format!("{label} 是武器，但 tags 里没有 weapon"));
        }
        validate_weapon(&label, spec)?;
    }

    // 修正：只有可装备物品带修正；数值必须能安全折叠。
    if let Some(list) = &modifiers {
        if !equippable {
            return Err(format!("{label} 有修正，必须同时声明 equippable 部件"));
        }
        for modifier in list {
            if !modifier.value.is_finite() {
                return Err(format!(
                    "{label} 的修正值必须是有限数，实得 {}",
                    modifier.value
                ));
            }
            if matches!(
                modifier.op,
                ModifierOp::MultiplyBase | ModifierOp::MultiplyTotal
            ) && modifier.value <= 0.0
            {
                return Err(format!(
                    "{label} 的乘区修正必须是正数，实得 {}",
                    modifier.value
                ));
            }
        }
    }

    validate_behaviors(&label, consumable, equippable, behaviors.as_deref().unwrap_or(&[]))?;

    let parts = build_parts(equippable, consumable, weapon, container, modifiers, behaviors);
    Ok(ItemDef {
        id: ItemId(id),
        name: leak_str(name),
        desc: leak_str(desc),
        quality,
        weight,
        max_stack,
        parts: leak_slice(parts),
        tags: leak_slice(tags),
    })
}

/// 解析一个 `parts` 数组项（先把 `"type"` 摘掉，再按类型的严格结构反序列化）。
///
/// 每种部件都用 `deny_unknown_fields` 的独立结构体：写错字段名/多写字段会在加载期
/// 报错，而不是被静默忽略（用户 m09676：强类型没问题，关键是别让每件物品都背字段）。
fn parse_part(label: &str, index: usize, value: Value) -> Result<ParsedPart, String> {
    let Value::Object(mut object) = value else {
        return Err(format!("{label} 的第 {} 个部件不是对象", index + 1));
    };
    let kind = match object.remove("type") {
        Some(Value::String(kind)) => kind,
        Some(_) => return Err(format!("{label} 的第 {} 个部件 type 不是字符串", index + 1)),
        None => return Err(format!("{label} 的第 {} 个部件缺少 type", index + 1)),
    };
    let remaining = Value::Object(object);
    let context = || format!("{label} 的 {} 部件", kind);
    match kind.as_str() {
        "equippable" => {
            let _: EmptyPart =
                serde_json::from_value(remaining).map_err(|error| format!("{}：{error}", context()))?;
            Ok(ParsedPart::Equippable)
        }
        "consumable" => {
            let _: EmptyPart =
                serde_json::from_value(remaining).map_err(|error| format!("{}：{error}", context()))?;
            Ok(ParsedPart::Consumable)
        }
        "weapon" => {
            let spec: RawWeapon =
                serde_json::from_value(remaining).map_err(|error| format!("{}：{error}", context()))?;
            Ok(ParsedPart::Weapon(spec))
        }
        "container" => {
            let raw: RawContainer =
                serde_json::from_value(remaining).map_err(|error| format!("{}：{error}", context()))?;
            let def = container_layout(&raw.layout)
                .ok_or_else(|| format!("{label} 引用了未知的容器布局「{}」", raw.layout))?;
            Ok(ParsedPart::Container(def))
        }
        "modifiers" => {
            let raw: RawModifiers =
                serde_json::from_value(remaining).map_err(|error| format!("{}：{error}", context()))?;
            Ok(ParsedPart::Modifiers(raw.list))
        }
        "behaviors" => {
            let raw: RawBehaviors =
                serde_json::from_value(remaining).map_err(|error| format!("{}：{error}", context()))?;
            Ok(ParsedPart::Behaviors(raw.list))
        }
        other => Err(format!("{label} 有未知的部件类型「{other}」")),
    }
}

/// 无字段的部件（`equippable` / `consumable`）：多写字段会被拒绝。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyPart {}

/// 武器字段校验（DsnE24）：攻速、力量上限、成长曲线都必须合法。
fn validate_weapon(label: &str, spec: &RawWeapon) -> Result<(), String> {
    if !spec.speed_ms.is_finite() || spec.speed_ms <= 0.0 {
        return Err(format!(
            "{label} 的武器攻速必须是正的有限数，实得 {}",
            spec.speed_ms
        ));
    }
    if !spec.strength_cap.is_finite() || spec.strength_cap <= 0.0 {
        return Err(format!(
            "{label} 的力量上限必须是正的有限数，实得 {}",
            spec.strength_cap
        ));
    }
    let growth = &spec.growth;
    if growth.max_level < 1 {
        return Err(format!(
            "{label} 的武器等级上限必须至少为 1，实得 {}",
            growth.max_level
        ));
    }
    if !growth.attack_per_level.is_finite() || growth.attack_per_level < 0.0 {
        return Err(format!(
            "{label} 的每级攻击必须是有限的非负数，实得 {}",
            growth.attack_per_level
        ));
    }
    if !growth.strength_cap_per_level.is_finite() || growth.strength_cap_per_level < 0.0 {
        return Err(format!(
            "{label} 的每级力量上限必须是有限的非负数，实得 {}",
            growth.strength_cap_per_level
        ));
    }
    if !growth.exp_base.is_finite() || growth.exp_base <= 0.0 {
        return Err(format!(
            "{label} 的升级基础经验必须是正的有限数，实得 {}",
            growth.exp_base
        ));
    }
    Ok(())
}

/// 把校验过的部件转成运行期静态部件列表（列表/行为表 leak 成 `&'static`）。
fn build_parts(
    equippable: bool,
    consumable: bool,
    weapon: Option<RawWeapon>,
    container: Option<&'static ContainerDef>,
    modifiers: Option<Vec<Modifier>>,
    behaviors: Option<Vec<RawBehavior>>,
) -> Vec<ItemPart> {
    let mut parts = Vec::new();
    if equippable {
        parts.push(ItemPart::Equippable);
    }
    if consumable {
        parts.push(ItemPart::Consumable);
    }
    if let Some(spec) = weapon {
        parts.push(ItemPart::Weapon(WeaponSpec {
            speed_ms: spec.speed_ms,
            strength_cap: spec.strength_cap,
            growth: spec.growth,
        }));
    }
    if let Some(def) = container {
        parts.push(ItemPart::Container(def));
    }
    if let Some(list) = modifiers {
        parts.push(ItemPart::Modifiers(leak_slice(list)));
    }
    if let Some(list) = behaviors {
        let behaviors: Vec<Behavior> = list
            .into_iter()
            .map(|behavior| Behavior {
                trigger: behavior.trigger,
                effects: leak_slice(behavior.effects),
            })
            .collect();
        parts.push(ItemPart::Behaviors(leak_slice(behaviors)));
    }
    parts
}

/// 行为矩阵校验（DsnE22 阶段 ③）：触发时机决定它能带哪些效果。
///
/// - `use`：只能出现在消耗品上，效果只能是 `heal` / `restore_magic`，数值为正有限数；
/// - `equipped`：只能出现在装备上，效果只能是 `ignore_source`。
///
/// 解释器因此不需要"未知组合"的兜底分支；写错的数据在启动期就被挡住。
fn validate_behaviors(
    label: &str,
    is_consumable: bool,
    is_equipment: bool,
    behaviors: &[RawBehavior],
) -> Result<(), String> {
    let mut use_behaviors = 0;
    for behavior in behaviors {
        if behavior.effects.is_empty() {
            return Err(format!("{label} 有一条空行为（没有效果）"));
        }
        match behavior.trigger {
            BehaviorTrigger::Use => {
                if !is_consumable {
                    return Err(format!("{label} 不是消耗品，不能写 use 行为"));
                }
                use_behaviors += 1;
                for effect in &behavior.effects {
                    match effect {
                        BehaviorEffect::Heal { amount }
                        | BehaviorEffect::RestoreMagic { amount } => {
                            if !amount.is_finite() || *amount <= 0.0 {
                                return Err(format!(
                                    "{label} 的消耗效果必须是正的有限数，实得 {amount}"
                                ));
                            }
                        }
                        BehaviorEffect::IgnoreSource { .. } => {
                            return Err(format!(
                                "{label} 的 use 行为不支持 ignore_source（那是被动效果）"
                            ));
                        }
                    }
                }
            }
            BehaviorTrigger::Equipped => {
                if !is_equipment {
                    return Err(format!("{label} 不是装备，不能写 equipped 行为"));
                }
                for effect in &behavior.effects {
                    match effect {
                        BehaviorEffect::IgnoreSource { .. } => {}
                        BehaviorEffect::Heal { .. } | BehaviorEffect::RestoreMagic { .. } => {
                            return Err(format!(
                                "{label} 的 equipped 行为不支持回复效果（目前没有持续恢复）"
                            ));
                        }
                    }
                }
            }
        }
    }
    if is_consumable && use_behaviors == 0 {
        return Err(format!("{label} 是消耗品，但没有任何 use 行为"));
    }
    Ok(())
}

/// 容器布局的名字 → 布局表（数据里只写名字，实现在 `container.rs`）。
fn container_layout(name: &str) -> Option<&'static ContainerDef> {
    match name {
        "traveler_pack" => Some(&TRAVELER_PACK),
        _ => None,
    }
}

fn leak_str(value: String) -> &'static str {
    Box::leak(value.into_boxed_str())
}

fn leak_slice<T>(values: Vec<T>) -> &'static [T] {
    Box::leak(values.into_boxed_slice())
}

/// 按名字查（配置与测试用；运行期一律走 [`ItemId`]）。
pub fn item_by_name(name: &str) -> Option<&'static ItemDef> {
    items().iter().find(|item| item.name == name)
}

/// 一件（堆）物品的总重：每单位重量 × 数量。
///
/// 重量是**派生数据**：只写在注册表一行里，存档/容器都不存第二份（DsnE20）。
/// `count == 0` 按 1 计——`Amount` 本身就不允许 0（空壳会被销毁）。
pub fn stack_weight(id: ItemId, count: u32) -> f32 {
    id.def().weight * count.max(1) as f32
}

#[cfg(test)]
#[path = "registry_tests.rs"]
mod tests;
