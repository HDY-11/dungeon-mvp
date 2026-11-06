//! 物品注册表：**一份数据文件 + 一个 ID**（GAME.md Gm7，DESIGN DsnE22）。
//!
//! # 数据在文件，加载在启动期
//!
//! 物品的数值、效果、重量、可装位标签全部写在 `ecs_core/assets/items.json` 里；构建时
//! `include_str!` 内嵌进二进制，第一次访问 [`ITEMS`] 时解析并**逐行校验**。运行时没有
//! "读文件失败"这条路径，但"数据写错了"会在启动/测试期直接报错，而不是变成某个玩家手里
//! "捡不起来"的装备。
//!
//! 为什么是 JSON 而不是别的格式：`serde_json` 已经是本 crate 的依赖（存档投影也用 serde），
//! 不再引新依赖；数据量是几十行，可读性优先。
//!
//! # 形状 vs 数据（DsnE22 阶段 ①）
//!
//! 这一轮只把**同一张表**从 Rust 字面量搬到数据文件：`ItemDef` / `ItemKind` / `Stat` /
//! `EffectSource` / `ItemTag` 的类型形状不变，因此规则、拾取、界面、存档全部零改动。
//! 后续阶段才会拆 `ItemKind`（② 属性词表、③ 行为数据化、④ 实例组件 + SaveV2）。
//!
//! 关键仍然是没有**中央分派**：代码里没有 `match item_id { .. }`，物品的行为由数据上的
//! 标签/修正/效果决定。
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

use crate::item::container::{ContainerDef, ItemTag, TRAVELER_PACK};
use crate::rules::{EffectSource, Modifier};

/// 旅者行囊的编号（初始化时给玩家挂一件，DsnE18）。
pub const TRAVELER_PACK_ITEM: ItemId = ItemId(41);

/// 物品数据文件的 schema 版本（DsnE22 的两层版本之一：数据版本）。
///
/// 改数据的**形状**（不是加一行）时升这里；加载器遇到不认识的版本直接失败，
/// 不做兼容推断。
pub const ITEMS_SCHEMA_VERSION: u32 = 1;

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

/// 消耗品的效果（GAME.md Gm7「消耗品」）。
///
/// 做成**闭合小词表**而不是"一串操作"：消耗品都是即时的一次性效果，
/// 一旦需要"持续发生的事"，它就该走技能三层结构的第 ③ 层（效果实体），
/// 而不是往这里加字段（DESIGN DsnE11）。
///
/// 数据文件里写成 `{"heal": 6.0}` / `{"restore_magic": 4.0}`（DsnE22 阶段 ①）。
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsumeEffect {
    /// 回复生命（受上限钳制；满血时不消耗）。
    Heal(f64),
    /// 回复法力（受上限钳制；满蓝时不消耗）。
    RestoreMagic(f64),
}

impl ConsumeEffect {
    pub fn describe(self) -> String {
        match self {
            ConsumeEffect::Heal(amount) => format!("回复 {amount:.0} 点生命"),
            ConsumeEffect::RestoreMagic(amount) => format!("回复 {amount:.0} 点法力"),
        }
    }
}

/// 物品种类：**它是什么**，以及它带来什么。
///
/// `Equipment` 带着 [`Modifier`] 列表而不是"攻击 +3"这种具体字段：装备与状态效果、
/// 地形减速走**同一套**求值侧（DsnE10），所以一件装备的加成和一条 buff 的加成
/// 在规则层看起来完全一样。
#[derive(Debug, Clone, Copy)]
pub enum ItemKind {
    /// 可装备：**部位由 `tags` 决定**（DsnE18），这里只写它带来的修正与无视的来源桶。
    Equipment {
        modifiers: &'static [Modifier],
        ignores: &'static [EffectSource],
    },
    /// 可食用/饮用：一次性回复。
    Consumable(ConsumeEffect),
    /// 材料：只是合成原料，没有直接效果。
    Material,
    /// 容器类：这件物品**自己是一个容器**（行囊、背包…）。
    ///
    /// 生成实体时除了 `Item + Amount` 还会挂上 [`Container`](crate::item::Container)；
    /// 布局来自这里（静态表），所以"行囊有几格"是内容，不是代码。
    Pack(&'static ContainerDef),
}

/// 武器攻速（毫秒）：`AV = 武器 speed ÷ 攻击速度`（GAME.md Gm7「武器攻速」）。
///
/// 它用 [`ItemDef::weapon_speed_ms`] 表达而不是塞进 [`ItemKind`]：那描述的是
/// "这件装备怎么用"（耗时），与"它带来什么数值"（修正）是两件事——
/// 一件装备可以有修正而没有耗时（盾牌），也可以只有耗时（空手）而没有物品。
/// 空手口径见 [`crate::balance::UNARMED_ATTACK_DURATION`]。
///
/// 一件物品的定义（GAME.md Gm7 表格的一行）。
#[derive(Debug, Clone, Copy)]
pub struct ItemDef {
    /// 编号（与 GAME.md 表格一致）。
    pub id: ItemId,
    /// 显示名。
    pub name: &'static str,
    /// 详情页的一句话描述。
    pub desc: &'static str,
    /// 每单位重量（DsnE20）。行囊载重上限就是靠它算出来的；
    /// 重量是**派生数据**，不进存档（读档后用注册表重算）。
    pub weight: f32,
    /// 堆叠上限（装备恒为 1）。
    pub max_stack: u32,
    /// 种类与效果。
    pub kind: ItemKind,
    /// **可装位标签**（DsnE18 的过滤词汇）：这件东西能进哪些部位的槽位。
    ///
    /// 空 = 任何有过滤的槽位都不收（材料、消耗品、模板）。身体各部位的 `SlotAccept::Only`
    /// 与这里求交，所以"这件物品能不能装"由两边的数据决定，规则里没有 `match`。
    pub tags: &'static [ItemTag],
    /// 作为主手武器时的攻击耗时（毫秒）；非武器为 `None`。
    pub weapon_speed_ms: Option<f64>,
}

impl ItemDef {
    /// 能不能堆叠（`max_stack > 1`）。装备恒 1，所以这条等价于"不是装备"。
    pub const fn is_stackable(&self) -> bool {
        self.max_stack > 1
    }

    /// 是不是可装备物品（"能进哪个部位"看 [`ItemDef::tags`] 与 `BODY` 的槽位）。
    pub const fn is_equipment(&self) -> bool {
        matches!(self.kind, ItemKind::Equipment { .. })
    }

    /// 这件物品自带容器布局（行囊类）；否则 `None`。
    pub const fn container_def(&self) -> Option<&'static ContainerDef> {
        match self.kind {
            ItemKind::Pack(def) => Some(def),
            _ => None,
        }
    }

    /// 这件装备带来的修正（非装备为空）。
    ///
    /// 返回静态切片而不是 `Vec`：求值侧每轮都要读它，不该为静态表分配内存。
    pub const fn modifiers(&self) -> &'static [Modifier] {
        match self.kind {
            ItemKind::Equipment { modifiers, .. } => modifiers,
            _ => &[],
        }
    }

    /// 这件装备声明"无视"的来源桶（非装备为空）。
    pub const fn ignores(&self) -> &'static [EffectSource] {
        match self.kind {
            ItemKind::Equipment { ignores, .. } => ignores,
            _ => &[],
        }
    }

    /// 消耗品的效果（非消耗品为 `None`）。
    pub const fn consume_effect(&self) -> Option<ConsumeEffect> {
        match self.kind {
            ItemKind::Consumable(effect) => Some(effect),
            _ => None,
        }
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
struct RawTable {
    schema_version: u32,
    items: Vec<RawItem>,
}

/// 数据文件里的一行。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawItem {
    id: u16,
    name: String,
    desc: String,
    weight: f32,
    max_stack: u32,
    kind: RawKind,
    #[serde(default)]
    tags: Vec<ItemTag>,
    #[serde(default)]
    weapon_speed_ms: Option<f64>,
}

/// 物品类型（数据里用 `"type"` 区分，DsnE22 阶段 ① 的过渡形状）。
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum RawKind {
    Equipment {
        #[serde(default)]
        modifiers: Vec<Modifier>,
        #[serde(default)]
        ignores: Vec<EffectSource>,
    },
    Consumable {
        effect: ConsumeEffect,
    },
    Material,
    Pack {
        layout: String,
    },
}

/// 解析并校验一份物品数据。
///
/// 校验是**加载期**的：编号顺序、空名、重量范围、堆叠上限、装备/容器不可堆叠、
/// 攻速与消耗品数值为正、Pack 布局存在。每条失败都带编号与名字，报错直接指到数据行。
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
        let RawItem {
            id,
            name,
            desc,
            weight,
            max_stack,
            kind,
            tags,
            weapon_speed_ms,
        } = raw;
        if let Some(previous) = previous_id
            && id <= previous
        {
            return Err(format!(
                "物品编号必须严格递增：{id} 排在 {previous} 之后（重复或乱序）"
            ));
        }
        previous_id = Some(id);

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
        if let Some(speed) = weapon_speed_ms
            && (!speed.is_finite() || speed <= 0.0)
        {
            return Err(format!("{label} 的武器攻速必须是正的有限数，实得 {speed}"));
        }

        let kind = match kind {
            RawKind::Equipment { modifiers, ignores } => {
                if max_stack != 1 {
                    return Err(format!("{label} 是装备，堆叠上限必须是 1"));
                }
                ItemKind::Equipment {
                    modifiers: leak_slice(modifiers),
                    ignores: leak_slice(ignores),
                }
            }
            RawKind::Consumable { effect } => {
                let amount = match effect {
                    ConsumeEffect::Heal(amount) | ConsumeEffect::RestoreMagic(amount) => amount,
                };
                if !amount.is_finite() || amount <= 0.0 {
                    return Err(format!(
                        "{label} 的消耗品效果必须是正的有限数，实得 {amount}"
                    ));
                }
                ItemKind::Consumable(effect)
            }
            RawKind::Material => ItemKind::Material,
            RawKind::Pack { layout } => {
                if max_stack != 1 {
                    return Err(format!("{label} 是容器类，堆叠上限必须是 1"));
                }
                let def = container_layout(&layout)
                    .ok_or_else(|| format!("{label} 引用了未知的容器布局「{layout}」"))?;
                ItemKind::Pack(def)
            }
        };

        defs.push(ItemDef {
            id: ItemId(id),
            name: leak_str(name),
            desc: leak_str(desc),
            weight,
            max_stack,
            kind,
            tags: leak_slice(tags),
            weapon_speed_ms,
        });
    }
    Ok(defs)
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
