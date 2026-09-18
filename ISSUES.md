> **⚠️ 修改前必须阅读或回忆 [RULE.md](RULE.md)——它定义了本文档的维护规则和更新时机。**

# 发现的问题记录 —— 根目录（跨 crate）

**归属范围：** 跨 crate 协同、工具链与流程、迁移动因；以及不属于任何单一 crate 的问题。

**编号：** `SYN1`、`SYN2`… 每个 crate 独立编号，见 [RULE.md](RULE.md) 与 [REFACTOR.md](REFACTOR.md) §13。
根目录只记**跨 crate 协同 / 工具链与流程 / 迁移动因**；单 crate 的问题记在对应 crate 的 ISSUES.md 里。

**优先级：** 🔴 高（影响正确性或游戏体验） / 🟡 中（维护性或功能缺口） / 🟢 低（整洁或边缘情况）

## 待处理

### SYN1 — 测试覆盖缺口（部分） 🟡 进行中

**现状：** dungeon-core 5 个（EventLog）、dungeon-action 14 个（含本次 6 个回归测试）、dungeon-world 2 个、场景 3 个、terrain-forge 26 个。

**剩余缺口：** dungeon-render 0 测试；应用层（main.rs 装备/投掷 UI 流程）0 测试。

**风险：** UI 流程（装备原子性 I58、投掷 Enter 验证 I59）依赖手动验证。

---



### SYN2 — 事件帧模式（Deferred — 触发条件达成时重新评估）

**问题：** 当前玩家确认行动后批量推进到玩家行动完成，中间所有怪物行动对玩家不可见。

**提议方案：** 增加可切换的"事件帧模式"（按 `s`），每帧只执行一个事件，Enter 步进。

**当前评估：** 暂缓实现。在当前战斗系统（纯数值 chase/flee/wander）下，事件帧模式提供的信息量不足以补偿节奏损失——玩家的最优策略不会因看到每个怪物单步移动而改变。

**触发条件：** 出现**足够复杂的战斗逻辑**，即新增的怪物/boss 有需要玩家在过程中作出反应的能力——例如范围攻击预警、状态效果倒计时、可打断的吟唱、地形变化。当单次 tick 内的行动序列构成决策信息时，事件帧模式从"nice to have"变为"need to have"。

---


### SYN3 — terrain-forge submodule 配置缺失：无 .gitmodules + 163 个未提交变更

**问题：** 主仓库将 terrain-forge 记录为 gitlink（`160000 c6d9d1f`）但仓库中不存在 `.gitmodules`（历史中也没有）。submodule 工作区有 163 个未提交的删除/修改（README、demo、.github、Cargo.toml 等被清理但从未提交）。

**影响：** 🟡 中 — 新克隆者无法 `git submodule update --init`，workspace 构建直接失败；本地清理状态未固化，随时可被覆盖丢失。

**位置：** 仓库根（.gitmodules 缺失）、`terrain-forge/`（git status 163 项脏变更）

---


### SYN4 — 玩家确认行动后无法取消（被 D5 锁定）

**问题：** tap-tap 双击确认后行动进入 `ActionQueue` 无法撤回。

**说明：** 事件帧模式（D5，已 defer）可以部分解决此问题——事件帧模式下玩家可以在自己行动执行前切换方向。在 D5 重新评估前此问题无解。


## ✅ 已修复

### SYN5 — 新 `core` 冒烟测试缺口：地图/移动/战斗/成长/视野/AI 无回归保障 ✅已修复

**问题：** 新 `core`（core crate）目前只有 4 个测试（AV 门禁 2 个、事件生命周期 1 个、最小闭环 1 个）。地图生成确定性、玩家移动与阻挡、攻击伤害只结算一次、死亡→经验→升级、FOV/记忆/占用图、快怪多动这些核心语义都没有测试；而 `ActionKind` → action 实体（ECS3）与 `Agility` → 速度组件（ECS7）两项重构即将改动同一批系统，没有安全网就无法判断行为漂移。

**影响：** 🟡 中高 — 重构期间任何行为漂移都只能在人工试玩中发现；不能让 `cargo test -p core` 的门禁停留在 4 个测试。

**位置：** `core/src/`（`world/init.rs`、`world/loop_.rs`、`system/mod.rs`、`action/execution/mod.rs`）；新增测试 helper `core/src/test_util.rs`

**状态：** ✅已修复（Phase A）— A1–A7 全部落地，`cargo test -p core` 从 4 个测试增加到 **18 个**通过（+1 个 doctest 目标 0）。

**修复后：**

- 新增 `core/src/test_util.rs`（`#[cfg(test)]`）：`test_world` / `fill_map` / `carve_single_floor` / `single_tile_scene` / `spawn_test_actor` / `spawn_test_player` / `spawn_test_monster` / `kill_entity` / `world_snapshot` 等 helper；

- A1 `world::loop_::tests::map_generation_is_deterministic`：同 seed 两次 `new_game` 的 tiles/rooms/出生点/楼梯/怪物全量快照相等；`different_seed_changes_the_world` 作反面对照；

- A2 `player_move_into_free_tile` / `player_move_blocked_by_wall` / `player_move_out_of_bounds_is_rejected` / `rejected_move_leaves_player_idle`：合法移动改变 `Position`，撞墙/越界/被占用被拒且位置不变、玩家不卡在 `Active`；

- A3 `system::tests::attack_applies_damage_once`：HP 精确扣 10、重复 settle 不再扣、日志恰好两条；

- A4 `monster_death_rewards_exp_and_levels_up` / `experience_below_threshold_does_not_level_up`：死亡 despawn、经验入账、跨阈值升级并重算 HP/MP 上限与回满；

- A5 `fov_memory_and_occupancy_update` / `occupancy_tracks_actors_but_not_stairs`：视野非空含自身、`MapMemory` 已探索、`OccupancyMap` 记录玩家与怪物、移动后新旧格同步、楼梯不占位；

- A6 `fast_actor_gets_more_actions` / `tick_advances_to_the_next_event_and_empties_only_the_fastest` / `only_the_action_whose_timer_hit_zero_executes`：同时间预算内 AV=100 的行动次数多于 AV=300；时间轴推进与旧实现 `timer_advances_to_next_event` 一致；

- 顺带修正 `tick_action_timers_system`：推进量抽取为 `positive_timer_delta`，`min == 0`（全部已归零）时仍补齐 `Ready`。该口径与旧架构 `dungeon-action/src/state_action/runtime.rs` 一致。

**关联：** REFACTOR.md §10.4 / §10.6 第 2 项 / §11.3 Phase A；ISSUES SYN1、SYN6、ECS3、ECS7。

---


**原编号：** `P9`（迁移前）

### SYN6 — `core` doctest 因 crate 名 `core` 与标准库冲突失败 ✅已修复（根因已消除：F5 改名 `ecs_core`）



**问题：** `core/src/resources.rs:65` 写 `core::convert::Infallible`；doctest 编译时 `core::` 解析到本地 `core` crate 而非标准库，`cargo test -p core` 的 doctest 失败（单测为 0）。



**影响：** 🟡 中 — `core` 的测试门禁不可用；crate 名 `core` 与 Rust 标准库同名是长期隐患。



**位置：** `core/src/resources.rs:65`



**状态：** ✅已修复。**改名已落地（F5）**：crate 名由 `core` 改为 `ecs_core`（目录同名），与标准库 `core` 的遮蔽从根上消除，`core::convert::Infallible` 这类必须绕行/特意写成 `std::` 的写法不再必要。决策见 DESIGN DsnX15。



**修复：**



- `core/src/resources.rs:65` 改为 `std::convert::Infallible`；

- `core/src/schedule.rs` 的 `ScheduleLabel` 改为手写 impl（`derive` 宏展开会引用 `core::fmt` / `core::hash`，在 crate 名为 `core` 时被本地 crate 遮蔽）；

- `cargo test -p core` 现在通过（3 个单测 + doctest）。



**关联：** REFACTOR.md §10.6 第 1 项



---




**原编号：** `I86`（迁移前）

### SYN7 — 根集成测试失效，`cargo test -p dungeon-app` 无法编译 ✅已修复



**修复前：** `tests/throw_test.rs` 引用不存在的 `dungeon_tui`；`tests/scenario_test.rs` 针对旧 `dungeon-*` 架构（`setup_world` / `advance_and_settle_parallel` / `dungeon_action` / `dungeon_render`）。`cargo test -p dungeon-app --no-run` 直接编译失败。



**修复后：**



- 两个失效文件 `git mv` 到 `archive/legacy-tests/`（不参与编译），附 `README.md` 说明失效原因、归档理由与替代品；

- 新增 `tests/core_loop_test.rs`：只用新 `core` 公共 API（`new_game` / `apply_player_command` / `player_alive` / `request_quit`）的 headless 端到端测试 8 个（初始化、等待、移动、被墙拒绝、击败相邻怪物、退出请求、game_over 拒绝命令、玩家死亡结束游戏）；

- `Cargo.toml` 增加 `[dev-dependencies] bevy_ecs`（集成测试直接操作 `core` 的世界，需要同一份 ECS 类型）；

- `cargo test -p dungeon-app` 通过（8 passed）。



**注意：** 渲染快照测试（`SceneFrame` golden / `TestBackend`）属于 Phase G，本轮不新增。



**位置：** `tests/`、`archive/legacy-tests/`、`Cargo.toml`



**关联：** REFACTOR.md §10.4 / §10.6 第 4 项 / §11.3 Phase F（F2）。



---




**原编号：** `I88`（迁移前）

### SYN8 — Gm9 投掷伤害表与自身公式/代码不符 ✅已修复

**修复前：** 公式 `基础=3+floor(楼层/2)` 下 F10 应为 8，表写 7；F5 对防 3 哥布林表写 2-4，实际 2-3。

**修复后：** 表格重算：F5 行 2-3、F10 行 8/8-9/5-6，与公式及代码一致。

**位置：** `GAME.md:471-475`

---


**原编号：** `D23`（迁移前）

### SYN9 — SL 刷掉落漏洞：RNG 状态不持久化，读档重放随机序列 ✅已修复

**修复前：** 存档只存 map_seed，restore 用 `GameRng::new(map_seed+42)` 从种子重建，读档后暴击/掉落/游荡随机序列重放，可存档→杀怪→读档重掷实现 SL 刷掉落；且与 descend 的含 floor 派生不一致。

**修复后：** GameRng 重写为可序列化的 xorshift64* 状态机（impl `rand::TryRng`，每次 `next` 计一步）；掉落 roll 改走 GameRng（不再绕过状态）；GameSave 存 `rng_state/rng_steps`，restore 用 `from_state` 精确恢复。旧档（无状态）保持旧派生种子行为。回归测试 3 个（状态持久/续接非重放/旧档默认）。

**位置：** `dungeon-core/src/resources.rs`（GameRng）、`dungeon-action/src/execute.rs`（handle_kill）、`dungeon-world/src/persist.rs`

---


**原编号：** `G32`（迁移前）

### SYN10 — RULE.md 编辑流程优化（移除宣誓 + 强化记录优先） ✅已修复

**修复前：** RULE.md 要求编辑前"宣誓"，AI 须在每回合第一次编辑前声明流程步骤。实际效果不佳（"反智能体"），且核心问题（先修后记）未被有效约束。

**修复后：**
1. 移除"宣誓"机制
2. 新增醒目 🚨 区块：**用户报告问题 → 先记录 ISSUES → 再修复**
3. 简化编辑前检查清单，保留三条核心检查
4. 同步保存高优先级 memory（`rulemd-bug-report-flow`），确保每轮启动可见

**位置：** `RULE.md` §六


**原编号：** `R1`（迁移前）

### SYN11 — 主循环空闲 sleep 1ms 导致有限机型 CPU 满载 ✅已修复

**修复前：** 主循环无输入时 `sleep(1ms)`，渲染一帧约 5ms，合计 6ms/帧 ≈ 166fps 空转。有限机型上单核 100% 满载，系统可能因过热/调度杀死进程。

**修复后：** 空闲 sleep 改为 32ms，渲染频率降到约 27fps。回合制终端游戏在无操作时不需要高刷新率。

**位置：** `src/main.rs:88`


**原编号：** `P8`（迁移前）

### SYN12 — 全部 `.unwrap()` 替换为 `.expect()` ✅已修复

**状态：** 全部 ~35 处 `.unwrap()` 已替换。生产代码零 unwrap。


**原编号：** `I17`（迁移前）

### SYN13 — 斜向键无 OS key-repeat（Won't Fix — 终端环境限制） ✅已修复

**问题：** 按住 Home/End/PgUp/PgDn 不放，角色不会连续斜向移动。多数终端不发斜向键的 OS key-repeat 事件。

**结论：** 终端环境引起，不在项目控制范围内。


**原编号：** `I10`（迁移前）
