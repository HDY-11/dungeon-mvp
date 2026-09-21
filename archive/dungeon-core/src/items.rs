//! 物品系统：物品 ID 常量、物品定义加载、装备/使用/合成/投掷判定等核心规则。

use crate::ext::{OptionLogExt, ResultLogExt};
use crate::{EventLog, Stats};
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
pub const ITEM_STONE: usize = 18;
pub const ITEM_TEMPLATE_BLADE: usize = 19;
pub const ITEM_TEMPLATE_SHIELD: usize = 20;
pub const ITEM_TEMPLATE_ARMOR: usize = 21;
pub const ITEM_TEMPLATE_RING: usize = 22;
pub const ITEM_STONE_HAMMER: usize = 23;
pub const ITEM_DAGGER: usize = 24;
// Dsn24 多类型地图掉落物
pub const ITEM_MUSHROOM: usize = 25; // 蘑菇（繁茂，回 HP）
pub const ITEM_MOSS: usize = 26; // 苔藓（繁茂，材料）
pub const ITEM_SPORE_SAC: usize = 27; // 孢子囊（繁茂，材料）
pub const ITEM_SHELL: usize = 28; // 贝壳（地海，材料）
pub const ITEM_PEARL: usize = 29; // 珍珠（地海，稀有材料）
pub const ITEM_FISH_BONE: usize = 30; // 鱼骨（地海，材料）
pub const ITEM_EEL_SKIN: usize = 31; // 鳗皮（地海，材料）
pub const ITEM_SEAWEED: usize = 32; // 海藻（地海，回 MP）

/// 判断物品是否可作为投掷物（MVP 仅石子）。
/// 投掷执行与副手装填前的验证入口，避免非投掷物被消耗。
pub fn is_throwable(item_id: usize) -> bool {
    item_id == ITEM_STONE
}

// ── 合成配方（Dsn19 Phase 1：模板碎片 = 一次性窄谱配方） ──

/// 一条合成配方：消耗 ingredients 产出 product
pub struct Recipe {
    pub product: usize,
    /// (item_id, count) 列表
    pub ingredients: Vec<(usize, usize)>,
}

/// 模板碎片 → 配方（I69）。未注册的模板返回 None。
pub fn template_recipe(template_id: usize) -> Option<Recipe> {
    match template_id {
        ITEM_TEMPLATE_BLADE => Some(Recipe {
            product: ITEM_RUSTY_SWORD,
            ingredients: vec![(ITEM_BIOMASS, 2), (ITEM_STICK, 1)],
        }),
        ITEM_TEMPLATE_SHIELD => Some(Recipe {
            product: ITEM_WOOD_SHIELD,
            ingredients: vec![(ITEM_BIOMASS, 2), (ITEM_CLOTH, 1)],
        }),
        ITEM_TEMPLATE_ARMOR => Some(Recipe {
            product: ITEM_LEATHER_ARMOR,
            ingredients: vec![(ITEM_CHITIN, 2), (ITEM_CLOTH, 1)],
        }),
        ITEM_TEMPLATE_RING => Some(Recipe {
            product: ITEM_ATTACK_RING,
            ingredients: vec![(ITEM_FANG, 2)],
        }),
        _ => None,
    }
}

/// 用模板碎片合成（Dsn19 Phase 1）：检查材料与背包空间 → 消耗 → 产出。
/// 失败时推送 EventLog 原因，不消耗模板。成功返回 true（模板由调用方移除）。
pub fn craft_with_template(world: &mut World, user: Entity, template_id: usize) -> bool {
    let Some(recipe) = template_recipe(template_id) else {
        return false;
    };
    // 材料检查
    let inv = world.get::<Inventory>(user);
    for (id, count) in &recipe.ingredients {
        let have = inv.map(|i| i.count_of(*id)).unwrap_or(0);
        if have < *count as u32 {
            let name = ItemRegistry::global()
                .get(*id)
                .map(|d| d.name.as_str())
                .unwrap_or("材料");
            world
                .resource_mut::<EventLog>()
                .push(crate::EventMessage::item(format!(
                    "材料不足：还缺 {}×{}",
                    name,
                    *count as u32 - have
                )));
            return false;
        }
    }
    // 空间检查（产出 1 件装备，占 1 格）。
    // G26: 材料消耗与模板消耗（调用方在成功后移除）都会腾出格子——当前背包放不下时，
    // 模拟「移除配方材料 + 模板」后的空间再判定，避免背包满但材料/模板占位时误报。
    let can_add = inv
        .map(|i| {
            if i.can_add(recipe.product, 1) {
                return true;
            }
            let mut sim = i.clone();
            for (id, count) in &recipe.ingredients {
                sim.remove_item(*id, *count as u32);
            }
            sim.remove_item(template_id, 1);
            sim.can_add(recipe.product, 1)
        })
        .unwrap_or(false);
    if !can_add {
        world
            .resource_mut::<EventLog>()
            .push(crate::EventMessage::item("背包已满".to_string()));
        return false;
    }
    // 消耗材料 + 产出
    for (id, count) in &recipe.ingredients {
        world
            .get_mut::<Inventory>(user)
            .expect_log("Inventory exists for craft")
            .remove_item(*id, *count as u32);
    }
    world
        .get_mut::<Inventory>(user)
        .expect_log("Inventory exists for craft")
        .add(recipe.product, 1);
    let product_name = ItemRegistry::global()
        .get(recipe.product)
        .map(|d| d.name.clone())
        .unwrap_or_default();
    world
        .resource_mut::<EventLog>()
        .push(crate::EventMessage::item(format!(
            "合成成功：{}",
            product_name
        )));
    true
}

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
    MainHand,
    OffHand,
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
    /// 武器行动耗时（毫秒，I67：仅主手武器生效）。None = 默认 300ms。
    #[serde(default)]
    pub speed: Option<u32>,
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
            // 已归档（archive/README.md）：多一层 `../`，指向仓库根的 assets/items.json。
            let data = include_str!("../../../assets/items.json");
            let defs: Vec<ItemDef> = serde_json::from_str(data).expect_log("Invalid items.json");
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
        ITEM_REGISTRY
            .get()
            .expect_log("ItemRegistry not loaded — call ItemRegistry::load() first")
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
        Self {
            item_id,
            count,
            meta: None,
        }
    }

    pub fn def(&self) -> Option<&'static ItemDef> {
        ItemRegistry::global().get(self.item_id)
    }

    pub fn name(&self) -> String {
        // I41: 优先使用自定义名称
        if let Some(ref meta) = self.meta
            && let Some(ref custom) = meta.display_name
        {
            return custom.clone();
        }
        self.def()
            .map(|d| d.name.clone())
            .unwrap_or_else(|| format!("未知物品({})", self.item_id))
    }

    pub fn description(&self) -> String {
        self.def()
            .map(|d| d.description.clone())
            .unwrap_or_default()
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

impl Default for Inventory {
    fn default() -> Self {
        Self::new(36)
    }
}
impl Inventory {
    pub fn new(capacity: usize) -> Self {
        Self {
            stacks: Vec::new(),
            capacity,
        }
    }

    /// 尝试添加指定数量的物品。自动堆叠，返回未能放入的数量。
    pub fn add(&mut self, item_id: usize, mut count: u32) -> u32 {
        if count == 0 {
            return 0;
        }
        let max_stack = ItemRegistry::global()
            .get(item_id)
            .map(|d| d.max_stack)
            .unwrap_or(1);

        // 1. 先尝试堆叠到已有同 ID 且未满的栈
        for stack in &mut self.stacks {
            if stack.item_id == item_id && !stack.is_full() {
                count -= stack.add_up_to(count);
                if count == 0 {
                    return 0;
                }
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

    /// 预检：是否能容纳指定数量的该物品（不修改背包）
    pub fn can_add(&self, item_id: usize, count: u32) -> bool {
        if count == 0 {
            return true;
        }
        let max_stack = ItemRegistry::global()
            .get(item_id)
            .map(|d| d.max_stack)
            .unwrap_or(1);
        let mut remaining = count;

        // 1. 先算已有同 ID 未满栈的剩余空间
        for stack in &self.stacks {
            if stack.item_id == item_id && !stack.is_full() {
                let space = stack.max_stack() - stack.count;
                remaining = remaining.saturating_sub(space);
                if remaining == 0 {
                    return true;
                }
            }
        }

        // 2. 算还需要多少个空格
        let needed_slots = remaining.div_ceil(max_stack);
        self.stacks.len() + needed_slots as usize <= self.capacity
    }

    /// 背包中某物品的总数量（I69 合成材料检查）
    pub fn count_of(&self, item_id: usize) -> u32 {
        self.stacks
            .iter()
            .filter(|s| s.item_id == item_id)
            .map(|s| s.count)
            .sum()
    }

    /// 按物品 ID 扣减数量（I69 合成消耗）。数量不足返回 false 且不修改。
    pub fn remove_item(&mut self, item_id: usize, mut count: u32) -> bool {
        if self.count_of(item_id) < count {
            return false;
        }
        for i in (0..self.stacks.len()).rev() {
            if self.stacks[i].item_id != item_id {
                continue;
            }
            let take = self.stacks[i].count.min(count);
            self.stacks[i].count -= take;
            count -= take;
            if self.stacks[i].count == 0 {
                self.stacks.remove(i);
            }
            if count == 0 {
                break;
            }
        }
        true
    }
}

// ── 装备组件 ────────────────────────────────────────

/// 装备槽直接持有物品（不占背包空间）。
/// 每个槽位存放完整的 ItemStack（通常 count=1）。
#[derive(Component, Clone, Debug, Serialize, Deserialize)]
pub struct Equipment {
    pub main_hand: Option<ItemStack>,
    pub off_hand: Option<ItemStack>,
    pub armor: Option<ItemStack>,
    pub ring: Option<ItemStack>,
}

impl Default for Equipment {
    fn default() -> Self {
        Self::new()
    }
}
impl Equipment {
    pub fn new() -> Self {
        Self {
            main_hand: None,
            off_hand: None,
            armor: None,
            ring: None,
        }
    }

    /// 按槽位索引取引用（I73 收敛：0=主手 1=副手 2=防具 3=戒指）
    pub fn slot(&self, idx: usize) -> &Option<ItemStack> {
        match idx {
            0 => &self.main_hand,
            1 => &self.off_hand,
            2 => &self.armor,
            _ => &self.ring,
        }
    }

    /// 按槽位索引取可变引用
    pub fn slot_mut(&mut self, idx: usize) -> &mut Option<ItemStack> {
        match idx {
            0 => &mut self.main_hand,
            1 => &mut self.off_hand,
            2 => &mut self.armor,
            _ => &mut self.ring,
        }
    }

    /// 获取所有已装备物品的迭代器（含主手、副手）
    pub fn equipped_stacks(&self) -> Vec<&ItemStack> {
        let mut v = Vec::new();
        if let Some(s) = &self.main_hand {
            v.push(s);
        }
        if let Some(s) = &self.off_hand {
            v.push(s);
        }
        if let Some(s) = &self.armor {
            v.push(s);
        }
        if let Some(s) = &self.ring {
            v.push(s);
        }
        v
    }
}

// ── 物品使用集中分派 ──────────────────────────────

/// 使用物品的集中分派函数。
/// 所有调用方通过此函数使用物品，避免 match item_id 散布在多个文件中。
/// 返回 true 表示消耗了该物品。
/// 物品是否可直接使用（L47/I66：UI 提示「r:使用/学习」与 use_item 共用此判定）。
/// 卷轴可学习、模板碎片可合成、蘑菇/海藻可食用；石子等不可直接使用。
pub fn is_usable(item_id: usize) -> bool {
    matches!(
        item_id,
        ITEM_SCROLL_HEAL
            | ITEM_SCROLL_SHIELD
            | ITEM_SCROLL_BERSERK
            | ITEM_TEMPLATE_BLADE
            | ITEM_TEMPLATE_SHIELD
            | ITEM_TEMPLATE_ARMOR
            | ITEM_TEMPLATE_RING
            | ITEM_MUSHROOM
            | ITEM_SEAWEED
    )
}

/// 详情页可用操作（L47/I66：ui.rs 渲染提示与 main.rs 处理器共用，防漂移）。
/// detail_source: 0=背包 1=装备槽 2=地面
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ItemAction {
    /// e: 装备（背包，物品有槽位）
    Equip,
    /// r: 使用/学习（背包，可消耗品）
    Use,
    /// d: 丢弃（背包）
    Drop,
    /// u: 卸载（装备槽）
    Unequip,
    /// g: 拾取（地面详情）
    Pickup,
}

/// 计算详情页可用操作列表。渲染与处理器都以此为准。
pub fn detail_item_actions(item: Option<&ItemStack>, detail_source: usize) -> Vec<ItemAction> {
    let mut v = Vec::new();
    match detail_source {
        0 => {
            if let Some(s) = item {
                if s.def().is_some_and(|d| d.slot.is_some()) {
                    v.push(ItemAction::Equip);
                }
                if is_usable(s.item_id) {
                    v.push(ItemAction::Use);
                }
                v.push(ItemAction::Drop);
            }
        }
        1 => v.push(ItemAction::Unequip),
        2 => v.push(ItemAction::Pickup),
        _ => {}
    }
    v
}

pub fn use_item(item_id: usize, world: &mut World, user: Entity) -> bool {
    use crate::SkillKind;
    if !is_usable(item_id) {
        return false;
    }
    match item_id {
        ITEM_SCROLL_HEAL => {
            crate::ops::learn_skill(world, user, &SkillKind::Heal { amount: 15 });
            true
        }
        ITEM_SCROLL_SHIELD => {
            crate::ops::learn_skill(
                world,
                user,
                &SkillKind::Shield {
                    def_boost: 5,
                    duration: 1,
                },
            );
            true
        }
        ITEM_SCROLL_BERSERK => {
            crate::ops::learn_skill(
                world,
                user,
                &SkillKind::Berserk {
                    atk_boost: 5,
                    duration: 1,
                },
            );
            true
        }
        // I69/Dsn19 Phase 1: 模板碎片 = 一次性配方（失败原因由 craft_with_template 推送）
        ITEM_TEMPLATE_BLADE | ITEM_TEMPLATE_SHIELD | ITEM_TEMPLATE_ARMOR | ITEM_TEMPLATE_RING => {
            craft_with_template(world, user, item_id)
        }
        // Dsn24: 地形消耗品——蘑菇回 HP、海藻回 MP（上限钳制） [⃞试调: 蘑菇+6 低于治愈卷轴，海藻+4]
        ITEM_MUSHROOM => {
            if let Some(mut st) = world.get_mut::<Stats>(user) {
                let heal = st.heal(6);
                world
                    .resource_mut::<EventLog>()
                    .push(crate::EventMessage::item(if heal > 0 {
                        format!("食用了蘑菇，恢复 {} HP", heal)
                    } else {
                        "生命已满，无需食用蘑菇".to_string()
                    }));
                heal > 0
            } else {
                false
            }
        }
        ITEM_SEAWEED => {
            if let Some(mut st) = world.get_mut::<Stats>(user) {
                let restore = st.restore_mp(4);
                world
                    .resource_mut::<EventLog>()
                    .push(crate::EventMessage::item(if restore > 0 {
                        format!("食用了海藻，恢复 {} MP", restore)
                    } else {
                        "法力已满，无需食用海藻".to_string()
                    }));
                restore > 0
            } else {
                false
            }
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
pub fn equipment_bonus(equip: &Equipment) -> StatBonus {
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

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_ecs::prelude::World;

    /// 构造一个恰好满 36 格的背包：模板 + 配方材料（生物血肉×2 同格、坚硬木棍×1）+ 其余占位
    fn full_backpack_with_recipe() -> (World, bevy_ecs::prelude::Entity) {
        ItemRegistry::load();
        let mut world = World::new();
        world.insert_resource(EventLog::new());
        let user = world.spawn((Inventory::new(36),)).id();
        {
            let mut inv = world.get_mut::<Inventory>(user).unwrap();
            // 配方材料：生物血肉×2（同格堆叠）+ 坚硬木棍×1
            inv.add(ITEM_BIOMASS, 2);
            inv.add(ITEM_STICK, 1);
            // 模板（剑刃）
            inv.add(ITEM_TEMPLATE_BLADE, 1);
            // 其余占满：0-32 排除已用 ID，加重复装备凑满 36 格
            let used = [ITEM_BIOMASS, ITEM_STICK, ITEM_TEMPLATE_BLADE];
            for id in 0..=32usize {
                if used.contains(&id) {
                    continue;
                }
                inv.add(id, 1);
            }
            // 3 个 max_stack=1 的重复项补满（0/1/2 已加入？0/1/2 不在 used 中已加入一次，max_stack=1 再加仍占新格）
            inv.add(ITEM_RUSTY_SWORD, 1);
            inv.add(ITEM_WOOD_SHIELD, 1);
            inv.add(ITEM_LEATHER_ARMOR, 1);
            assert_eq!(inv.stacks.len(), 36, "测试前提：背包满 36 格");
        }
        (world, user)
    }

    /// G26 回归：背包满但模板+材料占位 → 合成应成功（材料与模板移除后腾出空间）
    #[test]
    fn test_craft_with_template_full_backpack_succeeds() {
        let (mut world, user) = full_backpack_with_recipe();
        let ok = craft_with_template(&mut world, user, ITEM_TEMPLATE_BLADE);
        assert!(ok, "背包满但材料/模板占位时应合成成功（G26）");
        let inv = world.get::<Inventory>(user).unwrap();
        assert!(
            inv.stacks.iter().any(|s| s.item_id == ITEM_RUSTY_SWORD),
            "应产出锈铁剑"
        );
        assert_eq!(inv.count_of(ITEM_BIOMASS), 0, "生物血肉应被消耗");
        assert_eq!(inv.count_of(ITEM_STICK), 0, "坚硬木棍应被消耗");
        // 模板由调用方移除（craft 内部不移除）——这里断言背包中有模板（待调用方处理）
        assert!(inv.count_of(ITEM_TEMPLATE_BLADE) >= 1, "模板应由调用方移除");
    }

    /// 背包满且无材料 → 拒绝且不消耗模板
    #[test]
    fn test_craft_with_template_missing_materials_rejected() {
        let (mut world, user) = full_backpack_with_recipe();
        // 清掉材料（仅留模板）
        world
            .get_mut::<Inventory>(user)
            .unwrap()
            .remove_item(ITEM_BIOMASS, 2);
        world
            .get_mut::<Inventory>(user)
            .unwrap()
            .remove_item(ITEM_STICK, 1);
        let ok = craft_with_template(&mut world, user, ITEM_TEMPLATE_BLADE);
        assert!(!ok, "材料不足应失败");
        let inv = world.get::<Inventory>(user).unwrap();
        assert!(inv.count_of(ITEM_TEMPLATE_BLADE) >= 1, "失败不应消耗模板");
    }
}
