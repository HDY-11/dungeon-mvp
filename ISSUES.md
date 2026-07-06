# 发现的问题记录

本文档记录当前实现中与设计意图不一致或有改进空间的问题，
在后续开发中可作参考。

---

## ✅ P1 — 保活检查只检查即将执行的条目（已修复）

**问题：** `advance_action_queue()` 只在从队列中弹出条目时（`pop_ready()`）调用 `check_condition()`。队列中其他 `av > 0` 的条目在等待期间从未被重新验证。

**影响：** 一个 Chase 行动入队时玩家在视野内，但如果玩家在 Chase 执行前离开了视野，该条目仍留在队列中白白等待，直到 av 耗尽才被取消。这浪费了怪物的 AV，导致响应滞后。

**位置：** `dungeon-action/src/execute.rs:20-34`

**预期设计：** 每次队列推进时，重新验证队列中所有条目的条件。不满足的立即剔除，让实体可以重新决策。

---

## P2 — dungeon-core/src/api.rs 与 dungeon-world/ 大量代码重复

**问题：** 以下函数在 core 和 world 中各有一份近乎相同的实现：

| 重复项 | core 副本 | world 副本 |
|--------|----------|------------|
| `setup_world()` | `dungeon-core/src/api.rs` | `dungeon-world/src/init.rs` |
| `calculate_visible_tiles()` | `dungeon-core/src/api.rs` | `dungeon-world/src/fov.rs` |
| `rat_loot()` / `goblin_loot()` | `dungeon-core/src/api.rs` | `dungeon-world/src/loot.rs` |
| `fov_system`、`check_death_system`、`apply_exp_system`、`buff_tick_system` | `dungeon-core/src/systems.rs` | `dungeon-world/src/systems.rs` |

**根因：** 重构到一半——dungeon-world 被创建来承接世界生命周期逻辑，但旧文件未清理。core 副本保留是为了 `cargo test -p dungeon-core` 能用（测试依赖 core 的 `setup_world`）。

**建议方向：** 让 core 以 dev-dependency 依赖 dungeon-world，测试改用 `dungeon_world::setup_world()`，然后删除 core 中的重复项。

---

## ✅ P3 — 并行 Schedule 每帧重建（已评估 — 每帧构建开销 <1μs，为保持测试跨 World 兼容保留现状）

**问题：** `build_parallel_schedule()` 在 `advance_and_settle_parallel()` 中被每帧调用一次，每次都重新构建 `Schedule` 对象。

**位置：** `dungeon-world/src/tick.rs:11-17`

**影响：** 虽然每次重建的绝对开销很小（微秒级），但 Bevy 提供了 Schedule 缓存能力。每帧重建意味着丢失了 Schedule 带来的 system 排序优化可能。

**建议方向：** 用 `OnceLock` 或 lazy_static 缓存 Schedule，只在第一次构建。

---

## P4 — 两套 RNG 并存导致不可重现

**问题：** 项目中有两套独立的随机数生成器：

1. **ECS Resource `GameRng`** — 在 `setup_world` 时初始化，种子硬编码 `seed_from_u64(0)`
2. **线程局部 `RefCell<SmallRng>`** — 在 `dungeon-core/src/global.rs` 中，种子同样硬编码 `seed_from_u64(0)`

**影响：**
- 地图生成使用种子 `42`（硬编码在 `setup_world` 中）→ **每次游戏地图相同**
- 战斗暴击使用 `rand::random::<f32>()`（从系统熵源取种子）→ **战斗随机**
- 优先级仲裁使用线程局部 RNG 而非 `GameRng` → **仲裁不可复现**

三套不同的随机策略意味着游戏的随机行为不可能被复现——这对调试非常不利。

**位置：** `dungeon-core/src/global.rs`、`dungeon-world/src/init.rs`、`dungeon-world/src/persist.rs`、`dungeon-action/src/execute.rs:135`（`rand::random()`）

**建议方向：** 统一使用 ECS Resource `GameRng`，去除线程局部 RNG。地图生成、战斗掷骰、仲裁全部走同一个 RNG 源，可复现。

---

## P5 — crate 依赖链不诚实

**问题：** 文档中写的依赖链是 `core → action → world → render`，但实际依赖是：

```
core ← action ← world
  ↕
render（只依赖 core，不依赖 world）
```

`render_ui()` 直接从 ECS World 中查询组件（Map、Stats、Inventory 等）而不是从 world crate 接收预处理的帧数据。这意味着：

- world crate 的角色不是真正的"编排层"——它只是一组在 main.rs 中手动调用的库函数
- render 和 world 之间没有接口契约，两个 crate 通过"都知道 ECS 组件布局"来隐式耦合

**影响：** 如果修改了 ECS 组件（如在 core 中重命名 `Stats.hp`），render 和 world 都会静默编译失败——但如果修改的是渲染逻辑（如改变 HP 条颜色），world crate 不需要重新编译，这算是一个优点。

---

## ✅ P6 — action.rs 是空壳模块（已修复 — 已删除 action.rs，引用统一为 action_types）

**问题：** `dungeon-core/src/action.rs` 只有一行代码：

```rust
pub use crate::action_types::*;
```

真正的定义在 `action_types.rs` 中。

**影响：** 增加了模块树的深度，不提供任何价值。代码中有的地方 `use dungeon_core::action::ActionQueue`，有的地方 `use dungeon_core::action_types::ActionKindV3`，引用方式不统一。

**建议方向：** 删除 `action.rs`，所有引用改到 `action_types`。

---

## P7 — 玩家确认行动后无法取消

**问题：** tap-tap 双击确认后，行动进入 `ActionQueue` 就无法撤回。如果玩家误操作（如不小心确认了向怪物走去），只能等行动执行或被怪物杀死。

**位置：** `dungeon-action/src/player.rs:17-20`

**建议方向（待定）：** 是否加入取消机制需要设计讨论。一种可能的方案：方向键反方向取消上一个移动预览，或按特定键（如 `Esc` 在预览状态下清空预览，在已确认状态下弹出询问）。

---

## P8 — 测试覆盖不全

**问题：** `cargo test -p dungeon-core` 中的 6 个测试只覆盖了核心类型层：

| 覆盖 | 未覆盖 |
|------|--------|
| 队列推进逻辑 | 战斗伤害计算与暴击 |
| 条件函数逻辑 | 技能效果（火球/治愈/护盾/狂暴） |
| 反应时计算 | 物品系统（Inventory 堆叠/Equipment 装卸） |
| tap-tap 预览 | 存档/读档 roundtrip |
| 资源存在性 | 怪物 AI 决策 |
| | 下楼/穿越楼层 |
| | 死亡/游戏结束流程 |

`dungeon-action` 和 `dungeon-world` 没有任何测试。

**位置：** `dungeon-core/src/tests.rs`

---

## ✅ P9 — VisibleMemory 在视野边缘闪烁（已修复 — 加入 VISIBLE_FORGET_DELAY=3 遗忘延迟）

**问题：** `VisibleMemory` 在实体离开玩家视野时立即将其条目移除。但如果实体在视野边缘来回移动（一个单位格的距离），会导致该实体在渲染中反复出现和消失。

**位置：** `dungeon-core/src/ops.rs:107-125`

**影响：** 视觉上的闪烁，虽然不是功能性问题，但影响游戏体验的精致度。常见的解决方案是加入一个"遗忘延迟"——实体离开视野后 N 帧内保留记忆。

---

## ✅ P10 — 存档缺少对 ActionQueue 的序列化（已修复 — 位置映射方式保存/恢复，Attack 条目因 Entity 引用跳过）

**问题：** `GameSave` 没有保存和恢复当前 `ActionQueue` 内容、`ChaseIntents`/`FleeIntents`/`WanderIntents`。读档后这些状态重置为空。

**位置：** `dungeon-world/src/persist.rs`

**影响：** 读档后所有实体的行动被清空，需要等下一轮决策重新入队。玩家读档后可能会经历一小段"静默期"（怪物重新决策前的延迟）。对于 MVP 来说可接受，但如果存档/读档频繁则体验不佳。

---

## 问题优先级说明

- **P1** = 直接影响运行行为的正确性
- **P2-P5** = 影响代码质量和可维护性
- **P6-P10** = 功能缺口或体验细节
