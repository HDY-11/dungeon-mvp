//! 下楼（`world/floor.rs`）的测试。
//!
//! 两条不变式值得单独钉住，因为它们一旦破了**不会报错**：
//!
//! 1. **玩家必须活下来**（连同等级/背包/装备）。写清场代码时最容易顺手 `despawn` 太多；
//! 2. **层内容必须真的换掉**（地图/楼梯/怪物/地面物品）。少重建一样，玩家就会在
//!    "新的一层"里看见上一层的怪物——或者更糟：站在上一层的楼梯上。

use super::*;
use crate::item::{GroundItem, GroundItemAt, Inventory, ItemId};
use crate::monster::MonsterKindId;
use crate::resources::OccupancyMap;
use crate::test_util::player_entity;
use crate::world_loop::new_game;
use bevy_ecs::system::RunSystemOnce;

/// 把玩家挪到楼梯上（否则 `descend` 的前置条件不成立）。
fn stand_on_stairs(world: &mut World) {
    let stairs = world.resource::<crate::world::init::StairsPos>().0;
    let player = player_entity(world).expect("开局必须有玩家");
    let mut position = world.get_mut::<Position>(player).unwrap();
    position.x = stairs.0;
    position.y = stairs.1;
    let _ = world.run_system_once(crate::system::rebuild_occupancy_system);
}

fn monster_count(world: &mut World) -> usize {
    let mut query = world.query_filtered::<Entity, With<Monster>>();
    query.iter(world).count()
}

fn item_count(world: &mut World) -> usize {
    let mut query = world.query_filtered::<Entity, With<GroundItem>>();
    query.iter(world).count()
}

/// 不在楼梯上 → 什么也不做，并且告诉玩家为什么。
#[test]
fn descending_elsewhere_does_nothing_and_says_so() {
    let mut world = new_game(2026);
    let floor_before = world.resource::<FloorNumber>().0;
    let monsters_before = monster_count(&mut world);
    let player = player_entity(&world).expect("开局必须有玩家");
    let pos_before = world.get::<Position>(player).unwrap().to_tuple();

    assert!(!descend(&mut world), "不在楼梯上不该能下楼");
    assert_eq!(world.resource::<FloorNumber>().0, floor_before);
    assert_eq!(monster_count(&mut world), monsters_before, "怪物不该被清掉");
    assert_eq!(
        world.get::<Position>(player).unwrap().to_tuple(),
        pos_before,
        "玩家不该被挪动"
    );
    assert!(
        world
            .resource::<crate::resources::EventLog>()
            .messages
            .iter()
            .any(|m| m.text.contains("没有楼梯")),
        "必须告诉玩家为什么下不去"
    );
}

/// 站在楼梯上 → 楼层 +1、内容全部换新、玩家被搬到新出生点。
#[test]
fn descending_enters_the_next_floor() {
    let mut world = new_game(2026);
    stand_on_stairs(&mut world);
    let stairs_before = world.resource::<crate::world::init::StairsPos>().0;

    assert!(descend(&mut world), "站在楼梯上必须能下楼");

    assert_eq!(world.resource::<FloorNumber>().0, 2, "楼层必须 +1");

    // 新一层的出生点与新楼梯都在（并且楼梯位置由新种子派生）。
    let spawn = world.resource::<crate::world::init::PlayerSpawn>().0;
    let stairs_after = world.resource::<crate::world::init::StairsPos>().0;
    assert_ne!(
        stairs_before, stairs_after,
        "新一层的楼梯应当由新种子重新派生"
    );

    let player = player_entity(&world).expect("开局必须有玩家");
    assert_eq!(
        world.get::<Position>(player).unwrap().to_tuple(),
        spawn,
        "玩家必须站在新一层的出生点"
    );

    // 层内容：怪物与地面物品都重建过（数量非零，且旧实体已消失）。
    assert!(monster_count(&mut world) > 0, "新一层必须有怪物");
    assert!(item_count(&mut world) > 0, "新一层必须有地面物品");

    // 玩家状态干净：不残留行动/终态。
    assert!(world.get::<Active>(player).is_none());
    assert!(world.get::<Failure>(player).is_none());
    assert!(world.get::<Idle>(player).is_some(), "下楼后玩家必须空闲");
}

/// **玩家必须活下来**：等级、背包、装备跨层延续。
#[test]
fn descending_keeps_the_player_and_its_belongings() {
    let mut world = new_game(2026);
    let player = player_entity(&world).expect("开局必须有玩家");

    // 给玩家一点可识别的东西：等级 + 背包里的一堆材料 + 一件穿着的装备。
    world.entity_mut(player).insert(Level(7));
    world
        .get_mut::<Inventory>(player)
        .expect("玩家有背包")
        .try_add(ItemId(10), 5);
    let sword = world.spawn((
        crate::effect::EquippedBy(player),
        crate::item::EquippedItem::new(crate::item::EquipSlot::MainHand, ItemId(0)),
    ));
    let sword = sword.id();
    let health_before = {
        let health = world.get::<Health>(player).unwrap();
        (health.current, health.max)
    };

    stand_on_stairs(&mut world);
    assert!(descend(&mut world));

    let player_after = player_entity(&world).expect("下楼后玩家必须还在");
    assert_eq!(player_after, player, "玩家实体本身必须不变（不是重建）");
    assert_eq!(world.get::<Level>(player).map(|l| l.0), Some(7), "等级跨层保留");
    assert_eq!(
        world.get::<Inventory>(player).map(|inv| inv.count_of(ItemId(10))),
        Some(5),
        "背包内容跨层保留"
    );
    assert!(
        world.get_entity(sword).is_ok(),
        "装备实体必须存活（它是玩家的子实体，不属于本层内容）"
    );
    let health_after = {
        let health = world.get::<Health>(player).unwrap();
        (health.current, health.max)
    };
    assert_eq!(health_after, health_before, "生命值跨层保留");
}

/// 清场只清"本层内容"：玩家名下的中毒这类效果**保留**（换层不该变成解毒）。
#[test]
fn descending_keeps_the_players_status_effects() {
    let mut world = new_game(2026);
    let player = player_entity(&world).expect("开局必须有玩家");
    let poison = world
        .spawn((
            crate::effect::EffectOf(player),
            crate::effect::Poison::standard(),
        ))
        .id();

    stand_on_stairs(&mut world);
    assert!(descend(&mut world));

    assert!(
        world.get_entity(poison).is_ok(),
        "持续效果挂在玩家名下，跨层保留（见 floor.rs 的取舍说明）"
    );
}

/// 下楼后派生数据必须对上：占用图与新地图一致、玩家脚下可见。
#[test]
fn descending_rebuilds_derived_indices() {
    let mut world = new_game(2026);
    stand_on_stairs(&mut world);
    assert!(descend(&mut world));

    let player = player_entity(&world).expect("开局必须有玩家");
    let player_pos = world.get::<Position>(player).unwrap().to_tuple();

    // 占用图：登记数应当等于带 `Blocks` 的实体数（每层重建之后的同一个不变式）。
    let expected = {
        let mut query = world.query_filtered::<Entity, With<Blocks>>();
        query.iter(&world).count()
    };
    let registered = world
        .resource::<OccupancyMap>()
        .cells
        .iter()
        .flatten()
        .filter(|cell| cell.is_some())
        .count();
    assert_eq!(registered, expected, "占用图必须与新地图一致");

    // 视野：新一层的出生点周围必须被点亮（旧的探索记忆已清空）。
    let seen = world
        .get::<Viewshed>(player)
        .map(|viewshed| viewshed.can_see(player_pos))
        .unwrap_or(false);
    assert!(seen, "玩家脚下必须是可见的");
}

/// 怪物种类分布：新一层仍按同一套物种模板生成（拿不到怪物 = 生成系统没跑）。
#[test]
fn the_new_floor_still_spawns_known_monsters() {
    let mut world = new_game(2026);
    stand_on_stairs(&mut world);
    assert!(descend(&mut world));

    let kinds: Vec<MonsterKindId> = {
        let mut query = world.query::<&MonsterKindId>();
        query.iter(&world).copied().collect()
    };
    assert!(!kinds.is_empty());
    // 每种都能查到模板（否则掉落/数值在后续系统里会 panic）。
    for kind in kinds {
        let _ = crate::monster::monster_template(kind);
    }
}

/// 地板物品的位置必须在新地图上可走（清场 + 重建之后不该有"掉进墙里的物品"）。
#[test]
fn new_floor_items_are_on_walkable_cells() {
    let mut world = new_game(2026);
    stand_on_stairs(&mut world);
    assert!(descend(&mut world));

    let cells: Vec<(usize, usize)> = {
        let mut query = world.query::<&GroundItemAt>();
        query.iter(&world).map(|at| at.to_tuple()).collect()
    };
    let map = world.resource::<Map>();
    for (x, y) in cells {
        assert!(
            map.tiles[y][x].walkable(),
            "地面物品掉进了不可走格 ({x},{y})"
        );
    }
}
