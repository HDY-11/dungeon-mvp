# 发现的问题记录

本文档记录当前实现中与设计意图不一致或有改进空间的问题，
在后续开发中可作参考。

问题按维度分组：**设计 / 架构 / 实现 / 游戏逻辑**，组内按严重程度降序。
优先级标记：🔴 高（影响正确性或游戏体验） / 🟡 中（维护性或功能缺口） / 🟢 低（整洁或边缘情况）

---

## ✅ 已修复

### P1 — 保活检查只检查即将执行的条目

队列推进时对所有条目做批量保活检查，不满足的立即剔除，防止 Chase/Flee 在等待期间条件已失效却仍留在队列中白耗 AV。

### P3 — 并行 Schedule 每帧重建（Won't Fix）

每帧构建开销 <1μs，且保持测试跨 World 兼容，保留现状。

### P6 — action.rs 是空壳模块

删除 action.rs，所有引用统一到 action_types。

### P9 — VisibleMemory 在视野边缘闪烁

加入 VISIBLE_FORGET_DELAY=3 遗忘延迟，避免实体在视野边缘来回移动时闪烁。

### P10 — 存档缺少对 ActionQueue 的序列化

按位置映射保存/恢复队列条目，Attack 条目因 Entity 引用跳过。

### D1 — 三套 RNG 并存，游戏不可复现

`GameRng` 成为唯一随机源：新增便捷方法，`LootTable::roll()` 改为接受 `&mut impl Rng`，暴击/游荡/仲裁全部走 `GameRng`，删除线程局部 RNG，种子从硬编码 `0` 改为 `map_seed.wrapping_add(42)`。

### D2 — 存档/读档丢弃 Intent 缓冲区状态

`GameSave` 新增 `chase_intents` / `flee_intents` / `wander_intents` 字段，capture 按位置保存，restore 通过 position→entity 重映射恢复，`#[serde(default)]` 兼容旧存档。

### D3 — crate 依赖链文档与实际不符

修正 README.md 中 crate 划分树和依赖链描述，移除冗余的重复树结构。

### A1 — dungeon-core 与 dungeon-world 大量代码重复

以 core 的 systems 为 canon：`calculate_visible_tiles` 移入 ops.rs，删除 core 的 api.rs（`setup_world` 移入 tests.rs），删除 world 的 systems.rs，world 的 tick 改引用 core 的 systems。

### I1 — 对角穿墙角不对称：玩家可穿，怪物不可穿

移除 A\* 中的对角穿墙角检查，玩家和怪物行为一致（均可穿墙角）。

### I2 — 逃跑无退出条件（触发后永远逃跑）

引入滞回区间：`CanFlee::condition`（决策进入）保持 HP < 25%，`check_condition`（保活退出）改为 HP < 30%。

### I3 — 火球技能击杀无经验/无掉落，且会伤害玩家自身

删除整个 Firebolt 技能条目和相关代码，法师职业改为护盾+狂暴。

### I4 — 装备卸载回滚不完整

`Inventory` 新增 `can_add()` 预检方法，装备卸载前先检查背包容量，有空间再执行，避免部分添加后无法回滚。

### I5 — 怪物游荡使用确定性方向而非随机

从 `(FloorNumber + monster_count) % 8` 改为 `rand::random::<u8>() % 8`，每个怪物独立随机方向。

### I6 — apply_exp_system 在每个 ready 条目后调用（Won't Fix）

该函数有 early return（`pending.amount == 0`），非击杀条目开销 <1μs。事件帧模式下每个条目后调用反而是正确行为（即时反馈经验变化）。

### I7 — PendingLevelUp 悬空

删除整个 PendingLevelUp 机制，升级时不再累积属性点数，只提升等级和 HP/MP。

### I8 — 怪物生成数量固定 12 只

怪物生成尝试次数从固定 `12` 改为 `room_centers.len()`，地面物品数量改为 `room_centers.len().min(8)`，随可用房间数自动变化。

### I9 — 废弃注释和空白行

删除 core/systems.rs 中的 `// use crate::world; // 已移除` 注释和多余空行。

### I11 — 渲染层在已探索暗处直接渲染实体实时位置（X 射线透视）

渲染层遍历 renderables 时，删除 `else if explored[ey][ex]` 灰色渲染分支。暗处实体不再直接画出实时位置，改由 `visible_mem` 循环在已探索区域显示上次看到的位置。

### G2/G3 — 死后游戏仍推进

死后跳过 `advance_and_settle`，q 键直接退出（跳过确认弹窗）。

### G7 — 楼梯不可达

`Map` 新增 `ensure_connection_between()`：BFS 检查从出生点到楼梯是否有 walkable 路径，若无则用加权醉汉游走（70% 概率指向楼梯方向，30% 随机）挖掘通道。在 `setup_world` 和 `descend` 中楼梯放置后调用。

### A2 — 背包 UI 250+ 行在 main.rs

将 `InvPanel`/`DetailSource`/`Page` 枚举、`collect_ground_items_in`、`open_inventory` 整体提取到独立模块 `src/inventory.rs`。`lib.rs` 添加 `pub mod inventory`，main.rs 通过 `dungeon_tui::inventory::open_inventory` 调用。

---

## 一、设计层面（Design）

---

### 🟡 D5 — 行动系统设计：事件帧模式（Event-Frame Mode）— 计划中

**问题：** 当前玩家确认行动后 `advance_until_player_acted` 批量推进到玩家行动完成，中间所有怪物的行动对玩家不可见不可干涉。

**提议方案：** 增加一个可切换的"事件帧模式"，按 `s` 在两种模式间切换：

| 模式 | 行为 | 适用场景 |
|------|------|---------|
| **玩家行动模式**（默认） | 玩家确认→批量推进到玩家行动完成→渲染 | 快速推进 |
| **事件帧模式** | 每帧只执行一个事件→渲染→等待玩家按 Enter 或确认下一步 | 精细观察/战术决策 |

**核心改动：**
- `EventFrameMode` Resource + `s` 键切换 + UI 指示器
- `ActionQueue` 增加 `add_or_replace` 方法（事件模式下替换已有条目）
- `advance_one_event()` 推进到下一事件点并最多执行一个条目
- 事件模式下 Enter 键触发推进
- 怪物决策在队列空 + 意图缓冲区空时触发

**位置：** `dungeon-action/src/execute.rs`、`src/main.rs`（主循环）、`dungeon-action/src/player.rs`

---

### 🟢 D4 — 升级满血满蓝未文档化（优先级低）

**问题：** `apply_exp_system` 中升级后满血满蓝，在 GAME.md 和 DESIGN.md 中均未记录。

**说明：** 设计简化，方便测试不同楼层体验。如未来需要更严格的生存挑战，可改为回复 30%~50%。

---

## 二、架构层面（Architecture）

---

### 🟡 A3 — action/tick.rs 与 world/tick.rs 任务边界模糊

**问题：** `dungeon-action/src/tick.rs` 的串行 `advance_and_settle()` 包含怪物决策、碰撞图重建、FOV、视野记忆等，与 `dungeon-world/src/tick.rs` 的并行版本有重复。action 应只负责"推进和执行"，world 负责"编排和状态同步"，当前边界有重叠。

**位置：** `dungeon-action/src/tick.rs`、`dungeon-world/src/tick.rs`

---

### 🟡 A4 — 环境修饰方法全部塞在 Map 的 impl 块中

**问题：** `generate_water`、`carve_expand`、`generate_stalactites`、`ensure_connectivity`、`ensure_spawn_accessible` 等 ~350 行代码全在 Map 的 impl 块中。Map 的职责应为"容纳 tile 数据 + 基本查询"，而非完整的生成管线。

**位置：** `dungeon-core/src/lib.rs:170-500`

---

## 三、实现层面（Implementation）

---

### 🟢 I10 — 斜向键按住不放无法连续移动（无 OS key-repeat）— 终端环境限制，暂不处理

**问题：** 按住 Home/End/PgUp/PgDn 不放，角色不会连续斜向移动。因为多数终端不发 Home/End 的 OS key-repeat 事件，tap-tap 系统需要两个事件完成一次移动（预览+确认），但按住斜向键只产生一个事件。

**结论：** 由终端环境引起，不在本项目控制范围内，等未来统一运行环境后再修复。

**位置：** `src/main.rs:60-74`（输入线程）

---

## 四、游戏逻辑层面（Game Logic）

---

### 🟢 G8 — 水体生成保护距离可能过大（优先级低）

**问题：** `generate_water` 使用 `is_away_from_rooms(x, y, 6)` 保护房间中心不被水体覆盖。曼哈顿距离 6 对于半径 4-6 的圆形/菱形房间可能过大，导致水体偏少。

**位置：** `dungeon-core/src/lib.rs:241`

---

## 其他

### P7 — 玩家确认行动后无法取消（将被 D5 部分解决）

**问题：** tap-tap 双击确认后行动进入 `ActionQueue` 无法撤回。

**说明：** 事件帧模式（D5）部分解决了此问题——事件帧模式下玩家可以在自己行动执行前切换方向。

### P8 — 测试覆盖不全

`cargo test -p dungeon-core` 中 6 个测试只覆盖核心类型层。`dungeon-action` 和 `dungeon-world` 没有任何测试。
