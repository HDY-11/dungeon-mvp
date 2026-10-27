//! `super`（`item`）的测试：捡起 → 装备 → 效果生效这条链。

use super::*;
use crate::effect::OwnedEffects;
use crate::test_util::{spawn_test_player, test_world};
use bevy_ecs::system::RunSystemOnce;

/// 玩家站在物品上 → 物品离地、挂上 `EquippedBy`、并把授予的效果实例化到名下。
#[test]
fn picking_up_equips_the_item_and_instantiates_its_effect() {
    let mut world = test_world();
    let player = spawn_test_player(&mut world, (4, 4));
    let item = world
        .spawn((
            GroundItemAt::new(4, 4),
            GroundItem::new(ItemKind::TrailBoots),
        ))
        .id();

    world.run_system_once(pickup_system).expect("系统可运行");

    // 1. 地上的物品消失。
    assert!(
        world.get_entity(item).is_err(),
        "捡起后地面物品必须被移除（否则每轮都会重复触发拾取）"
    );

    // 2. 关系：装备挂在玩家名下（`EquippedBy` → `EquippedItems`）。
    let equipped: Vec<Entity> = world
        .get::<crate::effect::EquippedItems>(player)
        .map(|items| items.iter().collect())
        .unwrap_or_default();
    assert_eq!(equipped.len(), 1, "玩家名下必须有这一件装备");
    assert_eq!(
        world.get::<GroundItem>(equipped[0]).map(|item| item.kind),
        Some(ItemKind::TrailBoots)
    );

    // 3. 效果：装备授予的"无视地形"被实例化成**效果实体**（求值层只认这一组）。
    let effects: Vec<Entity> = world
        .get::<OwnedEffects>(player)
        .map(|owned| owned.iter().collect())
        .unwrap_or_default();
    let ignores: Vec<EffectSource> = effects
        .iter()
        .filter_map(|effect| world.get::<IgnoresSource>(*effect).map(|ignore| ignore.0))
        .collect();
    assert_eq!(
        ignores,
        vec![EffectSource::Terrain],
        "踏水靴必须授予「无视地形」这一条效果"
    );
}

/// 站在别处不捡：拾取必须按格子判定，不是"场上有物品就归我"。
#[test]
fn items_on_other_cells_are_not_picked_up() {
    let mut world = test_world();
    let player = spawn_test_player(&mut world, (4, 4));
    let item = world
        .spawn((
            GroundItemAt::new(9, 9),
            GroundItem::new(ItemKind::TrailBoots),
        ))
        .id();

    world.run_system_once(pickup_system).expect("系统可运行");

    assert!(world.get_entity(item).is_ok(), "远处的物品不得被捡走");
    assert!(
        world.get::<crate::effect::EquippedItems>(player).is_none(),
        "没有拾取就不该有装备关系（空列表表现为组件缺失）"
    );
}

/// 同一件物品不会被两轮重复拾取（幂等性：由"先 despawn 再 spawn"保证）。
#[test]
fn pickup_is_idempotent_across_rounds() {
    let mut world = test_world();
    let player = spawn_test_player(&mut world, (4, 4));
    world.spawn((
        GroundItemAt::new(4, 4),
        GroundItem::new(ItemKind::TrailBoots),
    ));

    world.run_system_once(pickup_system).expect("系统可运行");
    world.run_system_once(pickup_system).expect("系统可运行");

    let equipped = world
        .get::<crate::effect::EquippedItems>(player)
        .map(|items| items.len())
        .unwrap_or_default();
    assert_eq!(equipped, 1, "跑两轮不得装备两次");
}
