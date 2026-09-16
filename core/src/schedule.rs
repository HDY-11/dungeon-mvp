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

/// 行动链路：生成（玩家 + AI）→ 仲裁 → tick → 执行 → completion。
///
/// C7 起**已接进主循环**（`world/loop_.rs` 每轮运行它）。名字里的 “Poc” 是
/// Phase B 的遗留，为少改调用点而保留；Phase C 收尾时可改名为 `CoreActionSchedule`。
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

/// 玩家行动挂载：生成 + 仲裁，**不含**推进/执行。
///
/// `apply_player_command` 先单独跑这一次来确认命令是否被接受；见
/// `action::entity::build_player_mount_schedule` 的说明。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PlayerMountSchedule;

impl ScheduleLabel for PlayerMountSchedule {
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
