//! 世界生命周期测试
//!
//! 覆盖：GameSave 存档/读档回环、descend 下楼数据保持

use super::*;
use dungeon_core::{
    self as core, resources::*,
    Map,
    Position, Stats, Inventory, Equipment,
    PlayerClass, Skills,
    Stairs,
};
use bevy_ecs::prelude::*;
use crate::GameSave;

// ──────────────────────────────────────────────
// 辅助：从背包取出一件物品并装备到武器槽
// ──────────────────────────────────────────────
fn equip_first_weapon(world: &mut World) {
    let player = core::ops::player_entity(world).unwrap();
    let item_id = {
        let inv = world.get::<Inventory>(player).unwrap();
        inv.stacks.iter().find(|s| s.item_id == 0).map(|s| s.item_id)
    };
    if let Some(id) = item_id {
        let stack = {
            let mut inv = world.get_mut::<Inventory>(player).unwrap();
            let idx = inv.stacks.iter().position(|s| s.item_id == id).unwrap();
            inv.stacks.remove(idx)
        };
        let mut eq = world.get_mut::<Equipment>(player).unwrap();
        eq.main_hand = Some(stack);
    }
}

// ──────────────────────────────────────────────
// 测试：存档/读档回环 — 验证核心数据完整性
// ──────────────────────────────────────────────
#[test]
fn test_save_restore_roundtrip() {
    let mut world = setup_world();
    let player = core::ops::player_entity(&world).unwrap();

    // 给玩家一些独特的状态供验证
    {
        let mut stats = world.get_mut::<Stats>(player).unwrap();
        stats.exp = 12;
        stats.hp = stats.max_hp / 2;
        stats.mp = stats.max_mp / 3;
    }
    {
        let mut inv = world.get_mut::<Inventory>(player).unwrap();
        inv.add(0, 1);  // 锈铁剑
        inv.add(1, 1);  // 木盾
        inv.add(10, 3); // 生物血肉 x3
    }
    // 装备武器（通过分步方式避免并发借用）
    equip_first_weapon(&mut world);

    let floor_before = world.resource::<FloorNumber>().0;
    let pos_before = *world.get::<Position>(player).unwrap();
    let (stairs_pos_before, explored_count_before) = {
        let mut sq = world.query::<(&Stairs, &Position)>();
        let spos = sq.iter(&world).next().map(|(_, p)| (p.x, p.y)).unwrap();
        let mem = world.resource::<MapMemory>();
        let explored = mem.explored.iter().flatten().filter(|&&b| b).count();
        (spos, explored)
    };

    // ── capture → restore 到新世界 ──
    let save = GameSave::capture(&world);

    let mut restored = setup_world();
    save.restore(&mut restored);

    // ── 验证楼层 ──
    assert_eq!(restored.resource::<FloorNumber>().0, floor_before);

    // ── 验证地图 tiles（抽样检查，全量4800格太慢） ──
    let orig_map = world.resource::<Map>();
    let rest_map = restored.resource::<Map>();
    // 抽查四角 + 中心列
    for &(x, y) in &[(0, 0), (79, 0), (0, 59), (79, 59), (40, 0), (40, 30), (0, 30)] {
        assert_eq!(
            rest_map.tiles[y][x], orig_map.tiles[y][x],
            "Tile 不匹配 at ({}, {})", x, y,
        );
    }

    // ── 验证楼梯位置 ──
    {
        let mut sq = restored.query::<(&Stairs, &Position)>();
        let (sx, sy) = sq.iter(&restored).next().map(|(_, p)| (p.x, p.y)).unwrap();
        assert_eq!(sx, stairs_pos_before.0);
        assert_eq!(sy, stairs_pos_before.1);
    }

    // ── 验证探索记忆 ──
    {
        let mem = restored.resource::<MapMemory>();
        let explored = mem.explored.iter().flatten().filter(|&&b| b).count();
        assert_eq!(explored, explored_count_before);
    }

    // ── 验证玩家位置 ──
    let rest_player = core::ops::player_entity(&restored).unwrap();
    let rest_pos = *restored.get::<Position>(rest_player).unwrap();
    assert_eq!(rest_pos.x, pos_before.x);
    assert_eq!(rest_pos.y, pos_before.y);

    // ── 验证 Stats ──
    let rest_stats = restored.get::<Stats>(rest_player).unwrap();
    assert_eq!(rest_stats.exp, 12);
    assert_eq!(rest_stats.hp, rest_stats.max_hp / 2);
    assert_eq!(rest_stats.mp, rest_stats.max_mp / 3);

    // ── 验证 Inventory（剑已装备到武器槽，不在背包中） ──
    let rest_inv = restored.get::<Inventory>(rest_player).unwrap();
    assert!(
        rest_inv.stacks.iter().any(|s| s.item_id == 1),
        "应持有木盾"
    );
    assert!(
        rest_inv.stacks.iter().any(|s| s.item_id == 10 && s.count == 3),
        "应持有 3 个生物血肉"
    );
    assert!(
        !rest_inv.stacks.iter().any(|s| s.item_id == 0),
        "锈铁剑已装备，不应在背包中"
    );

    // ── 验证 Equipment ──
    let rest_eq = restored.get::<Equipment>(rest_player).unwrap();
    assert!(rest_eq.main_hand.is_some(), "应装备武器");
    assert_eq!(rest_eq.main_hand.as_ref().unwrap().item_id, 0, "应装备锈铁剑");
}

// ──────────────────────────────────────────────
// 测试：下楼后玩家数据保持
// ──────────────────────────────────────────────
#[test]
fn test_descend_preserves_data() {
    let mut world = setup_world();
    let player = core::ops::player_entity(&world).unwrap();

    // 给玩家背包添加物品
    {
        let mut inv = world.get_mut::<Inventory>(player).unwrap();
        inv.add(0, 1);  // 锈铁剑
        inv.add(3, 1);  // 攻击戒指
    }
    // 装备武器
    equip_first_weapon(&mut world);

    let floor_before = world.resource::<FloorNumber>().0;
    let stats_before = world.get::<Stats>(player).unwrap().clone();
    let inv_before = world.get::<Inventory>(player).unwrap().stacks.clone();
    let eq_before = world.get::<Equipment>(player).unwrap().clone();
    let pc_before = world.get::<PlayerClass>(player).unwrap().clone();

    // 下楼
    descend(&mut world);

    let floor_after = world.resource::<FloorNumber>().0;
    assert_eq!(floor_after, floor_before + 1, "楼层应 +1");

    let player_after = core::ops::player_entity(&world).unwrap();
    let stats_after = world.get::<Stats>(player_after).unwrap();
    let inv_after = world.get::<Inventory>(player_after).unwrap();
    let eq_after = world.get::<Equipment>(player_after).unwrap();
    let pc_after = world.get::<PlayerClass>(player_after).unwrap();

    // 验证 Stats 不变
    assert_eq!(stats_after.level, stats_before.level);
    assert_eq!(stats_after.hp, stats_before.hp);
    assert_eq!(stats_after.exp, stats_before.exp);

    // 验证 Inventory 不变
    assert_eq!(inv_after.stacks.len(), inv_before.len());
    for s in &inv_before {
        assert!(
            inv_after.stacks.iter().any(|a| a.item_id == s.item_id && a.count == s.count),
            "物品 id={} count={} 应保持",
            s.item_id, s.count,
        );
    }

    // 验证 Equipment 不变
    assert_eq!(
        eq_after.main_hand.as_ref().map(|s| s.item_id),
        eq_before.main_hand.as_ref().map(|s| s.item_id),
    );

    // 验证 PlayerClass 不变
    assert_eq!(*pc_after, pc_before);

    // 验证 Skills 由 PlayerClass 正确推导
    let skills_after = world.get::<Skills>(player_after).unwrap();
    assert_eq!(skills_after.list.len(), pc_after.skills().len());
    for (a, b) in skills_after.list.iter().zip(pc_after.skills().iter()) {
        assert_eq!(a.name, b.name);
    }
}
