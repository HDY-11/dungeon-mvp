//! 怪物：模板数据（`template`）与生成规则（`spawn`）。
//!
//! `mod.rs` 只做模块声明与重导出；物种**数值**与**出现概率**分别住在两个子模块里。
//!
//! 本轮不迁移掉落表（物品未迁移）。

pub mod spawn;
pub mod template;

pub use spawn::{kinds_for, monster_spawn_weight, roll_one_kind};
pub use template::{MonsterKindId, MonsterSpeeds, MonsterStats, MonsterTemplate, monster_template};

#[cfg(test)]
#[path = "monster_tests.rs"]
mod tests;
