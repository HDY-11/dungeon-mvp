//! 效果的**归属关系**：效果实体挂在谁身上，以及"拥有者没了效果还在不在"。
//!
use crate::rules::Modifier;
use bevy_ecs::prelude::*;

/// 一条**修正**挂在效果实体上（buff/debuff 的载体）。
///
/// 为什么包一层而不是让 `rules::Modifier` 直接 `derive(Component)`：`rules` 是纯函数层
/// （无 `World`、无 bevy 依赖，`LECS2` 的红利），让它认识 ECS 就等于把纯层拖进框架。
/// 这一层包装与 [`IgnoresSource`](crate::effect::IgnoresSource) 同形：
/// **规则层的数据结构 + 效果层的组件壳**。
///
/// 一件效果实体带**一条**修正：`Modifier` 的两个位描述的是"同一项数值的加与乘"，
/// 不同项（攻击 vs 速度）本就是两条独立的修正，各占一个实体最直白。
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct EffectModifier(pub Modifier);

impl EffectModifier {
    pub const fn new(modifier: Modifier) -> Self {
        Self(modifier)
    }
}

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

}
