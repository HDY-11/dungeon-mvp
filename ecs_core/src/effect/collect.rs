//! **收桶**（求值五步的第 ② 步）：把"actor 身上有什么、脚下这一格有什么"收成
//! 规则层能直接吃的两份列表 + 一份忽略表（DESIGN DsnE10 / DsnE12）。
//!
//! 为什么单独一层：规则（`rules::modifier`）是纯函数、不碰 `World`，而"装备/状态/地形
//! 分别带来什么"只能从世界里读出来（ISSUES ECS28）。本模块是那个**唯一的接缝**：
//! 系统层调 [`EffectContext::collect`]，拿到纯数据；纯函数只吃结果。
//!
//! # 用哪一格？
//!
//! 用 actor **当前所站的格子**，不是"要去的那一格"。理由来自 AV 的固化时机：
//!
//! - AV 在**生成期**算好并写进 `ActionTimer`，执行期只读剩余值（`SpeedRule` 的注释）；
//! - 玩家的 `Move` 在生成期知道方向，但 AI 的 `Chase`/`Flee`/`Wander` **不知道**
//!   ——方向是执行期在 `execute_*_system` 里跑 A* / 随机才定下来的；
//! - 所以"目标格"口径要求把 AI 的 AV 计算整体推到执行期，那会推翻"AV 挂载时固化"。
//!
//! 取脚下这一格则始终是生成期确定的事实。代价是**踏入减速格的这一步本身不变慢**
//! （下一步才开始慢）；若将来要"进入即变慢"，那是一次独立的换道，不是这里顺手改。

use crate::effect::cell::{
    CellAttributes, CellEffects, CellMoveCost, cell_attributes, cell_speed_modifier,
};
use crate::effect::{IgnoresSource, OwnedEffects};
use crate::map::{MAP_HEIGHT, MAP_WIDTH, Map};
use crate::rules::{EffectSource, Modifier};
use bevy_ecs::prelude::*;
use bevy_ecs::system::SystemParam;

/// 求值五步第 ② 步的产物：两个桶 + 一份忽略表，全是纯数据。
///
/// 字段顺序与 [`crate::action::entity::SpeedModifiers`] 的两个桶一一对应，
/// 调用方直接构造：`SpeedModifiers { actor: &c.actor, cell: &c.cell }`。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CollectedModifiers {
    /// actor 自带的修正（装备 / 状态）。**本轮恒空**：最小闭环的 actor 侧是"无视"，不是修正。
    pub actor: Vec<Modifier>,
    /// 脚下这一格带来的修正（`Terrain` 桶）。
    pub cell: Vec<Modifier>,
    /// 被无视的来源桶：装备/状态效果里声明的"无视某类"。
    pub ignored: Vec<EffectSource>,
}

impl CollectedModifiers {
    /// 是否什么都没收到（等价于 H8 的空桶口径）。
    pub fn is_empty(&self) -> bool {
        self.actor.is_empty() && self.cell.is_empty() && self.ignored.is_empty()
    }
}

/// 效果/格子的**只读世界视图**：一张 `SystemParam` 顶替四个查询参数。
///
/// 单独抽出来是为了让每个生成系统只多一行 `effects.collect(..)`，
/// 而不是各自重复"查关系 → 查子实体 → 查格索引 → 查地形"四步
/// （那四步一旦漏一步，失败模式是**静默地少算一个修正**）。
#[derive(SystemParam)]
pub struct EffectContext<'w, 's> {
    map: Res<'w, Map>,
    cell_effects: Res<'w, CellEffects>,
    owned: Query<'w, 's, &'static OwnedEffects>,
    ignores: Query<'w, 's, &'static IgnoresSource>,
    cell_costs: Query<'w, 's, &'static CellMoveCost>,
}

impl EffectContext<'_, '_> {
    /// 统一读取入口（系统层版本）：一格的属性，越界视为不可走。
    pub fn cell_attributes(&self, x: usize, y: usize) -> CellAttributes {
        if x >= MAP_WIDTH || y >= MAP_HEIGHT {
            return CellAttributes::OUT_OF_BOUNDS;
        }
        let props = self.map.tiles[y][x].props();
        let effect = self
            .cell_effects
            .entity_at(x, y)
            .and_then(|entity| self.cell_costs.get(entity).ok());
        cell_attributes(props, effect)
    }

    /// 脚下这一格的属性。
    pub fn attributes_at_position(&self, position: (usize, usize)) -> CellAttributes {
        self.cell_attributes(position.0, position.1)
    }

    /// actor 名下的效果声明的"无视来源"（去重；求值第 ③ 步的 `ignored`）。
    ///
    /// 只读 `EffectOf` 那一组（`OwnedEffects`）：装备走 `EquippedBy`，
    /// 因此"装备提供的无视"要由装备的 owner 系统把它转成一条效果再收进来——
    /// 这条缝故意留着不做（物品系统未迁移，见 REFACTOR §2.2 R2）。
    fn ignored_sources(&self, actor: Entity) -> Vec<EffectSource> {
        let mut ignored = Vec::new();
        let Ok(owned) = self.owned.get(actor) else {
            return ignored;
        };
        for effect in owned.iter() {
            if let Ok(ignore) = self.ignores.get(effect)
                && !ignored.contains(&ignore.0)
            {
                ignored.push(ignore.0);
            }
        }
        ignored
    }

    /// 收桶：脚下这一格的修正 + actor 自带的修正 + 忽略表。
    pub fn collect(&self, actor: Entity, cell: (usize, usize)) -> CollectedModifiers {
        let attributes = self.attributes_at_position(cell);
        let cell_modifiers = cell_speed_modifier(&attributes).into_iter().collect();
        CollectedModifiers {
            actor: Vec::new(),
            cell: cell_modifiers,
            ignored: self.ignored_sources(actor),
        }
    }
}
