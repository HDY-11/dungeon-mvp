//! **落地阶段**（`ActionPhase::Execute`）：把校验阶段定下的结果写进世界。
//!
//! # 它只有一件事可做
//!
//! 判定全部在 `ActionPhase::Validate`（[`crate::action::validation`]）：到点这一轮，
//! 校验器已经把"落在哪 / 打谁"定案，并**预留**了目标格。所以这里只有三种动作：
//!
//! | 校验器留下的 | 落地做什么 |
//! |---|---|
//! | [`MoveTarget`] | 写 `Position`，并腾出旧格（预留时目标格已经写上 actor） |
//! | [`AttackTarget`] | 写 `AttackIntentEvent`（伤害仍归结算链路） |
//! | [`CastTarget`] | 写 `SpellIntentEvent`（扣蓝/效果仍归结算链路，DsnE26） |
//! | 什么都没留 | 原地——仍然算这一步做完（`Wait`，以及被挡的 `Wander`/`Chase`/`Flee`） |
//!
//! # 为什么是**一个**系统而不是六个
//!
//! 判定被抽走之后，每个行动的落地动作完全同构：写位置 / 发意图 / 什么都不做。
//! 再按行动拆成六份就是六份相同的代码，而"新增一个行动忘了执行器"这个失败模式
//! （旧设计每个行动一个执行器）也随之消失。
//!
//! 扩展点因此在**校验器**那一侧：加一个行动 = 加生成器 + 加校验器。
//! 如果新行动连落地动作都不同（例如需要别的组件写入），那是本文件该长出新形状的信号，
//! 不是把它塞回某个校验器里的信号。
//!
//! # 为什么单独一个文件
//!
//! **「谁写 `Position`」必须是结构而不是约定**（ISSUES ECS45：四条移动路径各写一份，
//! 修了两次都没修净）。本文件是全项目唯一写 `Position` 的地方之一（另一个是生成期
//! `world/init.rs` 的出生点），`scripts/docs_check.py` 用白名单把这条钉成门禁——
//! 白名单窄才有意义，所以落地系统不能和大堆生成/仲裁代码躺在同一个文件里。

use crate::action::entity::{ActionEvents, ActiveAction};
use crate::action::validation::{AttackTarget, CastTarget, MoveTarget};
use crate::action::ownership::ActionOf;
use crate::components::{Position, Ready};
use crate::events::{AttackIntentEvent, SpellIntentEvent};
use crate::resources::OccupancyMap;
use crate::spell::CastSpell;
use bevy_ecs::prelude::*;

/// 落地器的查询：**到点**的行动 + 校验阶段留下的定案。
///
/// `Option` 本身就是"没有定案"的表达：校验器只对 `Move` 写 `MoveTarget`、只对
/// 攻击写 `AttackTarget`、只对施法写 `CastTarget`，别的行动（`Wait`）什么都不写——
/// 那个 `None` 就是"原地完成"。`CastSpell` 与 `CastTarget` 成对出现：前者是"放什么"，
/// 后者是"对谁放"。
/// 抽成别名同时解决 `clippy::type_complexity`，也让这件**契约**有了一个可引用的名字。
pub(crate) type ReadyDecision = (
    Entity,
    &'static ActionOf,
    Option<&'static MoveTarget>,
    Option<&'static AttackTarget>,
    Option<&'static CastTarget>,
    Option<&'static CastSpell>,
);

/// 落地：把校验阶段定下的落点/目标写进世界。
///
/// 它**不判定**：没有 `MoveTarget` 就没有移动，哪怕目标格此刻空着也不会自己走一步
/// （`validation_tests::settle_never_invents_a_step_without_a_decision` 钉住这条）。
pub fn settle_action_system(
    actions: Query<ReadyDecision, (With<ActiveAction>, With<Ready>)>,
    mut positions: Query<&mut Position>,
    mut occupancy: ResMut<OccupancyMap>,
    mut intents: EventWriter<AttackIntentEvent>,
    mut spells: EventWriter<SpellIntentEvent>,
    mut events: ActionEvents,
) {
    for (action, child_of, move_target, attack_target, cast_target, cast_spell) in &actions {
        let actor = child_of.parent();

        if let Some(MoveTarget(to)) = move_target {
            let Ok(mut position) = positions.get_mut(actor) else {
                events.fail(action, actor);
                continue;
            };
            let from = *position;
            // 腾出旧格：校验阶段预留时已经把 actor 写进目标格，所以这里只清旧的。
            // 只清"确实是自己的那一格"——占用图在整轮里都是权威，不能盲清。
            if occupancy.entity_at(from.x, from.y) == Some(actor) {
                occupancy.clear_at(from.x, from.y);
            }
            position.x = to.x;
            position.y = to.y;
            log::debug!(
                "落地: actor={actor:?} ({},{}) → ({},{})",
                from.x,
                from.y,
                to.x,
                to.y
            );
        }

        if let Some(AttackTarget(victim)) = attack_target {
            intents.write(AttackIntentEvent {
                attacker: actor,
                target: *victim,
            });
        }

        if let Some(CastTarget(target)) = cast_target {
            // `CastSpell` 与 `CastTarget` 是同一个校验器成对写下的；缺一半说明
            // 校验器有 bug，宁可失败也不发一个不知道是什么的法术。
            let Some(cast) = cast_spell else {
                events.fail(action, actor);
                continue;
            };
            spells.write(SpellIntentEvent {
                caster: actor,
                spell: cast.spell,
                target: *target,
            });
        }

        events.succeed(action, actor);
    }
}
