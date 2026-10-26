//! 效果的**归属关系**：效果实体挂在谁身上，以及"拥有者没了效果还在不在"。
//!
//! # 为什么是两个关系类型，而不是一个
//!
//! "拥有者 `despawn` 时效果要不要跟着消失"这个问题的答案**对不同效果是相反的**
//! （ISSUES ECS42）：
//!
//! | 效果 | 期望 | 用哪个关系 |
//! |---|---|---|
//! | 状态效果（中毒、加速）、技能授予的持续效果 | 人没了效果也没了 | [`EffectOf`] / [`OwnedEffects`]（**带** `linked_spawn`） |
//! | 装备 | 人没了**掉出来**给玩家捡 | [`EquippedBy`] / [`EquippedItems`]（**不带** `linked_spawn`） |
//!
//! 如果只用一个关系类型，就必须在某个 owner 系统里按种类分支决定"这次要不要级联"——
//! 而那正是本项目已经踩过两次的失败模式：**把语义写进条件分支，忘了一条就静默出错**。
//! 用一个装备演示一下后果：怪物死了，装备跟着尸体一起消失，玩家看不到任何报错，
//! 只在"击杀带装备的怪物"这一条路径上少掉战利品。
//!
//! 分成两个类型之后，"级联还是不级联"变成**挂在哪个组件上**这个一眼可见的事实，
//! 编译器与类型系统全程参与（照 `action/ownership.rs` 的做法）。
//!
//! # 为什么不用 `ChildOf`
//!
//! 同 `action/ownership.rs`：`ChildOf` 是通用层级关系，任何"查 `Children` 或查 `ChildOf`"
//! 的系统都会看见效果实体（ISSUES ECS30），且一个父只有一份 `Children` 列表，
//! 无法同时容纳行动子实体与效果子实体两组。专用关系让"效果不参与行动仲裁"成为类型保证。
//!
//! # 装备的"掉在地上"
//!
//! 实测（[`tests::equipment_survives_its_owner`]）：拥有者 despawn 后**装备实体存活**，
//! 而 `EquippedBy` 被关系机制**自动清掉**——正是一个"无主、待认领"的状态。
//! 于是"掉在地上"= owner 系统给这个无主实体补上位置表达，不需要手写"解除挂载"的代码。
//!
//! 对比同场景下的 `linked_spawn`：效果实体会在拥有者消失的同一瞬间被删掉。
//! 若装备也用它，玩家在"击杀带装备的怪物"这条路径上**永远**拿不到战利品，
//! 而且没有任何报错（ISSUES ECS42 的失败模式）。

use bevy_ecs::prelude::*;

/// 效果实体 → 它的拥有者 actor。
///
/// 读法：`EffectOf(actor)` 挂在**效果实体**上；`actor` 身上会出现 [`OwnedEffects`]。
#[derive(Component, Clone, PartialEq, Eq, Debug)]
#[relationship(relationship_target = OwnedEffects)]
pub struct EffectOf(pub Entity);

impl EffectOf {
    /// 这条效果属于哪个 actor。
    #[inline]
    pub fn parent(&self) -> Entity {
        self.0
    }
}

/// actor → 它名下的全部**持续效果**（由关系机制自动维护，**不要手动插入**）。
///
/// `linked_spawn`：actor 被 despawn 时，名下效果一并 despawn。
/// 这与"效果必须可寻靶、由恰好一个 owner 系统终结"（DsnE12 第 4 条）不冲突：
/// 级联处理的是"拥有者消失"，owner 系统处理的是"效果自己过期/被解除"。
#[derive(Component, Clone, PartialEq, Eq, Debug, Default)]
#[relationship_target(relationship = EffectOf, linked_spawn)]
pub struct OwnedEffects(Vec<Entity>);

impl OwnedEffects {
    /// 名下的效果实体（顺序 = 插入顺序）。
    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = Entity> + '_ {
        self.0.iter().copied()
    }

    /// 名下效果实体的数量。
    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// 名下是否没有效果。
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// 装备实体 → 装备它的 actor。
///
/// 与 [`EffectOf`] **结构相同、级联语义相反**——这不是重复，是有意让两个语义各占一个类型。
#[derive(Component, Clone, PartialEq, Eq, Debug)]
#[relationship(relationship_target = EquippedItems)]
pub struct EquippedBy(pub Entity);

impl EquippedBy {
    /// 谁装备着它。
    #[inline]
    pub fn owner(&self) -> Entity {
        self.0
    }
}

/// actor → 它装备的东西（由关系机制自动维护，**不要手动插入**）。
///
/// **故意不带 `linked_spawn`**：拥有者 despawn 时装备实体**存活**，由本组件的
/// owner 系统把"装备中"改写成"掉落在地上"（DsnE12 定案 ①、ISSUES ECS42）。
#[derive(Component, Clone, PartialEq, Eq, Debug, Default)]
#[relationship_target(relationship = EquippedBy)]
pub struct EquippedItems(Vec<Entity>);

impl EquippedItems {
    /// 装备着的实体（顺序 = 插入顺序）。
    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = Entity> + '_ {
        self.0.iter().copied()
    }

    /// 装备件数。
    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// 是否什么都没装备。
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 持续效果：双向链接 + 拥有者消失时**级联回收**。
    #[test]
    fn effect_cascades_with_its_owner() {
        let mut world = World::new();
        let actor = world.spawn_empty().id();
        let effect = world.spawn(EffectOf(actor)).id();

        assert_eq!(
            world.get::<EffectOf>(effect).map(EffectOf::parent),
            Some(actor)
        );
        let owned = world
            .get::<OwnedEffects>(actor)
            .expect("插入 EffectOf 后 actor 必须有 OwnedEffects");
        assert_eq!(owned.iter().collect::<Vec<_>>(), vec![effect]);

        world.despawn(actor);
        assert!(
            world.get_entity(effect).is_err(),
            "linked_spawn：拥有者 despawn 必须级联回收持续效果"
        );
    }

    /// 装备：双向链接**相同**，级联行为**相反**——这条测试就是 ECS42 的可执行版本。
    #[test]
    fn equipment_survives_its_owner() {
        let mut world = World::new();
        let actor = world.spawn_empty().id();
        let boots = world.spawn(EquippedBy(actor)).id();

        assert_eq!(
            world.get::<EquippedBy>(boots).map(EquippedBy::owner),
            Some(actor)
        );
        assert_eq!(
            world
                .get::<EquippedItems>(actor)
                .map(|items| items.iter().collect::<Vec<_>>()),
            Some(vec![boots])
        );

        world.despawn(actor);
        assert!(
            world.get_entity(boots).is_ok(),
            "装备**不能**随拥有者消失，否则战利品会静默丢失（ISSUES ECS42）"
        );
        // 关系机制会把指向已消失目标的 `EquippedBy` 清掉，装备因此变成**无主实体**
        // ——这正好是"掉在地上、等人来捡"需要的状态，owner 系统只需补位置。
        assert_eq!(
            world.get::<EquippedBy>(boots),
            None,
            "拥有者消失后关系必须被自动清掉，不能留下指向死实体的引用"
        );
    }

    /// 两套关系**互不可见**：效果不会出现在装备列表里，反之亦然。
    ///
    /// 这是"一个 actor 同时有状态效果和装备"能各查各的前提。
    #[test]
    fn the_two_relationships_do_not_see_each_other() {
        let mut world = World::new();
        let actor = world.spawn_empty().id();
        let effect = world.spawn(EffectOf(actor)).id();
        let boots = world.spawn(EquippedBy(actor)).id();

        assert_eq!(
            world
                .get::<OwnedEffects>(actor)
                .map(|owned| owned.iter().collect::<Vec<_>>()),
            Some(vec![effect])
        );
        assert_eq!(
            world
                .get::<EquippedItems>(actor)
                .map(|items| items.iter().collect::<Vec<_>>()),
            Some(vec![boots])
        );
    }

    /// 两个关系都是**单值**的：重插等于搬家，不是"同时属于两个拥有者"。
    #[test]
    fn both_relationships_move_rather_than_duplicate() {
        let mut world = World::new();
        let first = world.spawn_empty().id();
        let second = world.spawn_empty().id();

        let effect = world.spawn(EffectOf(first)).id();
        world.entity_mut(effect).insert(EffectOf(second));
        assert!(
            world
                .get::<OwnedEffects>(first)
                .is_none_or(OwnedEffects::is_empty),
            "搬家后旧拥有者名下不得再列出这条效果"
        );

        let boots = world.spawn(EquippedBy(first)).id();
        world.entity_mut(boots).insert(EquippedBy(second));
        assert!(
            world
                .get::<EquippedItems>(first)
                .is_none_or(EquippedItems::is_empty),
            "搬家后旧拥有者名下不得再列出这件装备"
        );
    }
}
