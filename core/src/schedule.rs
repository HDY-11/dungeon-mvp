//! core 的持久 Schedule 标签。
//!
//! 这些 Schedule 由 `insert_core_resources` 注册到 `World`，之后通过
//! `World::run_schedule(label)` 重复运行。**不要**在每次运行前重新
//! `Schedule::new(...)`：那会丢失 `EventReader` 游标、`Local` 等系统状态，
//! 导致事件被重复读取。

use bevy_ecs::label::DynEq;
use bevy_ecs::schedule::ScheduleLabel;
use std::hash::{Hash, Hasher};

/// 首次世界初始化。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CoreInitSchedule;

impl ScheduleLabel for CoreInitSchedule {
    fn dyn_clone(&self) -> Box<dyn ScheduleLabel> {
        Box::new(self.clone())
    }

    fn as_dyn_eq(&self) -> &dyn DynEq {
        self
    }

    fn dyn_hash(&self, mut state: &mut dyn Hasher) {
        Hash::hash(self, &mut state);
    }
}

/// 每轮结算：伤害、死亡、经验、FOV、记忆、占用图、事件更新。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CoreSettleSchedule;

impl ScheduleLabel for CoreSettleSchedule {
    fn dyn_clone(&self) -> Box<dyn ScheduleLabel> {
        Box::new(self.clone())
    }

    fn as_dyn_eq(&self) -> &dyn DynEq {
        self
    }

    fn dyn_hash(&self, mut state: &mut dyn Hasher) {
        Hash::hash(self, &mut state);
    }
}

/// 行动实体 PoC 链路（REFACTOR.md §11.3 Phase B）。
///
/// 仅供 PoC 测试使用：`world/loop_.rs` 仍走旧的 `decide_monster_actions` +
/// `mount_action`；Phase C 会把这条链路接进主循环。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ActionPocSchedule;

impl ScheduleLabel for ActionPocSchedule {
    fn dyn_clone(&self) -> Box<dyn ScheduleLabel> {
        Box::new(self.clone())
    }

    fn as_dyn_eq(&self) -> &dyn DynEq {
        self
    }

    fn dyn_hash(&self, mut state: &mut dyn Hasher) {
        Hash::hash(self, &mut state);
    }
}
