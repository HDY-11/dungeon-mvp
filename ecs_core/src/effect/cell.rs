//! 格子侧：**每格一个效果实体** + 格索引 + 格子属性的统一读取入口（DsnE12 定案 ①、DsnE13 第 2–3 条）。
//!
//! # 为什么格子效果要有自己的位置组件（不复用 `Position`）
//!
//! `rebuild_occupancy_system` 的查询是 `Query<(Entity, &Position, Option<&Stairs>)>`
//! ——**任何**带 `Position` 的实体都会被登记进占用图。若格子效果（泥潭、冻结块）
//! 也用 `Position`，它们会立刻变成"占着格子"的实体：玩家无法走进一块**本来就能走**
//! 的泥潭，而且没有任何报错。
//!
//! 这是本项目第三次遇到同一类问题（`ChildOf` 的候选查询、`Position` 的占用图、
//! 现在这个）：**复用通用组件 = 加入别人写的查询**。所以格子效果用自己的
//! [`CellEffectAt`]，占用图看不见它（与 `action/ownership.rs` 用 `ActionOf` 取代 `ChildOf` 同源）。
//!
//! # 为什么效果实体不放在 `resources.rs`
//!
//! `OccupancyMap` 是全局游戏事实，住在 `resources.rs`；[`CellEffects`] 是**效果子系统的派生索引**
//! ——只由本模块的维护系统写、只由本模块的读取入口读。放在效果模块旁边，
//! "谁维护它"在目录结构上就一目了然（改它的人不可能漏看维护系统）。
//!
//! # 一格一个效果实体
//!
//! 索引是 `Option<Entity>`（照 `OccupancyMap`），所以**一格的持续性效果必须合成一个实体**
//! （同格"冻结 + 泥潭"= 一个实体带两个 payload 组件）。同格登记第二个效果会让第一个
//! **不可达**（读不到、也删不掉），因此 [`rebuild_cell_effects_system`] 遇到冲突会告警，
//! 而不是静默覆盖。

use crate::map::{MAP_HEIGHT, MAP_WIDTH, Tile, TileProps};
use bevy_ecs::prelude::*;

/// 效果实体占据的格子。**专用组件**，理由见模块文档（复用 `Position` 会进占用图）。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellEffectAt {
    pub x: usize,
    pub y: usize,
}

impl CellEffectAt {
    pub fn new(x: usize, y: usize) -> Self {
        Self { x, y }
    }

    pub fn to_tuple(self) -> (usize, usize) {
        (self.x, self.y)
    }
}

/// 格索引：某格挂着哪个效果实体（`None` = 该格没有额外效果）。
///
/// 形状照 `OccupancyMap`：`[[Option<Entity>; 宽]; 高]` + 一个每轮重建的维护系统。
/// 每轮重建而不是增量维护，理由与占用图相同：**没有增量更新，就没有漏改导致的幽灵条目**。
#[derive(Resource)]
pub struct CellEffects {
    cells: [[Option<Entity>; MAP_WIDTH]; MAP_HEIGHT],
}

impl CellEffects {
    pub fn new() -> Self {
        Self {
            cells: [[None; MAP_WIDTH]; MAP_HEIGHT],
        }
    }

    /// 该格的效果实体；越界返回 `None`。
    pub fn entity_at(&self, x: usize, y: usize) -> Option<Entity> {
        if x >= MAP_WIDTH || y >= MAP_HEIGHT {
            return None;
        }
        self.cells[y][x]
    }

    /// 登记一个效果实体；返回该格**原先**登记的那个（`Some` 说明发生了覆盖）。
    ///
    /// 返回旧值而不是静默覆盖：覆盖会让原效果实体"读不到也删不掉"，
    /// 属于静默数据丢失，调用方要能看见。
    pub fn set(&mut self, x: usize, y: usize, entity: Entity) -> Option<Entity> {
        if x >= MAP_WIDTH || y >= MAP_HEIGHT {
            return None;
        }
        let previous = self.cells[y][x];
        self.cells[y][x] = Some(entity);
        previous
    }

    /// 清空某个格子。
    pub fn clear_at(&mut self, x: usize, y: usize) {
        if x < MAP_WIDTH && y < MAP_HEIGHT {
            self.cells[y][x] = None;
        }
    }

    /// 清空整张索引（维护系统的第一步）。
    pub fn clear(&mut self) {
        self.cells = [[None; MAP_WIDTH]; MAP_HEIGHT];
    }

    /// 当前登记的效果实体数量（测试与调试面板用）。
    pub fn count(&self) -> usize {
        self.cells.iter().flatten().filter(|cell| cell.is_some()).count()
    }
}

impl Default for CellEffects {
    fn default() -> Self {
        Self::new()
    }
}

/// 一格上的移动代价效果（H11 最小闭环的**格子侧**）。
///
/// `move_cost` 是"处于这一格"的代价倍率：`1.0` = 无影响，`2.0` = 花两倍 AV。
/// 它贡献一个 `EffectSource::Terrain` 桶的速度乘区 `1/move_cost`
/// （速度减半 → AV 翻倍），因此"无视地形减速"只要丢掉 `Terrain` 桶即可，
/// 规则层不需要知道"泥潭"这个词。
///
/// **这是本轮唯一的格子效果**：DsnE12 明确"不定义具体效果类型"，H11 只破例这一对
/// （配 actor 侧的 [`crate::effect::IgnoresSource`]），用来验证机制而不是玩内容。
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct CellMoveCost {
    pub move_cost: f64,
}

impl CellMoveCost {
    pub const fn new(move_cost: f64) -> Self {
        Self { move_cost }
    }
}

/// 一格的全部属性 = **地形属性表** + **该格效果**的结果（DsnE13 第 3 条的"统一读取入口"）。
///
/// 与 DsnE9 的"返回因子分解"同源：读取方拿到的是一个可解释的结构，
/// 而不是"某个函数内部查了表又查了实体"。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CellAttributes {
    pub walkable: bool,
    pub blocks_vision: bool,
    /// 处于/进入这一格的代价倍率：`地形表 × 该格效果`。`1.0` = 无影响。
    pub move_cost: f64,
    /// 该格有没有效果实体（区分"地形本身的代价"与"额外效果"）。
    pub has_effect: bool,
}

impl CellAttributes {
    /// 越界格的属性：不可走、挡视线、代价中性。
    pub const OUT_OF_BOUNDS: Self = Self {
        walkable: false,
        blocks_vision: true,
        move_cost: 1.0,
        has_effect: false,
    };
}

/// **统一读取入口（纯函数版）**：地形属性 + 该格效果 → 一格属性。
///
/// 取 `&TileProps` 而不是 `Tile`，是为了让"读取方真的消费了 `move_cost` 这一列"
/// 可以被单测钉住：测试可以造一个 `move_cost: 2.5` 的合成属性，
/// 不必改动 `TILE_PROPS`（那属于 GAME.md 的数值，当前全部是 1.0 占位）。
pub fn cell_attributes(props: &TileProps, effect: Option<&CellMoveCost>) -> CellAttributes {
    CellAttributes {
        walkable: props.walkable,
        blocks_vision: props.blocks_vision,
        move_cost: props.move_cost * effect.map_or(1.0, |effect| effect.move_cost),
        has_effect: effect.is_some(),
    }
}

/// 把"代价倍率"折算成**速度乘区**，并挡住内容错误。
///
/// 代价 `≤ 0`、`NaN`、`inf` 都是内容错误：`1.0 / 0.0 = inf`，而 `clamp_speed`
/// 会把 inf 夹到速度上限——结果"零代价格"变成**加速格**，且无任何报错。
/// 这里按中性 `1.0` 处理，把错误留给维护系统的告警（可见优先于静默）。
pub fn speed_multiplier_for(move_cost: f64) -> f64 {
    if move_cost.is_finite() && move_cost > 0.0 {
        1.0 / move_cost
    } else {
        1.0
    }
}

/// 该格效果折算出的速度修正（`Terrain` 桶）；代价中性时返回 `None`。
///
/// 返回 `None` 而不是 `Modifier::scaled(.., 1.0)`：中性修正留在列表里会让
/// "空桶 == 旧口径"这条不变量（H8 的验收）失效——列表不空，但值相同，
/// 于是"接上求值侧零行为变化"就不再是**结构上**成立的，只能靠断言值相等。
pub fn cell_speed_modifier(attributes: &CellAttributes) -> Option<crate::rules::Modifier> {
    let multiplier = speed_multiplier_for(attributes.move_cost);
    if multiplier == 1.0 {
        return None;
    }
    Some(crate::rules::Modifier::scaled(
        crate::rules::EffectSource::Terrain,
        multiplier,
    ))
}

/// 重建格索引：所有带 [`CellEffectAt`] 的实体各登记一格，**每轮重建**。
///
/// 同格冲突（两个效果实体登记同一格）会告警：后者覆盖前者，被覆盖的实体
/// 从此不可达（读不到、删不掉），属静默数据丢失，必须让它可见。
pub fn rebuild_cell_effects_system(
    effects: Query<(Entity, &CellEffectAt)>,
    mut index: ResMut<CellEffects>,
) {
    index.clear();
    for (entity, at) in effects.iter() {
        if at.x >= MAP_WIDTH || at.y >= MAP_HEIGHT {
            log::warn!(
                "格子效果 entity={entity:?} 的坐标越界: ({}, {})，跳过登记",
                at.x,
                at.y
            );
            continue;
        }
        if let Some(previous) = index.set(at.x, at.y, entity) {
            log::warn!(
                "格子 ({}, {}) 上已有效果 entity={previous:?}，被 entity={entity:?} 覆盖——\
                 一格的持续效果必须合成一个实体（见 effect/cell.rs 模块文档）",
                at.x,
                at.y
            );
        }
    }
}

/// 该格的地形（越界返回 `None`）。
pub fn tile_at(map: &crate::map::Map, x: usize, y: usize) -> Option<Tile> {
    if x >= MAP_WIDTH || y >= MAP_HEIGHT {
        return None;
    }
    Some(map.tiles[y][x])
}

#[cfg(test)]
#[path = "cell_tests.rs"]
mod tests;
