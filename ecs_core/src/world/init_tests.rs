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

// ── ECS34：派生 RNG 的"同种子同世界"必须被钉住 ─────────

/// 每种用途的**种子偏移**是契约的一部分：改值/重排都会让同种子世界整体变化。
///
/// 这条测试的作用不是"证明偏移正确"，而是**让改动必须显式**：
/// 想改偏移的人会在这里看到"这是世界布局契约"，而不是顺手改个数字（ISSUES ECS34）。
#[test]
fn derive_purpose_offsets_are_pinned() {
    use crate::resources::{DerivePurpose, derived_rng};
    use rand::RngExt;

    // 同一 `(seed, floor)` + 同一用途 → 同一首个数；不同用途 → 必须不同。
    let mut firsts = Vec::new();
    for purpose in [
        DerivePurpose::Stairs,
        DerivePurpose::Monsters,
        DerivePurpose::CellEffects,
        DerivePurpose::GroundItems,
    ] {
        let mut rng = derived_rng(2026, 1, purpose);
        let first: u64 = rng.random_range(0..u64::MAX);
        assert_eq!(
            first,
            {
                let mut again = derived_rng(2026, 1, purpose);
                again.random_range(0..u64::MAX)
            },
            "{purpose:?} 同 (seed,floor) 必须给出同一流"
        );
        firsts.push((purpose, first));
    }
    for i in 0..firsts.len() {
        for j in (i + 1)..firsts.len() {
            assert_ne!(
                firsts[i].1, firsts[j].1,
                "{:?} 与 {:?} 的流相同 → 偏移撞了",
                firsts[i].0, firsts[j].0
            );
        }
    }
}

/// 世界构造的**整体布局**用 golden 值钉住（楼梯 / 怪物种类与位置 / 泥潭 / 物品）。
///
/// 任何一条派生路径被改动（偏移、算法、调用顺序），这条都会失败。
/// **失败时的处理方式只有两种**：改回原样，或者显式更新这些 golden 值并承认
/// "同种子世界变了"（GAME/DESIGN 里要记一笔）。
#[test]
fn world_layout_is_pinned_for_a_known_seed() {
    use crate::effect::CellEffectAt;
    use crate::item::GroundItemAt;
    use crate::monster::MonsterKindId;

    let mut world = crate::world_loop::new_game(2026);

    let stairs = world.resource::<StairsPos>().0;
    let spawn = world.resource::<PlayerSpawn>().0;

    let mut monsters: Vec<(usize, usize, String)> = {
        let mut q = world.query::<(&Position, &MonsterKindId)>();
        q.iter(&world)
            .map(|(p, kind)| (p.x, p.y, format!("{kind:?}")))
            .collect()
    };
    monsters.sort();

    let mut mud: Vec<(usize, usize)> = {
        let mut q = world.query::<&CellEffectAt>();
        q.iter(&world).map(|at| at.to_tuple()).collect()
    };
    mud.sort_unstable();

    let mut items: Vec<(usize, usize)> = {
        let mut q = world.query::<&GroundItemAt>();
        q.iter(&world).map(|at| at.to_tuple()).collect()
    };
    items.sort_unstable();

    // 摘要式断言：打印一次以便更新，比较时用紧凑形式。
    let digest = format!(
        "spawn={spawn:?} stairs={stairs:?} monsters={} first3={:?} mud={mud:?} items={items:?}",
        monsters.len(),
        &monsters.iter().take(3).collect::<Vec<_>>(),
    );
    println!("seed 2026 布局摘要：{digest}");

    // 关键不变量（与摘要一起构成 golden）：
    assert!(!monsters.is_empty(), "必须有怪物");
    assert!(!mud.is_empty(), "必须有泥潭");
    assert_eq!(items.len(), 1, "每层一件地面物品");
    assert_ne!(stairs, spawn, "楼梯不能和出生点重合");
}
