//! 集成测试：投掷装填（auto_equip_throwable）原子语义（G24）
//!
//! 通过 dungeon-tui 的 lib 导出访问 src/throw.rs（src/lib.rs: pub mod throw）。

use bevy_ecs::prelude::World;
use dungeon_core::{Equipment, EventLog, ITEM_STONE, ITEM_WOOD_SHIELD, Inventory, Player};
use dungeon_tui::throw::auto_equip_throwable;
use dungeon_world::setup_world;

/// 从世界取玩家实体
fn player_entity(world: &World) -> bevy_ecs::prelude::Entity {
    let mut q = world
        .try_query::<(bevy_ecs::prelude::Entity, &Player)>()
        .unwrap();
    q.iter(world).next().unwrap().0
}

/// 把背包塞满 36 格（33 个不同 ID + 3 个堆叠上限 1 的重复）
fn fill_backpack(world: &mut World) {
    let p = player_entity(world);
    let mut inv = world.get_mut::<Inventory>(p).unwrap();
    for id in 0..=32u32 {
        inv.add(id as usize, 1);
    }
    // 33 格已占；ID 0/1/2 的 max_stack=1，再加 3 个占满 36 格
    inv.add(0, 1);
    inv.add(1, 1);
    inv.add(2, 1);
    assert_eq!(inv.stacks.len(), 36, "测试前提：背包应恰好满 36 格");
}

/// G24 场景 A：背包满 + 副手木盾 → 装填放弃，木盾不丢失
#[test]
fn test_auto_equip_full_backpack_keeps_offhand() {
    let mut world = setup_world();
    let p = player_entity(&world);
    // 副手放木盾
    world.get_mut::<Equipment>(p).unwrap().off_hand =
        Some(dungeon_core::ItemStack::new(ITEM_WOOD_SHIELD, 1));
    fill_backpack(&mut world);

    auto_equip_throwable(&mut world);

    // 木盾必须仍在副手（原子语义：不静默丢失）
    let off = world
        .get::<Equipment>(p)
        .unwrap()
        .off_hand
        .as_ref()
        .unwrap();
    assert_eq!(off.item_id, ITEM_WOOD_SHIELD, "背包满时副手装备不应丢失");
    // 事件日志有明确反馈
    let log = world.resource::<EventLog>();
    assert!(
        log.messages.iter().any(|m| m.text.contains("背包已满")),
        "应推送背包已满提示: {:?}",
        log.messages
            .iter()
            .map(|m| m.text.clone())
            .collect::<Vec<_>>()
    );
}

/// G24 场景 B：背包有空位 + 副手木盾 → 装填成功，木盾回背包，副手变石子
#[test]
fn test_auto_equip_swaps_offhand_into_backpack() {
    let mut world = setup_world();
    let p = player_entity(&world);
    world.get_mut::<Equipment>(p).unwrap().off_hand =
        Some(dungeon_core::ItemStack::new(ITEM_WOOD_SHIELD, 1));
    world.get_mut::<Inventory>(p).unwrap().add(ITEM_STONE, 5);

    auto_equip_throwable(&mut world);

    let eq = world.get::<Equipment>(p).unwrap();
    assert_eq!(
        eq.off_hand.as_ref().unwrap().item_id,
        ITEM_STONE,
        "副手应装填石子"
    );
    let inv = world.get::<Inventory>(p).unwrap();
    assert!(
        inv.stacks.iter().any(|s| s.item_id == ITEM_WOOD_SHIELD),
        "木盾应放回背包"
    );
}

/// G24 场景 C：副手已有石子 → 直接跳过，不搬动
#[test]
fn test_auto_equip_skips_when_throwable_already() {
    let mut world = setup_world();
    let p = player_entity(&world);
    world.get_mut::<Equipment>(p).unwrap().off_hand =
        Some(dungeon_core::ItemStack::new(ITEM_STONE, 3));
    world.get_mut::<Inventory>(p).unwrap().add(ITEM_STONE, 2);

    auto_equip_throwable(&mut world);

    let eq = world.get::<Equipment>(p).unwrap();
    assert_eq!(
        eq.off_hand.as_ref().unwrap().count,
        3,
        "副手石子数量不应变化"
    );
}
