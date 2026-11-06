//! 物品注册表的测试。
//!
//! 一张数据表最怕的不是"写错一个数"，而是**表本身不完整**：漏一个 `max_stack`、
//! 槽位写成 `None`、法力和生命的效果挂反。这些都不会报错，只会在某个玩家手里
//! 表现为"这件装备捡不起来"或"吃了没反应"。所以这里的用例几乎都是**枚举全表**的。

use super::*;

/// 每一行的 `id` 都必须与"它所在的位置"一致（表里第 i 行 = 第 i 个 `ItemDef`）。
///
/// 这条守的是**编号与表的对应关系**：`ITEMS` 来自内嵌的 `assets/items.json`，加载器
/// 保证编号严格递增，但没有东西阻止有人把 `ItemId(25)` 改成 `ItemId(23)`——那样清单与
/// 数据就对不上了（`def()` 只会返回先找到的那个）。
///
/// 注意它是**手工镜像清单**，不读 `GAME.md`（门禁也不解析文档表格，见 AUDIT-data D14）：
/// 改内容时 GAME.md 与这张清单要一起改，单改文档不会让这条测试变红。
#[test]
fn every_item_row_matches_the_pinned_list() {
    let doc_ids: &[(u16, &str)] = &[
        (0, "锈铁剑"),
        (1, "木盾"),
        (2, "皮甲"),
        (3, "攻击戒指"),
        (4, "皮帽"),
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
        // 内容轮产物（GAME.md 物品表在 2026-11 数据审计时已补录）。
        (40, "踏水靴"),
        (41, "旅者行囊"),
    ];

    let all = items();
    assert_eq!(all.len(), doc_ids.len(), "表里有未登记的行（或漏了行）");
    for (def, (id, name)) in all.iter().zip(doc_ids) {
        assert_eq!(def.id.0, *id, "{} 的编号与 GAME.md 不一致", def.name);
        assert_eq!(def.name, *name, "编号 {id} 的显示名与 GAME.md 不一致");
    }
}

/// 编号唯一：撞号会让 `ItemId::def()` 只返回靠前那一行，另一行永远查不到。
#[test]
fn item_ids_are_unique() {
    let all = items();
    for (index, def) in all.iter().enumerate() {
        for other in &all[index + 1..] {
            assert_ne!(def.id, other.id, "编号 {} 撞车", def.id.0);
        }
    }
}

/// 每一行的编号都能反查到自己（`def()` 的往返一致性）。
#[test]
fn every_item_id_resolves() {
    for def in items() {
        assert_eq!(def.id.def().name, def.name, "编号 {} 反查失败", def.id.0);
        assert!(def.id.exists());
    }
    // 表外编号必须能识别（`exists()` 不 panic，`def()` 才 panic）。
    assert!(!ItemId(9999).exists());
}

/// 单件物品（堆叠上限 1）只能是**装备**或**容器**；其余物品必须可堆叠。
///
/// 容器一个槽位放一个实体，装备与行囊因此不会被"并进别的堆"；材料/消耗品则必须能堆，
/// 否则背包会被一串 ×1 塞满（Gm7 的 `max_stack` 说的就是这件事）。
#[test]
fn single_items_are_equipment_or_containers() {
    for def in items() {
        if def.max_stack == 1 {
            assert!(
                def.is_equipment() || def.container_def().is_some(),
                "{} 是单件物品，但既不是装备也不是容器",
                def.name
            );
        } else {
            assert!(!def.is_equipment(), "{} 是装备，必须一件一格", def.name);
            assert!(
                def.container_def().is_none(),
                "{} 是容器，必须一件一格",
                def.name
            );
        }
    }
}

/// 武器必须给出攻击耗时，非武器必须不给：`weapon_speed_ms` 与"是不是主手武器"同真同假。
#[test]
fn only_main_hand_weapons_carry_a_swing_speed() {
    for def in items() {
        let is_weapon = def.tags.contains(&crate::item::ItemTag::Weapon);
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

/// 身体每个部位**都至少有一件**能穿的装备：否则玩家会遇到"这个部位永远是空的"。
///
/// 这条与 `every_equippable_item_fits_exactly_one_body_slot`（正向）互补：
/// 一个防"物品穿不上"，一个防"部位没内容"。
#[test]
fn every_body_slot_has_something_that_fits() {
    use crate::item::container::BODY;

    for slot in BODY.slots {
        assert!(
            items().iter().any(|def| slot.accept.accepts(def.tags)),
            "部位「{}」没有任何物品穿得进去",
            slot.name
        );
    }
}

/// 装备的修正必须**作用于它该作用的那一项**：武器/戒指加攻击，其余部位加防御，
/// 或者加生命/法力上限（DsnE23 允许装备撑血撑蓝）。
///
/// 反例（这条用例存在的意义）：把皮甲的修正写成 `StatId::ATTACK`，玩家会变成"穿甲打得疼"，
/// 而所有读取 `Defense` 的地方都看不出问题。
#[test]
fn equipment_modifiers_target_the_right_stat() {
    use crate::rules::StatId;

    for def in items() {
        if !def.is_equipment() {
            continue;
        }
        // 部位决定该加什么：武器/戒指加攻击；其余部位加防御或上限。
        let offensive = def
            .tags
            .iter()
            .any(|tag| matches!(tag, crate::item::ItemTag::Weapon | crate::item::ItemTag::Ring));
        for modifier in def.modifiers() {
            let allowed = matches!(
                (offensive, modifier.stat),
                (true, StatId::ATTACK)
                    | (false, StatId::DEFENSE | StatId::MAX_HEALTH | StatId::MAX_MANA)
            );
            assert!(
                allowed,
                "{}（tags={:?}）的修正作用在 {:?}，与部位不符",
                def.name, def.tags, modifier.stat
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

/// 消耗品必须有 `use` 行为、非消耗品必须没有：`Consumable` 这个变体不该是空壳。
///
/// 加载器已经守着这条（`validate_behaviors`），这里再从**表本身**读一遍，
/// 防止将来有人放宽加载器却漏掉语义。
#[test]
fn consumables_always_do_something() {
    for def in items() {
        if def.is_consumable() {
            let effects: Vec<BehaviorEffect> = def.use_effects().collect();
            assert!(!effects.is_empty(), "{} 是消耗品却没有 use 行为", def.name);
            for effect in effects {
                assert!(!effect.describe().is_empty(), "{} 的效果没有描述", def.name);
            }
        } else {
            assert_eq!(
                def.use_effects().count(),
                0,
                "{} 不是消耗品却带 use 行为",
                def.name
            );
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
        ItemId(25).def().use_effects().collect::<Vec<_>>(),
        vec![BehaviorEffect::Heal { amount: 6.0 }]
    );
    assert_eq!(
        ItemId(32).def().use_effects().collect::<Vec<_>>(),
        vec![BehaviorEffect::RestoreMagic { amount: 4.0 }]
    );
}

/// 踏水靴保留 C4/C5 的核心语义：**只无视地形，不提供数值修正**。
#[test]
fn trail_boots_only_ignore_the_terrain_bucket() {
    let boots = ItemId(40).def();
    assert!(boots.is_equipment());
    assert_eq!(boots.tags, &[crate::item::ItemTag::Feet]);
    assert!(boots.modifiers().is_empty(), "踏水靴不该加任何数值");
    assert_eq!(
        boots.ignored_sources().collect::<Vec<_>>(),
        vec![crate::rules::EffectSource::Terrain],
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

/// **标签 × 身体槽位**的交叉校验（DsnE18）：每件可装备物品都至少有一个部位收得下，
/// 而且**恰好**一个部位——多一个就说明标签写宽了（例如戒指也被头盔位接受）。
///
/// 这是 `ItemDef.tags` 与 `container::BODY` 两份内容表的唯一联系点：任何一边写错，
/// 表现都是"这件装备永远穿不上"或"能穿在两个部位"，且都不会报错。
#[test]
fn every_equippable_item_fits_exactly_one_body_slot() {
    use crate::item::container::BODY;

    for def in items() {
        let fits: Vec<&str> = BODY
            .slots
            .iter()
            .filter(|slot| slot.accept.accepts(def.tags))
            .map(|slot| slot.name)
            .collect();
        if !def.tags.is_empty() {
            assert_eq!(
                fits.len(),
                1,
                "{} 应当恰好能穿进一个部位，实得 {fits:?}（tags={:?}）",
                def.name,
                def.tags
            );
        } else {
            assert!(
                fits.is_empty(),
                "{} 没有可装位标签，却能被部位 {fits:?} 接受",
                def.name
            );
        }
    }
}



/// 每件物品都必须有正重量（DsnE20）：0 重会让"载重"这条规则对某些物品形同虚设。
#[test]
fn every_item_has_a_positive_weight() {
    for def in items() {
        assert!(def.weight > 0.0, "{} 的重量必须为正", def.name);
        assert!(
            def.weight <= 10.0,
            "{} 的重量 {:.1} 不像一件随身物",
            def.name,
            def.weight
        );
    }
}

/// 同一件东西：数量翻倍重量翻倍（堆叠按单位重算）。
#[test]
fn stack_weight_scales_with_count() {
    let one = crate::item::stack_weight(ItemId(25), 1);
    let three = crate::item::stack_weight(ItemId(25), 3);
    assert!((three - one * 3.0).abs() < 1e-6, "蘑菇 ×3 = {three}");
    assert!((crate::item::stack_weight(ItemId(0), 1) - 3.0).abs() < 1e-6);
}

// ── 数据加载（DsnE22 阶段 ①） ──────────────────────
//
// 下面这些用例喂的是**最小 JSON 片段**，不是数据文件本身：加载器的每一条校验
// 都必须能被单独触发，否则"写错数据会报错"只是口号。

/// 内嵌数据本身必须能通过加载器（启动期 panic 的同一路径）。
#[test]
fn the_embedded_data_file_loads() {
    let defs = load_items(include_str!("../../assets/items.json"))
        .expect("内嵌物品数据必须能加载");
    assert_eq!(defs.len(), 23, "数据文件的行数应与 GAME.md 物品表一致");
}

/// 用最小表包一层 JSON（`items` 片段由调用方给）。
fn table(items: &[&str]) -> String {
    format!(
        r#"{{"schema_version":{ITEMS_SCHEMA_VERSION},"items":[{}]}}"#,
        items.join(",")
    )
}

/// 一行最小的合法材料（`extra` 用来覆盖单个字段）。
fn material(id: u16, extra: &str) -> String {
    format!(
        r#"{{"id":{id},"name":"测试材料","desc":"测试用","weight":1.0,"max_stack":10,"quality":"common","kind":{{"type":"material"}}{extra}}}"#
    )
}

#[test]
fn an_unsupported_schema_version_is_rejected() {
    let json = r#"{"schema_version":999,"items":[]}"#;
    let error = load_items(json).unwrap_err();
    assert!(error.contains("999"), "错误应提到版本号：{error}");
}

#[test]
fn an_empty_item_table_is_rejected() {
    let error = load_items(&table(&[])).unwrap_err();
    assert!(error.contains("为空"), "错误应说明表为空：{error}");
}

#[test]
fn duplicate_or_unsorted_item_ids_are_rejected() {
    let json = table(&[&material(5, ""), &material(3, "")]);
    let error = load_items(&json).unwrap_err();
    assert!(error.contains("严格递增"), "错误应说明编号顺序：{error}");
}

#[test]
fn a_non_positive_weight_is_rejected() {
    let json = table(&[
        r#"{"id":0,"name":"测试材料","desc":"测试用","weight":0.0,"max_stack":10,"quality":"common","kind":{"type":"material"}}"#,
    ]);
    let error = load_items(&json).unwrap_err();
    assert!(error.contains("重量"), "错误应提到重量：{error}");
}

/// 负重量（方向与 0 相反，但走同一条校验）。
#[test]
fn a_negative_weight_is_rejected() {
    let json = table(&[
        r#"{"id":0,"name":"测试材料","desc":"测试用","weight":-1.0,"max_stack":10,"quality":"common","kind":{"type":"material"}}"#,
    ]);
    let error = load_items(&json).unwrap_err();
    assert!(error.contains("重量"), "错误应提到重量：{error}");
}

#[test]
fn an_unknown_tag_is_rejected() {
    let json = table(&[&material(0, r#","tags":["laser"]"#)]);
    let error = load_items(&json).unwrap_err();
    assert!(error.contains("laser"), "错误应提到未知标签：{error}");
}

#[test]
fn an_unknown_pack_layout_is_rejected() {
    let json = table(&[
        r#"{"id":0,"name":"测试箱子","desc":"测试用","weight":1.0,"max_stack":1,"quality":"common","kind":{"type":"pack","layout":"chest"}}"#,
    ]);
    let error = load_items(&json).unwrap_err();
    assert!(error.contains("chest"), "错误应提到未知布局：{error}");
}

#[test]
fn equipment_and_packs_must_not_stack() {
    let json = table(&[
        r#"{"id":0,"name":"测试箱子","desc":"测试用","weight":1.0,"max_stack":2,"quality":"common","kind":{"type":"pack","layout":"traveler_pack"}}"#,
    ]);
    let error = load_items(&json).unwrap_err();
    assert!(error.contains("堆叠上限"), "错误应提到堆叠上限：{error}");
}

// ── 行为数据（DsnE22 阶段 ③） ──────────────────────
//
// 行为 = 触发时机 + 效果列表。加载器守住触发/效果矩阵，
// 所以运行期的解释器不需要"未知组合"的兜底分支。

/// 行为能原样读出来：`use` + `heal`。
#[test]
fn behaviors_are_loaded_with_their_trigger_and_effects() {
    let json = table(&[
        r#"{"id":0,"name":"测试药","desc":"测试用","weight":1.0,"max_stack":5,"quality":"common","kind":{"type":"consumable"},"behaviors":[{"trigger":"use","effects":[{"type":"heal","amount":2.0}]}]}"#,
    ]);
    let defs = load_items(&json).expect("合法的行为数据必须能加载");
    assert_eq!(
        defs[0].use_effects().collect::<Vec<_>>(),
        vec![BehaviorEffect::Heal { amount: 2.0 }]
    );
    assert!(defs[0].ignored_sources().next().is_none());
}

#[test]
fn a_consumable_without_a_use_behavior_is_rejected() {
    let json = table(&[
        r#"{"id":0,"name":"测试药","desc":"测试用","weight":1.0,"max_stack":5,"quality":"common","kind":{"type":"consumable"}}"#,
    ]);
    let error = load_items(&json).unwrap_err();
    assert!(error.contains("use 行为"), "错误应提到缺少 use 行为：{error}");
}

#[test]
fn an_empty_behavior_is_rejected() {
    let json = table(&[
        r#"{"id":0,"name":"测试药","desc":"测试用","weight":1.0,"max_stack":5,"quality":"common","kind":{"type":"consumable"},"behaviors":[{"trigger":"use","effects":[]}]}"#,
    ]);
    let error = load_items(&json).unwrap_err();
    assert!(error.contains("空行为"), "错误应提到空行为：{error}");
}

#[test]
fn a_use_behavior_on_a_non_consumable_is_rejected() {
    let json = table(&[&material(
        0,
        r#","behaviors":[{"trigger":"use","effects":[{"type":"heal","amount":2.0}]}]"#,
    )]);
    let error = load_items(&json).unwrap_err();
    assert!(error.contains("不是消耗品"), "错误应指出种类不符：{error}");
}

#[test]
fn an_equipped_behavior_on_a_non_equipment_is_rejected() {
    let json = table(&[&material(
        0,
        r#","behaviors":[{"trigger":"equipped","effects":[{"type":"ignore_source","source":"terrain"}]}]"#,
    )]);
    let error = load_items(&json).unwrap_err();
    assert!(error.contains("不是装备"), "错误应指出种类不符：{error}");
}

#[test]
fn an_unknown_behavior_trigger_is_rejected() {
    let json = table(&[
        r#"{"id":0,"name":"测试药","desc":"测试用","weight":1.0,"max_stack":5,"quality":"common","kind":{"type":"consumable"},"behaviors":[{"trigger":"on_hit","effects":[]}]}"#,
    ]);
    let error = load_items(&json).unwrap_err();
    assert!(error.contains("on_hit"), "错误应提到未知触发：{error}");
}

#[test]
fn a_heal_amount_must_be_positive() {
    let json = table(&[
        r#"{"id":0,"name":"测试药","desc":"测试用","weight":1.0,"max_stack":5,"quality":"common","kind":{"type":"consumable"},"behaviors":[{"trigger":"use","effects":[{"type":"heal","amount":0.0}]}]}"#,
    ]);
    let error = load_items(&json).unwrap_err();
    assert!(error.contains("消耗效果"), "错误应提到消耗效果：{error}");
}

#[test]
fn an_equipped_behavior_cannot_heal() {
    let json = table(&[
        r#"{"id":0,"name":"测试护符","desc":"测试用","weight":1.0,"max_stack":1,"quality":"common","kind":{"type":"equipment"},"tags":["ring"],"behaviors":[{"trigger":"equipped","effects":[{"type":"heal","amount":2.0}]}]}"#,
    ]);
    let error = load_items(&json).unwrap_err();
    assert!(error.contains("不支持回复"), "错误应指出效果不配对：{error}");
}

#[test]
fn only_equipment_declares_a_positive_durability() {
    let sword = ItemId(0).def();
    assert_eq!(sword.durability_max, Some(40.0), "锈铁剑应该有耐久上限");
    assert_eq!(ItemId(25).def().durability_max, None, "蘑菇没有耐久");

    for def in items() {
        if let Some(max) = def.durability_max {
            assert!(def.is_equipment(), "{} 不是装备却声明了耐久", def.name);
            assert!(max.is_finite() && max > 0.0, "{} 的耐久上限必须为正", def.name);
        }
    }
}

#[test]
fn a_non_positive_durability_is_rejected() {
    let json = table(&[
        r#"{"id":0,"name":"测试装备","desc":"测试用","weight":1.0,"max_stack":1,"quality":"common","durability_max":0.0,"kind":{"type":"equipment"}}"#,
    ]);
    let error = load_items(&json).unwrap_err();
    assert!(error.contains("耐久"), "错误应提到耐久：{error}");
}

#[test]
fn a_durability_on_a_non_equipment_is_rejected() {
    let json = table(&[&material(5, r#","durability_max":10.0"#)]);
    let error = load_items(&json).unwrap_err();
    assert!(error.contains("耐久"), "错误应提到耐久：{error}");
}

#[test]
fn an_unknown_quality_is_rejected() {
    let json = table(&[
        r#"{"id":0,"name":"测试材料","desc":"测试用","weight":1.0,"max_stack":10,"quality":"mythic","kind":{"type":"material"}}"#,
    ]);
    let error = load_items(&json).unwrap_err();
    assert!(
        error.contains("quality") || error.contains("mythic"),
        "错误应指向未知品质：{error}"
    );
}

#[test]
fn every_item_declares_a_quality() {
    // 品质是蓝图字段（DsnE23）：内容里至少要覆盖到中间档，全部是默认值说明没人填。
    let mut ranks = std::collections::BTreeSet::new();
    for def in items() {
        assert!(!def.quality.label().is_empty(), "{} 的品质没有名字", def.name);
        ranks.insert(def.quality.rank());
    }
    assert!(
        ranks.len() >= 3,
        "物品品质应分布在多个档位，实际：{ranks:?}"
    );
}
