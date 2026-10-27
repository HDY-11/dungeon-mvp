//! `super`（`world::init`）的测试：**内容生成**（C2 泥潭）的确定性与放置规则。
//!
//! 通过 `init.rs` 末尾的 `#[cfg(test)] #[path = "init_tests.rs"] mod tests;` 引入。

use super::*;
use crate::effect::{CellAttributes, CellEffectAt, CellEffects, EffectContext};

/// 所有格子效果实体的坐标（排序后，便于直接比较两次运行）。
fn cell_effect_cells(world: &mut World) -> Vec<(usize, usize)> {
    let mut query = world.query::<&CellEffectAt>();
    let mut cells: Vec<(usize, usize)> = query.iter(world).map(|at| at.to_tuple()).collect();
    cells.sort_unstable();
    cells
}

/// 找一格"可走、且没有效果"的普通地板，用于对照。
fn plain_walkable_cell(world: &mut World) -> (usize, usize) {
    let mud = cell_effect_cells(world);
    let map = world.resource::<Map>();
    for (y, row) in map.tiles.iter().enumerate() {
        for (x, tile) in row.iter().enumerate() {
            if tile.walkable() && !mud.contains(&(x, y)) {
                return (x, y);
            }
        }
    }
    panic!("地图上必须存在一格没有效果的可走格");
}

/// 同种子两次开局，泥潭落点逐格相同（内容生成也走 `MapSeed` 派生，与地图同源）。
#[test]
fn mud_placement_is_deterministic_for_the_same_seed() {
    let mut first = crate::world_loop::new_game(2026);
    let mut second = crate::world_loop::new_game(2026);

    let a = cell_effect_cells(&mut first);
    let b = cell_effect_cells(&mut second);
    assert_eq!(a, b, "同种子的泥潭落点必须逐格相同");
    assert!(!a.is_empty(), "F1 应当至少有一格泥潭（否则这条内容等于没上线）");
}

/// 放置规则：数量有界、同格不重复、避开出生点与楼梯、只落在可走格。
#[test]
fn mud_respects_placement_rules() {
    let mut world = crate::world_loop::new_game(2026);
    let spawn = world.resource::<PlayerSpawn>().0;
    let stairs = world.resource::<StairsPos>().0;
    let cells = cell_effect_cells(&mut world);

    assert!(cells.len() <= MUD_PATCHES, "泥潭数量不得超过上限");

    let mut unique = cells.clone();
    unique.dedup();
    assert_eq!(unique.len(), cells.len(), "同一格不得登记两个效果");

    for &(x, y) in &cells {
        assert_ne!((x, y), spawn, "泥潭不得盖住玩家出生点");
        assert_ne!((x, y), stairs, "泥潭不得盖住楼梯");
        assert!(
            world.resource::<Map>().tiles[y][x].walkable(),
            "泥潭 ({x},{y}) 落在了不可走格上"
        );
    }
}

/// **初始化末尾就要把格索引建好**：否则开局那一帧的泥潭是不可见的
/// （`CellEffects` 是派生数据，`init` 与每轮结算各维护一次）。
#[test]
fn cell_effect_index_is_populated_right_after_init() {
    let mut world = crate::world_loop::new_game(2026);
    let cells = cell_effect_cells(&mut world);
    let indexed = world.resource::<CellEffects>().count();
    assert_eq!(indexed, cells.len(), "开局后格索引必须与实体一一对应");
    assert!(indexed > 0);
}

/// 泥潭的代价必须**经统一读取入口**读出来，而不是被某处特判。
#[test]
fn mud_and_plain_cells_report_their_cost_through_the_unified_entry() {
    let mut world = crate::world_loop::new_game(2026);
    let mud = cell_effect_cells(&mut world)[0];
    let plain = plain_walkable_cell(&mut world);

    let mut system = bevy_ecs::system::SystemState::<EffectContext>::new(&mut world);
    let context = system.get_mut(&mut world);

    let mud_attrs: CellAttributes = context.cell_attributes(mud.0, mud.1);
    assert!(mud_attrs.has_effect, "泥潭格必须报告「有效果」");
    assert_eq!(mud_attrs.move_cost, MUD_MOVE_COST);
    assert!(mud_attrs.walkable, "泥潭仍然可以走进去（它只是更贵）");

    let plain_attrs = context.cell_attributes(plain.0, plain.1);
    assert!(!plain_attrs.has_effect);
    assert_eq!(plain_attrs.move_cost, 1.0, "普通地板代价必须是中性");
}
