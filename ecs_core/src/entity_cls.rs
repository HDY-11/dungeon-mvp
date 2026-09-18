//! 实体类别标记。
//!
//! 只有**查询用 ZST**：`Player` / `Monster` / `Stairs` 回答“这个实体是什么”，
//! 用于过滤与归属判断。物种级数据（数值、名字、字形、生成权重）一律查
//! [`crate::monster::monster_template`]，不再用一层身份 ZST 重复表达
//! （REFACTOR.md §10.8：`Rat`/`Scorpion`/… 与 `MonsterKindId` 重复，已删除）。
//!
//! 已删除的“范畴”枚举（`EntityClass` / `CreatureKind`）：两者都只有写入方、
//! 没有读取方——`EntityClass::Item` 的唯一判断在 `rebuild_occupancy_system`，
//! 而全库从未插入过 `Item`，该判断恒为假。等真需要「按范畴分派公式」时再加，
//! 那时会有具体的读取方（REFACTOR.md §10.8 / A43 的判据：每个保留的抽象必须有
//! 真实读取方）。

use bevy_ecs::prelude::*;

/// 玩家标记。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Player;

/// 怪物通用标记。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Monster;

/// 楼梯标记。
///
/// 楼梯占格但**不参与占用图**（可以站上去），见 `system/occupancy.rs`。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Stairs;