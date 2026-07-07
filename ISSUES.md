# 发现的问题记录

本文档记录当前实现中与设计意图不一致或有改进空间的问题，
在后续开发中可作参考。

问题按维度分组：**设计 / 架构 / 实现 / 游戏逻辑**，组内按严重程度降序。
优先级标记：🔴 高（影响正确性或游戏体验） / 🟡 中（维护性或功能缺口） / 🟢 低（整洁或边缘情况）

---

## ✅ 已修复（历史记录）

### ✅ P1 — 保活检查只检查即将执行的条目

**问题：** `advance_action_queue()` 只在从队列中弹出条目时（`pop_ready()`）调用 `check_condition()`。队列中其他 `av > 0` 的条目在等待期间从未被重新验证。

**影响：** 一个 Chase 行动入队时玩家在视野内，但如果玩家在 Chase 执行前离开了视野，该条目仍留在队列中白白等待，直到 av 耗尽才被取消。浪费怪物的 AV，导致响应滞后。

**修复：** 队列推进时对所有 `av_remaining > 0` 的条目做批量保活检查，不满足的立即剔除。

---

### ✅ P3 — 并行 Schedule 每帧重建（Won't Fix — 开销 <1μs，为跨 World 兼容保留现状）

**问题：** `build_parallel_schedule()` 每帧被调用一次，每次都重新构建 `Schedule` 对象。

**影响：** 绝对开销很小（微秒级），但丢失了 Schedule 的 system 排序缓存优化。

**结论：** 每帧构建开销 <1μs，且保持测试跨 World 兼容，保留现状。

---

### ✅ P6 — action.rs 是空壳模块

**问题：** `dungeon-core/src/action.rs` 只有一行 `pub use crate::action_types::*;`，增加模块树深度且引用方式不统一（有的用 `action::` 有的用 `action_types::`）。

**修复：** 删除 `action.rs`，所有引用统一到 `action_types`。

---

### ✅ P9 — VisibleMemory 在视野边缘闪烁

**问题：** 实体离开视野时立即从 VisibleMemory 移除。视野边缘来回移动的实体导致渲染闪烁。

**修复：** 加入 VISIBLE_FORGET_DELAY=3 遗忘延迟。

---

### ✅ P10 — 存档缺少对 ActionQueue 的序列化

**问题：** `GameSave` 没有保存/恢复 ActionQueue、ChaseIntents/FleeIntents/WanderIntents。读档后状态重置为空。

**修复：** 位置映射方式保存/恢复队列条目，Attack 条目因 Entity 引用跳过。

---

## 一、设计层面（Design）

---

### 🔴 D1（旧 P4）— 三套 RNG 并存，游戏不可复现

**问题：** 项目中有三套独立的随机数生成器：

| # | RNG 源 | 用途 | 种子策略 |
|---|--------|------|---------|
| 1 | 局部 `SmallRng`（init 时创建） | 地图生成 + 怪物生成掷骰 | `rand::random()` 每局随机 |
| 2 | 线程局部 `RefCell<SmallRng>` | 仲裁 system 随机重排 | 硬编码 `0` |
| 3 | `rand::random::<f32>()` 直接调用 | 暴击判定、掉落掷骰 | 系统熵源 |

**影响：**
- 地图种子用 `rand::random()` → 每局地图不同 ✓（但测试不可复现）
- 暴击用系统熵 → 不可复现
- 掉落用 `rand::random()` [components.rs:35] 和 `dungeon_core::global::rand_u8()` [global.rs:19] 两种——仲裁使用线程局部 RNG
- `GameRng { rng: SmallRng }` ECS 资源在 `setup_world` 初始化 [init.rs:20] 后从未被消费

**结论：** 同样的种子不可能复现同一场战斗。统一使用 `GameRng` 并移除线程局部 RNG 是正确方向但需要较大改动。

**位置：** `dungeon-core/src/global.rs:11`、`dungeon-core/src/components.rs:35`、`dungeon-action/src/execute.rs:131`、`dungeon-world/src/init.rs:20`、`dungeon-world/src/persist.rs:168`

---

### 🟡 D2 — 存档/读档丢弃 Intent 缓冲区状态

**问题：** `GameSave::capture()` 保存了 `ActionQueue`（Attack 条目因 Entity 引用被跳过），但 `ChaseIntents` / `FleeIntents` / `WanderIntents` 三个意图缓冲区未保存。`restore()` 直接重置为 `default()`。

**影响：** 读档后怪物需等下一次并行决策（下一帧）才能行动。对于 MVP 可接受，但频繁存档/读档时体验不佳。

**位置：** `dungeon-world/src/persist.rs:171-176`

---

### 🟡 D3（旧 P5）— crate 依赖链文档与实际不符

**问题：** 文档中写的依赖链是 `core → action → world → render`，但实际：

```
core ──→ action ──→ world
  │
  └────────→ render
```

`render_ui()` 直接从 ECS World 查询组件，而非从 world crate 接收预处理的帧数据。

**影响：** render 和 core 的组件布局隐式耦合——重命名 `Stats.hp` 会导致 render 静默编译失败。但好处是修改渲染逻辑不会触发 world crate 重编译。

**位置：** README.md（文档）、`dungeon-render/Cargo.toml`（`dungeon-render` 不依赖 `dungeon-world`）

---

### 🟢 D4 — 升级满血满蓝未文档化（优先级低）

**问题：** `apply_exp_system` 中升级后 `player.hp = player.max_hp; player.mp = player.max_mp` [systems.rs:39-40]，在 GAME.md 和 DESIGN.md 中均未记录。

**说明：** 这是有意的设计简化——非传统 Roguelike 行为，但方便快速测试游戏循环不同阶段的体验。优先保留，不做改动。如果未来需要更严格的生存挑战，可以改为回复 30%~50%。

**建议方向：** 在 GAME.md 中补充说明，或未来改为部分回复。

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

## 二、架构层面（Architecture）

---

### 🔴 A1（旧 P2）— dungeon-core 与 dungeon-world 大量代码重复

**问题：** 以下函数在 `dungeon-core` 和 `dungeon-world` 中各有一份近乎相同的实现：

| 重复项 | core 副本 | world 副本 |
|--------|----------|------------|
| `setup_world()` | `dungeon-core/src/api.rs` | `dungeon-world/src/init.rs` |
| `fov_system` | `dungeon-core/src/systems.rs:7-10` | `dungeon-world/src/systems.rs:11-14` |
| `check_death_system` | 同上:13-19 | 同上:16-22 |
| `apply_exp_system` | 同上:22-42 | 同上:24-44 |
| `buff_tick_system` | 同上:44-50 | 同上:46-52 |

**根因：** 重构到一半——`dungeon-world` 被创建来承接世界生命周期逻辑，但旧文件未清理。core 副本保留是为了 `cargo test -p dungeon-core` 可用。

**建议方向：** 让 core 以 dev-dependency 依赖 dungeon-world，测试改用 `dungeon_world::setup_world()`，然后删除 core 中的重复项。

---

### 🔴 A2 — 背包 250+ 行 UI + 交互逻辑内嵌在 src/main.rs

**问题：** `open_inventory()` 约 250 行（列表浏览、详情查看、装备/卸载/丢弃/拾取等完整交互）全部在 `src/main.rs` 中，而非独立 crate。

**影响：** main.rs 的职责应该是"启动游戏、主循环调度"，不应承载完整的背包子系统。可测试性差（背包交互无法在测试中覆盖）。

**位置：** `src/main.rs:224-474`

---

### 🟡 A3 — action/tick.rs 与 world/tick.rs 任务边界模糊

**问题：** `dungeon-action/src/tick.rs` 定义了串行 `advance_and_settle()`，包含怪物决策、碰撞图重建、FOV、视野记忆、death/buff 系统。`dungeon-world/src/tick.rs` 又定义了 `advance_and_settle_parallel()` 和 `advance_and_settle_serial()`，几乎重新包装了同样流程。

**理想边界：** action 只负责"推进和执行"；world 负责"编排和状态同步"。当前有重叠。

**位置：** `dungeon-action/src/tick.rs:28-44`、`dungeon-world/src/tick.rs:43-54`

---

### 🟡 A4 — 环境修饰方法全部塞在 Map 的 impl 块中

**问题：** `generate_water`、`carve_expand`、`generate_stalactites`、`ensure_connectivity`、`ensure_spawn_accessible` 全都在 `dungeon-core/src/lib.rs` 的 `impl Map` 块中（~350 行）。Map 的职责应为"容纳 tile 数据 + 基本查询"，而非完整的生成管线。

**影响：** Map 模块膨胀到 ~550 行。生成管线逻辑与数据结构耦合。

**位置：** `dungeon-core/src/lib.rs:170-500`

---

## 三、实现层面（Implementation）

---

### ✅ I1（旧 P11）— 对角穿墙角不对称：玩家可穿，怪物不可穿

**问题：** `handle_player_direction` 只检查目标格的 walkable + occupied，不做对角验证。A* 寻路中有穿墙角检查（两个直边都必须 walkable 才允许走对角）。

**影响：** 玩家可以走对角穿过墙角凸起，怪物寻路则在拐角处绕远路。

**修复：** 移除 A* 中的对角穿墙角检查，玩家和怪物均可穿墙角（等效行为）。

**位置：** `dungeon-core/src/ops.rs:162-170`（A* 对角检查）

---

### 🔴 I2（旧 P12）— 逃跑无退出条件（触发后永远逃跑）

**问题：** Flee 的 `check_condition` 要求 `hp_ratio < 0.25`。触发条件与退出条件相同。怪物没有回血机制，一旦进入 HP<25% 就永远逃跑直到死亡或走到地图边缘。

**位置：** `dungeon-action/src/execute.rs:53`

**建议方向：** 引入滞回区间——进入逃跑 HP < 25%，退出条件 HP > 30%。

---

### ✅ I3 — 火球技能击杀无经验/无掉落，且会伤害玩家自身（已修复 — 删除火球技能）

**问题：**
1. `execute_skill::Firebolt` 击杀后直接 `world.entity_mut(*me).despawn()`，未触发 `PendingExp` 和 LootTable。
2. 查询 `(Entity, &mut Stats, &Position, &EntityName)` 未排除玩家实体——玩家站在怪物旁释放火球时自己也会被击中并扣血 [execute.rs:218-221]。

**修复：** 删除 Firebolt 技能条目和相关代码。法师职业改为护盾+狂暴（与战士共享技能组）。

**位置：** `dungeon-action/src/execute.rs:200-241`、`dungeon-core/src/components.rs:131-145`

---

### ✅ I4 — 装备卸载回滚不完整（与 DESIGN.md 第 11 条矛盾）

**问题：** DESIGN.md 要求原子语义"全部成功或全部失败"。但 `inv.add()` 是"尽可能添加"——如果背包只剩 1 格但装备 `count` 为 1 则不会出问题，但如果 count > 1（虽然当前装备都是 1，但不安全）：

```rust
let leftover = inv.add(stack.item_id, stack.count);
if leftover > 0 {
    slot.replace(stack);  // 放回装备槽
    // 但已成功添加的部分未从背包移除
}
```

**影响：** 潜在物品复制 bug，当前 `count=1` 不会触发，但语义错误。

**位置：** `src/main.rs:414-425`

---

### 🟡 I5 — 怪物游荡使用确定性方向而非随机

**问题：** `execute_wander` 使用 `(FloorNumber + monster_count) % 8` 作为游荡方向索引。同一楼层所有怪物朝同一方向游荡。

**位置：** `dungeon-action/src/execute.rs:105`

---

### ✅ I6 — apply_exp_system 在每个 ready 条目后调用（Won't Fix）

**问题：** `advance_action_queue` 循环中每个 ready 条目执行后都调用一次 `apply_exp_system` [execute.rs:33]。

**评估：**
- 该函数有 `if pending.amount == 0 { return; }` 早返回，非击杀条目的调用开销 <1μs
- 在**事件帧模式（D5）**下，每个事件步需要即时反馈经验变化/升级——每个条目后调用反而是正确行为
- 在**玩家行动模式**下，即使批量执行多个条目，只有击杀的那次调用会进入实际逻辑

**结论：** 保留现状，无需修改。

---

### ✅ I7 — PendingLevelUp 悬空（已修复 — 删除此机制）

**问题：** `PendingLevelUp { points: u32 }` 在升级时累积 3 点 [systems.rs:36]，但代码中没有"分配属性点"的任何路径。玩家实际无法使用这 3 个点数。

**建议方向：** 删除 `PendingLevelUp` 资源及其相关代码。升级时只提升等级、重新计算 HP/MP、不留下未完成的"加点"入口。

**位置：** `dungeon-core/src/resources.rs:18-19`、`dungeon-world/src/systems.rs:34-36`

---

### 🟡 I8 — 怪物生成数量固定 12 只，地面物品固定 8 件，不随楼层递增

**问题：** `roll_monster_kinds(12, ...)` 在 `setup_world` 和 `descend` 中都写死 `room_count=12`。地面物品 `ground_item_ids` 固定为 `[0,1,2,3,0,1,3,2]`。即使地图有更多房间或楼层层数增加，密度不变。

**位置：** `dungeon-world/src/init.rs:67`、`dungeon-world/src/init.rs:141`

---

### 🟢 I9 — 废弃注释和空白行

**问题：** `dungeon-core/src/systems.rs` 中存在 `// use crate::world; // 已移除` 注释和连续空行。

**位置：** `dungeon-core/src/systems.rs:3`

---

## 四、游戏逻辑层面（Game Logic）

---

### ✅ G2/G3 — 死后游戏仍推进（已修复）

**问题：** `TurnManager.game_over = true` 后，主循环仍调用 `advance_and_settle`：

```rust
if has_action {
    advance_and_settle(world);  // 死后仍推进
}
```

玩家死后怪物继续行动，渲染仅跳过地图显示但逻辑不停。

**位置：** `src/main.rs:89-93`、`dungeon-world/src/systems.rs:18-20`

---

### 🟢 G7 — 水体生成保护距离可能过大（优先级低）

**问题：** `generate_water` 使用 `is_away_from_rooms(x, y, 6)` [lib.rs:241] 保护房间中心不被水体覆盖。曼哈顿距离 `min_dist=6` 对于半径 4-6 的圆形/菱形房间可能过大，导致水体永远无法出现在合理位置。

**影响：** 视觉效果上水体偏少。不破坏功能，但影响地图多样性。

---

## 其他

### P7 — 玩家确认行动后无法取消（将被 D5 部分解决）

**问题：** tap-tap 双击确认后行动进入 `ActionQueue` 无法撤回。

**位置：** `dungeon-action/src/player.rs:17-20`

**说明：** 事件帧模式（D5）部分解决了此问题——玩家在事件帧模式下可以在自己行动执行前切换方向。玩家行动模式下仍无取消能力，但可通过"预览不匹配 → 自动取消"的 tap-tap 语义自然覆盖。

---

### P8 — 测试覆盖不全

**问题：** `cargo test -p dungeon-core` 中 6 个测试只覆盖核心类型层，未覆盖战斗、技能、物品、存档/读档、怪物 AI、下楼等。

`dungeon-action` 和 `dungeon-world` 没有任何测试。

**位置：** `dungeon-core/src/tests.rs`

---

## 问题优先级说明

- **🔴** = 直接影响运行正确性或游戏体验
- **🟡** = 影响代码质量和可维护性
- **🟢** = 整洁或边缘问题

旧 P 标号保留用于追踪延续性问题（P2→A1、P4→D1、P5→D3、P7→保留、P8→保留、P11→I1、P12→I2）。
