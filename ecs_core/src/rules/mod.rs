//! 规则层：**纯函数**的求值形状（DESIGN DsnE10 / DsnE9 / Phase H2 + H8）。
//!
//! 这里放"效果 → 纯函数输入"的折算逻辑，以及需要"结构化输入 / 因子分解返回"的
//! 具体规则。它与 [`crate::combat`] 的分工：
//!
//! - `combat`：**位置与目标**类规则（能不能打、8 方向相邻、目标是否存活）——
//!   这些是"行动是否可以发生"，不产生数值；
//! - `rules`：**数值**类规则 —— 通用修正器形状（[`modifier`]）与近战伤害
//!   （[`damage`]）。二者都不碰 `World`：求值的第 ①② 步（收桶）在系统层完成，
//!   第 ③④⑤ 步（过滤/折叠/纯结果）在这里，因此可以在无 ECS 的单测里逐条钉住。

pub mod damage;
pub mod modifier;

pub use damage::{CritProfile, MeleeBreakdown, MeleeInput, melee};
pub use modifier::{EffectSource, Modifier, ModifierOutcome, apply_modifiers, evaluate_modifiers};
