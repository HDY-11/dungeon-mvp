//! core 的持久 Schedule 标签。
//!
//! 这些 Schedule 由 `insert_core_resources` 注册到 `World`，之后通过
//! `World::run_schedule(label)` 重复运行。**不要**在每次运行前重新
//! `Schedule::new(...)`：那会丢失 `EventReader` 游标、`Local` 等系统状态，
//! 导致事件被重复读取。构建一律用 [`core_schedule`]。

use bevy_ecs::label::DynEq;
use bevy_ecs::schedule::{LogLevel, Schedule, ScheduleBuildSettings, ScheduleLabel};
use std::hash::{Hash, Hasher};

/// 本项目**唯一**允许用来构建 core Schedule 的入口。
///
/// # 为什么不用 `Schedule::new` 直接建
///
/// bevy 的 `ambiguity_detection` 默认是 `LogLevel::Ignore`，也就是**零检查**。
/// 而执行器语义是"无冲突的系统可以并行"，等价于"**你声明了顺序的才有顺序**"。
/// 本项目的确定性完全依赖 `.chain()`，而那是一条**靠每个新加系统的人记得**的纪律
/// ——正是我们在其他设计里一直在消除的那种"靠自觉"的约束。
///
/// 这里把 `ambiguity_detection` 提到 `LogLevel::Error`：任何"访问冲突但顺序未声明"
/// 的系统对，都会在**构建 Schedule 时**（即 `insert_core_resources`，也就是每个测试的
/// 第一句）直接失败，而不是等到某一天行为悄悄变了才由人回想。
///
/// 代价：构建时多做一次冲突检测（四个 Schedule 各一次，可忽略）；并且以后新增系统
/// 若与既有系统有隐性顺序依赖，会被**迫**把顺序写出来。后者不是限制而是收益：
/// 把"隐性顺序"变成"显性边"。
pub fn core_schedule(label: impl ScheduleLabel) -> Schedule {
    let mut schedule = Schedule::new(label);
    schedule.set_build_settings(ScheduleBuildSettings {
        ambiguity_detection: LogLevel::Error,
        ..Default::default()
    });
    schedule
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_ecs::prelude::*;

    /// 两个"访问同一资源、顺序未声明"的系统——用来验证严格度真的生效。
    fn bump(mut log: ResMut<crate::resources::EventLog>) {
        let _ = &mut log;
    }
    fn peek(log: Res<crate::resources::EventLog>) {
        let _ = &log;
    }

    /// **护栏本身的测试**：`core_schedule` 必须拒绝"访问冲突但顺序未声明"的系统。
    ///
    /// 没有这条，"把 `ambiguity_detection` 改回默认的 `Ignore`"就是一次静默回退：
    /// 其他测试照旧全绿，而护栏已经不在了。这条就是那道防回退的锁。
    #[test]
    fn core_schedule_rejects_ambiguous_systems() {
        let mut world = World::new();
        world.insert_resource(crate::resources::EventLog::new());

        let mut schedule = core_schedule(CoreSettleSchedule);
        schedule.add_systems((bump, peek));
        assert!(
            schedule.initialize(&mut world).is_err(),
            "访问冲突且顺序未声明时，构建 Schedule 必须失败（ambiguity_detection = Error）"
        );
    }

    /// 反向：显式声明顺序之后，同一对系统必须能构建。
    ///
    /// 与上一条成对，证明失败的原因是"没声明顺序"，而不是这两个系统本身有问题。
    #[test]
    fn core_schedule_accepts_the_same_pair_once_ordered() {
        let mut world = World::new();
        world.insert_resource(crate::resources::EventLog::new());

        let mut schedule = core_schedule(CoreSettleSchedule);
        schedule.add_systems((bump, peek).chain());
        assert!(
            schedule.initialize(&mut world).is_ok(),
            "声明了顺序就不能再报歧义"
        );
    }
}

