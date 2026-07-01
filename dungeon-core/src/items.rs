use bevy_ecs::prelude::*;
use ratatui::style::Color;
use serde::{Deserialize, Serialize};

// ── 物品 / 装备 ─────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EquipmentSlot { Weapon, Armor, Ring }

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct StatBonus {
    pub strength: i32, pub dexterity: i32, pub intelligence: i32, pub vitality: i32,
    pub hp: i32, pub attack: i32, pub defense: i32,
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
            description: "一把生锈的铁剑，ATK+3".into(),
        },
        ItemInstance {
            name: "木盾".into(), glyph: '[', color: Color::Rgb(139, 90, 43),
            slot: EquipmentSlot::Armor,
            bonus: StatBonus { defense: 2, ..Default::default() },
            description: "简陋的木盾，DEF+2".into(),
        },
        ItemInstance {
            name: "皮甲".into(), glyph: ']', color: Color::LightYellow,
            slot: EquipmentSlot::Armor,
            bonus: StatBonus { defense: 1, vitality: 1, ..Default::default() },
            description: "轻便皮甲，DEF+1 VIT+1".into(),
        },
        ItemInstance {
            name: "力量戒指".into(), glyph: '=', color: Color::LightRed,
            slot: EquipmentSlot::Ring,
            bonus: StatBonus { strength: 2, ..Default::default() },
            description: "力量之戒，STR+2".into(),
        },
    ]
}
