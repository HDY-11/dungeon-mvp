//! 怪物：模板数据（`template`）与生成规则（`spawn`）。
//!
//! `mod.rs` 只做模块声明与重导出。**一个物种的全部信息都在 `template.rs` 的一行里**
//! （数值 / 能力 / 生成权重）；`spawn.rs` 只剩种类池与加权挑选（Phase H6/H7）。
//!
//! 本轮不迁移掉落表（物品未迁移）。

pub mod spawn;
pub mod template;

pub use spawn::{
    attach_abilities, kinds_for, monster_spawn_weight, pick_by_weight, roll_one_kind,
};
pub use template::{
    Ability, MonsterKindId, MonsterSpeeds, MonsterStats, MonsterTemplate, SpawnWeight,
    SpawnWeightFor, monster_template,
};

#[cfg(test)]
#[path = "monster_tests.rs"]
mod tests;
