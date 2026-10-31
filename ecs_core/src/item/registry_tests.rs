//! 物品注册表的测试。
//!
//! 一张数据表最怕的不是"写错一个数"，而是**表本身不完整**：漏一个 `max_stack`、
//! 槽位写成 `None`、法力和生命的效果挂反。这些都不会报错，只会在某个玩家手里
//! 表现为"这件装备捡不起来"或"吃了没反应"。所以这里的用例几乎都是**枚举全表**的。

use super::*;

/// 每一行的 `id` 都必须与"它所在的位置"一致（表里第 i 行 = 第 i 个 `ItemDef`）。
///
/// 这条守的是**编号与表的对应关系**：`ITEMS` 是手写的常量数组，没有东西阻止
/// 有人把 `ItemId(25)` 写成 `ItemId(23)`——那样两行会撞号，而 `def()` 只会返回先找到的那个。
#[test]
fn every_item_row_matches_game_doc() {
    let doc_ids: &[(u16, &str)] = &[
        (0, "锈铁剑"),
        (1, "木盾"),
        (2, "皮甲"),
        (3, "攻击戒指"),
        (10, "生物血肉"),
        (11, "破布"),
        (12, "坚硬木棍"),
        (13, "染血兽牙"),
        (14, "黑色甲壳"),
        (18, "石子"),
        (23, "石锤"),
        (24, "匕首"),
        (25, "蘑菇"),
        (26, "苔藓"),
        (27, "孢子囊"),
        (28, "贝壳"),
        (29, "珍珠"),
        (30, "鱼骨"),
        (31, "鳗皮"),
        (32, "海藻"),
        // 不在 GAME.md 号段里的内容轮产物：踏水靴（C4/C5）。
        (40, "踏水靴"),
    ];

    assert_eq!(ITEMS.len(), doc_ids.len(), "表里有未登记的行（或漏了行）");
    for (def, (id, name)) in ITEMS.iter().zip(doc_ids) {
        assert_eq!(def.id.0, *id, "{} 的编号与 GAME.md 不一致", def.name);
        assert_eq!(def.name, *name, "编号 {id} 的显示名与 GAME.md 不一致");
    }
}

/// 编号唯一：撞号会让 `ItemId::def()` 只返回靠前那一行，另一行永远查不到。
#[test]
fn item_ids_are_unique() {
    for (index, def) in ITEMS.iter().enumerate() {
        for other in &ITEMS[index + 1..] {
            assert_ne!(def.id, other.id, "编号 {} 撞车", def.id.0);
        }
    }
}

/// 每一行的编号都能反查到自己（`def()` 的往返一致性）。
#[test]
fn every_item_id_resolves() {
    for def in ITEMS {
        assert_eq!(def.id.def().name, def.name, "编号 {} 反查失败", def.id.0);
        assert!(def.id.exists());
    }
    // 表外编号必须能识别（`exists()` 不 panic，`def()` 才 panic）。
    assert!(!ItemId(9999).exists());
}

/// 装备的堆叠恒为 1、非装备恒 > 1：这条不是洁癖，`Inventory::try_add` 依赖它
/// （`max_stack == 1` 时一格一件，装备因此不会被"并进别的堆"）。
#[test]
fn equipment_never_stacks_and_other_items_do() {
    for def in ITEMS {
        if def.equip_slot().is_some() {
            assert_eq!(def.max_stack, 1, "{} 是装备，堆叠上限必须为 1", def.name);
            assert!(!def.is_stackable());
        } else {
            assert!(
                def.is_stackable(),
                "{} 不是装备，堆叠上限应当 > 1（否则它永远一格一件）",
                def.name
            );
        }
    }
}

/// 武器必须给出攻击耗时，非武器必须不给：`weapon_speed_ms` 与"是不是主手武器"同真同假。
#[test]
fn only_main_hand_weapons_carry_a_swing_speed() {
    for def in ITEMS {
        let is_weapon = def.equip_slot() == Some(EquipSlot::MainHand);
        assert_eq!(
            def.weapon_speed_ms.is_some(),
            is_weapon,
            "{}：主手武器与攻速必须同时有/同时无",
            def.name
        );
    }
    // 三档攻速与 GAME.md Gm7「武器攻速」一致。
    assert_eq!(ItemId(24).def().weapon_speed_ms, Some(200.0), "匕首 200ms");
    assert_eq!(ItemId(0).def().weapon_speed_ms, Some(300.0), "锈铁剑 300ms");
    assert_eq!(ItemId(23).def().weapon_speed_ms, Some(450.0), "石锤 450ms");
}

/// 四条装备槽位**都至少有一件**能穿的装备：否则玩家会遇到"这个槽位永远是空的"。
#[test]
fn every_equipment_slot_has_at_least_one_item() {
    for slot in EquipSlot::ALL {
        assert!(
            ITEMS
                .iter()
                .any(|def| def.equip_slot() == Some(slot)),
            "{:?} 没有任何可装备物品",
            slot
        );
    }
}

/// 装备的修正必须**作用于它该作用的那一项**：武器加攻击、防具加防御。
///
/// 反例（这条用例存在的意义）：把皮甲的修正写成 `Stat::Attack`，玩家会变成"穿甲打得疼"，
/// 而所有读取 `Defense` 的地方都看不出问题。
#[test]
fn equipment_modifiers_target_the_right_stat() {
    for def in ITEMS {
        let Some(slot) = def.equip_slot() else {
            continue;
        };
        for modifier in def.modifiers() {
            let expected = match slot {
                EquipSlot::MainHand | EquipSlot::Ring => crate::rules::Stat::Attack,
                EquipSlot::OffHand | EquipSlot::Armor => crate::rules::Stat::Defense,
            };
            // 踏水靴是唯一例外：它不加数值，只无视地形那一桶。
            if !def.ignores().is_empty() {
                continue;
            }
            assert_eq!(
                modifier.stat, expected,
                "{}（{:?}）的修正作用在 {:?}，与槽位不符",
                def.name, slot, modifier.stat
            );
            assert_eq!(
                modifier.source,
                crate::rules::EffectSource::Equipment,
                "{} 的修正来源必须是装备",
                def.name
            );
        }
    }
}

/// 消耗品必须带效果、非消耗品必须不带：`Consumable` 这个变体不该是空壳。
#[test]
fn consumables_always_do_something() {
    for def in ITEMS {
        if let ItemKind::Consumable(effect) = def.kind {
            let amount = match effect {
                ConsumeEffect::Heal(amount) | ConsumeEffect::RestoreMagic(amount) => amount,
            };
            assert!(amount > 0.0, "{} 的回复量必须为正", def.name);
            assert!(!effect.describe().is_empty());
        }
    }
}

/// 数值与 GAME.md Gm7「消耗品」一致（蘑菇 +6 HP、海藻 +4 MP）。
///
/// 钉死具体值而不是"大于 0"：这两个数是文档里写明的，改它们必须先改文档
/// （`docs_check.py` 保证 GAME.md 的表格与地形表一致，但管不到这里——所以由测试管）。
#[test]
fn consumable_values_match_the_design_doc() {
    assert_eq!(
        ItemId(25).def().consume_effect(),
        Some(ConsumeEffect::Heal(6.0))
    );
    assert_eq!(
        ItemId(32).def().consume_effect(),
        Some(ConsumeEffect::RestoreMagic(4.0))
    );
}

/// 踏水靴保留 C4/C5 的核心语义：**只无视地形，不提供数值修正**。
#[test]
fn trail_boots_only_ignore_the_terrain_bucket() {
    let boots = ItemId(40).def();
    assert_eq!(boots.equip_slot(), Some(EquipSlot::Armor));
    assert!(boots.modifiers().is_empty(), "踏水靴不该加任何数值");
    assert_eq!(
        boots.ignores(),
        &[crate::rules::EffectSource::Terrain],
        "踏水靴的无视桶必须恰好是地形"
    );
}

/// 名字反查：日志、存档与手工测试都靠它。
#[test]
fn items_can_be_looked_up_by_name() {
    assert_eq!(item_by_name("锈铁剑").map(|def| def.id), Some(ItemId(0)));
    assert_eq!(item_by_name("踏水靴").map(|def| def.id), Some(ItemId(40)));
    assert!(item_by_name("不存在的物品").is_none());
}
