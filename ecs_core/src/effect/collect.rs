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
use crate::effect::{EffectModifier, IgnoresSource, OwnedEffects};
use crate::item::{Container, Item, ItemTag, WeaponProgress};
use crate::map::{MAP_HEIGHT, MAP_WIDTH, Map};
use crate::rules::{EffectSource, Modifier, StatId};
use bevy_ecs::prelude::*;
use bevy_ecs::system::SystemParam;

/// 求值五步第 ② 步的产物：两个桶 + 一份忽略表，全是纯数据。
///
/// 字段顺序与 [`crate::action::entity::SpeedModifiers`] 的两个桶一一对应，
/// 调用方直接构造：`SpeedModifiers { actor: &c.actor, cell: &c.cell }`。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CollectedModifiers {
    /// actor 自带的修正：**装备**（按 `EquippedBy` 关系读注册表）+ 状态效果（`EffectOf` 实体）。
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

/// 效果/格子的**只读世界视图**：一张 `SystemParam` 顶替五个查询参数。
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
    equipped: Query<'w, 's, &'static Container>,
    equipped_items: Query<'w, 's, &'static Item>,
    /// 武器实体的额外成长修正（DsnE24 阶段 ②）：等级带来的攻击加成在这里合成。
    weapon_progress: Query<'w, 's, &'static WeaponProgress>,
    /// 效果实体身上的修正（将来的 buff：`(EffectOf(actor), Modifier)`）。
    ///
    /// 现在**没有生产者**（中毒只按 AV 掉血，不给修正），但读取口先就位：
    /// 于是"加一条 buff"只写数据，不必再改求值侧（LECS22 的"加第 N 个改几处"= 0）。
    effect_modifiers: Query<'w, 's, &'static EffectModifier>,
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
    /// 两个来源都要读，因为它们走的是**两条不同**的归属关系（ECS42 定案）：
    /// - `EffectOf` 实体（状态效果）：自己带 [`IgnoresSource`]；
    /// - 装备：无视声明写在**注册表那一行**的行为数据里（`Equipped` 触发的
    ///   `IgnoreSource` 效果，DsnE22 阶段 ③），所以这里查的是物品定义，
    ///   而不是"装备名下有个效果实体"。
    fn ignored_sources(&self, actor: Entity) -> Vec<EffectSource> {
        let mut ignored = Vec::new();
        if let Ok(owned) = self.owned.get(actor) {
            for effect in owned.iter() {
                if let Ok(ignore) = self.ignores.get(effect)
                    && !ignored.contains(&ignore.0)
                {
                    ignored.push(ignore.0);
                }
            }
        }
        for def in self.equipped_defs(actor) {
            for source in def.ignored_sources() {
                if !ignored.contains(&source) {
                    ignored.push(source);
                }
            }
        }
        ignored
    }

    /// actor **身体槽位**里那些物品的定义（DsnE18：装备就是身体容器里的物品）。
    ///
    /// 走"身体容器 → 槽位 → 物品实体 → 注册表"这条链，不读任何副本：
    /// 于是"卸下装备"只是把物品移出槽位，加成自动消失，不需要清理派生状态。
    fn equipped_defs(&self, actor: Entity) -> Vec<&'static crate::item::ItemDef> {
        let Ok(body) = self.equipped.get(actor) else {
            return Vec::new();
        };
        body.iter()
            .filter_map(|(_, occupant)| {
                self.equipped_items
                    .get(occupant)
                    .ok()
                    .map(|item| item.def())
            })
            .collect()
    }

    /// actor 自带的修正（装备 + 状态效果）。**不含脚下这一格**。
    ///
    /// 攻击/防御这类"与格子无关"的数值用它；速度用 [`Self::collect`]。
    pub fn collect_actor(&self, actor: Entity) -> Vec<Modifier> {
        let mut mods = Vec::new();
        if let Ok(owned) = self.owned.get(actor) {
            for effect in owned.iter() {
                if let Ok(modifier) = self.effect_modifiers.get(effect) {
                    mods.push(modifier.0);
                }
            }
        }
        for def in self.equipped_defs(actor) {
            mods.extend_from_slice(def.modifiers());
        }
        // 武器等级（DsnE24 阶段 ②）：等级加成不是蓝图静态修正，按实例等级现场合成一条
        // 装备来源的加法修正——求值侧仍然只看见 `Modifier`，没有"武器特例"分支。
        if let Ok(body) = self.equipped.get(actor) {
            for (_, occupant) in body.iter() {
                let Ok(item) = self.equipped_items.get(occupant) else {
                    continue;
                };
                let def = item.def();
                let Some(growth) = def.weapon_growth else {
                    continue;
                };
                let level = self
                    .weapon_progress
                    .get(occupant)
                    .map(|progress| progress.level)
                    .unwrap_or(1);
                let bonus = growth.attack_bonus(level);
                if bonus > 0.0 {
                    mods.push(Modifier::delta(StatId::ATTACK, EffectSource::Equipment, bonus));
                }
            }
        }
        mods
    }

    /// 主手武器实体（用来加经验）；没装备武器时 `None`。
    pub fn main_hand_weapon_entity(&self, actor: Entity) -> Option<Entity> {
        let body = self.equipped.get(actor).ok()?;
        body.iter().find_map(|(_, occupant)| {
            let item = self.equipped_items.get(occupant).ok()?;
            item.def()
                .tags
                .contains(&ItemTag::Weapon)
                .then_some(occupant)
        })
    }

    /// 主手武器的定义；没装备武器时 `None`。
    ///
    /// 主手槽只接受带 `Weapon` 标签的物品（DsnE18 的 `BODY` 过滤表），所以这里
    /// 按标签找即可，不需要再查槽位下标——"哪一把是主手"由身体容器的槽位唯一决定。
    pub fn main_hand_weapon(&self, actor: Entity) -> Option<&'static crate::item::ItemDef> {
        self.equipped_defs(actor)
            .into_iter()
            .find(|def| def.tags.contains(&ItemTag::Weapon))
    }

    /// 主手武器的攻击耗时（毫秒）；没装备武器时 `None`（用空手基准）。
    ///
    /// GAME.md Gm7「武器攻速」：`AV = 武器 speed ÷ 攻击速度`。武器覆盖的是**基础耗时**，
    /// 不是速度——所以石锤与匕首的区别在于"每一击要多久"，而不是"速度倍率多少"。
    pub fn main_hand_weapon_speed(&self, actor: Entity) -> Option<f64> {
        self.main_hand_weapon(actor)
            .and_then(|def| def.weapon_speed_ms)
    }

    /// 主手武器的力量加成上限 S_m（DsnE24）；空手时由调用方用
    /// [`crate::balance::UNARMED_STRENGTH_CAP`] 兜底。
    ///
    /// 这是**基础上限 + 等级加成**：武器升级会放宽能吃的力量（成长曲线管这件事）。
    pub fn main_hand_strength_cap(&self, actor: Entity) -> Option<f64> {
        let weapon = self.main_hand_weapon_entity(actor)?;
        let def = self.equipped_items.get(weapon).ok()?.def();
        let base = def.strength_cap?;
        let level = self
            .weapon_progress
            .get(weapon)
            .map(|progress| progress.level)
            .unwrap_or(1);
        let bonus = def
            .weapon_growth
            .map(|growth| growth.strength_cap_bonus(level))
            .unwrap_or(0.0);
        Some(f64::from(base) + bonus)
    }

    /// 收桶：脚下这一格的修正 + actor 自带的修正 + 忽略表。
    pub fn collect(&self, actor: Entity, cell: (usize, usize)) -> CollectedModifiers {
        let attributes = self.attributes_at_position(cell);
        let cell_modifiers = cell_speed_modifier(&attributes).into_iter().collect();
        CollectedModifiers {
            actor: self.collect_actor(actor),
            cell: cell_modifiers,
            ignored: self.ignored_sources(actor),
        }
    }
}
