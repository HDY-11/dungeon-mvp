//! 怪物：模板数据（`template`）、生成规则（`spawn`）与仇恨（`aggro`）。
//!
//! `mod.rs` 只做模块声明与重导出。**一个物种的全部信息都在 `template.rs` 的一行里**
//! （数值 / 能力 / 生成权重）；`spawn.rs` 只剩种类池与加权挑选（Phase H6/H7）；
//! `aggro.rs` 是仇恨/威胁表骨架（DsnE29）。

pub mod aggro;
pub mod spawn;
pub mod template;

/// 一具**尸体**：世界对象 + 容器（DsnE18 第②片）。
///
/// 存物种与名字，是为了让渲染复用怪物字形（`VisualKey::Monster`）并把名字念成
/// "哥布林的尸体"，而不必给尸体单独造一套外观数据。
#[derive(bevy_ecs::prelude::Component, Debug, Clone, Copy)]
pub struct Corpse {
    pub kind: MonsterKindId,
    pub name: &'static str,
}

impl Corpse {
    pub const fn new(kind: MonsterKindId, name: &'static str) -> Self {
        Self { kind, name }
    }

    /// 显示名：`哥布林的尸体`。
    pub fn display_name(self) -> String {
        format!("{}的尸体", self.name)
    }
}

pub use aggro::{
    Aggro, ThreatEntry, record_threat_system, threat_decay_system, threat_system,
};
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
