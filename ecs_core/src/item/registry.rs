//! 物品注册表：**一张静态表 + 一个 ID**（GAME.md Gm7）。
//!
//! # 为什么是 Rust 静态表，不是运行时加载的 JSON
//!
//! 旧实现（`archive/dungeon-core/src/items.rs`）把物品定义放在 `assets/items.json`，运行时读成
//! `ItemRegistry`（`OnceLock` 全局单例）。这套东西的成本全在**边界**上：读文件失败怎么办、
//! ID 指向不存在的物品怎么办、JSON 里写错的槽位谁来发现——每一条都要在运行期兜。
//!
//! 本项目已经在两处做过同样的选择并拿到收益：`TILE_PROPS`（地形属性，ECS31）与
//! `MonsterTemplate`（物种一行，ECS32）。物品表照抄这个形状：
//!
//! | | JSON 注册表 | 静态表（本模块） |
//! |---|---|---|
//! | 加一件物品 | 改 JSON + 重启加载 | 加一行常量 |
//! | 引用不存在的物品 | 运行期 `Option`/兜底 | **编译期**就写不出来（`&'static ItemDef`） |
//! | 表本身的完整性 | 只有运行时知道 | 一条 `/tests` 用例枚举所有行 |
//!
//! **条目本身仍然是数据**：数值写在表里、代码里没有 `match item_id { .. }`。
//! 这与"数据驱动"不矛盾——数据可以编译进二进制，关键是没有**中央分派**。
//!
//! # 编号
//!
//! [`ItemId`] 的数值与 GAME.md Gm7 的表格**一一对应**（0/1/2/3 装备、10–14 材料、
//! 18–32 消耗品与模板）。留空号是有意的：GAME.md 的表格是权威，代码跟着它走，
//! 而不是反过来重排号段让"看起来紧凑"。
//!
//! # 加一件物品要改几处（LECS22）
//!
//! 1. 本文件加一行 [`ItemDef`]（顺带回答"它授予什么"：修正 / 效果 / 配方）；
//! 2. 想让它掉出来 → 在 `monster/template.rs` 对应物种的 `loot` 里加一条。
//!
//! 第 2 步是可选的（很多物品只由模板/配方产出）。**不需要**改任何系统。

use crate::item::container::{ContainerDef, ItemTag, TRAVELER_PACK};

/// 旅者行囊的编号（初始化时给玩家挂一件，DsnE18）。
pub const TRAVELER_PACK_ITEM: ItemId = ItemId(41);
use crate::rules::{Modifier, Stat, EffectSource};

/// 消耗品的效果（GAME.md Gm7「消耗品」）。
///
/// 做成**闭合小词表**而不是"一串操作"：消耗品都是即时的一次性效果，
/// 一旦需要"持续发生的事"，它就该走技能三层结构的第 ③ 层（效果实体），
/// 而不是往这里加字段（DESIGN DsnE11）。
#[derive(Debug, Clone, Copy, PartialEq)]
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
/// 一件物品的静态定义（GAME.md Gm7 表格的一行）。
#[derive(Debug, Clone, Copy)]
pub struct ItemDef {
    /// 编号（与 GAME.md 表格一致）。
    pub id: ItemId,
    /// 显示名。
    pub name: &'static str,
    /// 详情页的一句话描述。
    pub desc: &'static str,
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
    /// 返回 `&'static ItemDef` 而不是 `Option`：`ItemId` 只由本模块的常量产生
    /// （表外编号要靠 `ItemId(999)` 手写），所以这里 panic 是"你写了一个不存在的号"，
    /// 而不是"玩家数据坏了"。`every_item_id_resolves` 枚举全表，确认没有漏行。
    pub fn def(self) -> &'static ItemDef {
        ITEMS
            .iter()
            .find(|item| item.id == self)
            .unwrap_or_else(|| panic!("物品编号 {} 不在注册表里", self.0))
    }

    /// 编号是否存在（给 `TryFrom`/反序列化这类外部输入用）。
    pub fn exists(self) -> bool {
        ITEMS.iter().any(|item| item.id == self)
    }
}

// ── 修正器常量（装备加成） ───────────────────────────
//
// 数值来源：GAME.md Gm7「武器攻速」与 Gm4「装备加成」。
// 写成常量而不是直接内联进表，是为了让"哪几件装备用同一个加成"一眼可见。

/// 锈铁剑：攻击 +3（基准武器，300ms）。
const SWORD_MODS: &[Modifier] = &[Modifier::delta(Stat::Attack, EffectSource::Equipment, 3.0)];
/// 石锤：攻击 +4，但慢（450ms）。
const HAMMER_MODS: &[Modifier] = &[Modifier::delta(Stat::Attack, EffectSource::Equipment, 4.0)];
/// 匕首：攻击 +2，但快（200ms）。
const DAGGER_MODS: &[Modifier] = &[Modifier::delta(Stat::Attack, EffectSource::Equipment, 2.0)];
/// 木盾：防御 +2（Gm4 只说"提供防御加成"，取值属内容，本轮试调）。
const SHIELD_MODS: &[Modifier] = &[Modifier::delta(Stat::Defense, EffectSource::Equipment, 2.0)];
/// 皮甲：防御 +4。
const ARMOR_MODS: &[Modifier] = &[Modifier::delta(Stat::Defense, EffectSource::Equipment, 4.0)];
/// 攻击戒指：攻击 +2。
const RING_MODS: &[Modifier] = &[Modifier::delta(Stat::Attack, EffectSource::Equipment, 2.0)];
/// 皮帽：防御 +1（头部装备，取值属内容）。
const HAT_MODS: &[Modifier] = &[Modifier::delta(Stat::Defense, EffectSource::Equipment, 1.0)];

/// 踏水靴：移动速度 +0，但**无视地形**那一桶（C4/C5 的内容，泥潭与浅水的代价对它无效）。
const BOOTS_MODS: &[Modifier] = &[];

// ── 物品表 ──────────────────────────────────────────
//
// 顺序按编号排列（便于与 GAME.md 对照）；`every_item_row_matches_game_doc` 校验
// 表里每一行都写了与文档一致的编号，`item_ids_are_unique_and_sorted` 校验不重不漏。

/// **物品注册表**（GAME.md Gm7）。加物品 = 加一行。
pub const ITEMS: &[ItemDef] = &[
    ItemDef {
        id: ItemId(0),
        name: "锈铁剑",
        desc: "一把生锈的铁剑。",
        max_stack: 1,
        kind: ItemKind::Equipment {
            modifiers: SWORD_MODS,
            ignores: &[],
        },
        tags: &[ItemTag::Weapon],
        weapon_speed_ms: Some(300.0),
    },
    ItemDef {
        id: ItemId(1),
        name: "木盾",
        desc: "简陋的木盾。",
        max_stack: 1,
        kind: ItemKind::Equipment {
            modifiers: SHIELD_MODS,
            ignores: &[],
        },
        tags: &[ItemTag::Shield],
        weapon_speed_ms: None,
    },
    ItemDef {
        id: ItemId(2),
        name: "皮甲",
        desc: "轻便皮甲。",
        max_stack: 1,
        kind: ItemKind::Equipment {
            modifiers: ARMOR_MODS,
            ignores: &[],
        },
        tags: &[ItemTag::Chest],
        weapon_speed_ms: None,
    },
    ItemDef {
        id: ItemId(3),
        name: "攻击戒指",
        desc: "攻击之戒。",
        max_stack: 1,
        kind: ItemKind::Equipment {
            modifiers: RING_MODS,
            ignores: &[],
        },
        tags: &[ItemTag::Ring],
        weapon_speed_ms: None,
    },
    ItemDef {
        id: ItemId(4),
        name: "皮帽",
        desc: "缝得粗糙的皮帽。聊胜于无。",
        max_stack: 1,
        kind: ItemKind::Equipment {
            modifiers: HAT_MODS,
            ignores: &[],
        },
        tags: &[ItemTag::Head],
        weapon_speed_ms: None,
    },
    ItemDef {
        id: ItemId(10),
        name: "生物血肉",
        desc: "怪物的血肉，散发出腥臭味。",
        max_stack: 64,
        kind: ItemKind::Material,
        tags: &[],
        weapon_speed_ms: None,
    },
    ItemDef {
        id: ItemId(11),
        name: "破布",
        desc: "破烂的布料，没什么用。",
        max_stack: 64,
        kind: ItemKind::Material,
        tags: &[],
        weapon_speed_ms: None,
    },
    ItemDef {
        id: ItemId(12),
        name: "坚硬木棍",
        desc: "还算结实的木棍，可以当柴烧。",
        max_stack: 64,
        kind: ItemKind::Material,
        tags: &[],
        weapon_speed_ms: None,
    },
    ItemDef {
        id: ItemId(13),
        name: "染血兽牙",
        desc: "沾满血迹的尖牙，散发出危险的气息。",
        max_stack: 64,
        kind: ItemKind::Material,
        tags: &[],
        weapon_speed_ms: None,
    },
    ItemDef {
        id: ItemId(14),
        name: "黑色甲壳",
        desc: "变异蝎子的坚硬外壳。",
        max_stack: 64,
        kind: ItemKind::Material,
        tags: &[],
        weapon_speed_ms: None,
    },
    ItemDef {
        id: ItemId(18),
        name: "石子",
        desc: "边缘锋利的石块，适合投掷。",
        max_stack: 20,
        kind: ItemKind::Material,
        tags: &[],
        weapon_speed_ms: None,
    },
    ItemDef {
        id: ItemId(23),
        name: "石锤",
        desc: "沉重的石制战锤，挥舞缓慢但势大力沉。",
        max_stack: 1,
        kind: ItemKind::Equipment {
            modifiers: HAMMER_MODS,
            ignores: &[],
        },
        tags: &[ItemTag::Weapon],
        weapon_speed_ms: Some(450.0),
    },
    ItemDef {
        id: ItemId(24),
        name: "匕首",
        desc: "轻巧的短刃，出手极快但伤害有限。",
        max_stack: 1,
        kind: ItemKind::Equipment {
            modifiers: DAGGER_MODS,
            ignores: &[],
        },
        tags: &[ItemTag::Weapon],
        weapon_speed_ms: Some(200.0),
    },
    ItemDef {
        id: ItemId(25),
        name: "蘑菇",
        desc: "繁茂洞穴的荧光蘑菇。食用回复少量生命。",
        max_stack: 20,
        kind: ItemKind::Consumable(ConsumeEffect::Heal(6.0)),
        tags: &[],
        weapon_speed_ms: None,
    },
    ItemDef {
        id: ItemId(26),
        name: "苔藓",
        desc: "潮湿的苔藓，散发着泥土的气息。",
        max_stack: 64,
        kind: ItemKind::Material,
        tags: &[],
        weapon_speed_ms: None,
    },
    ItemDef {
        id: ItemId(27),
        name: "孢子囊",
        desc: "蘑菇傀儡体内取出的孢子囊。",
        max_stack: 9,
        kind: ItemKind::Material,
        tags: &[],
        weapon_speed_ms: None,
    },
    ItemDef {
        id: ItemId(28),
        name: "贝壳",
        desc: "地海常见的硬壳。",
        max_stack: 64,
        kind: ItemKind::Material,
        tags: &[],
        weapon_speed_ms: None,
    },
    ItemDef {
        id: ItemId(29),
        name: "珍珠",
        desc: "洞穴蟹体内孕育的稀有珍珠。",
        max_stack: 9,
        kind: ItemKind::Material,
        tags: &[],
        weapon_speed_ms: None,
    },
    ItemDef {
        id: ItemId(30),
        name: "鱼骨",
        desc: "洞穴鱼的骨架，细长而尖锐。",
        max_stack: 64,
        kind: ItemKind::Material,
        tags: &[],
        weapon_speed_ms: None,
    },
    ItemDef {
        id: ItemId(31),
        name: "鳗皮",
        desc: "深鳗的皮革，湿滑坚韧。",
        max_stack: 64,
        kind: ItemKind::Material,
        tags: &[],
        weapon_speed_ms: None,
    },
    ItemDef {
        id: ItemId(32),
        name: "海藻",
        desc: "地海的海藻。食用回复少量法力。",
        max_stack: 20,
        kind: ItemKind::Consumable(ConsumeEffect::RestoreMagic(4.0)),
        tags: &[],
        weapon_speed_ms: None,
    },
    ItemDef {
        id: ItemId(40),
        name: "踏水靴",
        desc: "鞋底刻着水纹的旧靴子。走泥潭与浅水如履平地。",
        max_stack: 1,
        kind: ItemKind::Equipment {
            modifiers: BOOTS_MODS,
            ignores: &[EffectSource::Terrain],
        },
        tags: &[ItemTag::Feet],
        weapon_speed_ms: None,
    },
    ItemDef {
        id: ItemId(41),
        name: "旅者行囊",
        desc: "用了很多年的粗布行囊，能塞下不少东西。",
        max_stack: 1,
        kind: ItemKind::Pack(&TRAVELER_PACK),
        tags: &[ItemTag::Pack],
        weapon_speed_ms: None,
    },
];

/// 按名字查（配置与测试用；运行期一律走 [`ItemId`]）。
pub fn item_by_name(name: &str) -> Option<&'static ItemDef> {
    ITEMS.iter().find(|item| item.name == name)
}

#[cfg(test)]
#[path = "registry_tests.rs"]
mod tests;
