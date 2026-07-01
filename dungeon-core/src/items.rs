use bevy_ecs::prelude::*;
use ratatui::style::Color;
use serde::{Deserialize, Serialize};

// ── 物品 / 装备 ─────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EquipmentSlot { Weapon, Armor, Ring }

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct StatBonus {
    /// 攻击
    pub attack: i32,
    /// 防御
    pub defense: i32,
    /// 法术精通
    pub magic_mastery: i32,
    /// 敏捷
    pub agility: i32,
    pub hp: i32,
    pub crit_rate: f32,
    pub crit_damage: f32,
}

#[derive(Clone, Debug)]
pub struct ItemInstance {
    pub name: String, pub glyph: char, pub color: Color,
    pub slot: EquipmentSlot, pub bonus: StatBonus, pub description: String,
}

#[derive(Component)]
pub struct Inventory { pub items: Vec<ItemInstance>, pub capacity: usize }
impl Inventory { pub fn new(cap: usize) -> Self { Self { items: Vec::new(), capacity: cap } } }

#[derive(Component)]
pub struct Equipment {
    pub weapon: Option<usize>, pub armor: Option<usize>, pub ring: Option<usize>,
}
impl Equipment { pub fn new() -> Self { Self { weapon: None, armor: None, ring: None } } }

#[derive(Component)]
pub struct ItemPickup { pub item: ItemInstance }

pub fn make_items() -> Vec<ItemInstance> {
    vec![
        ItemInstance {
            name: "锈铁剑".into(), glyph: '/', color: Color::LightCyan,
            slot: EquipmentSlot::Weapon,
            bonus: StatBonus { attack: 3, ..Default::default() },
            description: "一把生锈的铁剑，攻击+3".into(),
        },
        ItemInstance {
            name: "木盾".into(), glyph: '[', color: Color::Rgb(139, 90, 43),
            slot: EquipmentSlot::Armor,
            bonus: StatBonus { defense: 2, ..Default::default() },
            description: "简陋的木盾，防御+2".into(),
        },
        ItemInstance {
            name: "皮甲".into(), glyph: ']', color: Color::LightYellow,
            slot: EquipmentSlot::Armor,
            bonus: StatBonus { defense: 1, agility: 1, ..Default::default() },
            description: "轻便皮甲，防御+1 敏捷+1".into(),
        },
        ItemInstance {
            name: "攻击戒指".into(), glyph: '=', color: Color::LightRed,
            slot: EquipmentSlot::Ring,
            bonus: StatBonus { attack: 2, ..Default::default() },
            description: "攻击之戒，攻击+2".into(),
        },
    ]
}
