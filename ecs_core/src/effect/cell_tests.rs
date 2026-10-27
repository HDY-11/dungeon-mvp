//! `super`（`effect::cell`）的测试：格索引行为 + 统一读取入口 + "格子效果不进占用图"。

use super::*;
use crate::map::{TILE_PROPS, TileProps};

/// `cell_attributes` 真的**消费**地形表的 `move_cost` 列（而不是只把它摆在结构里）。
///
/// 用合成的 `TileProps` 而不是改 `TILE_PROPS`：后者是 GAME.md 的数值，当前全部是
/// `1.0` 占位（"只接线不填值"），拿它做断言无法区分"读表"与"写死 1.0"。
fn props_with_cost(move_cost: f64) -> TileProps {
    TileProps {
        tile: Tile::Floor,
        id: Tile::Floor.id(),
        glyph: '.',
        walkable: true,
        blocks_vision: false,
        move_cost,
    }
}

#[test]
fn cell_attributes_consume_the_terrain_move_cost_column() {
    let plain = cell_attributes(&props_with_cost(1.0), None);
    assert_eq!(plain.move_cost, 1.0);
    assert!(!plain.has_effect);

    // 地形自带代价（未接线到 GAME 数值，但机制必须已经读它）。
    let swamp = cell_attributes(&props_with_cost(2.0), None);
    assert_eq!(swamp.move_cost, 2.0, "地形表的 move_cost 没被读进来");

    // 地形 × 该格效果：两者都在 `Terrain` 桶里，代价相乘。
    let mud = CellMoveCost::new(3.0);
    let both = cell_attributes(&props_with_cost(2.0), Some(&mud));
    assert_eq!(both.move_cost, 6.0, "地形与格子效果的代价必须相乘");
    assert!(both.has_effect);

    // 通行与视线仍然来自表，不受代价影响。
    assert!(both.walkable);
    assert!(!both.blocks_vision);
}

/// 代价倍率 → 速度乘区：**双向**都成立（>1 变慢，<1 变快），并挡住内容错误。
#[test]
fn speed_multiplier_is_the_reciprocal_of_cost() {
    assert_eq!(speed_multiplier_for(1.0), 1.0);
    assert_eq!(speed_multiplier_for(2.0), 0.5, "两倍代价 = 一半速度");
    assert_eq!(speed_multiplier_for(0.5), 2.0, "半价格 = 两倍速度");
    // 内容错误按中性处理（否则 0 会除出 inf 并被 clamp_speed 夹成加速格）。
    for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert_eq!(
            speed_multiplier_for(bad),
            1.0,
            "非法代价 {bad} 必须按中性处理，不能变成加速/减速"
        );
    }
}

/// 中性代价**不产生修正器**：这是"空桶 == 旧口径"这条不变量成立的前提。
#[test]
fn neutral_cost_produces_no_modifier() {
    let neutral = cell_attributes(&props_with_cost(1.0), None);
    assert_eq!(cell_speed_modifier(&neutral), None);

    let slow = cell_attributes(&props_with_cost(2.0), None);
    let modifier = cell_speed_modifier(&slow).expect("两倍代价必须产生一条修正");
    assert_eq!(modifier.source, crate::rules::EffectSource::Terrain);
    assert_eq!(modifier.multiplier, 0.5);
    assert_eq!(modifier.base_delta, 0.0);
}

/// 地形自带的移动代价：**逐格钉死**（C5 起只有浅水非中性）。
///
/// 这条测试的身份变过一次，值得记下来：H4/H9 时它叫
/// `terrain_move_cost_is_still_a_neutral_placeholder`，断言"全表都是 1.0"——那是为了守住
/// "只接线、不填值"。C5 给浅水一个真实代价之后，那条断言就变成了**错误的设计**
/// （它会把内容决策当成违规）。现在改成"逐格列出应有值"：任何改动都得显式改这张表。
#[test]
fn terrain_move_costs_are_pinned_per_tile() {
    use crate::map::Tile;
    let expected: [(Tile, f64); 11] = [
        (Tile::Wall, 1.0),
        (Tile::Floor, 1.0),
        (Tile::ShallowWater, 1.5),
        (Tile::DeepWater, 1.0),
        (Tile::Stalactite, 1.0),
        (Tile::Mycelium, 1.0),
        (Tile::FungalPatch, 1.0),
        (Tile::HangingVine, 1.0),
        (Tile::Sand, 1.0),
        (Tile::Seagrass, 1.0),
        (Tile::CoralReef, 1.0),
    ];
    assert_eq!(expected.len(), TILE_PROPS.len(), "表行数变了，请同步这张期望表");
    for (tile, cost) in expected {
        assert_eq!(
            tile.props().move_cost,
            cost,
            "{tile:?} 的移动代价变了：它属 GAME.md 的数值口径（Gm10），改之前先改文档"
        );
    }
}

#[test]
fn cell_index_stores_looks_up_and_clears() {
    let mut index = CellEffects::new();
    assert_eq!(index.entity_at(3, 4), None);
    assert_eq!(index.count(), 0);

    let mut world = World::new();
    let effect = world.spawn_empty().id();
    assert_eq!(index.set(3, 4, effect), None, "首次登记不该有旧值");
    assert_eq!(index.entity_at(3, 4), Some(effect));
    assert_eq!(index.entity_at(4, 3), None, "坐标不能写反");
    assert_eq!(index.count(), 1);

    let second = world.spawn_empty().id();
    assert_eq!(
        index.set(3, 4, second),
        Some(effect),
        "覆盖必须把旧值交回调用方（静默覆盖 = 静默数据丢失）"
    );
    assert_eq!(index.entity_at(3, 4), Some(second));

    index.clear_at(3, 4);
    assert_eq!(index.entity_at(3, 4), None);
    assert_eq!(index.count(), 0);
}

/// 越界读写是**无操作**而不是 panic：地图尺寸是编译期常量，越界只可能来自错误调用。
#[test]
fn cell_index_ignores_out_of_bounds() {
    let mut index = CellEffects::new();
    let mut world = World::new();
    let effect = world.spawn_empty().id();
    assert_eq!(index.set(MAP_WIDTH, 0, effect), None);
    assert_eq!(index.set(0, MAP_HEIGHT, effect), None);
    assert_eq!(index.entity_at(MAP_WIDTH, 0), None);
    assert_eq!(index.count(), 0);
}

/// 维护系统：每轮重建 → 实体移动后旧格必须被清掉（不留幽灵条目）。
#[test]
fn rebuild_registers_and_clears_stale_entries() {
    let mut world = crate::test_util::test_world();
    let effect = world.spawn(CellEffectAt::new(5, 6)).id();

    world.run_schedule(crate::schedule::CoreSettleSchedule);
    {
        let index = world.resource::<CellEffects>();
        assert_eq!(index.entity_at(5, 6), Some(effect));
        assert_eq!(index.count(), 1);
    }

    // 效果搬家：下一轮重建后旧格必须干净。
    world.entity_mut(effect).insert(CellEffectAt::new(7, 8));
    world.run_schedule(crate::schedule::CoreSettleSchedule);
    let index = world.resource::<CellEffects>();
    assert_eq!(index.entity_at(5, 6), None, "旧格残留了幽灵条目");
    assert_eq!(index.entity_at(7, 8), Some(effect));

    // 效果被移除：索引随之清空。
    world.despawn(effect);
    world.run_schedule(crate::schedule::CoreSettleSchedule);
    assert_eq!(world.resource::<CellEffects>().count(), 0);
}

/// **格子效果不进占用图**——这条守着 `CellEffectAt` 为什么必须是专用组件。
///
/// 若哪天有人图省事把格子效果的位置改成 `Position`，`rebuild_occupancy_system`
/// 会把它当成"站着东西的格子"，玩家从此走不进一块本来能走的泥潭，
/// 而且没有任何报错。本测试会在那一刻失败。
#[test]
fn cell_effects_do_not_enter_the_occupancy_map() {
    let mut world = crate::test_util::test_world();
    world.spawn(CellEffectAt::new(5, 6));
    world.run_schedule(crate::schedule::CoreSettleSchedule);

    let occupancy = world.resource::<crate::resources::OccupancyMap>();
    assert!(
        !occupancy.is_occupied(5, 6),
        "格子效果不得占用格子：它用的是 CellEffectAt，不是 Position"
    );
}
