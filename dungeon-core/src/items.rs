use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

// ── 物品 ID 常量 ────────────────────────────────────
// 与 assets/items.json 中的 id 字段对应。
// 用于替代裸 usize 字面量，使 grep 可追踪、重构可定位。
pub const ITEM_RUSTY_SWORD: usize = 0;
pub const ITEM_WOOD_SHIELD: usize = 1;
pub const ITEM_LEATHER_ARMOR: usize = 2;
pub const ITEM_ATTACK_RING: usize = 3;
pub const ITEM_BIOMASS: usize = 10;
pub const ITEM_CLOTH: usize = 11;
pub const ITEM_STICK: usize = 12;
pub const ITEM_FANG: usize = 13;
pub const ITEM_CHITIN: usize = 14;
pub const ITEM_SCROLL_HEAL: usize = 15;
pub const ITEM_SCROLL_SHIELD: usize = 16;
pub const ITEM_SCROLL_BERSERK: usize = 17;

// ── 物品分类（显示用） ──────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ItemClass {
    Weapon,
    Armor,
    Ring,
    Consumable,
    Material,
    Quest,
}

impl ItemClass {
    pub fn display_name(&self) -> &'static str {
        match self {
            ItemClass::Weapon => "武器",
            ItemClass::Armor => "防具",
            ItemClass::Ring => "戒指",
            ItemClass::Consumable => "消耗品",
            ItemClass::Material => "材料",
            ItemClass::Quest => "任务物品",
        }
    }

    /// 便捷图标（终端友好的 ASCII 字符）
    pub fn icon(&self) -> &'static str {
        match self {
            ItemClass::Weapon => "/",
            ItemClass::Armor => "]",
            ItemClass::Ring => "=",
            ItemClass::Consumable => "!",
            ItemClass::Material => "&",
            ItemClass::Quest => "*",
        }
    }
}

// ── 稀有度 ──────────────────────────────────────────

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Rarity {
    #[default]
    Common,
    Uncommon,
    Rare,
    Epic,
}

impl Rarity {
    pub fn display_name(&self) -> &'static str {
        match self {
            Rarity::Common => "普通",
            Rarity::Uncommon => "优秀",
            Rarity::Rare => "稀有",
            Rarity::Epic => "传说",
        }
    }
}

// ── 物品槽位 ────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EquipmentSlot {
    Weapon,
    Armor,
    Ring,
}

// ── 属性加成 ────────────────────────────────────────

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct StatBonus {
    pub attack: i32,
    pub defense: i32,
    pub magic_mastery: i32,
    pub agility: i32,
    pub hp: i32,
    pub crit_rate: f32,
    pub crit_damage: f32,
}

// ── 物品实例元数据（ItemStack 的可选附加数据）─────────

/// ItemStack 实例级元数据。`None` = 模板原始状态。
/// `#[serde(default)]` 保证旧存档兼容。
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ItemMeta {
    /// 自定义名称，覆盖模板 `ItemDef.name`
    #[serde(default)]
    pub display_name: Option<String>,
    /// 品质层级（0=基础，1=+1，2=+2…）
    #[serde(default)]
    pub tier: u32,
    /// 耐久度（暂缓实现）
    #[serde(default)]
    pub durability: Option<u32>,
    /// 实例级标签（"已诅咒"/"已鉴定"等），不污染模板
    #[serde(default)]
    pub tags: Vec<String>,
}

// ── 物品行为 trait ───────────────────────────────────

/// 可交互物品的行为抽象。
/// 每种行为种类一个 impl，避免 match item_id。
pub trait UsableItem: Send + Sync {
    /// 使用物品。返回 true 表示消耗了该物品
    fn use_on(&self, _world: &mut World, _user: Entity) -> bool { false }
    /// 使用提示文本，如"学习"/"饮用"/"阅读"
    fn use_verb(&self) -> &'static str { "使用" }
}

// ── 物品定义（注册表中的模板） ──────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ItemDef {
    pub id: usize,
    pub name: String,
    pub description: String,
    pub glyph: char,
    pub color: (u8, u8, u8),
    pub class: ItemClass,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slot: Option<EquipmentSlot>,
    pub max_stack: u32,
    pub bonus: StatBonus,
    #[serde(default)]
    pub rarity: Rarity,
    #[serde(default)]
    pub tags: Vec<String>,
}

impl ItemDef {
    /// 检查是否属于某标签
    pub fn has_tag(&self, tag: &str) -> bool {
        self.tags.iter().any(|t| t == tag)
    }
}

// ── 注册表（OnceLock 全局单例）─────────────────────

static ITEM_REGISTRY: OnceLock<ItemRegistry> = OnceLock::new();

#[derive(Debug)]
pub struct ItemRegistry {
    items: Vec<Option<ItemDef>>,
}

impl ItemRegistry {
    /// 从 assets/items.json 加载并初始化全局注册表。
    pub fn load() -> &'static Self {
        ITEM_REGISTRY.get_or_init(|| {
            let data = include_str!("../../assets/items.json");
            let defs: Vec<ItemDef> = serde_json::from_str(data).expect("Invalid items.json");
            let max_id = defs.iter().map(|d| d.id).max().unwrap_or(0);
            let mut items = vec![None; max_id + 1];
            for def in defs {
                let id = def.id;
                items[id] = Some(def);
            }
            Self { items }
        })
    }

    /// 获取全局注册表引用（必须在 load 之后调用）
    pub fn global() -> &'static Self {
        ITEM_REGISTRY.get().expect("ItemRegistry not loaded — call ItemRegistry::load() first")
    }

    pub fn get(&self, id: usize) -> Option<&ItemDef> {
        self.items.get(id).and_then(|o| o.as_ref())
    }
}

// ── ItemStack ────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ItemStack {
    pub item_id: usize,
    pub count: u32,
    /// 实例级元数据（I41）。None = 模板原始状态，`#[serde(default)]` 兼容旧存档
    #[serde(default)]
    pub meta: Option<Box<ItemMeta>>,
}

impl ItemStack {
    pub fn new(item_id: usize, count: u32) -> Self {
        Self { item_id, count, meta: None }
    }

    pub fn def(&self) -> Option<&'static ItemDef> {
        ItemRegistry::global().get(self.item_id)
    }

    pub fn name(&self) -> String {
        // I41: 优先使用自定义名称
        if let Some(ref meta) = self.meta {
            if let Some(ref custom) = meta.display_name {
                return custom.clone();
            }
        }
        self.def().map(|d| d.name.clone()).unwrap_or_else(|| format!("未知物品({})", self.item_id))
    }

    pub fn description(&self) -> String {
        self.def().map(|d| d.description.clone()).unwrap_or_default()
    }

    pub fn glyph(&self) -> char {
        self.def().map(|d| d.glyph).unwrap_or('?')
    }

    pub fn color(&self) -> (u8, u8, u8) {
        self.def().map(|d| d.color).unwrap_or((255, 255, 255))
    }

    pub fn max_stack(&self) -> u32 {
        self.def().map(|d| d.max_stack).unwrap_or(1)
    }

    pub fn is_full(&self) -> bool {
        self.count >= self.max_stack()
    }

    pub fn space(&self) -> u32 {
        self.max_stack().saturating_sub(self.count)
    }

    /// 尝试往这个栈里加 count，返回实际加了多少
    pub fn add_up_to(&mut self, count: u32) -> u32 {
        let space = self.space();
        let actual = count.min(space);
        self.count += actual;
        actual
    }
}

// ── 背包组件 ────────────────────────────────────────

#[derive(Component, Clone, Debug, Serialize, Deserialize)]
pub struct Inventory {
    pub stacks: Vec<ItemStack>,
    pub capacity: usize,
}

impl Default for Inventory { fn default() -> Self { Self::new(36) } }
impl Inventory {
    pub fn new(capacity: usize) -> Self {
        Self { stacks: Vec::new(), capacity }
    }

    /// 尝试添加指定数量的物品。自动堆叠，返回未能放入的数量。
    pub fn add(&mut self, item_id: usize, mut count: u32) -> u32 {
        if count == 0 { return 0; }
        let max_stack = ItemRegistry::global().get(item_id)
            .map(|d| d.max_stack).unwrap_or(1);

        // 1. 先尝试堆叠到已有同 ID 且未满的栈
        for stack in &mut self.stacks {
            if stack.item_id == item_id && !stack.is_full() {
                count -= stack.add_up_to(count);
                if count == 0 { return 0; }
            }
        }

        // 2. 不足时创建新栈
        while count > 0 && self.stacks.len() < self.capacity {
            let put = count.min(max_stack);
            self.stacks.push(ItemStack::new(item_id, put));
            count -= put;
        }

        count
    }

    /// 移除指定栈的 count 个物品。如果栈清空则删除该条目。
    pub fn remove(&mut self, index: usize, count: u32) -> u32 {
        if let Some(stack) = self.stacks.get_mut(index) {
            let actual = count.min(stack.count);
            stack.count -= actual;
            if stack.count == 0 {
                self.stacks.remove(index);
            }
            actual
        } else {
            0
        }
    }

    /// 丢弃一整格
    pub fn drop_stack(&mut self, index: usize) -> Option<ItemStack> {
        if index < self.stacks.len() {
            Some(self.stacks.remove(index))
        } else {
            None
        }
    }

    /// 预检：是否能容纳指定数量的该物品（不修改背包）
    pub fn can_add(&self, item_id: usize, count: u32) -> bool {
        if count == 0 { return true; }
        let max_stack = ItemRegistry::global().get(item_id)
            .map(|d| d.max_stack).unwrap_or(1);
        let mut remaining = count;

        // 1. 先算已有同 ID 未满栈的剩余空间
        for stack in &self.stacks {
            if stack.item_id == item_id && !stack.is_full() {
                let space = stack.max_stack() - stack.count;
                remaining = remaining.saturating_sub(space);
                if remaining == 0 { return true; }
            }
        }

        // 2. 算还需要多少个空格
        let needed_slots = remaining.div_ceil(max_stack);
        self.stacks.len() + needed_slots as usize <= self.capacity
    }
}

// ── 装备组件 ────────────────────────────────────────

/// 装备槽直接持有物品（不占背包空间）。
/// 每个槽位存放完整的 ItemStack（通常 count=1）。
#[derive(Component, Clone, Debug, Serialize, Deserialize)]
pub struct Equipment {
    pub weapon: Option<ItemStack>,
    pub armor: Option<ItemStack>,
    pub ring: Option<ItemStack>,
}

impl Default for Equipment { fn default() -> Self { Self::new() } }
impl Equipment {
    pub fn new() -> Self {
        Self { weapon: None, armor: None, ring: None }
    }

    /// 获取所有已装备物品的迭代器
    pub fn equipped_stacks(&self) -> Vec<&ItemStack> {
        let mut v = Vec::new();
        if let Some(s) = &self.weapon { v.push(s); }
        if let Some(s) = &self.armor { v.push(s); }
        if let Some(s) = &self.ring { v.push(s); }
        v
    }
}

// ── 物品使用集中分派 ──────────────────────────────

/// 使用物品的集中分派函数。
/// 所有调用方通过此函数使用物品，避免 match item_id 散布在多个文件中。
/// 返回 true 表示消耗了该物品。
pub fn use_item(item_id: usize, world: &mut World, user: Entity) -> bool {
    use crate::SkillKind;
    match item_id {
        ITEM_SCROLL_HEAL => {
            crate::ops::learn_skill(world, user, &SkillKind::Heal { amount: 15 });
            true
        }
        ITEM_SCROLL_SHIELD => {
            crate::ops::learn_skill(world, user, &SkillKind::Shield { def_boost: 5, duration: 3 });
            true
        }
        ITEM_SCROLL_BERSERK => {
            crate::ops::learn_skill(world, user, &SkillKind::Berserk { atk_boost: 5, duration: 3 });
            true
        }
        _ => false,
    }
}

// ── 地面拾取物组件 ──────────────────────────────────

#[derive(Component, Clone, Debug)]
pub struct ItemPickup {
    pub stack: ItemStack,
}

// ── 工具函数 ────────────────────────────────────────

/// 计算装备加成的总和（装备直接持有物品，不再查背包）
pub fn equipment_bonus(_inv: &Inventory, equip: &Equipment) -> StatBonus {
    let mut total = StatBonus::default();
    for stack in equip.equipped_stacks() {
        if let Some(def) = stack.def() {
            let b = &def.bonus;
            total.attack += b.attack;
            total.defense += b.defense;
            total.magic_mastery += b.magic_mastery;
            total.agility += b.agility;
            total.hp += b.hp;
            total.crit_rate += b.crit_rate;
            total.crit_damage += b.crit_damage;
        }
    }
    total
}
