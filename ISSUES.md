> **⚠️ 修改前必须阅读或回忆 [RULE.md](RULE.md)——它定义了本文档的维护规则和更新时机。**

# 发现的问题记录

本文档记录当前实现中与设计意图不一致或有改进空间的问题，
在后续开发中可作参考。

问题按维度分组：**设计 / 架构 / 实现 / 游戏逻辑**，组内按严重程度降序。
优先级标记：🔴 高（影响正确性或游戏体验） / 🟡 中（维护性或功能缺口） / 🟢 低（整洁或边缘情况）

> **编号状态** — D: D1~D28 | A: A1~A40（含 A4L/A4La 子条目）| I: I1~I85（含 I27L/I27La 子条目）| G: G1~G34（含 G4L/G4La 子条目）| P: P1~P9 | R: R1

## ✅ 已修复

### P9 — 新 `core` 冒烟测试缺口：地图/移动/战斗/成长/视野/AI 无回归保障 ✅已修复

**问题：** 新 `core`（core crate）目前只有 4 个测试（AV 门禁 2 个、事件生命周期 1 个、最小闭环 1 个）。地图生成确定性、玩家移动与阻挡、攻击伤害只结算一次、死亡→经验→升级、FOV/记忆/占用图、快怪多动这些核心语义都没有测试；而 `ActionKind` → action 实体（A42）与 `Agility` → 速度组件（G35）两项重构即将改动同一批系统，没有安全网就无法判断行为漂移。

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

**关联：** REFACTOR.md §10.4 / §10.6 第 2 项 / §11.3 Phase A；ISSUES I23、I86、A42、G35。

---

### I86 — `core` doctest 因 crate 名 `core` 与标准库冲突失败 ✅已修复



**问题：** `core/src/resources.rs:65` 写 `core::convert::Infallible`；doctest 编译时 `core::` 解析到本地 `core` crate 而非标准库，`cargo test -p core` 的 doctest 失败（单测为 0）。



**影响：** 🟡 中 — `core` 的测试门禁不可用；crate 名 `core` 与 Rust 标准库同名是长期隐患。



**位置：** `core/src/resources.rs:65`



**状态：** ✅已修复（crate 改名仍待长期评估）。



**修复：**



- `core/src/resources.rs:65` 改为 `std::convert::Infallible`；

- `core/src/schedule.rs` 的 `ScheduleLabel` 改为手写 impl（`derive` 宏展开会引用 `core::fmt` / `core::hash`，在 crate 名为 `core` 时被本地 crate 遮蔽）；

- `cargo test -p core` 现在通过（3 个单测 + doctest）。



**关联：** REFACTOR.md §10.6 第 1 项



---



### I87 — `sys` 独立构建缺少 `log/std` ✅已修复



**修复前：** `sys/src/logger.rs:46` 调用 `log::set_boxed_logger`，但 `sys` 的 `log` 依赖未显式启用 `std` feature；`cargo test -p sys` 独立编译失败（`cannot find function set_boxed_logger`），workspace 构建只靠其他 crate 的 feature 合并偶然通过。



**修复后：** `sys/Cargo.toml` 改为 `log = { workspace = true, features = ["std"] }` 并加注释说明原因；`cargo test -p sys` 独立通过（0 测试，不再依赖 feature 合并）。



**位置：** `sys/Cargo.toml:8`、`sys/src/logger.rs:46`



**关联：** REFACTOR.md §10.4 / §10.6 第 3 项 / §11.3 Phase F（F1）。



---



### I88 — 根集成测试失效，`cargo test -p dungeon-app` 无法编译 ✅已修复



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



### I89 — AV 门禁缺失：`ActionTimer` 未参与执行判断 ✅已修复



**问题：** `tick_action_timers_system` 把所有 `Active` 计时器减去最小正剩余 AV，只有最快的一个归零；但 `execute_*_system` 的查询只有 `With<Active>` + 行动组件，没有检查 `remaining_av <= 0`。结果所有 `Active` 行动每轮都会执行，AV/敏捷/速度不控制执行顺序或频率。



**影响：** 🔴 高 — 速度/AV 系统实际无效；REFACTOR.md / GAME.md 的 AV 描述与实现不符；在修复前更换速度公式没有意义。



**位置：** `core/src/action/execution/mod.rs:21-38`（tick）、`:42-303`（execute_*）、`:305-313`（run_action_cycle）；`core/src/world/loop_.rs:36-43`（advance_until_player_acted）



**修复：**



- 新增 `Ready` 组件（`core/src/components.rs`）；`tick_action_timers_system` 在 `remaining_av <= 0` 时插入 `Ready`；

- 所有 `execute_*_system` 查询加 `With<Ready>`；`mount_action` / `finish_action_*` / `player.rs::mount_player_action` 清理 `Ready`；

- `advance_until_player_acted` 每轮生成怪物行动 + tick/执行 + 结算，快怪可以在玩家行动期间执行多次；

- 回归测试：`action::execution::tests::av_gate_only_executes_ready_actions`、`zero_timer_is_marked_ready_without_positive_peers`。



**状态：** ✅已修复。



**关联：** A41、A42、G35。



---



### I90 — 事件生命周期错误：`EventReader` 每轮重读历史事件 ✅已修复



**问题：** `build_core_schedule()` / `run_settle_systems()` 每次调用都新建 Schedule，`EventReader` 游标随之归零；同时 `insert_core_resources` 只注册 `Events<T>`，从未调用 `Events::update()`。因此每轮结算都会重读历史上所有 `AttackIntentEvent` / `AttackEvent`，旧伤害被反复结算，事件缓冲无限增长。



**影响：** 🔴 高 — 战斗结果不可信（伤害重复、死亡日志重复、内存增长）；任何基于事件的扩展点都不可靠。



**位置：** `core/src/system/mod.rs:257-282`（build_core_schedule/run_settle_systems）、`core/src/world/init.rs:70-76`（Events 注册）、`core/src/world/init.rs:393-416`（build_init_schedule/run_initialization）



**修复：**



- `insert_core_resources` 注册持久 Schedule（`CoreInitSchedule` / `CoreSettleSchedule`）；

- `build_core_schedule` 使用 `Schedule::new(CoreSettleSchedule)`，`run_settle_systems` 用 `world.run_schedule(CoreSettleSchedule)`，不再每轮重建；

- 新增 `update_events_system` 在结算末尾对 7 种事件调用 `Events::update()`；

- 回归测试：`system::tests::settle_does_not_reapply_old_events`。



**状态：** ✅已修复。



**关联：** A41、REFACTOR.md §3.6.8 / §5。



---


### 🟡 D21 — Gm4 玩家初始 HP 文档算术错误：28 vs 实现 33 ✅已修复

**修复前：** Gm4 标注 `HP = 20 + 等级×5 + 防御×2 = 28`，代码 `max_hp_for(1,4)=33`，文档漏加 `等级×5=5`。

**修复后：** Gm4 表改 `= 33（1级防4时）` 并展开计算过程 `20+5+8=33`，标注改 [⃞计算]。

**位置：** `GAME.md:152`

---

### 🟡 D22 — Gm6 熟练度表格与公式/代码错位一级 ✅已修复

**修复前：** 表格「熟练度 1」行写零加成，公式/代码熟练度 1 即有加成（治愈 +3、护盾/狂暴 +2）；Gm5 表同步错位。

**修复后：** Gm6 表格修正为熟练度 1 = `15+精通+3 / +7 / +7`，并加说明「熟练度 1 即有加成（熟练度×系数）」；Gm5 表护盾/狂暴熟练度 1 修正为 +7。

**位置：** `GAME.md:194-195`（Gm5）、`GAME.md:223-228`（Gm6）

---

### 🟡 D23 — Gm9 投掷伤害表与自身公式/代码不符 ✅已修复

**修复前：** 公式 `基础=3+floor(楼层/2)` 下 F10 应为 8，表写 7；F5 对防 3 哥布林表写 2-4，实际 2-3。

**修复后：** 表格重算：F5 行 2-3、F10 行 8/8-9/5-6，与公式及代码一致。

**位置：** `GAME.md:471-475`

---

### 🟡 D24 — 按键去重阈值：文档三处 50ms vs 实现 33ms ✅已修复

**修复前：** Gm11/Dsn17/README 均声明 50ms，实现（L46 修复后）为 33ms + KeyEventKind 过滤。

**修复后：** 三处文档统一为 33ms；Dsn17 保留「50→33 收窄」历史上下文并关联 L46。

**位置：** `GAME.md:524/530`、`DESIGN.md:378-389`（Dsn17）、`README.md:87`

---

### 🟡 D25 — Gm11 模态对话框描述过时：modal_flag 已是死代码 ✅已修复

**修复前：** Gm11 写「AtomicBool 暂停输入线程」，页栈（Dsn21）已接管，`modal_flag` 为死代码。

**修复后：** Gm11 改为「页栈按键路由」，标注旧方案已废弃；`modal_flag` 死代码清理并入 A36。

**位置：** `GAME.md:534`

---

### 🟡 D26 — Gm1 移动耗时 300ms 与 Gm7 武器攻速矛盾，CanMove.duration 成死字段 ✅已修复

**修复前：** Gm1 行动表写「移动 300ms」，I67 后耗时由主手武器 speed 决定。

**修复后：** Gm1 行动表改为「武器 speed（无武器 300）」并标注 [⃞计算]，交叉引用 Gm7；CanMove.duration 死字段注释同步（清理并入 A36）。

**位置：** `GAME.md:14`

---

### 🟢 D27 — Gm3 升级经验表 round vs 实现 trunc（每级差 1） ✅已修复

**修复前：** 表格按四舍五入，代码 `as u64` 截断（2→3=90 表写 91 等）。

**修复后：** 公式标注「结果向下取整（trunc）」，表格改为截断值（90/159/329/645/890）。

**位置：** `GAME.md:106-120`

---

### 🟢 D28 — Gm6 卷轴「每层 1-3 张」未记载深层增量 ✅已修复

**修复前：** 实现为 `1-3 + ⌊(楼层-1)/5⌋`，文档只写 1-3。

**修复后：** Gm6 掉落描述补充深层增量公式。

**位置：** `GAME.md:201`

---

### 🟡 I82 — 怪物种群补足循环无进展保证，深层/水域地图可死循环 ✅已修复

**修复前：** `while positions.len() < min_count` 无迭代上限、无候选格不足判定，可行走格不足时死循环卡死。

**修复后：** 迭代上限 40×期望数，未达期望时 log::warn 降级（不足比卡死好）。回归测试 1 个（全墙 6 格地图 + floor=20 不死循环）。

**位置：** `dungeon-world/src/population.rs`（generate_monster_population 补足段）

---

### 🟡 I83 — 单房间地面物品放置 random_range 空区间 panic ✅已修复

**修复前：** `random_range(2..r.w.saturating_sub(2))` 在房间 bounding box ≤4 时区间为空 panic。

**修复后：** 房间过小时跳过采样，充底到中心最近可行走格（与多房间路径语义一致）。

**位置：** `dungeon-world/src/init.rs`（place_ground_items 单房间分支）

---

### 🟢 I84 — pick_stair_pos 兜底坐标未钳制，可引发越界 panic ✅已修复

**修复前：** 螺旋搜索全失败时兜底 `(spx+15, spy)` 未 clamp，spx>64 时越界坐标传给 ensure_connection_between 索引越界 panic。

**修复后：** 兜底坐标 `saturating_add(15).min(MAP_WIDTH-1)` 钳制 + `nearest_walkable` 保证可行走。回归测试 1 个（spawn(75,59) 返回界内 walkable 坐标）。

**位置：** `dungeon-world/src/init.rs`（pick_stair_pos 兜底）

---

### 🟡 G33 — 护盾/狂暴实际时长约为文档宣称 3 倍，技能超模 ✅已修复

**修复前：** duration=3 → 3000 AV；玩家单次行动 AV 约 310ms，实际覆盖 7-13 次行动，远超「3 次行动」设计意图。

**修复后：** duration 3→1（1000 AV ≈ 3 次玩家行动），Gm2/Gm5 数值表同步更新（标注 [⃞试调] 调整轨迹）。回归测试 1 个（技能 duration=1 断言）。

**位置：** `dungeon-core/src/items.rs`（use_item 卷轴学习）、`GAME.md:98-99/194-195`

---

### 🟡 G32 — SL 刷掉落漏洞：RNG 状态不持久化，读档重放随机序列 ✅已修复

**修复前：** 存档只存 map_seed，restore 用 `GameRng::new(map_seed+42)` 从种子重建，读档后暴击/掉落/游荡随机序列重放，可存档→杀怪→读档重掷实现 SL 刷掉落；且与 descend 的含 floor 派生不一致。

**修复后：** GameRng 重写为可序列化的 xorshift64* 状态机（impl `rand::TryRng`，每次 `next` 计一步）；掉落 roll 改走 GameRng（不再绕过状态）；GameSave 存 `rng_state/rng_steps`，restore 用 `from_state` 精确恢复。旧档（无状态）保持旧派生种子行为。回归测试 3 个（状态持久/续接非重放/旧档默认）。

**位置：** `dungeon-core/src/resources.rs`（GameRng）、`dungeon-action/src/execute.rs`（handle_kill）、`dungeon-world/src/persist.rs`

---

### 🟡 A35 — 存档静默丢弃玩家 Attack 行动 ✅已修复

**修复前：** capture 对 `ActionKindV3::Attack` 直接 `return None`，攻击已入队未执行时存档，读档后攻击消失无提示。

**修复后：** `SavedActionKind` 末尾追加 `Attack { tx, ty }` 变体（bincode 旧档兼容）；capture 按目标坐标保存，restore 按坐标反查 Monster 实体重映射；查不到则取消并记 warn 日志。回归测试 1 个（Attack 存读档后 target 反查成功）。

**位置：** `dungeon-world/src/persist.rs`（capture/restore/SavedActionKind）

---

### 🟢 A30 — GameSave 无版本号：bincode 下 `#[serde(default)]` 不提供 schema 级兼容 ✅已修复

**修复前：** GameSave 无版本字段，bincode 按字段顺序读写，schema 变更（加字段）会让旧存档 EOF 反序列化失败，无迁移路径。

**修复后：** 磁盘格式引入 magic 前缀 `DSV1`（4 字节）+ 新 GameSave；旧格式（裸 bincode GameSaveV0）保留专门结构，`load_game` 双格式自动识别并字段级迁移（新字段取默认：容量 36、RNG 无状态）。以后 schema 变更只需新 magic 版本 + V{N} 转换函数。回归测试 1 个（V0 旧档读入成功）。

**位置：** `dungeon-world/src/persist.rs`（SAVE_MAGIC/GameSaveV0/save_game/load_game）

---

### 🟢 A37 — 读档链路在 main.rs 与 game.rs 重复实现 ✅已修复

**修复前：** 两处各自实现 read→deserialize→restore→post_load_refresh，路径硬编码两次，日志行为不一致。

**修复后：** 收敛为 `dungeon_world::save_game`/`load_game` 单入口（含 magic 双格式兼容与 post_load_refresh），两处调用方只做路径选择与结果日志；存档/读档失败现在有明确的「已保存/已读档/失败」提示（旧实现静默吞错误）。回归测试 1 个（save/load 回环）。

**位置：** `dungeon-world/src/persist.rs`（save_game/load_game）、`src/main.rs`、`src/pages/game.rs`

---

### 🟢 A40 — restore 硬编码 Inventory capacity=36，capture 不存容量 ✅已修复

**修复前：** capture 只存 stacks，restore 写死 36；descend 却保留原容量——未来容量可变时读档静默重置。

**修复后：** GameSave 增加 `inv_capacity` 字段（0/缺省=旧档默认 36），capture 写入、restore 读回。回归测试 1 个（容量 40 存读档回环）。

**位置：** `dungeon-world/src/persist.rs`

---

### 🟢 I85 — 读档 map_tiles/explored 长度未校验，损坏存档越界 panic ✅已修复

**修复前：** restore 按 `i/MAP_WIDTH` 直接索引写入，反序列化的 Vec 长度 > 4800（被篡改/损坏但 Tile tag 合法）时越界 panic，与 I72「读档永不崩溃」目标相悖。

**修复后：** 长度校验 + `take` 截断（多余丢弃、缺失保持 Wall/false），不一致时 log::warn。回归测试 1 个（超长 4850 格存档读档不 panic）。

**位置：** `dungeon-world/src/persist.rs`（restore 地图恢复段）

---

### 🔴 A31 — 读档怪物缺 LastKnownPlayerPos：追击 AI 失效（L44 模式第五次） ✅已修复

**修复前：** restore 怪物 spawn 缺 `LastKnownPlayerPos` 组件（setup_world/descend 均有），`chase_decision_system` 查询要求该组件——读档后所有怪物被查询过滤，永不追击玩家。

**修复后：** restore 怪物 spawn 补 `LastKnownPlayerPos::default()`。回归测试 1 个（capture→restore 后所有 Monster 断言持有该组件——存在性断言而非数据字段，L44 补充教训）。

**位置：** `dungeon-world/src/persist.rs`（restore 怪物 spawn）

**教训见 LESSONS.md L44**

---

### 🔴 G28 — 背包满时 g 快速拾取静默销毁脚下全部地面物品 ✅已修复

**修复前：** `pickup_ground` 循环内无条件 despawn 每个地面物品实体——背包满（picked==0）时物品仍被销毁；部分空间时 leftover 一并销毁。卷轴/装备/稀有材料永久丢失。

**修复后：** 仅当 `picked>0 且 leftover==0`（全部装下）才 despawn；装不下时保留实体并写回剩余数量，推「背包已满」日志。回归测试 2 个（背包满保留实体 / 部分装下写回剩余）。

**位置：** `dungeon-core/src/ops.rs`（pickup_ground）

---

### 🟡 G29 — 玩家近战攻击无距离校验，可隔空命中已离开的怪物 ✅已修复

**修复前：** 玩家确认攻击后若怪逃跑离开，execute_attack 仍全额结算伤害；怪物侧攻击先判 8 方向邻接。check_condition 的 Attack 分支只查 target 仍是 Monster。

**修复后：** 新增 `adjacent_8` 距离判定，check_condition 的 Attack 分支与 execute_attack 执行入口双重校验（L48 执行层兜底），玩家与怪物规则对称。回归测试 1 个（非邻接目标攻击被取消）。

**位置：** `dungeon-action/src/execute.rs`（check_condition/execute_attack/adjacent_8）

---

### 🟡 G30 — 逃跑怪物永不回头：滞回未落地 + 卡墙角原地挨打 ✅已修复

**修复前：** 注释声称滞回（进入<25% 退出>30%）但实现仅 <25%；execute_flee 无路可逃时原地不动不反击。

**修复后：** 新增 `FLEE_HP_RATIO_EXIT=0.30`，保活检查用退出阈值（滞回落地：25%-30% 区间内已入队的逃跑继续有效，≥30% 取消）；提取共享 `monster_attack_player`，execute_flee 死角且邻接玩家（视野内）时兜底反击。回归测试 3 个（阈值关系 / 超阈值取消 / 死角反击）。

**位置：** `dungeon-core/src/ops.rs`（FLEE_HP_RATIO_EXIT）、`dungeon-action/src/execute.rs`（check_condition/execute_flee/monster_attack_player）

---

### 🟡 G31 — 对角穿墙（corner-cutting）：玩家与怪物均可斜穿墙角 ✅已修复

**修复前：** can_move_to 注释声称验证不穿墙角但实现没有；玩家入队与怪物 A* 均不检查。

**修复后：** can_move_to 增加对角约束（两侧正交格须可通行且未被占用）；`handle_player_direction` 入队前预检同规则；A* 8 方向遍历同步检查。回归测试 4 个（can_move_to 两侧墙/单侧墙/开放 + 玩家入队拒绝 + A* 不穿墙角 + A* 开放对角）。

**位置：** `dungeon-action/src/execute.rs`（can_move_to）、`dungeon-action/src/player.rs`、`dungeon-core/src/pathfinding.rs`（astar）

---

### 🟢 G34 — 下楼不清空 ActionQueue，残留失效条目 ✅已修复

**修复前：** descend despawn 全部实体但 ActionQueue/三个意图缓冲区未清，下楼后第一次推进产生 no-op 与「行动被取消」日志噪音。

**修复后：** descend 在 despawn 后同步清空 ActionQueue 与 ChaseIntents/FleeIntents/WanderIntents。回归测试 1 个（入队 4 条后下楼全部清空）。

**位置：** `dungeon-world/src/init.rs`（descend）

---

### 🔴 I77 — 投掷 UI 确认链路断裂：`handle_timed_action` 无 Throw 分支 + Enter 提前 pop 页栈，投掷永远无法执行 ✅已修复

**修复前：** `throw_aim.rs` Enter 分支先 pop 页栈再走 `handle_timed_action`（tap-tap 双确认），而确认匹配只有 Move/Wait/Skill 三个 arm——`Throw` 永远落入 `_ => false`。第一次 Enter 只设预览并弹回 Game 页，第二次 Enter 被 keymap（无 Enter 绑定）吞掉。投掷 UI 链路自页栈迁移起从未可用（I59/G15 测试只覆盖 `execute_throw` 层）。

**修复后：** 新增 `dungeon_action::confirm_throw`（execute.rs）：校验 valid_target（L48 执行入口兜底）→ 清理 UI 状态 → `enqueue_or_replace` 入队。`throw_aim` Enter 一次确认直接入队，与 README/Gm9「Enter 投掷」文档语义一致（D20 随之关闭）。回归测试 3 个（一次确认入队+消耗石子 / 无效目标拒绝 / 替换旧行动）。

**位置：** `dungeon-action/src/execute.rs`（confirm_throw）、`src/pages/throw_aim.rs`

**提交：** `fda7f21`

### 🔴 I78 — 状态面板按 UTF-8 字节切片中文装备名 → panic 崩溃 ✅已修复

**修复前：** `ui.rs` 装备名截断 `mh[..mh.len().min(10)]` 按字节操作——4 字中文名（12 字节）`[..10]` 落在字符中间直接 panic。攻击戒指为每层装备池常客，装备后渲染即崩溃。

**修复后：** 新增 `truncate_name`（按字符截断，5 字符 ≈ 原 10 字节显示宽度意图），主手/副手/防具/戒指四槽统一。回归测试 2 个（长中文名面板渲染不 panic + truncate_name 字符安全）。

**位置：** `dungeon-render/src/ui.rs`（truncate_name）

**提交：** `701cc0a`

### 🟡 I79 — 读档玩家缺 AttackName 组件（L44 模式第三次） ✅已修复

**修复前：** `restore` 玩家 spawn 缺少 `AttackName`（setup_world/descend 均有"斩击"），读档后攻击日志名退化为"攻击"。

**修复后：** restore 玩家补 `AttackName("斩击")`，三路径（setup/descend/restore）组件集合一致。回归测试 1 个（含组件存在性断言——L44 第四次，教训强化见 LESSONS.md L44）。

**位置：** `dungeon-world/src/persist.rs`（restore 玩家 spawn）

**提交：** `2547cf0`

### 🟢 I80 — 读档怪物 AttackName 仍用 glyph 反推（A24 同根因残留） ✅已修复

**修复前：** restore 怪物攻击名按 `m.glyph` 分支（'r'/'s'/其他→"重击"）——Dsn24 新怪（m/M/f/c/e）读档后攻击名全部错误显示"重击"。

**修复后：** 按已存档的 `kind` 查 `monster_def::monster_attack_name`，旧存档（kind=None）先按 glyph 推断 kind 再查表，攻击名全对。回归测试 1 个（深鳗读档攻击名"缠绕"）。

**位置：** `dungeon-world/src/persist.rs`（restore 怪物 spawn）

**提交：** `2547cf0`

### 🟡 G24 — `auto_equip_throwable` 背包满时静默丢失副手装备（违反 Dsn10 原子语义） ✅已修复

**修复前：** `throw.rs` 装填时 `inv.add(old.item_id, old.count)` 返回值被忽略——副手木盾 + 背包满 + 按 t → 木盾卸下放不回背包，永久消失。

**修复后：** 装填前预检旧副手能否放回背包（can_add），放不回则回滚保持副手原状并推送"背包已满"提示。回归测试 3 个（背包满保留副手 / 正常换装 / 已有投掷物跳过）。

**位置：** `src/throw.rs`（auto_equip_throwable）

**提交：** `a26d620`

### 🟡 G25 — `ensure_connectivity` 直线收尾挖单格而非 2x2（G22 修复不完整） ✅已修复

**修复前：** G22 修复声明通道挖 2x2 块，但 `ensure_connectivity` 的 Bresenham 收尾用单格 `carve_channel`——区域间通道对角转折处仍可能 4 方向断裂（`ensure_connection_between` 已 2x2，两处不一致）。

**修复后：** 抽取共用 `carve_2x2`，`ensure_connectivity` 游走与收尾全部 2x2。回归测试 2 个（隔离区域连通 20 种子 + carve_2x2 挖 4 格）。

**位置：** `dungeon-core/src/map_gen.rs`（carve_2x2）

**提交：** `63866f2`

### 🟢 G26 — `craft_with_template` 空间预检时序：模板移除前检查 ✅已修复

**修复前：** 空间检查先于材料/模板消耗执行——背包满但材料与模板占位时永远误报"背包已满"，合成不可用。

**修复后：** 空间检查模拟「移除配方材料 + 模板」后的背包再判定。回归测试 2 个（背包满+材料齐全合成成功 / 材料不足拒绝且不消耗模板）；既有合成测试断言同步更新（原"背包满应失败"改"应成功"）。

**位置：** `dungeon-core/src/items.rs`（craft_with_template）

**提交：** `5967c69`、`15dbeac`

### 🟢 G27 — 背包列表 take(14) 硬截断不可滚动，光标可移出可见区 ✅已修复

**修复前：** 背包列表只渲染前 14 个物品且无滚动——>14 物品时选中光标移出屏幕（无视觉反馈），且 README 声称的 0-9/a-z 快捷选中从未实现（UI 显示热键但处理器不响应，L47 违规）。

**修复后：** 列表改为以选中项为中心的滚动窗口（`backpack_window_start`）；处理器补 `hotkey_to_idx`（'0'-'9'/'a'-'z' ↔ 背包第 0-35 个物品，与 UI 热键显示一致；'g' 拾取语义优先）。回归测试 2 个（窗口边界 6 例 + 热键映射 6 例）。

**位置：** `dungeon-render/src/ui.rs`、`src/pages/inventory.rs`

**提交：** `dd5e78a`

### 🟢 I81 — 8 个 unused import 警告 + init.rs 卷轴放置死代码 ✅已修复

**修复前：** 架构批重构后回归：monster.rs/ui.rs/dialog.rs/game.rs/throw_select.rs 未用导入 8 处；init.rs `kinds` 数组整体死代码（卷轴技能类型由 use_item 学习时重建，放置不依赖）。

**修复后：** 全部清理，`cargo build` 警告 8 → 0。

**位置：** `dungeon-action/src/monster.rs`、`dungeon-render/src/ui.rs`、`dungeon-world/src/init.rs`、`src/pages/{dialog,game,throw_select}.rs`

**提交：** `3ca4d26`

### 🟢 D20 — 投掷确认语义与文档不符：README/Gm9 的「Enter 投掷」（一次确认）vs 实现走 tap-tap 双确认 ✅已修复

**修复前：** README/Gm9 写「Enter 投掷」，实现走 tap-tap 双确认且 `Throw` 无确认分支（I77 根因）。

**修复后：** I77 修复后 Enter 一次确认直接入队，实现与文档一致。

**位置：** `src/pages/throw_aim.rs`、`dungeon-action/src/execute.rs`

**提交：** `fda7f21`

### I76 — 架构批 5（A 渲染快照化·核心子集）：状态面板脱离 ECS + render 首测 ✅已修复

**修复前：** dungeon-render 每帧直接查询 ECS（约 20 处），`build_stats_panel` 单函数 10+ 查询；dungeon-render 零测试（I23 最大缺口）。

**修复后：** `RenderScene` 扩展状态面板快照（stats/equip/buffs/skills/floor），`extract_scene` 一次收集；`panel_header(scene)` 纯函数生成面板头部（HP/MP/EXP/攻防/暴击/装备/楼层），`build_stats_panel` 只处理坐标/计时/光标段；技能段改用快照。**dungeon-render 首个单测**（3 个：快照生成面板、无数据降级、低血红色阈值）。渲染层仍有光标查看/背包/时间轴等段直查 ECS——记录为后续（render 完整快照化里程碑）。

**位置：** `dungeon-render/src/{pipeline,ui}.rs`

### I75 — 架构批 4（D 存档瘦身）：SavedStats 手工层删除 ✅已修复


### I75 — 架构批 4（D 存档瘦身）：SavedStats 手工层删除 ✅已修复

**修复前：** `SavedStats` 13 字段手工逐字段复制 Stats（From/into_stats 约 30 行），而 Stats 已 derive Serialize 且字段序一致。

**修复后：** 存档直接存 `Stats`（GameSave.st / SavedMonster.st）——bincode 布局二进制兼容（字段序一致验证）；删除 SavedStats + From + into_stats。`save_restore_roundtrip` 回环测试通过证明兼容。

**保留项（记录理由）：** SavedStack/SavedActiveBuff/SavedSkill 保留——ItemStack 有 meta 字段、Buff 有 stack_type 字段，直接换序列化布局会破坏旧存档，与"存档兼容"原则冲突。

**位置：** `dungeon-world/src/persist.rs`

### I74 — 架构批 3（B+E 收敛）：投掷链路与重复模式收敛 ✅已修复


### I74 — 架构批 3（B+E 收敛）：投掷链路与重复模式收敛 ✅已修复

**修复前：** ① 进入 ThrowAim 初始化块在 game.rs/throw_select.rs 逐字重复、副手可投掷判定 4 处；② 装备槽位 match 在 inventory.rs 多处；③ 读档/下楼后刷新序列 3 处重复；④ 游标移动 look/throw_aim 双实现（且投掷时光标高亮不跟随光标——既有瑕疵）。

**修复后：** `throw.rs::try_enter_throw_aim`/`has_throwable_offhand` 收敛投掷链路；`Equipment::slot/slot_mut` 收敛槽位访问；`ops::post_load_refresh` 收敛读档/下楼刷新（3 处调用）；`pages::move_cursor` 收敛游标移动（look 用），投掷页 `move_throw_cursor` 同步 ThrowPreview+LookCursor（修复高亮不跟随）。

**跳过项（低收益/高风险，记录）：** BFS 连通区泛型化（collect_walkable_regions/detect_cave_regions 语义不同，合并风险高）、醉汉游走 4 处合并（各带不同参数）、玩家查询全量替换（机械改动收益低）。

**位置：** `src/throw.rs`、`src/pages/{game,throw_select,throw_aim,look,mod}.rs`、`dungeon-core/src/{ops,items}.rs`、`dungeon-world/src/persist.rs`

### I73 — 架构批 2（F 死代码清理） ✅已修复


### I73 — 架构批 2（F 死代码清理） ✅已修复

**修复前：** 迁移后遗留死代码：MovingDir 组件（spawn 从不读取）+ ops::set_player_dir；UsableItem trait + SkillScroll 组件（悬空抽象，ISSUES 自认"抽象债务"）；PlayerClass::display_name、ItemClass::icon、ItemDef::has_tag、Inventory::drop_stack、Room::tiles、Map::carve_corridor、Map::render、count_neighbor_tile 零调用；run_monster_decision 包装（仅 re-export）；dungeon-world 冗余 re-export（check_death_system/apply_exp_system）。

**修复后：** 全部删除；PlayerClass 保留 skills()（Dsn13 无职业设计）；Rarity 枚举保留（误删后恢复）。79 测试通过、clippy 0。

**位置：** `dungeon-core/src/{components,items,ops,lib}.rs`、`dungeon-action/src/{monster,lib}.rs`、`dungeon-world/src/{init,persist,lib}.rs`、`dungeon-action/src/tests.rs`

### I72 — 架构批 1（G 加固）：读档容错 / DialogKind / 平衡常量单源 ✅已修复


### I72 — 架构批 1（G 加固）：读档容错 / DialogKind / 平衡常量单源 ✅已修复

**修复前：** ① 损坏存档（未知物品 ID）读档直接 panic；② 对话框行为按标题字符串匹配（改文案即断）；③ 平衡数值多源：射程 5 双份（execute.rs/throw.rs）、投掷耗时 190.0 双份、低血阈值 0.3 vs 1/3 **不一致 bug**（状态面板与行动轴显示不同）、逃跑阈值 0.25 vs 0.30 双份（触发与保活矛盾）。

**修复后：** ① 读档跳过未知物品 + log::warn；② `DialogKind` 枚举（Quit/Descend），行为按种类分派，标题只作显示；③ core 平衡常量单源：`THROW_RANGE`/`THROW_DURATION`/`LOW_HP_RATIO`/`FLEE_HP_RATIO`，全部引用点统一（逃跑统一 0.25，低血统一 0.3）。

**位置：** `dungeon-world/src/persist.rs`、`dungeon-action/src/types.rs`（DialogKind）、`dungeon-core/src/ops.rs`（常量）、`dungeon-render/src/ui.rs`+`timeline.rs`、`src/pages/{dialog,mod,game,throw_aim}.rs`、`src/throw.rs`

### I71 — Dsn23 expect_log 未落地：生产代码仍在用裸 expect/unwrap ✅已修复


### I71 — Dsn23 expect_log 未落地：生产代码仍在用裸 expect/unwrap ✅已修复

**修复前：** Dsn23 设计 `expect_log`（panic 前经 log::error! 写入开发者日志，`#[track_caller]` 记录调用点）用于替换标准库 `expect`，但 `ext.rs` 零调用方——生产代码 44 处 `.expect("...")` + 10 处裸 `.unwrap()` 崩溃前无日志，崩溃现场无法追溯（violates Dsn23 设计意图）。

**修复后：** 生产代码全部改用 `expect_log`（59 处，覆盖 dungeon-core/action/world/render/tui 15 个文件）：注册断言（`try_query::<...>().expect_log("...registered at init")`）、预检保证的 `get_mut`（`expect_log("Player inventory exists")`）、锁中毒（`FileLogger mutex poisoned`）、digit 转换等。裸 `expect`/`unwrap` 生产路径清零。测试代码保持 expect（不需要日志）。日志未初始化时 `log::error!` 为 no-op，测试安全。

**位置：** `dungeon-core/src/ext.rs` + 15 个生产文件

### G23 — 地图类型单一，楼层无视觉/生态区分 ✅已修复


### G23 — 地图类型单一，楼层无视觉/生态区分 ✅已修复

**修复前：** 所有楼层均为同一洞穴地形；楼梯单一；无生态差异。

**修复后：** 多类型地图落地（Dsn24）：`MapKind`（Cavern/LushCavern/Undersea）+ `map_kind_for(seed, floor)` 确定性派生（F1 固定 Cavern）；6 新方块（菌丝/蘑菇丛/垂藤/沙岸/海草/珊瑚礁）；5 新怪物（孢子怪/蘑菇傀儡/洞穴鱼/洞穴蟹/深鳗）按类型生态权重生成；8 新掉落物（蘑菇/海藻地形消耗品 + 材料）。关键修复：carve_expand 只挖墙、通道挖掘 `carve_channel` 深水变涉水浅水——水域不再被挖穿。分支楼梯暂缓（待类型系统稳定）。

**位置：** `dungeon-core/src/lib.rs`（MapKind/Tile）、`map_gen.rs`、`monster_def.rs`、`dungeon-world/src/population.rs`、`init.rs`、`assets/items.json`

### I70 — 新方块/怪物/掉落物实现 ✅已修复

**修复前：** 无（Dsn24 配套内容）。

**修复后：** Tile serde tag 5-10（旧存档兼容）；MonsterKindId 变体 3-7；物品 ID 25-32；蘑菇/海藻经 is_usable/use_item 回血回蓝（L47 共享判定自动生效）。回归测试：Tile serde 回环、类型特征、怪物定义完整性、按类型种群生成（30 种子×3 类型）、消耗品使用与上限钳制。全 workspace 79 测试通过、clippy 0。

**位置：** 同 G23


### I69 — Dsn19 Phase 1：模板碎片合成系统（材料出口） ✅已修复


### I69 — Dsn19 Phase 1：模板碎片合成系统（材料出口） ✅已修复

**修复前：** 5 种材料无消耗渠道（G11）；模板碎片系统只有定案设计（Dsn19）。

**修复后：** 4 个模板碎片（19-22 剑刃/盾面/甲片/兽牙指环模板）——items.json + ITEM_* 常量 + `template_recipe`/`craft_with_template`（材料检查/空间预检/消耗产出，失败不消耗模板）+ 背包 r 键使用（is_usable 扩展）+ 哥布林 5% 掉落。Inventory 新增 `count_of`/`remove_item`。回归测试 `test_craft_with_template`（成功/材料不足/背包满三路径）。GAME.md Gm7 物品表+掉落表+配方表记录。G11 主条目更新：Phase 1 落地，Phase 2/3 保留 Deferred。

**位置：** `assets/items.json`、`dungeon-core/src/items.rs`、`dungeon-core/src/monster_def.rs`、`src/pages/inventory.rs`、`GAME.md`

### I68 — 投掷 LOS/射程判定双实现未收敛 ✅已修复

**修复前：** `validate_throw`（execute.rs）与 `update_throw_path`（throw.rs）各一份 Bresenham+blocks_vision+射程实现。

**修复后：** core 新增 `ops::los_clear(map, from, to)` 与 `ops::chebyshev(a, b)` 作为唯一实现，两处调用。回归测试 `test_los_clear_and_chebyshev`。

**位置：** `dungeon-core/src/ops.rs`、`dungeon-action/src/execute.rs`、`src/throw.rs`

### I67 — 武器无差异化 ✅已修复

**修复前：** 所有武器共享 300ms 行动耗时，换武器只有数值差异。

**修复后：** `ItemDef.speed` 字段（毫秒）；玩家移动/攻击耗时 = 主手武器 speed × 敏捷系数（无武器 300ms 基准）；新增石锤（450ms 攻击+4 慢速高攻）与匕首（200ms 攻击+2 快速低攻），哥布林 12% 掉落 + 地面物品池。GAME.md Gm7 装备表 + 攻速表 + 数值标注。回归测试 `test_weapon_speed_affects_av`（匕首<空手<石锤）。

**位置：** `assets/items.json`、`dungeon-core/src/items.rs`、`dungeon-action/src/player.rs`、`dungeon-core/src/monster_def.rs`、`dungeon-world/src/init.rs`、`GAME.md`


### A27 — main.rs 单体 → 按页拆分为 pages/ 模块 ✅已修复

**修复前：** `src/main.rs` 662 行集中了全部页面处理器（process_game_key / process_look_key / process_throw_select_key / process_throw_aim_key / process_inventory_key / process_dialog_key），每加一个 UI 元素都要碰 main.rs。

**修复后：** 按 `dungeon_action::Page` 枚举一一拆分为 `src/pages/` 模块（每页一个文件 + mod.rs 分派入口）。main.rs 瘦身至 ~180 行（入口/主循环/标题画面）。`process_key` 分派逻辑进入 pages/mod.rs；`pickup_ground`/`on_stairs` 薄包装删除（各页直接调 `ops::*`）；顺带清理死代码 `throw.rs::get_offhand_name`（零调用方）。重构无行为变化：全 workspace 68 测试通过、clippy 0 警告。README 架构注释同步。

**位置：** `src/main.rs`、`src/pages/`（新增）


### I66 — 背包操作提示与处理器判定未共享（L47 落地） ✅已修复

**修复前：** ui.rs 详情提示与 main.rs 处理器分支两处独立维护；发现 3 处不一致：① 石子/材料详情显示「r:使用/学习」但按 r 提示"不能直接使用"；② 地面详情显示「g:拾取」但处理器 'g' 仅在列表模式生效——**按 g 无反应**；③ 地面详情按 d 会错误地尝试从背包移除；④ 空槽位按 u 误报"背包已满"。

**修复后：** core 新增 `detail_item_actions(item, detail_source) -> Vec<ItemAction>` + `is_usable(item_id)` 共享判定（use_item 复用）；ui.rs 提示与 main.rs 处理器（'e' 装备判定）全部由共享函数驱动。'g' 分支新增地面详情单物品拾取（`ops::pickup_ground_item`，预检背包空间）；'d' 限定背包详情；'u' 区分空槽与背包满。回归测试 `test_detail_item_actions`/`test_pickup_ground_item` 覆盖。

**位置：** `dungeon-core/src/items.rs`、`dungeon-core/src/ops.rs`、`dungeon-render/src/ui.rs`、`src/main.rs`

### D19 — README 无完整操作手册，Gm9「背包 r 键进入投掷瞄准」与实现不符 ✅已修复

**修复前：** README 操作表缺 `x`（查看）/`t`（投掷）/斜向移动/`r`（使用/学习）；架构注释残留已删除的 `Reaction`；Gm9 记载"背包 r 键进入投掷瞄准"（实际 r 键是使用/学习）。

**修复后：** README 操作部分重写（游戏页/背包页两表 + 技能/投掷专节 + "提示什么就能按什么"说明）；架构注释、楼梯落点（G22）同步；GAME.md Gm9 使用方式修正。

**位置：** `README.md`、`GAME.md` Gm9


### G18 — 治愈公式缺法术精通：Gm6 公式与实现不一致 ✅已修复

**修复前：** `execute_skill` 的 Heal 分支只实现 `amount(15) + 熟练度 × 3`，法术精通完全不参与（玩家初始法术精通 8，治愈量少 8 点）。

**修复后：** `execute_skill` 捕获 `stats.magic_mastery`，公式改为 `amount + 法术精通 × 1 + 熟练度 × 3`，与 Gm6 一致。回归测试 `test_heal_includes_magic_mastery` 覆盖。

**位置：** `dungeon-action/src/execute.rs`（execute_skill）

### G19 — 怪物 Stats 的 crit 字段是死数据 ✅已修复

**修复前：** 怪物有 crit_rate/crit_damage 字段但 `execute_chase` 从不暴击。

**修复后：** 怪物邻接攻击接入 Gm2 通用暴击公式（`calc_crit`，无装备加成），日志带「（暴击）」标记。GAME.md Gm2 补充说明怪物适用。

**位置：** `dungeon-action/src/execute.rs`（execute_chase）

### G20 — `execute_throw` 的 `_attacker` 参数未使用 ✅已修复

**修复前：** 投掷硬编码查玩家实体消耗副手，怪物投掷设计预留失效。

**修复后：** `execute_throw` 全程使用 `attacker` 参数（验证副手、消耗副手），不再查询玩家。

**位置：** `dungeon-action/src/execute.rs`

### G21 — population.rs 怪物生成公式未标注（Gm10） ✅已修复

**修复前：** 阈值/扩散概率/数量公式是裸数值，GAME.md 无记录。

**修复后：** GAME.md Gm10 补充公式与 `[⃞直觉]` 标注（阈值 `max(0.38-0.012f, 0.15)`、扩散 0.35、数量 `2f+4~4f+8`）。

**位置：** `GAME.md` Gm10

### D18 — `place_ground_items` 单房间退路硬编码 `SmallRng::seed_from_u64(42)` ✅已修复

**修复前：** 单房间分支用固定种子 42 的独立 RNG，物品位置固定且违反 Dsn16。

**修复后：** `place_ground_items` 增加 `rng` 参数，单房间分支使用传入的楼层 RNG，setup_world/descend 两入口一致。

**位置：** `dungeon-world/src/init.rs`

### A21 — `ops::equip_throwable_to_off_hand` 死代码 ✅已修复

**修复前：** 共享函数零调用方，装填逻辑实际走 `throw.rs::auto_equip_throwable`（I48 ④ 声称统一但未执行）。

**修复后：** 删除死函数。`auto_equip_throwable` 承担全部装填职责（含 I60 换装逻辑）。

**位置：** `dungeon-core/src/ops.rs`

### A22 — `ModalKind`/`ModalRequest`/`ModalState`/`ConfirmAction` 死代码 ✅已修复

**修复前：** 页栈迁移后 4 个模态类型零引用（D17 清理遗漏）。

**修复后：** 全部删除。

**位置：** `dungeon-core/src/resources.rs`

### A23 — `Reaction` 组件悬空：spawn 但零读取 ✅已修复

**修复前：** 所有实体 spawn `Reaction`，三个决策 system 全部忽略它，实际每次现场算 `agility_to_reaction`。

**修复后：** 删除 `Reaction` 组件（类型、init/persist/tests 的 spawn、monster.rs 查询参数），`agility_to_reaction`/`agility_speed_factor` 保留为纯函数。

**位置：** `dungeon-action/src/types.rs`、`dungeon-world/src/init.rs`、`dungeon-world/src/persist.rs`、`dungeon-action/src/monster.rs`

### A24 — 存档用 glyph 反推怪物类型 ✅已修复

**修复前：** `SavedMonster` 无 kind 字段，restore 用 `match glyph` 推断种类，渲染字符与游戏数据耦合。

**修复后：** `MonsterKindId` 组件化（derive Component + Serialize），spawn 时挂载，`SavedMonster.kind: Option<MonsterKindId>`（`#[serde(default)]` 旧存档按 glyph 兜底）。

**位置：** `dungeon-core/src/components.rs`、`dungeon-world/src/init.rs`、`dungeon-world/src/persist.rs`

### A25 — L31 查询约束违例：玩家查询多处缺少 `With<Player>` ✅已修复

**修复前：** persist capture、descend、pickup_ground、execute_chase、timeline（字符串判断玩家）5 处依赖隐式假设。

**修复后：** 全部改为显式含 `&Player` 组件或 `Without<Player>` 过滤的查询。

**位置：** `dungeon-world/src/persist.rs`、`dungeon-world/src/init.rs`、`dungeon-core/src/ops.rs`、`dungeon-action/src/execute.rs`、`dungeon-render/src/timeline.rs`

### A26 — `process_game_key` 三个死参数 + `equipment_bonus` 无用 `_inv` 参数 ✅已修复

**修复前：** `terminal`/`modal_flag`/`game_start` 传而不用；`equipment_bonus(_inv, ...)` 全调用方白传 Inventory。

**修复后：** `process_key`/`process_game_key` 精简为 `(code, world)`；`equipment_bonus(equip)` 单参数，`effective_attack/defense` 同步去参，4 个调用方（execute×2、ops×2、ui）更新。

**位置：** `src/main.rs`、`dungeon-core/src/items.rs`、`dungeon-core/src/ops.rs`、`dungeon-render/src/ui.rs`

### A16 — InputBuffer 资源创建但从未使用 ✅已修复

**修复前：** `InputBuffer`/`RecognizedInput` 定义+插入但全代码库零消费（A16 记录）。

**修复后：** 删除类型与两处 `insert_resource`（init/persist/tests）。

**位置：** `dungeon-action/src/types.rs`、`dungeon-world/src/init.rs`、`dungeon-world/src/persist.rs`、`dungeon-action/src/tests.rs`

### A18 — `ActiveCooldowns` 悬空功能 ✅已修复

**修复前：** 组件有定义有推进但无任何写入点（A18 记录）。

**修复后：** 删除 `ActiveCooldowns`/`Cooldown` 类型与 `advance_action_queue` 中的推进逻辑。技能冷却需求（I24c）标记 Won't Fix——当前 3 技能均无冷却设计，将来需要时重新引入。

**位置：** `dungeon-core/src/components.rs`、`dungeon-action/src/execute.rs`

### D14 — `place_skill_scrolls` 缺少 exclude 参数 ✅已修复

**修复前：** 文档记录卷轴可能生成在楼梯/出生点上（ISSUES 开放区 D14）。

**修复后：** 核实代码：`place_skill_scrolls` 早已带 `exclude` 参数且 setup_world/descend 均传入（与 spawn_monsters/place_ground_items/scatter_stones 一致）。纯文档状态更新，无代码改动。

**位置：** `dungeon-world/src/init.rs:94`

### I61 — 技能卷轴断链：`use_item` 零调用方 + 背包 'r' 键缺失 ✅已修复

**修复前：** 卷轴拾取后无法学习（UI 提示「r:使用/学习」但处理器无 'r' 分支），整个技能系统不可达。

**修复后：** `process_inventory_key` 新增 `'r'` 分支：调 `use_item`（卷轴→learn_skill），消耗 1 个；不可直接使用（如石子）推送提示。回归测试 `test_learn_skill_and_use_item` 覆盖。I24b「技能少且职业锁定」随 Dsn13 无职业设计+卷轴获取落地而关闭。

**位置：** `src/main.rs`、`dungeon-core/src/items.rs`
**教训见：** LESSONS.md L47（UI 提示与处理器同源）

### I59 — 投掷可穿墙 + 超射程：视线/射程检查只在渲染层生效 ✅已修复

**修复前：** `valid_target` 只影响轨迹颜色，Enter 直接入队，`execute_throw` 不验证射程/视线，可穿墙/超距命中。

**位置：** `src/main.rs`、`dungeon-action/src/execute.rs`
**教训见：** LESSONS.md L48（验证必须存在于执行入口）

### I60 — 非投掷物副手被当投掷物消耗 ✅已修复

**修复前：** 副手有物品（如木盾）即可投掷，`consume_off_hand` 无类型检查，木盾被扔出永久消失。

**修复后：** 新增 `items::is_throwable(item_id)`（MVP 仅石子）；Throw 分支/ThrowSelect 确认/`execute_throw` 验证三级检查；`auto_equip_throwable` 副手为不可投掷物时先卸下回背包再装填。回归测试 `test_throw_non_throwable_offhand_cancelled` 覆盖。

**位置：** `dungeon-core/src/items.rs`、`src/main.rs`、`src/throw.rs`、`dungeon-action/src/execute.rs`

### I57 — 背包「装备」无 slot 物品直接 panic ✅已修复

**修复前：** `def.slot.unwrap()` 对材料/卷轴/石子（slot=None）panic；UI 有 guard 但处理器没有。

**修复后：** 'e' 分支改为 `let Some(slot) = def.and_then(|d| d.slot) else { 推送「该物品不能装备」 }`，与 UI 提示一致。

**位置：** `src/main.rs`
**教训见：** LESSONS.md L47（UI 提示与处理器同源）

### I58 — 装备换装旧装备静默丢失（违反 Dsn10 原子语义） ✅已修复

**修复前：** `inv.add(old_stack...)` 忽略返回值，背包满时旧装备永久消失。

**修复后：** 换装前 `can_add` 预检旧装备回背包空间，失败推送「背包已满，无法换装」并放弃操作；预检保证 add 不失败（Dsn10 原子语义）。

**位置：** `src/main.rs`

### I62 — tap-tap Attack 通配确认：任意方向单次触发 ✅已修复

**修复前：** `(Some(Attack{..}), Attack{..}) => true` 通配——预览攻击时按任意方向键立即执行，preview 与执行目标不一致。

**修复后：** 删除通配分支；`handle_player_direction` 对 Attack 单独处理——**同目标**才确认，不同目标仅更新预览，与 Move/Wait/Skill 语义一致。

**位置：** `dungeon-action/src/player.rs`

### I63 — 投掷退出后 LookCursor 残留 ✅已修复

**修复前：** 退出 ThrowAim（Enter/Esc/x）不重置 `LookCursor.active`，地图残留光标高亮。

**修复后：** 两个退出分支均重置 `LookCursor.active = false`。

**位置：** `src/main.rs`

### I64 — `restore_skills` 用 `Box::leak` 每次读档泄漏 ✅已修复

**修复前：** Skill 用 `&'static str` 存文本，读档 `Box::leak` 泄漏。

**修复后：** `Skill.name`/`description` 改为 `String`，`restore_skills` 直接 clone。`skill_from_kind` 同步改 `.to_string()`。

**位置：** `dungeon-core/src/components.rs`、`dungeon-core/src/ops.rs`、`dungeon-world/src/persist.rs`

### I65 — monster_def.rs 掉落表裸数字 item_id（D12 补全） ✅已修复

**修复前：** `monster_loot` 用裸数字（10/11/12/13/14/18）。

**修复后：** 改用 `ITEM_BIOMASS`/`ITEM_CLOTH`/`ITEM_STICK`/`ITEM_FANG`/`ITEM_CHITIN`/`ITEM_STONE` 命名常量。

**位置：** `dungeon-core/src/monster_def.rs`

### G12 — 地面物品每层完全相同 ✅已修复

**修复前：** `ground_item_ids` 固定 8 件（剑盾甲戒各 2），每层一样（G12 记录）。

**修复后：** 新增 `roll_ground_item_ids(rng)`：从基础装备池随机抽 4-8 件，setup_world/descend 共用。GAME.md Gm10 同步更新为「4-8 随机 [⃞试调]」。

**位置：** `dungeon-world/src/init.rs`、`GAME.md`

### G14 — `execute_throw` 中 GameRng 多次 `resource_mut` 调用脆弱 ✅已修复

**修复前：** 3 处独立 `resource_mut::<GameRng>()`（G14 记录）。

**修复后：** 统一绑定为单次 `let (extra, crit_roll) = { let mut rng = ... }`。

**位置：** `dungeon-action/src/execute.rs`

### I48 — throw.rs 架构混乱（剩余项） ✅已修复

**修复前：** ① execute_throw ~75 行职责过多；⑤ `update_throw_path` borrow dance；⑥ 可 panic 路径。

**修复后：** ① 验证逻辑提取为 `validate_throw`（execute_throw 主体显著缩短）；⑤ drop dance 改为块作用域（符合 L42 且 clippy 干净）；⑥ 应用层旧 `.expect()` 已随 D17 删除，剩余 expect 均为 L34 认可的注册断言。

**位置：** `dungeon-action/src/execute.rs`、`src/main.rs`

### I22 — clippy 警告未处理 ✅已修复

**修复前：** 32 个警告（type_complexity、needless_drop、range_loop 等）。

**修复后：** 全部清零：type alias（`RenderableView`/`EntityRenderable`/`GridCell`）、块作用域替代 drop、迭代器改写 range loop、`?` 替代 let-else 等。`cargo clippy --workspace` 0 警告。

### I23 — 测试覆盖缺口（部分修复） 🟡 进行中

**修复前：** dungeon-core 零测试、render 零测试、应用层零测试。

**修复后：** dungeon-core 已有 5 个（EventLog）；本次新增 6 个回归测试（投掷验证×4、卷轴学习、治愈公式），dungeon-action 8→14。dungeon-render 与应用层（main.rs 装备/投掷 UI 流程）仍无测试，需手动验证。

**位置：** `dungeon-action/src/tests.rs`


### I56 — 输入线程未过滤 `KeyEventKind::Release`，导致同键触发 2-3 次 ✅已修复

**修复前：** 输入线程仅靠 50ms 同键去重过滤重复按键。`KeyEventKind::Release` 事件与 `Press` 的 `key.code` 相同，依赖去重窗口过滤。但 Release 的到达时间受终端调度影响不可控（可跨 1-3 个 poll 周期），当 50ms 窗口刚好闭合时 Release 通过，产生"按一次触发 2-3 次"的效果。

**修复后：** 双重过滤：
1. 环境自适应：丢弃 `key.kind != KeyEventKind::Press`（现代终端区分事件类型，传统终端所有事件为 Press，不受影响）
2. 去重窗口 50ms→33ms，与帧率（30FPS）对齐，tap-tap 不受影响

**位置：** `src/main.rs:62-72`（输入线程事件循环）
**教训见：** LESSONS.md L46

**修复前：** `render_inventory_overlay` 在 `inv_state.detail == true` 时仅显示操作提示行，无物品名称/属性/描述。

**修复后：** 根据 `detail_source`（装备/背包/地面）获取物品，渲染完整的详情视图：标签、名称（黄色加粗）、数量、类别、属性加成、描述、上下文操作提示。参考旧版 `inventory.rs:81-133` 的详情渲染逻辑。

**位置：** `dungeon-render/src/ui.rs`（render_inventory_overlay detail 分支）

### D17 — 页栈迁移后大量旧阻塞式 UI 死代码未清理 ✅已修复

**修复前：** `src/inventory.rs`（open_inventory, ~300行）、`src/input.rs`（InputDriver/EventBus, ~110行）、`src/throw.rs`（open_throw_select/open_throw_aim, ~160行）、`src/main.rs`（open_modal/open_look_mode）共 6 处旧阻塞式 UI 死代码保留在代码库中，违反 LESSONS L20。

**修复后：**
1. 删除 `src/inventory.rs`（整个文件）
2. 删除 `src/input.rs`（整个文件）
3. 从 `src/lib.rs` 移除 `pub mod inventory;`
4. 删除 `src/throw.rs` 中的 `open_throw_select` 和 `open_throw_aim`（保留 `update_throw_path`、`get_offhand_name`、`auto_equip_throwable`）
5. 删除 `src/main.rs` 中的 `open_modal`、`open_look_mode` 及 `#[allow(dead_code)]`

**位置：** 见上表

### I52 — 背包页栈缺少 'u' 键卸载装备 ✅已修复

**修复前：** `process_inventory_key` 的 match 分支包含 `Esc/Left/Right/Up/Down/Enter/e/d/g`，但没有 `KeyCode::Char('u')` 处理。玩家无法从装备槽卸载装备回背包。

**修复后：** 添加 `KeyCode::Char('u') if detail && detail_source == 1` 分支。先读取（不可变借）Equipment/Inventory 检查背包容量和物品信息，再（可变借）执行卸载。背包满时推送"背包已满"日志。

**位置：** `src/main.rs`（process_inventory_key）

### I51 — 背包页栈渲染 `render_inventory_overlay` 完全缺失装备栏 ✅已修复

**修复前：** `render_inventory_overlay` 仅渲染背包物品列表，没有装备槽位显示。`left_total` 仅取 `inv.stacks.len()`，渲染与选中逻辑的索引体系不一致。第 201 行 `if i >= 4 { " (装备)" }` 是旧索引残留。

**修复后：**
1. 左栏改为装备段（4行：`[主]`/`[副]`/`[防]`/`[戒]`）+ 背包段（`── 背包 (x/y) ──` 分隔线 + 物品列表）
2. `left_total` 改为 `4 + inv.stacks.len()`
3. 移除误导性 `" (装备)"` 标注
4. 同时修复 `process_inventory_key` 中 `detail_idx + 4` 的索引 bug（应是 `detail_idx`，因为 `detail_idx` 已是段内偏移）

**位置：** `dungeon-render/src/ui.rs`（render_inventory_overlay）、`src/main.rs`（process_inventory_key）

### G17 — 弹道轨迹渲染覆盖实体 glyph 且丢失地形背景 ✅已修复

**修复前：** 弹道轨迹在实体叠加层之后渲染，使用 `Color::Reset` 背景覆盖实体 glyph 并丢失地形纹理。（位置：`pipeline.rs:114`）

**修复后：** 弹道轨迹渲染时保留该格已有背景色 `lines[idx][jdx].2`，轨迹 `*` 不再覆盖实体 glyph，且背景色与地形一致。

**位置：** `dungeon-render/src/pipeline.rs:114`

### I55 — 查看模式页栈缺少 Home/End 快捷键 ✅已修复

**修复前：** `process_look_key` 只有方向键和 Esc/x 退出，无 Home/End 跳转。玩家无法一键跳到地图角落。

**修复后：** 添加 `KeyCode::Home` → `(0,0)` 和 `KeyCode::End` → `(MAP_WIDTH-1, MAP_HEIGHT-1)` 处理分支。

**位置：** `src/main.rs:164-171`（process_look_key）

### I54 — 投掷选择页栈缺少无投掷物反馈 ✅已修复

**修复前：** `process_throw_select_key` 在 `auto_equip_throwable` 失败后静默 fall-through，玩家无任何反馈。

**修复后：** 添加 `else` 分支推送 `"没有可投掷的物品"` 到 EventLog。

**位置：** `src/main.rs:194`

### A20 — `ops::consume_off_hand` 共享函数存在但未被 `execute_throw` 调用 ✅已修复

**修复前：** `execute_throw` 使用 8 行内联副手消耗实现，未调用 `ops::consume_off_hand` 共享函数，违反 DRY。

**修复后：** 替换为 `ops::consume_off_hand(world, p)` 调用。

**位置：** `dungeon-action/src/execute.rs:420-421`、`dungeon-core/src/ops.rs:97-108`

---

### I50 — `place_skill_scrolls` 的 `_floor` 参数投入使用 ✅已修复

**修复前：** `_floor: u32` 带下划线前缀标注未使用，函数体内从未使用。

**修复后：** 去掉 `_` 前缀，卷轴生成数量加入楼层缩放：`count = rng.random_range(1..=3) + floor.saturating_sub(1) / 5`。F1-4 保持 1-3 张，F5-9 变成 2-4 张，F10+ 变成 3-5 张。

**位置：** `dungeon-world/src/init.rs:94`

### I44 — `descend` 中 `GameRng` 种子与 `setup_world` 不一致 ✅已修复

**修复前：** `setup_world` 种 `GameRng` 为 `map_seed.wrapping_add(42)`，但 `descend` 不创建或重置 `GameRng`，下楼后旧 RNG 状态继续使用。

**修复后：** `descend` 中插入 `GameRng::new(base_seed.wrapping_add(f as u64).wrapping_add(42))`，下楼后 RNG 种子可复现，与 `setup_world` 模式一致。

**位置：** `dungeon-world/src/init.rs`

---


### I43 — `calc_player_crit` 与 execute_attack 内联暴击计算重复 ✅已修复

**修复前：** `execute_attack`（行 302-305）和 `calc_player_crit`（行 374-417）各自独立实现暴击率/倍率计算，逻辑几乎相同但代码重复。未来修改需同步两处。

**修复后：** 提取 `calc_crit(stats, bonus, crit_roll) -> (bool, f32)` 共享函数。`execute_attack` 和 `calc_player_crit` 统一调用此函数。同时状态面板暴击率改为显示有效值（含 `equipment_bonus().crit_rate`）。

**位置：** `dungeon-action/src/execute.rs`、`dungeon-render/src/ui.rs`

---

### A14 — `open_look_mode` 迁移到页栈 ✅已修复

**修复前：** `open_look_mode` 直接在函数体内 `terminal.draw()` 渲染 + `event::read()` 处理输入，完全绕过了主循环的渲染编排。

**修复后：** 查看模式由页栈 `Page::Look` 管理。按键由 `process_look_key` 处理（方向键移动光标、x/Esc 退出），渲染走管道主线（`render_map_grid` 中由 `LookCursor` 叠加光标高亮，状态面板显示光标信息）。消除了独立的 `event::read()` 循环。

**位置：** `src/main.rs`、`dungeon-render/src/ui.rs`、`dungeon-action/src/types.rs`

---


### D15 — Skills 组件下楼和存档丢失 ✅已修复

**修复前：** `descend()` query 遗漏 `&Skills`，下楼后用 `player_class.skills()` 重建空列表。`GameSave::capture()` query 同样遗漏，`restore()` 用 `pc.skills()` 重建空列表。所有已学技能在下楼和存档读档后丢失。

**修复后：**
1. `descend` query 加入 `&Skills`，重建时使用已捕获的 skills 列表
2. `components.rs`: `Skills` 加 `Clone` derive
3. `persist.rs`: `GameSave` 新增 `skills` 字段（`#[serde(default)]` 兼容旧存档），capture/restore 路径加入 Skills 序列化/反序列化（SavedSkill 中转）

**位置：** `dungeon-world/src/init.rs:258-265`、`dungeon-world/src/persist.rs:68-247`、`dungeon-core/src/components.rs:172`

### I49 — 玩家死亡无事件日志推送 ✅已修复

**修复前：** `check_death_system` 中检测到 `stats.hp <= 0` 时仅设置 `game_over = true`，未向 EventLog 推送死亡消息。

**修复后：** `check_death_system` 增加 `mut event_log: ResMut<EventLog>` 参数，死亡时 `event_log.push("你死了")`。

**位置：** `dungeon-core/src/systems.rs:16-22`

### I42 — 投掷暴击日志格式与近战攻击不一致 ✅已修复

**修复前：** 投掷暴击日志为 "石子命中老鼠！造成8伤害暴击"（暴击标记在末尾、缺"点"、缺分隔符）。

**修复后：** 改为 "石子命中了老鼠！暴击，造成8点伤害"，与近战攻击日志格式一致。

**位置：** `dungeon-action/src/execute.rs:389`

### G16 — `enqueue_if_absent` 语义可能导致操作被吞 ✅已修复

**修复前：** `handle_timed_action` 在确认后调用 `enqueue_if_absent`，按**实体**去重（同实体有任何行动在队列即拒绝入队）。玩家 Move 排队时无法再 Attack，按键无声无反应。

**修复后：** `ActionKindV3` 加 `PartialEq` derive。`enqueue_if_absent` 改为 `enqueue_or_replace`（替换语义：移除实体旧行动→添加新行动）。`handle_timed_action` 调用 `enqueue_or_replace`。

**位置：** `dungeon-action/src/types.rs:119-124`、`dungeon-action/src/player.rs:16`

### R1 — RULE.md 编辑流程优化（移除宣誓 + 强化记录优先） ✅已修复

**修复前：** RULE.md 要求编辑前"宣誓"，AI 须在每回合第一次编辑前声明流程步骤。实际效果不佳（"反智能体"），且核心问题（先修后记）未被有效约束。

**修复后：**
1. 移除"宣誓"机制
2. 新增醒目 🚨 区块：**用户报告问题 → 先记录 ISSUES → 再修复**
3. 简化编辑前检查清单，保留三条核心检查
4. 同步保存高优先级 memory（`rulemd-bug-report-flow`），确保每轮启动可见

**位置：** `RULE.md` §六

### P8 — 主循环空闲 sleep 1ms 导致有限机型 CPU 满载 ✅已修复

**修复前：** 主循环无输入时 `sleep(1ms)`，渲染一帧约 5ms，合计 6ms/帧 ≈ 166fps 空转。有限机型上单核 100% 满载，系统可能因过热/调度杀死进程。

**修复后：** 空闲 sleep 改为 32ms，渲染频率降到约 27fps。回合制终端游戏在无操作时不需要高刷新率。

**位置：** `src/main.rs:88`

### I47 — `throw.rs` 残存 `.unwrap()` 违反 I17 ✅已修复

**修复前：** `execute_throw` 中 `player.unwrap()` 是 I17 修复后引入的新 `.unwrap()`。虽然 `player.is_none() ||` 短路保护了它，但重构时脆弱。

**修复后：** 改为 `match player { None => ..., Some(p) => ... }` 彻底消除 unwrap。

**位置：** `src/throw.rs:253`

### I46 — bevy_ecs resource_mut 双重借用导致崩溃 ✅已修复

**修复前：** `open_throw_aim` 中 `resource_mut::<ThrowPreview>()` 返回 `Mut<ThrowPreview>` 后未 drop，就调用 `update_throw_path(world)`——后者内部再次 `resource_mut::<ThrowPreview>()`。bevy_ecs 内部 UnsafeCell 运行时检测到同一资源的二次可变访问并 panic。这是「按 t → 回车」后崩溃的直接根因。

**修复后：** `Muts` 作用域在调用 `update_throw_path` 前结束（`{ let mut tp = ...; }` block 提前 drop）。同时 Enter 分支中的嵌套 `get_mut` 改为三阶段顺序操作。

**教训：** bevy_ecs 的 `Mut<T>` 存活期间不得再通过任何路径调用 `world.resource_mut::<T>()`——编译器不报错（内部 UnsafeCell），但运行时 panic。

**位置：** `src/throw.rs:163-167`

### I45 — 光标穿墙显示怪物真实位置 ✅已修复

**修复前：** `build_stats_panel` 中光标位置的实体查询不检查可见性。光标移动到已探索但当前不可见格（如墙后）时，仍然显示该格的怪物名称和 HP。

**修复后：** 只有光标在**当前可见格**时才显示实体名+HP。已探索不可见格只显示地形名+"(已探索)"，未探索格显示"(未探索)"。

**位置：** `dungeon-render/src/ui.rs:280-320`

### A15 — 主手/副手系统重构 ✅已修复

**修复前：** `Equipment` 只有一个 `weapon` 槽位，无法区分主手和副手。木盾装备在 `Armor` 槽（与皮甲同槽），投掷动作没有来源位置。

**修复后：**
1. `EquipmentSlot`：`Weapon` → `MainHand`，新增 `OffHand`
2. `Equipment`：`weapon` 改为 `main_hand`，新增 `off_hand`
3. `items.json`：锈铁剑 slot → `MainHand`，木盾 slot → `OffHand`
4. `t` 键：打开投掷物选择弹窗 → `r`/`y` 切换 → 自动装副手 → 瞄准模式
5. 投掷伤害不包含主手武器攻击力加成
6. 状态栏显示主手/副手/防具/戒指 4 行装备信息

**位置：** `dungeon-core/src/items.rs`（EquipmentSlot + Equipment）、`src/throw.rs`（投掷选单+瞄准）、`src/inventory.rs`（装备/卸装）、`dungeon-render/src/ui.rs`（状态栏装备显示）

### G10 — 暴击率纳入装备加成 ✅已修复

**修复前：** `execute_attack()` 中暴击判定只用 `attacker_stats.crit_rate`（基础值 5%），`equipment_bonus()` 返回的 `StatBonus.crit_rate` 从未被使用。背包详情页显示的 crit_rate 加成不生效。

**修复后：** 计算有效暴击率时加入 `bonus.crit_rate`，`total_crit_rate = attacker_stats.crit_rate + bonus.crit_rate`，上限钳制为 1.0。

**位置：** `dungeon-action/src/execute.rs:302`

### I25 — `CanMove::condition()` 已删除 ✅已修复

**修复前：** `CanMove::condition()` 是 Action 组件中唯一有定义无调用的静态条件方法。对比 `CanChase::condition`（被 `chase_decision_system` 调用）和 `CanFlee::condition`（被 `flee_decision_system` 调用），`CanMove::condition` 处于悬空状态。

**修复后：** 删除 `CanMove::condition()` 方法。Move 的保活检查由 `check_condition` 中的 `can_move_to()` 内联处理。

**位置：** `dungeon-action/src/types.rs:61-65`

### A13 — 地面拾取逻辑统一使用 ops::pickup_ground ✅已修复

**修复前：** `inventory.rs` 的 'g' 键分支包含一份与 `ops::pickup_ground()` 几乎相同的拾取逻辑实现（查玩家位置→查同格 ItemPickup→添加 Inventory→despawn→推日志）。两处代码重复，未来变更需同步修改。

**修复后：** `inventory.rs` 的 'g' 键直接调用 `ops::pickup_ground(world)`，消除重复。

**位置：** `src/inventory.rs:260-280`

### A12 — `descend` 中 player_data 改用具名变量 ✅已修复

**修复前：** `descend()` 用一个 9 元组 `player_data` 传递玩家数据，成员通过 `.0`/`.1`/…`.8` 索引访问。索引易错、增加组件时需手动同步编号。

**修复后：** 元组解构改为 7 个具名局部变量（`player_stats`/`player_inv_stacks`/`player_equip`/`player_class`/`player_atk_name`/`player_active_buffs_vec`），赋值处按名称引用。

**位置：** `dungeon-world/src/init.rs:250-281`

### D12 — 物品 ID 提取为命名常量 ✅已修复

**修复前：** 物品 ID（0/1/2/3/10/11/12/13/14/15/16/17）在 `init.rs`（`scroll_ids` 数组和 `ground_item_ids`）、`inventory.rs`（`match item_id`）等多处以裸 `usize` 字面量出现。不可 grep、不可追踪，重构时无声错位。

**修复后：** 在 `dungeon-core/src/items.rs` 中定义 `pub const ITEM_RUSTY_SWORD = 0` 等 12 个命名常量。`init.rs` 中的 `scroll_ids` 和 `ground_item_ids` 改用常量引用。`inventory.rs` 的 'r' 键不再直接引用 ID（见 D12）。

**位置：** `dungeon-core/src/items.rs:7-18`（常量定义）

### D11 — 物品使用行为集中分派（use_item 函数） ✅已修复

**修复前：** `UsableItem` trait 和 `SkillScroll` 的 impl 已存在，但 `inventory.rs` 的 'r' 键仍然使用 `match item_id { 15 => ..., 16 => ..., 17 => ... }` 硬编码 match。trait 是悬空的抽象债务。

**修复后：** `items.rs` 新增 `pub fn use_item(item_id, world, user) -> bool` 集中分派函数，内置 match 逻辑。`inventory.rs` 的 'r' 键通过 `dungeon_core::use_item(id, world, player)` 调用。所有物品使用行为集中到 items.rs 一处管理，不再散布在 UI 层。

**位置：** `dungeon-core/src/items.rs`（use_item 函数）、`src/inventory.rs`（'r' 键调用方）

### D10 — `Buffs` 旧组件死代码已删除 ✅已修复

**修复前：** `buff_tick_system` 在 D10 中删除，但 `Buffs` 结构体（含 4 个废弃字段 `shield_turns`/`shield_def`/`berserk_turns`/`berserk_atk`）及其 impl 仍完整保留在 `components.rs` 中。`descend` 中 `Buffs::new()` 作为占位符传入，存档中的 `SavedBuffs` 仍在序列化。违反 LESSONS L39 Phase 3。

**修复后：** 删除 `Buffs` 结构体、所有 impl 块、`SavedBuffs` 序列化结构。清理 `descend` 中的 `Buffs::new()` 占位参数。清理 `persist.rs` 中的 `buffs` 字段和 `SavedBuffs` 转换。清理 `tests.rs` 中的 `Buffs` 导入和 `Buffs::new()` 调用。

**位置：** `dungeon-core/src/components.rs`、`dungeon-world/src/init.rs`、`dungeon-world/src/persist.rs`、`dungeon-action/src/tests.rs`
**教训见：** `LESSONS.md L41`

### A9 — 事件日志显示条数回归（take(5)→take(12)） ✅已修复

**修复前：** `ui.rs` 使用 `.take(5)`，战斗密集时事件日志关键信息快速滚出屏幕。G13 声称修复了但代码未改。

**修复后：** `.take(5)` → `.take(12)`。

**位置：** `dungeon-render/src/ui.rs:162`

### I36 — `lib.rs` pathfinding 注释矛盾（误导注释已清理） ✅已修复

**修复前：** 同一文件同时有生效的 `pub mod pathfinding;` 和声称"已移除"的注释。

**修复后：** 删除两条误导注释。

**位置：** `dungeon-core/src/lib.rs:7-9`

### I40 — 技能键索引与快捷键错位：已学习但按对应键无反应 ✅已修复

**问题：** 卷轴学习将技能追加到 `Skills.list` 末尾。`handle_skill` 用固定索引访问（按键 1→idx=0，按键 2→idx=1...），但技能的实际位置取决于学习顺序。例如先学护盾（快捷键 2）→ 护盾在 `list[0]`，按 2 键却查 `list[1]`→ 返回 None，技能无声失败。**这不是没学的问题，是学了但位置不对。**

**修复后：**
1. `handle_skill`（`dungeon-action/src/player.rs:68`）：改为按键索引 `0→'1'、1→'2'…`，在技能列表中按 `sk.key` 字符查找实际位置，不再假设顺序
2. 传入 action 的索引是 `real_idx`（技能在 list 中的实际位置），`execute_skill` 直接命中

**位置：** `dungeon-action/src/player.rs:68-80`

### D8 — 下楼不保存 ActiveBuffs ✅已修复

**问题：** `descend()` 中 `player_data.5` 硬编码为 `Buffs::new()`（空），且 `ActiveBuffs` 组件完全未在 `descend` 中捕获和重建。下楼后玩家身上的护盾/狂暴 Buff 全部丢失。

```rust
// init.rs:196 — 永远空的
Buffs::new(), cls.clone(), atk.0.clone())
// init.rs:207 — 下楼后插入的也是空的
cmd.insert(ActiveBuffs::new());
```

**对比：** 存档/读档（`persist.rs`）正确保存和恢复了 `ActiveBuffs`——说明下楼丢失不是有意设计，而是遗漏。

**影响：** 🟡 中 — 玩家在楼梯口开 Shield 下楼→Buff 消失，与存档读档行为不一致。

**位置：** `dungeon-world/src/init.rs:193-207`


### D9 — buff_tick_system 已删除 ✅已修复

**问题：** `buff_tick_system` 每帧修改旧 `Buffs` 组件的 `shield_turns`/`berserk_turns`/`shield_def`/`berserk_atk` 字段。但 `effective_attack`/`effective_defense` 已在 G14 修复中改为只读新 `ActiveBuffs`。旧 Buffs 的修改永远不会被消费。

```rust
// systems.rs:47-50 — 仍在运行，产生无用副作用
pub fn buff_tick_system(mut query: Query<&mut Buffs, With<Player>>) {
    for mut b in query.iter_mut() {
        if b.shield_turns > 0 { b.shield_turns -= 1; if b.shield_turns <= 0 { b.shield_def = 0; } }
        if b.berserk_turns > 0 { b.berserk_turns -= 1; if b.berserk_turns <= 0 { b.berserk_atk = 0; } }
    }
}
```

**违反 LESSONS L39：** 双系统共存应推进到 Phase 3（移除旧系统），当前停留在 Phase 1 且 `buff_tick_system` 仍在 Schedule 中注册并每帧运行。

**位置：** `dungeon-core/src/systems.rs:47-50`、`dungeon-world/src/tick.rs:13`

---

### I37 — effective_attack/defense 删除废弃 _buffs 参数 ✅已修复

**问题：** `effective_attack` 和 `effective_defense` 带有 `_buffs: Option<&Buffs>` 参数，前缀下划线表示"不使用"。G8 修复时移除了求和逻辑但保留了参数占位，所有调用方仍在传入 `world.get::<Buffs>(entity)` 做无用查询。

```rust
pub fn effective_attack(
    stats: &Stats, inv: &Inventory, equip: &Equipment,
    _buffs: Option<&Buffs>,           // ← 废弃参数，从不使用
    active_buffs: Option<&ActiveBuffs>,
) -> u32
```

**违反 LESSONS L39：** 新旧系统共存应推进到 Phase 3（移除旧系统引用），当前停留在 Phase 1 未进展。

**位置：** `dungeon-core/src/ops.rs:24-50`；调用点：`execute.rs:293`、`ui.rs:134`

### I38 — 物品系统行为抽象层（UsableItem trait 已定义） ✅已修复

**问题：** MC 的物品系统有三层：`Registry → ItemStack → Item 虚方法`。本项目的物品只有前两层——`ItemDef` 是纯数据结构，没有任何方法。物品"能做什么"的逻辑必须散落在外部 match 中：

```rust
// 没有统一的 use() 抽象，只能 match item_id
match item_id {
    20 => learn_skill(world, SkillKind::Heal),
    21 => learn_skill(world, SkillKind::Shield),
    // 加一种可消耗品 → 加一个 arm
    _ => {},
}
```

**MC 的做法（Item 虚方法）：**
```java
public class Item {
    public InteractionResult use(Level level, Player player, InteractionHand hand) { ... }
    public void inventoryTick(ItemStack stack, Level level, Entity entity, int slot, boolean selected) { ... }
}
```

每件物品通过继承/impl 定义自己的行为，调用方只需 `item.use(...)`——不需要 match。

**影响：** 当前仅装备类有行为（加 stat），材料类完全无用途。即将做的技能卷轴、未来的药水/食物/卷轴都需要行为抽象。没有的话每加一种可交互物品都要改 inventory.rs 和/或 process_key。

**建议方向：** 定义 `UsableItem` trait（与 A11 的 `MonsterBehavior` 同一类问题——用虚表代替枚举 match）：

```rust
pub trait UsableItem {
    fn use_on(&self, world: &mut World, user: Entity) -> bool;
    fn use_verb(&self) -> &'static str;
    fn can_use(&self, world: &World, user: Entity) -> bool;
}
```

**位置：** `dungeon-core/src/items.rs`（ItemDef 定义处，无方法）

### I39 — ItemStack 增加 ItemMeta（NBT 等价物） ✅已修复

**问题：** `ItemStack` 只有 `(item_id, count)` 两个字段，没有存储任意元数据的容器。MC 的 `CompoundTag`（NBT）支持自定义名称、附魔、耐久度、品质/层级等任意键值对。

```rust
// 当前 ItemStack — 无扩展空间
pub struct ItemStack {
    pub item_id: usize,
    pub count: u32,
    // 没有第 3 个字段
}
```

**影响：** 🔴 高 — 以下功能在没有 NBT 等价物的情况下要么不可能，要么需要绕路：
| 功能 | 无 NBT 的代价 |
|------|-------------|
| 装备层级（+1/+2/+3） | 加字段到 ItemStack 或另加 ECS 组件 |
| 附魔/自定义属性 | 需要新组件 + 查询链 |
| 自定义名称 | 不可能——永远是模板名称 |
| 耐久度 | 需要新组件 + 存档迁移 |
| 词缀（前缀/后缀） | 不可能——无法存"锋利的长剑"vs"迅捷的长剑" |

**建议方向：** 在 `ItemStack` 中加一个通用元数据容器，`#[serde(default)]` 兼容旧存档：

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ItemMeta {
    pub name: Option<String>,
    pub tier: u32,
    pub enchantments: Vec<Enchantment>,
    pub durability: Option<u32>,
    pub tags: Vec<String>,
}

pub struct ItemStack {
    pub item_id: usize,
    pub count: u32,
    #[serde(default)]
    pub meta: Option<Box<ItemMeta>>,
}
```


### I35 — 怪物颜色统一使用 Renderable 组件（地图+行动轴） ✅已修复

**修复前：** `timeline.rs` 和 `ui.rs` 使用 `entity_color(entity.to_bits(), 0)` 实时哈希计算怪物颜色。读档后 Entity ID 重建导致颜色不一致。更根本的问题是：独特色是"渲染时实时计算的"，不被持久化。

**修复后：**
1. `entity_color` + `hsv_to_rgb` 从 `dungeon-render` 移至 `dungeon-core/src/color.rs`（纯数学，无 TUI 依赖）
2. `spawn_monsters` 在 spawn 后立即用 `entity_color(entity.to_bits(), 0)` 写入 `Renderable.color`——独特色在生成时固定
3. `timeline.rs` 改为直接读取 `Renderable.color`，不再实时哈希
4. `ui.rs` 移除怪物 entity_color 覆写，直接使用 renderable 的已存颜色

**效果：** 独特色在存档中持久化（SavedMonster 的 r/g/b 字段），读档/下楼后地图和行动轴颜色一致。

**位置：** `dungeon-core/src/color.rs`（新模块）、`dungeon-world/src/init.rs:67`、`dungeon-render/src/timeline.rs:41`、`dungeon-render/src/ui.rs:126`
**教训见：** `LESSONS.md L40`（新增——独特色应在生成时固定存储于组件，而非渲染时实时计算）

### A10 — 删除 Stats::monster() 死代码 ✅已修复

**修复前：** `components.rs` 中 `Stats::monster(glyph, floor)` 无调用方，缺少蝎子匹配，功能完全重复于 `monster_def::monster_stats()`。

**修复后：** 删除整个方法（~30 行死代码）。

**位置：** `dungeon-core/src/components.rs:120-156`

### A4La — Map 残留 generate_water / is_away_from_rooms / count_walkable_neighbors 死方法 ✅已修复

**修复前：** A4/A4L 后 Map impl 仍有三个零调用的方法。同模式第三次发生。

**修复后：** 删除三个方法。Map impl 仅保留 `count_tile` / `count_neighbor_tile` / `carve_corridor` / `render` / `spawn_point`。

**位置：** `dungeon-core/src/lib.rs`
**教训见：** `LESSONS.md L38`

### I33 — 丢弃物品产生地面拾取物 ✅已修复

**修复前：** 背包详情页按 `d` 直接 `inv.drop_stack(idx)` 删除物品栈，物品永久消失。丢弃是唯一不可逆的物品销毁路径。

**修复后：** 丢弃时获取玩家位置，在地面 spawn ItemPickup 实体（含 glyph/color）。事件日志显示"丢弃了xxx在脚下"。

**位置：** `src/inventory.rs:268-280`

### I34 — ActiveBuffs 未加入存档 ✅已修复

**修复前：** `GameSave` 仅保存旧 `Buffs`，玩家在 Buff 持续期间存档后，读档后 Buff 丢失。

**修复后：** `GameSave` 新增 `active_buffs: Vec<SavedActiveBuff>` 字段（`#[serde(default)]` 兼容旧存档），capture 时序列化玩家 ActiveBuffs，restore 时重建 Buff 列表。

**位置：** `dungeon-world/src/persist.rs`

### I32 — SkillKind::duration 单位歧义（回合/秒） ✅已修复

**修复前：** `duration: i32` 可负值；旧 Buffs 系统读作 3 帧 ≈ 50ms，新 ActiveBuffs 读作 3 秒；技能描述写"持续3回合"。

**修复后：** `duration` 改为 `u32`（禁止负值）；技能描述统一为"持续3秒"。

**位置：** `dungeon-core/src/components.rs:160-164`

### G8 — 护盾/狂暴技能双倍叠加（执行层移除旧系统写入） ✅已修复

**修复前：** `execute_skill` 同时写入旧 `Buffs` 和新 `ActiveBuffs`，`effective_attack`/`effective_defense` 对两者求和。每次使用 Shield/Berserk 时护盾/狂暴数值在 ~3 帧内翻倍（+10 而非 +5）。

**修复后：** `execute_skill` 移除了旧 `Buffs` 写入路径，`effective_attack`/`effective_defense` 只读新 `ActiveBuffs`（旧 Buffs 参数保留但不再参与计算）。使用技能护盾/狂暴正确只加 +5。

**位置：** `dungeon-action/src/execute.rs:315-340`、`dungeon-core/src/ops.rs:24-50`
**教训见：** `LESSONS.md L39`

### I27 — 怪物颜色可区分性差 ✅已修复

**问题：** 不同怪物的 glyph 颜色过于接近，玩家难以区分。首次分配颜色时相邻实体 ID 的哈希差异太小，视觉上像是同一个颜色。

**最终方案：** 用 `SipHash(entity_bits ⊕ seed)` 的高位直接映射 RGB，无基准色限制，微小 ID 变化经 hash 后产生大幅颜色跳跃。

**位置：** `dungeon-render/src/color.rs:12-20`

### I27L — unique_color 哈希扩散不足（第一次修复不完整） ✅已修复

**问题：** `unique_color` 取 `entity.to_bits()` 低 6 位偏移 ±32，相邻 ID 色差 <1。增量方法太线性。

**修复：** 改用黄金比例 `wrapping_mul` 扩散 + 范围采样 ±64。但极端基准色（老鼠 `255,0,0`）的通道被 clamp 吞噬，仍无差异。

### I27La — 颜色区分度仍不足（第二次修复仍不完整） ✅已修复

**问题：** 黄金比例扩散 + 范围采样被通道 clamp 吞噬，特定基准色下相邻实体仍无视觉差异。

**修复：** 废弃基准色方案，改用 `SipHash` 高位直接映射 RGB。见 I27 最终方案。

**关联：** I27（主条目）

### I29 — 泛型 Buff 系统（ActiveBuffs + AV 推进） ✅已修复

**修复前：** Buff 使用回合计数（`shield_turns: i32`），与 AV 时间轴脱钩。每帧减 1 回合，不同帧消耗速度不同。技能只能通过职业锁定。

**修复后：** 新增 `ActiveBuffs(Vec<Buff>)` 和 `ActiveCooldowns(Vec<Cooldown>)` 泛型组件，`advance_action_queue` 中与队列同步推进（`remaining_av -= dist`）。`effective_attack/defense` 查询 ActiveBuffs。旧 `Buffs` 组件保留过渡期兼容。

**位置：** `dungeon-core/src/components.rs`、`dungeon-action/src/execute.rs`、`dungeon-core/src/ops.rs`

### I30 — UI 整合：Buff/视野/HP 标注移至行动轴 ✅已修复

**修复前：** Buff 显示在 stats 面板（文本行），视野实体显示在 stats 面板底部，行动轴只显示行动名和倒计时。信息分散。

**修复后：** 行动轴整合为三区：①队列条目（符号+行动+耗时）②分割线③实体状态（符号+怪物名+血量）④次级标注（Buff，dim 样式）。stats 面板移除 Buff 和视野段。

**位置：** `dungeon-render/src/timeline.rs`、`dungeon-render/src/ui.rs`

### I31 — x 键光标查看模式 ✅已修复

**修复前：** 无查看模式，玩家无法了解地图上未知位置的详细信息。

**修复后：** 新增 `LookCursor` 资源 + `open_look_mode`（方向键移动、x/Esc 退出）。地图上光标格叠加暗黄色背景高亮。stats 面板底部显示光标位置的地形名和实体名+HP。

**位置：** `dungeon-core/src/resources.rs`、`src/main.rs`、`dungeon-render/src/ui.rs`

### I28 — 事件日志从 stats 面板移至地图下方 ✅已修复

**修复前：** 事件日志位于右侧 stats 面板底部，占用了属性显示空间且不易阅读。

**修复后：** 地图区增加垂直分割，地图占 `VIEWPORT_HEIGHT`，下方独立显示事件日志（`── 事件 ──` 分隔线，最近 5 条）。

**位置：** `dungeon-render/src/ui.rs`

### G7 — 玩家面板显示不应公开的调试信息（房间数/怪物数） ✅已修复

**修复前：** 属性面板中显示 `房间 N` 和 `怪物 N`，这些是地图生成和种群统计的调试数据，玩家不应看到。行动轴宽度 22 偏高，压缩了地图和属性区的可用空间。事件日志仅显示 5 条，战斗密集时关键信息快速滚出屏幕。

**修复后：** 删除房间/怪物数量行。行动轴收窄至 16，释放水平空间。事件日志增至 12 条。

**位置：** `dungeon-render/src/ui.rs`

### G5 — `rooms[0].center()` 不可行走导致出生卡墙 ✅已修复

**修复前：** `spawn_point()` 直接返回 `rooms[0].center()`，不做 walkable 校验。`generate_stalactites` 在房间内每格 7% 概率将 Floor 变 Stalactite，可能覆盖房间中心点；`ensure_spawn_accessible` 只检查邻居不检查中心自身。下楼后玩家可能在不可行走格上出生，无法移动。

**修复后：** `spawn_point()` 先检查中心是否 walkable，若否则以螺旋搜索（半径 1→20）寻找最近的可行走格。确保返回值永远可通行。

**位置：** `dungeon-core/src/lib.rs:421-442`
**触发条件：** `generate_stalactites` 在 room[0] 每格 7% 概率 → 约每 14 次下楼触发一次。

### A7 — 拆分 ops.rs 为 fov / pathfinding / ops ✅已修复

**修复前：** `ops.rs` 是万能工具袋——FOV、A\*、公式、查询、拾取、碰撞图、渲染收集等 9 个无关功能挤在同一个文件中。

**修复后：** 提取 `dungeon-core/src/fov.rs`（`calculate_visible_tiles`）和 `dungeon-core/src/pathfinding.rs`（`astar` + `AStarNode`）。ops.rs 保留剩余的紧密相关工具函数（公式、属性计算、实体查询、拾取、碰撞图、视野记忆、渲染收集）。

**统计：**
| 文件 | 行数 | 职责 |
|------|------|------|
| `fov.rs` | ~25 | 对称阴影投射视野计算 |
| `pathfinding.rs` | ~80 | A\* 8 方向寻路 |
| `ops.rs`（剩余） | ~120 | 公式/查询/记忆/碰撞/渲染 |

### I17 — 全部 `.unwrap()` 替换为 `.expect()` ✅已修复

**状态：** 全部 ~35 处 `.unwrap()` 已替换。生产代码零 unwrap。

### I10 — 斜向键无 OS key-repeat（Won't Fix — 终端环境限制） ✅已修复

**问题：** 按住 Home/End/PgUp/PgDn 不放，角色不会连续斜向移动。多数终端不发斜向键的 OS key-repeat 事件。

**结论：** 终端环境引起，不在项目控制范围内。

### G3 — 水体生成保护距离调整（6→3） ✅已修复

**修复前：** `is_away_from_rooms(x, y, 6)` 保护距离 6，对半径 4-6 的房间偏大，水体几乎不出现。

**修复后：** 保护距离改为 3。视窗内可见 ~9-19 格水体（约 1-2% 地图面积），以水洼和窄溪流形式分布在通道边缘和房间过渡带，不淹没房间内部。

**评估：** 当前密度适合洞穴环境，也为未来的水体减速/加速 Buff 预留了触发空间——每层自然涉水 3-5 次，有存在感但不泛滥。

### A8 — 渲染层直接查询 ECS（Deferred — 条件触发时重新评估） ✅已修复

**当前评估：** 不做 ViewData 重构。理由：
- 当前 render 的 ~8 处 `try_query().expect()` 在 I17 后已有足够信息量
- 组件重命名会触发编译错误（编译期隔离足够）
- ViewData 方案会新增 ~50% 代码量并增加每帧填充开销

**触发条件：** 以下任意一条满足时重新评估：
1. render 中 `try_query` 模式超过 **15 种**（从当前 ~8 增长）
2. **同一组件重组导致 render 连续两次以上需要修改**时

### D4 — 升级满血满蓝已文档化（有意设计） ✅已修复

**修复前：** `apply_exp_system` 中升级后 HP/MP 全恢复，但 GAME.md 和 DESIGN.md 均未记录。属于"有意但未说明"的行为，新开发者看到会困惑。

**修复后：** GAME.md 升级效果中增加 `HP/MP 全恢复（设计简化，方便体验不同楼层）` 行，并注明参见 D4。

### A6 — 行动类型从 dungeon-core 移至 dungeon-action ✅已修复

**修复前：** `dungeon-core/src/action_types.rs` 包含 `ActionQueue`、`ActionKindV3`、`CanMove`/`Chase`/`Flee`等行动领域类型。它们被放在 core 中只因依赖方向限制，导致 core 被行动系统的改动拖慢。

**修复后：** 整个 `action_types.rs` 迁移到 `dungeon-action/src/types.rs`。所有引用路径更新：
- `dungeon-action` 各模块：`crate::types::*`
- `dungeon-world`：`dungeon_action::*`
- `dungeon-render`：新增依赖 `dungeon-action`
- `dungeon-core`：删除 `pub mod action_types`，测试迁至 `dungeon-action`

**删除文件：** `dungeon-core/src/action_types.rs`、`dungeon-core/src/tests.rs`

### I19 — 提取 setup_world/descend 共享函数 + 修复 G9/G10/I16 ✅已修复

**修复内容（四项在同一个重构中完成）：**

**I16 — 单房间物品为 0**：`place_ground_items` 当 `rooms.len() == 1` 时退回到 `rooms[0]` 内随机偏移放置。

**G10 — 怪物阻挡关键位置**：`generate_monster_population` 新增 `exclude: &[(usize, usize)]` 参数，收集和随机补充阶段跳过排除坐标。`setup_world` 和 `descend` 传入 `[spawn, stairs_pos]`。

**I19 — 重复代码**：提取 `spawn_monsters`、`place_ground_items`、`pick_stair_pos` 三个共享函数，`setup_world` 和 `descend` 分别调用。消除 ~55 行重复代码。

**教训：** 三个不同的问题（重合、阻挡、缺物品）共享同一根因（单房间退化）和同一修复点（init.rs）。将其一次性解决比分开修更高效。共享函数提取应在修复的同时进行，而非先提取再修复——否则两次修改同一区域。

### I18 — `on_stairs()` 修复：过滤 Player 组件 ✅已修复

**修复前：** `try_query::<&Position>()` 查询任意实体位置，迭代顺序不确定性导致可能读到怪物/物品的位置而非玩家，使下楼判定失效。

**修复后：** 改为 `try_query::<(&Player, &Position)>()`，只查询玩家的位置。无玩家时返回 false。

**教训：** 任何"判断玩家状态"的函数都应在查询组件时显式加入 Player filter。`&Position` 可能匹配到任何实体——编译器不会警告，行为在运行时才暴露。

### I20 — 移除 `advance_and_settle_parallel` 末位重复 rebuild ✅已修复

**修复前：** `advance_until_player_acted`（内部每 action 后 rebuild）→ schedule.run → `rebuild_occupancy`（末位）。调度器不改变实体位置，末位 rebuild 冗余。

**修复后：** 删除末位 `rebuild_occupancy` 调用。碰撞图仅由 `advance_action_queue` 在每 action 后维护，职责清晰。

### D7 — EventLog 容量提升至 50 ✅已修复

**修复前：** max=10，战斗密集时关键信息 2-3 回合后被滚出屏幕。

**修复后：** max=50。

### I21 — Position 增加 `#[derive(PartialEq, Eq)]` ✅已修复

**修复前：** `Position` 无 PartialEq，测试中需逐字段比较 x 和 y。

**修复后：** 增加 `#[derive(PartialEq, Eq)]`。测试代码可直接 `assert_eq!(pos1, pos2)`。

**教训：** 值类型（所有字段都是 Copy 的简单结构体）应默认实现 PartialEq + Eq，无需等待测试需要时才加。

### D6 — GAME.md 升级描述与代码一致 ✅已修复

**修复前：** GAME.md 仍写着"获得 3 个属性点（待分配）"，但 PendingLevelUp 已在 I7 中删除。

**修复后：** 该行标记为 `~~已移除~~`，并注明参见 I7。GAME.md 的"升级效果"描述与 `apply_exp_system` 的实际行为一致。

**教训：** 代码与设计文档之间没有自动同步机制。每次删除游戏机制（如 I7）后应在 GAME.md 中搜索相关文字。ISSUES.md 的已修复列表应包含文档更新。

### I14 — 下楼时 PlayerClass 与 Skills 联动保障 ✅已修复

**修复前：** `descend()` 中 Skills 通过 `player_data.6`（`sk.list.clone()`）持有独立副本，与 `PlayerClass` 字段无编译期联动。如果将来添加职业特有技能，两个字段可能 drift。

**修复后：** Skills 改为从 `PlayerClass::skills()` 推导，与 `setup_world` 和 `restore` 一致。同步清理了不再需要的 `Skills` 组件查询和旧 Position 字段。

**教训：** 派生数据不应手动复制，应从权威源推导。`descend()` 中有三个不同路径（setup_world / restore / descend）重建玩家，它们生成 Skills 的方式应统一。

### I13 — Tile 序列化合约由类型管理 ✅已修复

**修复前：** `map_tiles` 用 `tile as u8` 保存、`if v == 0 { Wall } else { Floor }` 恢复。判别值隐式依赖编译器分配，且丢失了 ShallowWater/DeepWater/Stalactite。

**修复后：** 由 I15 的 Tile 自定义 Serde 统一解决——序列化合约归类型自身管理，调用方只需 push/pull Tile。`Vec<Tile>` 与旧版 `Vec<u8>` 二进制格式一致，无须迁移旧存档。

### I12 — 主循环 F9 读档后刷新视野记忆和碰撞图 ✅已修复

**修复前：** `process_key` 中 F9 读档后（`title_screen` 中做了但这里遗漏了）不跑 `fov_system`、`update_map_memory`、`update_visible_memory`、`rebuild_occupancy`，导致读档后第一帧黑屏/灰色空地图、怪物和碰撞图不可用。

**修复后：** F9 读档后立即执行完整的刷新链，与 `title_screen` 的读档逻辑一致。`process_key` 和 `title_screen` 之间不再有隐藏的不一致。

**教训：** 同一功能的跨入口实现（title_screen vs process_key 的 F9）应提取为公共方法，或至少确保双方逻辑一致。"一个地方修了、另一个没修"是重复代码的经典隐患。

### P5 — 测试覆盖不全 ✅已修复

**修复前：** `dungeon-action` 和 `dungeon-world` 零测试。仅 `dungeon-core` 有 6 个单元测试 + 3 个场景集成测试。

**修复后：**
| crate | 前 | 后 | 新增内容 |
|-------|-----|-----|---------|
| dungeon-core | 6 | 6 | 不变 |
| dungeon-action | 0 | 6 | 队列推进/等待/保活检查/tap-tap 方向/tap-tap 等待/攻击流程 |
| dungeon-world | 0 | 2 | 存档读档回环（Tile+Stats+Inventory+Equipment）、下楼数据保持 |
| 场景测试 | 3 | 3 | 不变 |
| **总计** | **9** | **17** | |

**教训：** 测试编写中的两个关键发现：
1. `rooms[0].center()` 可能返回非 walkable 格（矩形 bounding box 的墙点）——这是生成流程中一个隐藏的脆弱点，测试迫使它暴露
2. `world.get_mut()` 不能同时借两个不同组件——须分两步操作（取物品 → 再装备），这和主流程中 `descend` 的做法一致

### A3 — action/world tick 边界清理 ✅已修复

**修复前：** `dungeon-action/src/tick.rs` 的串行 `advance_and_settle()` 与 `dungeon-world/src/tick.rs` 的并行版功能重复。串行版从未被调用（`main.rs` 使用并行版，`scenario_test.rs` 也使用并行版），属于死代码。

**修复后：** 
- 删除 `dungeon-action/src/tick.rs` 中的 `advance_and_settle()`（action 只保留 `advance_until_player_acted`）
- 删除 `dungeon-world/src/tick.rs` 中的 `advance_and_settle_serial()`（world 只暴露并行版）
- 更新两个 crate 的 `lib.rs` 导出

职责边界：action 负责"队列推进和执行"，world 负责"编排和状态同步"。

### A4 — 环境修饰从 Map impl 提取到独立模块 ✅已修复

**修复前：** `generate_water`、`carve_expand`、`generate_stalactites`、`ensure_connectivity`、`ensure_spawn_accessible`、`ensure_connection_between`、`has_path_between`、`collect_walkable_regions`、`is_away_from_rooms`、`detect_cave_regions` 等 ~450 行代码全部在 `Map` 的 `impl` 块中。Map 职责膨胀——既要容纳 tile 数据还要管理完整的生成管线。

**修复后：** 新建 `dungeon-core/src/map_gen.rs` 模块，将上述方法全部移入作为自由函数（如 `map_gen::generate_water(map, ...)`）。Map 只保留 `generate()` 入口 + 基础查询方法（`count_tile`、`count_walkable_neighbors`、`count_neighbor_tile`、`carve_corridor`、`render`）。

**统计：**
| Map impl | 前 | 后 |
|----------|-----|-----|
| 方法数 | ~18 | ~7 |
| 行数 | ~600 | ~160 |

**教训：** 序列化合约和生成管线都应从核心类型中分离——Serialize/Deserialize 归 Tile、生成管线归 map_gen、基本查询留 Map。

### A5 — global.rs 空壳模块 ✅已修复

**修复前：** `dungeon-core/src/global.rs` 仅含两行注释（"全局 World 不再使用 OnceLock"、"线程局部 RNG 已移除"），无任何代码。`lib.rs` 仍 `pub mod global;`，全局无引用。

**修复后：** 删除 `pub mod global;` 行 + 删除 `global.rs` 文件。注释内容已在 DESIGN.md 和 LESSONS.md 中有足够记录。

**教训：** 代码移除后公共模块声明也应同步清理。P6 曾清理了 action.rs，但 global.rs 被遗忘——每次移除整个模块后都应 grep `pub mod` 确认。

### I15 — 存档 Tile 精度丢失（自定义 Serde 长期方案）✅已修复

**修复前：** `GameSave::capture` 用 `tile as u8` 保存 Tile，`restore` 用 `if v == 0 { Wall } else { Floor }` 恢复。Tile 的 5 种变体（Wall/Floor/ShallowWater/DeepWater/Stalactite）中 `1~4` 全部映射为 Floor，读档后全部水体+钟乳石消失。

**修复后：** Tile 实现自定义 `Serialize`/`Deserialize`，以 u8 判别值序列化（保持与旧版 `Vec<u8>` 相同的二进制格式），restore 直接读取 Tile 值，不再丢失精度。新增变体须在末尾追加。

**教训（L27）：** 自定义 Serde 实现使类型的序列化合约由类型本身管理，而非分散在 capture/restore 两处。同时保持与旧存档的二进制兼容——所有用 `as u8` 序列化 enum 的地方都应改用此模式，避免判别值隐式依赖编译器分配。

---

### P1 — 保活检查只检查即将执行的条目 ✅已修复

队列推进时对所有条目做批量保活检查，不满足的立即剔除，防止 Chase/Flee 在等待期间条件已失效却仍留在队列中白耗 AV。

### P2 — 并行 Schedule 每帧重建（Won't Fix） ✅已修复

每帧构建开销 <1μs，且保持测试跨 World 兼容，保留现状。

### P3 — action.rs 是空壳模块 ✅已修复

删除 action.rs，所有引用统一到 action_types。

### P6 — VisibleMemory 在视野边缘闪烁 ✅已修复

加入 VISIBLE_FORGET_DELAY=3 遗忘延迟，避免实体在视野边缘来回移动时闪烁。

### P7 — 存档缺少对 ActionQueue 的序列化 ✅已修复

按位置映射保存/恢复队列条目，Attack 条目因 Entity 引用跳过。

### D1 — 三套 RNG 并存，游戏不可复现 ✅已修复

`GameRng` 成为唯一随机源：新增便捷方法，`LootTable::roll()` 改为接受 `&mut impl Rng`，暴击/游荡/仲裁全部走 `GameRng`，删除线程局部 RNG，种子从硬编码 `0` 改为 `map_seed.wrapping_add(42)`。

### D2 — 存档/读档丢弃 Intent 缓冲区状态 ✅已修复

`GameSave` 新增 `chase_intents` / `flee_intents` / `wander_intents` 字段，capture 按位置保存，restore 通过 position→entity 重映射恢复，`#[serde(default)]` 兼容旧存档。

### D3 — crate 依赖链文档与实际不符 ✅已修复

修正 README.md 中 crate 划分树和依赖链描述，移除冗余的重复树结构。

### A1 — dungeon-core 与 dungeon-world 大量代码重复 ✅已修复

以 core 的 systems 为 canon：`calculate_visible_tiles` 移入 ops.rs，删除 core 的 api.rs（`setup_world` 移入 tests.rs），删除 world 的 systems.rs，world 的 tick 改引用 core 的 systems。

### I1 — 对角穿墙角不对称：玩家可穿，怪物不可穿 ✅已修复

移除 A\* 中的对角穿墙角检查，玩家和怪物行为一致（均可穿墙角）。

### I2 — 逃跑无退出条件（触发后永远逃跑） ✅已修复

引入滞回区间：`CanFlee::condition`（决策进入）保持 HP < 25%，`check_condition`（保活退出）改为 HP < 30%。

### I3 — 火球技能击杀无经验/无掉落，且会伤害玩家自身 ✅已修复

删除整个 Firebolt 技能条目和相关代码，法师职业改为护盾+狂暴。

### I4 — 装备卸载回滚不完整 ✅已修复

`Inventory` 新增 `can_add()` 预检方法，装备卸载前先检查背包容量，有空间再执行，避免部分添加后无法回滚。

### I5 — 怪物游荡使用确定性方向而非随机 ✅已修复

从 `(FloorNumber + monster_count) % 8` 改为 `rand::random::<u8>() % 8`，每个怪物独立随机方向。

### I6 — apply_exp_system 在每个 ready 条目后调用（Won't Fix） ✅已修复

该函数有 early return（`pending.amount == 0`），非击杀条目开销 <1μs。事件帧模式下每个条目后调用反而是正确行为（即时反馈经验变化）。

### I7 — PendingLevelUp 悬空 ✅已修复

删除整个 PendingLevelUp 机制，升级时不再累积属性点数，只提升等级和 HP/MP。

### I8 — 怪物生成数量固定 12 只 ✅已修复

怪物生成尝试次数从固定 `12` 改为 `room_centers.len()`，地面物品数量改为 `room_centers.len().min(8)`，随可用房间数自动变化。

### I9 — 废弃注释和空白行 ✅已修复

删除 core/systems.rs 中的 `// use crate::world; // 已移除` 注释和多余空行。

### I11 — 渲染层在已探索暗处直接渲染实体实时位置（X 射线透视） ✅已修复

渲染层遍历 renderables 时，删除 `else if explored[ey][ex]` 灰色渲染分支。暗处实体不再直接画出实时位置，改由 `visible_mem` 循环在已探索区域显示上次看到的位置。

### G1 — 死后游戏仍推进 ✅已修复

死后跳过 `advance_and_settle`，q 键直接退出（跳过确认弹窗）。

### G2 — 楼梯不可达 ✅已修复

`Map` 新增 `ensure_connection_between()`：BFS 检查从出生点到楼梯是否有 walkable 路径，若无则用加权醉汉游走（70% 概率指向楼梯方向，30% 随机）挖掘通道。在 `setup_world` 和 `descend` 中楼梯放置后调用。

### A2 — 背包 UI 250+ 行在 main.rs ✅已修复

将 `InvPanel`/`DetailSource`/`Page` 枚举、`collect_ground_items_in`、`open_inventory` 整体提取到独立模块 `src/inventory.rs`。`lib.rs` 添加 `pub mod inventory`，main.rs 通过 `dungeon_tui::inventory::open_inventory` 调用。

---

### A4L — A4 重构遗漏：Map impl 残留两套重复方法 ✅已修复

**修复前：** A4 将 `collect_walkable_regions` 和 `detect_cave_regions` 复制到 `map_gen.rs` 作为自由函数，但原 impl 方法**未删除**。两套代码完全一致。A4 的统计表显示 Map impl 方法数从 ~18 降到 ~7，但实际应为 ~5。

**修复后：** 两个 impl 方法已删除。所有调用方已走 `map_gen.rs` 自由函数版本。

**教训：** 重构跨文件移动方法后应检查原位置是否仍有残余。

### I26 — arbitration_system 排序比较器违反全序契约 ✅已修复

**修复前：** `arbitration_system` 中同 priority 的实体用 `random_range()` 做 tiebreaker，每次比较产生新随机值，违反 `sort_by` 的全序契约。标准库排序算法在检测到不一致比较时会 panic。下楼至第 3 层时固定触发。

**修复后：** 移除随机 tiebreaker。仲裁器只关心**同实体**的优先级排序（同实体高优先级先入队，低优先级被 `has_entity` 过滤），跨实体同优先级的顺序无意义。直接用 `pb.cmp(pa)` 降序，稳定排序保留插入顺序即可。

**教训：** `sort_by` 的比较器必须是全序（total order）——`a < b` 和 `b < a` 不能同时成立。混入随机数的比较器看似聪明，实际是未定义行为，标准库可能在任意数据分布下 panic。

**位置：** `dungeon-action/src/monster.rs:67-70`

### G4 — 玩家与楼梯重合 ✅已修复

**问题：** `pick_stair_pos` 用 `farthest_room_from(spawn)` 取得离出生点最远房间的中心作为楼梯位置。单房间时返回自身，导致楼梯位置 = 玩家出生点。

**最终方案：** 增加单房间守卫 + 醉汉游走 + 螺旋搜索三重保险，见子条目。

**教训：** 两条逻辑路径（正常 + 退化）都要确认退化路径的兜底本身是否有 bug。

### G4L — 醉汉游走死代码（第一次修复无效） ✅已修复

**问题：** I19 在尾部加入醉汉游走检测 `rooms.len() <= 1`，但醉汉游走在 `farthest_room_from` 之后，而该方法对任意非空 rooms 都返回 `Some`，醉汉游走是死代码。

**修复：** 增加 `map.rooms.len() > 1` 守卫使醉汉游走可达。但 60 步失败后的兜底 `(spx, spy)`——即出生点本身，仍未解决。

**位置：** `dungeon-world/src/init.rs`

### G4La — 螺旋搜索兜底（第三次修复完整） ✅已修复

**问题：** 前两次修复后，60 步醉汉游走失败的兜底仍是出生点，单房间时仍重合。

**修复：** 兜底改为螺旋搜索半径 15~40 的最近可行走格，保证不返回出生点。

**关联：** G4（主条目）、G4L（第一次修复）

### G6 — 渲染层叠顺序未定义：怪物与掉落物在同一格时谁在上层不确定 ✅已修复

**修复前：** `collect_renderables` 查询所有 `(Position, Renderable)` 实体并按 ECS 迭代顺序返回，仅对玩家 `@` 做了特殊排序（放最后）。怪物、物品、楼梯在同一格时，哪一层渲染在上方由迭代顺序决定，不可预测。怪物站在物品上时可能被物品盖住。

**修复后：** 收集时增加 Entity 查询，在排序阶段区分实体类型。图层优先级：物品/楼梯 (0) → 怪物 (1) → 玩家 (2)。同层保持原迭代顺序。

**位置：** `dungeon-core/src/ops.rs:150-163`

### A17 — 存档未保存副手投掷物 (off_hand) ✅已修复

**修复前：** `GameSave` 保存了主手、防具、戒指，但从未保存副手字段。restore 中硬编码 `off_hand: None`，存档后副手石子永久丢失。

**修复后：** `GameSave` 新增 `off_hand_item_id`、`off_hand_count`（`#[serde(default)]` 兼容旧存档），capture 时序列化副手栈，restore 时恢复。投掷物存档后不再丢失。

**位置：** `dungeon-world/src/persist.rs`

### D13 — 投掷不经过 AV 行动系统 ✅已修复

**修复前：** 投掷是唯一绕过 `ActionQueue` AV 行动系统的玩家行动。`process_key` 中 `t` 键返回硬编码 `Ok(false)`，主循环据此跳过 `advance_and_settle`。投掷后世界时间静止。

**修复后：** `ActionKindV3::Throw{tx,ty}` 新增枚举变体，`execute_throw` 下沉至 `dungeon-action/execute.rs` 并复用 `equipment_bonus`。瞄准确认后 `enqueue` AV 行动计算耗时 190ms，返回 `true` 触发世界推进。投掷与移动/攻击/技能走同一生命周期。

**位置：** `dungeon-action/src/types.rs`、`dungeon-action/src/execute.rs`、`src/main.rs`、`src/throw.rs`

### I41 — `line_bresenham` 零长度路径导致无限循环崩溃 ✅已修复

**修复前：** `line_bresenham(x0,y0, x1,y1)` 在 `x0==x1 && y0==y1`（起点等于终点）时，方向推导 `sx = if x0 < x1 { 1 } else { -1 }` 和 `sy = if y0 < y1 { 1 } else { -1 }` 因 `x0<x1` 为假而得到 `sx = -1, sy = -1`，每一步朝远离目标的方向走，永不终止。坐标递减至负值后 `as usize` 回绕到 `usize::MAX`，在后续 `map.tiles[py][px]` 越界 panic。

**触发场景：** 进入投掷瞄准模式时，光标初始化为玩家位置。`update_throw_path` 立即调用 `line_bresenham(px, py, px, py)` 计算弹道，触发退化路径。

**修复后：** 函数入口加 `if x0 == x1 && y0 == y1 { return Vec::new(); }`，零长度路径直接返回空向量。

**教训：** 方向派生自比较的迭代算法（Bresenham、DDA 等）在起终点相同时，所有方向的比较都为假，推导出"反向"步进——必须显式处理退化情形。

**位置：** `dungeon-core/src/ops.rs:196`

### G15 — 投掷无伤害（怪物先于投掷行动） ✅已修复

**修复前：** 投掷耗时 400ms，玩家投掷 AV=70+400×0.80=390ms，怪物追击 AV=85+250×0.90=310ms。怪物先执行，移动后投掷落空，始终显示"石子落在地上"。

```
怪物追击 AV=310 < 投掷 AV=390 → 先执行 → 怪物移动 → 投掷落空
```

**修复后：** 投掷耗时改为 **190ms**，玩家投掷 AV=70+190×0.80=**222ms**，快于怪物追击（310ms）。投掷在怪物移动前命中。

```
怪物追击 AV=310 > 投掷 AV=222 → 后执行 → 投掷命中 → 怪物移动
```

**位置：** `src/throw.rs:176`、`GAME.md` 行动表、`DESIGN.md` Dsn12

### G9 — Buff 持续时长新旧系统差异 60 倍 ✅已修复

**修复前：** `SkillKind { duration: 3 }` 传入两个系统得到不同时长：旧 Buffs 读作 3 帧（~50ms），新 ActiveBuffs 读作 3s（3000 AV），相差 60 倍。

**修复后：** 旧 `Buffs` 系统已由 D11 完整移除，ActiveBuffs 为唯一 Buff 系统。60 倍差异随旧系统消失而自然消除。

**关联：** D11（移除旧 Buffs 结构体）、D10（移除 buff_tick_system）

---

## 一、设计层面（Design）

---

### 🟡 D5 — 事件帧模式（Deferred — 触发条件达成时重新评估）

**问题：** 当前玩家确认行动后批量推进到玩家行动完成，中间所有怪物行动对玩家不可见。

**提议方案：** 增加可切换的"事件帧模式"（按 `s`），每帧只执行一个事件，Enter 步进。

**当前评估：** 暂缓实现。在当前战斗系统（纯数值 chase/flee/wander）下，事件帧模式提供的信息量不足以补偿节奏损失——玩家的最优策略不会因看到每个怪物单步移动而改变。

**触发条件：** 出现**足够复杂的战斗逻辑**，即新增的怪物/boss 有需要玩家在过程中作出反应的能力——例如范围攻击预警、状态效果倒计时、可打断的吟唱、地形变化。当单次 tick 内的行动序列构成决策信息时，事件帧模式从"nice to have"变为"need to have"。

---

### 🟡 D16 — 页栈迁移后对话框不再暂停输入线程，非对话框按键被消耗丢失（Won't Fix）

**问题：** 旧阻塞式 UI（open_modal）通过 `modal_flag = true` 暂停输入线程，对话框期间所有按键由主线程 `event::read()` 处理，非 'y'/'n' 按键保留在 crossterm 缓冲区。页栈迁移后 `modal_flag` 未被使用（传入 `process_game_key` 但从未读写），输入线程持续运行，非对话框按键通过 `try_recv()` 取出后被 `process_dialog_key` 静默丢弃（匹配不到 'y'/'n'/Esc 即 fall-through）。

```rust
// main.rs:57-78 — 输入线程，modal_flag 永远为 false
if thread_flag.load(Ordering::Relaxed) { ... }  // 无代码写入该变量
// main.rs:469-502 — process_dialog_key 只处理 y/n/Esc
```

**影响：** 🟢 低 — 对话框出现概率低（退出/下楼确认），非对话框按键在对话框期间被丢弃的行为实际上比旧版更安全（避免下楼后意外触发方向键），但与旧版行为不一致。

**状态：** Won't Fix — 页栈模式下按键由栈顶页面处理器消费，非对话框按键被丢弃是设计行为（比旧版 modal_flag 更安全）。`modal_flag` 已随 A26 从处理器签名中移除。

**位置：** `src/main.rs:57-78`（输入线程）、`src/main.rs:469-502`（process_dialog_key）

---

### 🟡 D29 — 行动实体 + 速度组件：AV 系统与 `ActionKind` 的替代设计（草案）

**问题：** 当前行动系统由 `ActionKind` 中央 enum + actor 上的 ZST 行动组件 + exclusive `&mut World` 系统组成；AV 计时器没有参与执行门禁，事件生命周期也不正确。继续加行动/行为会继续增加中央 match 和耦合。

**决策草案：** 按 REFACTOR.md §3.6 改为 action 实体方案：

- 一个行动 = 一个 actor 的子实体（`ChildOf` + `ActionPriority` + `ActionTimer` + ZST/payload + `Candidate/ActiveAction/Ready`）；
- `Can*` 保持 actor 上的 ZST 组件，不子实体化；
- 生成系统只 spawn 候选；仲裁系统唯一写入 actor 行动状态；执行系统按行动类型专用 query，零中央 match；completion 系统消费 `ActionSucceeded/FailedEvent`；
- 删除 `ActionKind` 与 `mount_action` 中央 match；
- 速度按 REFACTOR.md §2.6 改为 `MoveSpeed` / `AttackSpeed`，删除 `Agility`。

**状态：** 草案；AV 门禁（I89）与事件生命周期（I90）已修；§11.6 已按推荐确认（逐行动迁移、倍率速度、先删反应时、`Wait` 固定、怪物速度先保行为、Phase E 先记录）；下一步 Phase A 冒烟测试。

**关联：** REFACTOR.md §2.6 / §3.6 / §10.6；ISSUES A41/A42/A43、I89/I90、G35。

---

## 二、架构层面（Architecture）

### 🟡 A32 — dungeon-core 承载执行逻辑，违反「纯数据/纯查询」分层

**问题：** ops.rs 顶部注释自称「纯读写，无执行逻辑」「不包含何时/如何行动的判断逻辑」，但同文件 `pickup_ground`/`pickup_ground_item`/`consume_off_hand`/`learn_skill` 与 items.rs 的 `use_item`/`craft_with_template` 都执行 `get_mut`/`resource_mut`/`spawn`/`despawn`——有副作用的世界变更逻辑下沉到 core，action 层被架空。

**影响：** 🟡 中 — 分层承诺失效；core 深度耦合 ECS 可变语义。

**位置：** `dungeon-core/src/ops.rs:74-131/302-339`、`dungeon-core/src/items.rs:67-104/505-546`

---

### 🟡 A33 — 核心玩法规则泄漏到 src 应用层 UI 处理器

**问题：** 装备原子换装预检（I58）、卸装、丢弃、物品使用分派、副手装填回滚（G24）全部写在 `src/pages/inventory.rs`/`src/throw.rs` 按键处理器中，无 action/world 层 API；与渲染层提示通过字符串/枚举约定松散关联。

**影响：** 🟡 中 — 游戏规则散落 UI 层无法单测（I23 应用层 0 测试的根因之一），重复维护风险。

**位置：** `src/pages/inventory.rs:79-199`、`src/throw.rs:61-97`

---

### 🟡 A34 — 追击/逃跑条件三处重复实现，改阈值需同步三处

**问题：** 「玩家可见或 LastKnownPlayerPos 有值」追击判定与「HP<25%」逃跑判定，各在 ①并行决策 system、②GameAction::check_condition、③check_condition 的 kind match 分支独立实现。A11 引入 trait 对象后因无法序列化，读档 action=None 必须保留 kind match 副本，形成三套并行行为定义。逃跑滞回注释（进入<25% 退出>30%）与实际（仅<25%）不符。

**影响：** 🟡 中 — 三处漂移风险；注释与实现不符误导。

**位置：** `dungeon-action/src/execute.rs:70-108`、`dungeon-action/src/monster.rs:14-50`、`dungeon-action/src/actions.rs:13-31`

---

### 🟡 A36 — 死代码：GameAction 三方法 + CanThrow + CanMove

**问题：** ①`GameAction::priority/av_cost/as_any` 全仓库零调用（A11 声称五方法，实际只用 execute/check_condition/display_name）；②`CanThrow` 从未被 spawn/insert/query（Dsn12「支持怪物投掷」未落地）；③`CanMove.duration/priority` 从未被读取——玩家移动耗时由 weapon_speed 决定。

**影响：** 🟡 中 — 声明未接线的符号误导维护者；组件式行动授权双轨重构未完成。

**位置：** `dungeon-action/src/types.rs:29-39/126-134/297-306`、`dungeon-action/src/actions.rs:17-52`

---

### 🟢 A38 — execute_attack 重复查询 Equipment 且处理方式不一致

**问题：** 同函数内第 272 行 `expect_log`（必须存在）与 282 行 Option 形式各查一次 Equipment；expect_log 隐式假设攻击者必有 Equipment（仅玩家触发 Attack，脆弱耦合）。

**影响：** 🟢 低 — 重复查询 + 两套错误处理。

**位置：** `dungeon-action/src/execute.rs:272-273/282-283`

---

### 🟢 A39 — world.query() 与 try_query().expect_log() 混用

**问题：** L34 约定「组件已注册时用 try_query().expect_log() 替换 query()」，但 pickup_ground、execute_chase 等仍用裸 query()（panic 无上下文日志）。

**影响：** 🟢 低 — 崩溃时丢失调用点信息。

**位置：** `dungeon-core/src/ops.rs:91`、`dungeon-action/src/execute.rs:142/168`

---

### 🟡 A28 — terrain-forge submodule 配置缺失：无 .gitmodules + 163 个未提交变更

**问题：** 主仓库将 terrain-forge 记录为 gitlink（`160000 c6d9d1f`）但仓库中不存在 `.gitmodules`（历史中也没有）。submodule 工作区有 163 个未提交的删除/修改（README、demo、.github、Cargo.toml 等被清理但从未提交）。

**影响：** 🟡 中 — 新克隆者无法 `git submodule update --init`，workspace 构建直接失败；本地清理状态未固化，随时可被覆盖丢失。

**位置：** 仓库根（.gitmodules 缺失）、`terrain-forge/`（git status 163 项脏变更）

---

### 🟢 A29 — ItemMeta 不参与存档序列化

**问题：** I41 落地的 `ItemMeta`（display_name/tier/durability/tags）在 `GameSave::capture` 的 `SavedStack` 中只存 `(item_id, count)`，meta 静默丢弃；restore 时 `meta: None`。

**影响：** 🟢 低 — 当前无使用场景，但 Dsn14 声称 ItemMeta 已实现；未来实例级数据（自定义名/品质）会静默丢失。

**位置：** `dungeon-world/src/persist.rs:17-21`（SavedStack）、`dungeon-world/src/persist.rs:301`

---

### A11 — ActionKindV3 枚举解耦：引入 GameAction trait ✅已修复

**修复前：** `ActionKindV3` 枚举同时承载玩家行动和怪物行为，两种扩展节奏不同的东西捆绑在一起。每新增一种怪物行为需修改 8 个 match 点。

**修复后：**
1. 新增 `GameAction` trait（execute/check_condition/display_name/priority/av_cost）
2. `ActionEntry` 增加 `action: Option<Box<dyn GameAction>>` 字段
3. `execute_entry`/`check_condition` 优先检查 `action` 字段
4. `timeline.rs` 渲染优先取 `action.display_name()`
5. 保持 `ActionKindV3` 不变向后兼容，序列化走旧路径

**收益：** 加新怪物行为从 8 个文件 → 1 个新 struct + 1 个 impl GameAction。

**位置：** `dungeon-action/src/types.rs`、`dungeon-action/src/execute.rs`、`dungeon-render/src/timeline.rs`

**提交：** `695dd07`

---


### 🟢 A19 — ratatui 内置 widget 闲置（Gauge/List/Clear/Scrollbar/Table 未使用）（Deferred）

**问题：** 项目中使用的 ratatui widget 仅限于 `Paragraph` + `Span` + `Layout` + `Block`，五个内置 widget 完全未使用，对应功能由手写代码替代：

| widget | 手写替代位置 | 手写行数 | 可简化到 |
|--------|------------|---------|---------|
| `Gauge` | `ui.rs` 中 `bar()` 函数 | ~8 | 2 行构造 |
| `List` | `inventory.rs` 背包列表循环（选中态+▸+滚动） | ~30 | 5 行 |
| `Clear` | 模态弹窗覆盖逻辑 | 依赖 modal | 1 行 |
| `Scrollbar` | 事件日志 `take(12)` 硬截断 | ~5 | 无截断+滚动条 |
| `Table` | `ui.rs` 属性面板手算 `{:>3}` + `"   "` 分隔 | ~15 | 3 行列定义 |

**影响：** 🟢 低 — 正确性不受影响。代码量约多写 50 行，背包列表的可维护性（选中态/滚动边界）不如 `List` 开箱即用。仅在新增类似 UI（合成台、技能树）时值得一次性迁移。

**状态：** Deferred — 触发条件：新增合成台/技能树等列表型 UI 时一次性迁移（Gauge/List/Scrollbar）。

**位置：** `dungeon-render/src/ui.rs`（Gauge/Table/Scrollbar）、`src/inventory.rs`（List）

---

### 🟡 A41 — 玩法系统用 exclusive `&mut World` 代替正常系统

**问题：** `action/execution/mod.rs` 的 tick/执行系统、`action/generation/ai.rs::decide_monster_actions`、`action/mod.rs::mount_action/finish_action_*`、`world/loop_.rs` 的推进函数都收 `&mut World`，手动 query/改实体/发事件；`run_action_cycle` 手动顺序调用系统。玩法逻辑被写成过程式代码，无法用 `Query`/`Commands`/`EventWriter` 组合，也无法并行。

**影响：** 🟡 中高 — 阻断 action 实体方案（REFACTOR §3.6）的落地；每次新增行动/行为都要改多个 exclusive 函数。

**位置：** `core/src/action/execution/mod.rs`、`core/src/action/generation/ai.rs`、`core/src/action/mod.rs`、`core/src/world/loop_.rs`、`core/src/combat/mod.rs`（直接执行辅助）

**状态：** 部分修复 — AV 门禁 / 事件生命周期已修（I89/I90）；exclusive `&mut World` 执行/生成系统仍待 action 实体方案（A42）改造。

**关联：** REFACTOR.md §3.6 / §8.1；ISSUES A42、I89。

---

### 🟡 A42 — 删除 `ActionKind`：行动改为 action 子实体

**问题：** `ActionKind` 是中央分派 enum，`mount_action` 对它做 match；新增行动要改 enum + 中央 match + 生成/执行分支。`REFACTOR.md §3.2–3.5` 描述的 `ActionIntent + 仲裁` 方案从未落地，当前 `ai.rs::choose_action` 是独占的 `if/else` 函数。

**影响：** 🟡 中 — 扩展成本高；与 `Can*` + ZST + 专用 query 的 ECS 方向不一致。

**决策：** 按 REFACTOR.md §3.6 改为 action 实体方案；`Can*` 保持组件；不再需要 `ActionKind`。

**位置：** `core/src/action/mod.rs`、`core/src/action/generation/ai.rs`、`core/src/action/generation/player.rs`、`core/src/world/loop_.rs`

**状态：** 部分落地 — **Phase B（action 实体 PoC）已完成**：`core/src/action/entity.rs` 实现 `ActionPriority` / `ActionSource` / `Candidate` / `ActiveAction` / `ActionName` + 生成（Wander/Flee）/ 仲裁（全序 `(priority, to_bits())`）/ tick（`Ready` 门禁）/ 执行（`Move` exclusive + `Wander` 参数化）/ completion（消费 `ActionSucceeded/FailedEvent` 并回收 action 实体）全链路，9 个 PoC 测试通过；旧模型（`ActionKind` / `mount_action` / `choose_action`）**仍在使用且未删除**——PoC 未接主循环，删除留到 Phase C（逐行动迁移 Wait → Move → BasicAttack → Wander → Chase → Flee）。

**关联：** D29、REFACTOR.md §3.6 / §11.3 Phase B。

---

### 🟡 A43 — 同类死抽象/重复表示清理

**问题：** 与 `ActionKind` 同类的中央 token / 提前抽象 / 重复表示：

- 身份 ZST `Rat/Scorpion/...` 由 `MonsterKindId` match 后插入，但无任何读取方；与 `MonsterKindId` 重复；
- `CreatureKind` 无读取方；`EntityClass` 唯一读取是未迁移的 `Item` 判断；
- `ActionSucceededEvent` / `ActionFailedEvent` 无人发/读；`DeathEvent` / `LevelUpEvent` 写入但无消费者；`ThreatEvent` / `ThreatTable` 占位未接线；
- `BeAttacked` / `NeedRecordBeAttacked` 写入但无读取；`PendingExp` 是绕过 `DeathEvent` 的旁路；
- `MeleeResult` + `prepare_attack_event` / `resolve_melee` / `damage_entity` 是死代码/重复战斗路径；
- `MonsterStats` / `WorldInitConfig` 是低优先级中间层。

**影响：** 🟡 中 — 死抽象让文档/代码看起来比实际复杂，且容易误以为扩展点已存在。

**位置：** 见 REFACTOR.md §10.8 逐项清单。

**状态：** 清单已记录（REFACTOR.md §10.8）。用户决定**先记录，不执行删除**；Phase A/B/C 期间不阻塞，Phase E 开始前逐项确认是否删除/接线。原则是“每个保留的抽象必须有真实读取方/消费者”。

**确认记录：** 2026-09 对话；§11.6 第 6 项。

**关联：** REFACTOR.md §10.8。

---

## 三、实现层面（Implementation）

### 🟡 I24 — Buff/Skill 系统缺陷（子项 I24b/I24c 已关闭）

**I24b — 技能数量少且职业锁定 （Won't Fix — 已被 Dsn13 取代）**
无职业设计已实现（`PlayerClass::skills()` 返回空），技能全部通过卷轴获取（I61 修复后链路畅通）。

**I24c — 无冷却维度 （Won't Fix — 当前无需求）**
`ActiveCooldowns` 已删除（A18）。当前 3 个技能（治愈/护盾/狂暴）均无冷却设计，MP 消耗已足够平衡。将来引入强技能需要冷却时，按 Dsn13「冷却下限约 1000 AV」重新实现。

**位置：** 无（设计层面结论）

### 🟡 I23 — 测试覆盖缺口（部分） 🟡 进行中

**现状：** dungeon-core 5 个（EventLog）、dungeon-action 14 个（含本次 6 个回归测试）、dungeon-world 2 个、场景 3 个、terrain-forge 26 个。

**剩余缺口：** dungeon-render 0 测试；应用层（main.rs 装备/投掷 UI 流程）0 测试。

**风险：** UI 流程（装备原子性 I58、投掷 Enter 验证 I59）依赖手动验证。

---


## 四、游戏逻辑层面（Game Logic）

### G22 — 楼梯/地面物品可能生成在不可行走格上 + 通道 4 方向断裂 ✅已修复

**成因链（四层）：**
1. `generate_stalactites` 在房间内每格 7% 概率放钟乳石（**含房间中心**），`generate_water` 扩散也可能波及
2. `pick_stair_pos` 多房间分支直接返回 `farthest_room_from`（最远房间中心）**不检查 walkable**（出生点 `spawn_point()` 有螺旋兜底，楼梯没有）→ 楼梯落在 Stalactite/DeepWater 上
3. `ensure_connection_between` 醉汉游走提前停止（距离<3 break）或 500 步耗尽 → 通道挖不到终点
4. **8 方向通道 4 方向断裂**：游走/Bresenham 路径可对角相邻，而玩家移动与 `has_path_between` 是 4 方向——单格宽通道在锯齿处断裂，玩家走不过去

**同类问题：** `place_ground_items` 多房间分支同样直接取房间中心（物品落不可走格捡不到）；`ensure_connectivity` 通道同样可能挖不到位且对角断裂。

**修复：**
- 治本：`generate_stalactites` 跳过房间中心（中心永远 Floor）
- 兜底：新增 `Map::nearest_walkable`（螺旋搜索），`pick_stair_pos`/`place_ground_items`（含 +1 偏移落点）/`persist.rs` 读档楼梯全部走兜底
- 收尾：`ensure_connection_between`/`ensure_connectivity` 游走后用 Bresenham 直线强制打通终点
- 连通性：通道改为挖 **2x2 块**（非单格），保证 4 方向连通

**回归测试：** `test_stalactites_skips_room_centers`（20 轮钟乳石生成中心仍 Floor）、`test_nearest_walkable_fallback`、`test_pick_stair_pos_always_walkable_and_reachable`（60 种子：落点 walkable + 完整流程后出生点→楼梯可达）

**位置：** `dungeon-world/src/init.rs`（pick_stair_pos/place_ground_items）、`dungeon-world/src/persist.rs`、`dungeon-core/src/map_gen.rs`（generate_stalactites/ensure_connection_between/ensure_connectivity）、`dungeon-core/src/lib.rs`（nearest_walkable）

### 🟡 G11 — 材料物品无消耗渠道

**问题：** 生物血肉（id=10）、破布（11）、坚硬木棍（12）、染血兽牙（13）、黑色甲壳（14）五种材料物品只能拾取和堆积，没有任何消耗途径。背包 36 格在 4-5 层后会被材料大量占用，玩家被迫在"拾取所有材料"和"留空间给有用物品"之间做无趣的选择。

```rust
// 当前材料的全部用途：占背包格
// 没有任何合成/升级/交换/消耗机制消费它们
```

**影响：** 🟡 中 — 材料的存在感为零。玩家的理性选择是"忽略所有材料掉落"。

**方案：** 模板碎片系统（DESIGN.md Dsn19）。碎片作为一次性消耗品，用材料合成指定物品。Phase 1 材料开始有出口，Phase 2 引入核心→完整模板。渐进实现。

**状态：** 部分修复 — **Phase 1 已落地（I69）**：模板碎片作为独立消耗品掉落和使用，材料已有消耗渠道。Phase 2（模板核心）与 Phase 3（实验级碎片）保持 Deferred。

**位置：** `assets/items.json` items 10-14

### 🟢 G13 — 战斗公式缺乏层次深度（Won't Fix — MVP 范围决策）

**表现：** 当前 `max(攻击 - 防御, 1)` 的差值公式完全线性，1 点攻击永远对应 1 点伤害。无穿甲穿透、无元素属性/抗性、无距离衰减、无背后/侧击加成。装备增强集中在 +攻击/+防御 两个维度。

**影响：** 🟢 低 — MVP 阶段可以接受。但扩展到 8+ 种怪物、3+ 种武器类型时，所有战斗都会感觉"差不多"——只有数值差异，没有策略差异。当需要设计"抗高攻怪"和"抗高防怪"两种不同策略时，当前公式无法提供区分度。

**状态：** Won't Fix — MVP 范围决策。触发条件：怪物种类 ≥8 或武器类型 ≥3 时重新评估。

**位置：** `dungeon-action/src/execute.rs:285-310`（execute_attack）

### 🟡 G35 — 删除 `Agility`，改为 `MoveSpeed` / `AttackSpeed`

**问题：** 当前 `Agility` 同时承担“反应时”和“耗时修正”，公式为 `AV = max(100 - agility*3, 20) + duration * max(1 - agility*0.02, 0.5)`。它把速度绑在一个聚合数值上，装备/防具/Buff 无法分别影响移动与攻击节奏；且 I89 修复前该公式对执行没有实际影响。

**决策（REFACTOR.md §2.6）：** 删除 `Agility`；新增 `MoveSpeed(f64)` / `AttackSpeed(f64)` 两个倍率组件（1.0 基准，越高越快）：

- `AV = base_duration / speed.clamp(MIN_SPEED, MAX_SPEED)`；
- `Move/Chase/Flee/Wander → MoveSpeed`；`BasicAttack → AttackSpeed`；`Wait` 固定 duration（或后续 `WaitSpeed`，待定）；
- 删除 `agility_to_reaction` / `agility_speed_factor` / 旧 `action_av`；
- 玩家/怪物模板按旧敏捷映射初值，再用 GAME.md `[⃞试调]` 校准。

**影响：** 🟡 中 — 平衡改动，不是等价重构；必须先修 I89（AV 门禁），否则速度不影响行为；需同步 GAME.md（反应时/耗时章节、玩家/怪物敏捷表、武器速度章节）、DESIGN.md、REFACTOR.md §2.6/§10。

**位置：** `core/src/components.rs:171`、`core/src/balance.rs:21-48`、`core/src/world/init.rs:112`、`core/src/monster/mod.rs`（模板/`MonsterStats`）、`core/src/action/generation/player.rs:38/87`、`core/src/action/generation/ai.rs:78-87`

**状态：** 待实现；§11.6 已确认：`MoveSpeed` / `AttackSpeed` 倍率、`AV = base_duration / speed`、先删反应时、`Wait` 固定、怪物速度先按旧敏捷保行为；Phase C 完成后执行（REFACTOR §11 Phase D）。

**关联：** D29、I89、REFACTOR.md §2.6 / §3.6.7。

---

## 其他


### 🟢 P4 — 玩家确认行动后无法取消（被 D5 锁定）

**问题：** tap-tap 双击确认后行动进入 `ActionQueue` 无法撤回。

**说明：** 事件帧模式（D5，已 defer）可以部分解决此问题——事件帧模式下玩家可以在自己行动执行前切换方向。在 D5 重新评估前此问题无解。
