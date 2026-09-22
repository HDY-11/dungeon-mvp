//! 行动归属关系：**行动子实体挂在谁身上**。
//!
//! # 为什么不用 `ChildOf`（Phase H / ISSUES ECS30 + ECS41）
//!
//! 行动实体是 actor 的瞬态子实体（REFACTOR.md §3.6）。最初用的是 bevy 的通用层级
//! 关系 `ChildOf`，但实测暴露了两个问题：
//!
//! 1. **`ChildOf` 的候选查询不看实体类型**：`action_arbitration_system` 的候选查询是
//!    `Query<(Entity, &ActionPriority, &ChildOf), (With<Candidate>, Without<ActiveAction>)>`，
//!    落选者一律 `despawn`。今天不误伤只因为 filter 恰好含
//!    `With<Candidate> + &ActionPriority`——这是**巧合式安全**，任何新挂在 actor 名下的
//!    子实体只要带了这两个组件之一就会被静默删除（ECS30）。
//! 2. **`ChildOf` 一个父只有一份 `Children` 列表**：无法把"行动子实体"与
//!    "效果子实体"分成两个分组，于是每种新子实体都要靠 filter 小心避开别人的查询。
//!
//! 改成专用关系类型后：
//!
//! - **"只有行动实体参与行动仲裁"是类型保证**：别的子实体类型上根本没有 [`ActionOf`]，
//!   过滤条件写错也匹配不到；
//! - 效果/装备将来用各自的 `EffectOf`（DESIGN DsnE12），互不干扰；
//! - 关系是 `bevy_ecs` 的公开派生机制（`bevy_ecs-0.16.1/src/relationship/mod.rs`），
//!   `Children` 那套自动反向索引由派生保证，不需要手写 unsafe。
//!
//! # 级联销毁：故意保留
//!
//! `linked_spawn` 让**父实体 despawn 时自动 despawn 子实体**（与 bevy 的 `Children`
//! 同一语义），因此这里没有关掉它。理由：action 实体的**唯一**回收者是
//! `action_completion_system`（DESIGN DsnE12 第 4 条），而 actor 死亡时 completion
//! 不会再被调用——没有级联就是**子实体泄漏**。这条与"装备不能级联"（ISSUES ECS42）
//! 不矛盾：那条要的正是"拥有者死了效果要活下来"，所以装备**不该**用本条的关系。

use bevy_ecs::prelude::*;

/// 行动实体 → 它的 actor。
///
/// 读法：`ActionOf(actor)` 挂在**行动实体**上；`actor` 身上会出现 [`ActionChildren`]。
#[derive(Component, Clone, PartialEq, Eq, Debug)]
#[relationship(relationship_target = ActionChildren)]
pub struct ActionOf(pub Entity);

impl ActionOf {
    /// 这个行动属于哪个 actor。
    #[inline]
    pub fn parent(&self) -> Entity {
        self.0
    }
}

/// actor → 它名下的全部行动实体（由关系机制自动维护，**不要手动插入**）。
///
/// `linked_spawn`：actor 被 despawn 时，它名下的行动实体一并 despawn——
/// 这是"actor 死亡不留残留 action 子实体"的保证。
#[derive(Component, Clone, PartialEq, Eq, Debug, Default)]
#[relationship_target(relationship = ActionOf, linked_spawn)]
pub struct ActionChildren(Vec<Entity>);

impl ActionChildren {
    /// 名下的行动实体（顺序 = 插入顺序）。
    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = Entity> + '_ {
        self.0.iter().copied()
    }

    /// 名下行动实体的数量。
    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// 名下是否没有行动实体。
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 归属关系要能**真的被创建**（LESSONS LECS22 的配套纪律：新组件必须配一条
    /// "真能被创建"的测试，否则 `With<A>` 写了却没人 spawn `A` 也不会有报错）。
    #[test]
    fn relationship_links_both_directions_and_cascades_on_despawn() {
        let mut world = World::new();
        let actor = world.spawn_empty().id();
        let action = world.spawn(ActionOf(actor)).id();

        // 正向：行动实体知道自己属于谁。
        assert_eq!(
            world.get::<ActionOf>(action).map(ActionOf::parent),
            Some(actor)
        );

        // 反向：actor 身上自动出现 `ActionChildren`（派生维护，无需手插）。
        let children = world
            .get::<ActionChildren>(actor)
            .expect("插入 ActionOf 后 actor 必须有 ActionChildren");
        assert_eq!(children.len(), 1);
        assert_eq!(children.iter().collect::<Vec<_>>(), vec![action]);

        // 级联：actor 没了，行动实体一并回收（避免残留子实体）。
        world.despawn(actor);
        assert!(
            world.get_entity(action).is_err(),
            "linked_spawn：父实体 despawn 必须级联回收行动实体"
        );

        // 反向：移除关系后 actor 不再挂这个行动。
        let actor = world.spawn_empty().id();
        let action = world.spawn(ActionOf(actor)).id();
        world.entity_mut(action).remove::<ActionOf>();
        assert!(
            world
                .get::<ActionChildren>(actor)
                .is_none_or(ActionChildren::is_empty),
            "移除 ActionOf 后 actor 名下不得再列这个行动"
        );
    }

    /// **本关系是排他的**：一个行动实体只能属于一个 actor。
    ///
    /// 这条不是"我们希望如此"，而是 bevy 关系机制的语义：`ActionOf` 是
    /// 单值关系，重插等于搬家。写下来是为了让"行动实体同时属于两个 actor"
    /// 这种设想在文档层面就被否掉（那会让 completion 的回收归属不确定）。
    #[test]
    fn an_action_belongs_to_exactly_one_actor() {
        let mut world = World::new();
        let first = world.spawn_empty().id();
        let second = world.spawn_empty().id();
        let action = world.spawn(ActionOf(first)).id();

        world.entity_mut(action).insert(ActionOf(second));

        assert_eq!(
            world.get::<ActionOf>(action).map(ActionOf::parent),
            Some(second),
            "重插 ActionOf 是搬家，不是同时属于两个 actor"
        );
        assert!(
            world
                .get::<ActionChildren>(first)
                .is_none_or(ActionChildren::is_empty),
            "搬家之后旧 actor 名下不得再列出这个行动"
        );
        assert_eq!(
            world
                .get::<ActionChildren>(second)
                .map(ActionChildren::len),
            Some(1)
        );
    }
}
