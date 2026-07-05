# Dungeon MVP

Rust 终端 Roguelike，基于 `ratatui` + `crossterm` + `bevy_ecs`。

## 架构

```
dungeon-core/         ← 纯逻辑（无渲染依赖）
  action.rs           ← 行动系统 v3：Action 组件 + ActionQueue + 保活检查 + 执行引擎
  components.rs       ← ECS 组件（Stats, Buffs, Player, Monster, LootTable, …）
  resources.rs        ← ECS 资源（PendingExp, EventLog, VisibleMemory, …）
  systems.rs          ← 基础系统（movement, FOV, 死亡检测, 技能, buff_tick, …）
  items.rs            ← ItemRegistry, ItemStack, Inventory, Equipment, ItemPickup
  api.rs              ← setup_world, descend, FOV, 碰撞图, 视野记忆
  save.rs             ← 存档/读档
  global.rs           ← 全局 World（OnceLock<RwLock<World>> + world!() 宏）
  tests.rs            ← 8 个单元测试

dungeon-render/       ← 渲染层（依赖 core + ratatui）
  color.rs            ← (u8,u8,u8) → ratatui::Color 转换
  timeline.rs         ← build_timeline（行动表）
  ui.rs               ← render_ui + build_stats_panel（含 VisibleMemory 灰色渲染）
  title.rs            ← draw_title

src/main.rs           ← 应用层：独立输入线程 + 主循环 + tap-tap + 背包界面
```

## 行动系统 v3

采用 **AV 统一值 + 全局单队列 + 保活检查 + 持续推进** 模型。

### 核心概念

- **行动即组件**：`CanMove`、`CanChase`、`CanFlee`、`CanWander`、`CanWait`
- **AV = 反应时 + 耗时**：单一值入队，av_remaining 递减至 0 自动执行
- **`ActionQueue` 全局单队列**：玩家与怪物混排，按 av 值决定顺序
- **事件式推进**：`next_event_distance()` → 同步推进 → `pop_ready()` → 保活检查 → 执行

### 行动流程

```
玩家: tap(预览) → tap(确认) → enqueue_if_absent → advance_until_player_acted
怪物: run_monster_decision() → 条件检查 → 仲裁(priority) → enqueue → 等待推进

advance_action_queue():
  next_event_distance() → 推进所有条目 av_remaining → pop_ready()
  → 逐个 check_condition() 保活检查 → execute_entry()
  → 每条目执行后 rebuild_occupancy()（防重叠）

advance_until_player_acted():
  循环 advance_action_queue 直到玩家行动被消费（或队列空）
  怪物决策只在外层跑一次
```

### 保活检查

执行前验证条件是否仍满足：

| 行动 | 检查内容 |
|------|----------|
| Move | 目标格是 Floor 且未被占用 |
| Attack | 目标实体仍是 Monster |
| Chase | 玩家仍在视野内 |
| Flee | HP 比率仍低于 25% |
| Wander/Wait/Skill | 始终通过 |

## 输入系统

独立输入线程 + 主循环非阻塞接收。

```
┌─ 输入线程 ─────────────────────┐
│ loop:                            │
│   poll(16ms) ← 限流              │
│   50ms 同键去重                  │
│   有按键 → send(channel)         │
└──────────────┬───────────────────┘
               │ try_recv()
               ▼
┌─ 主循环 ────────────────────────┐
│ loop:                            │
│   try_recv()                     │
│   有按键 → process_key()         │
│   ├ 预览(false) → 仅设 preview   │
│   ├ 确认(true) → advance()      │
│   └ 非行动键 → 即时执行         │
│   无按键 → sleep(1ms)            │
│   render_ui()                    │
└──────────────────────────────────┘
```

### tap-tap 输入

| 操作 | 一次按 | 二次按（同键） |
|------|--------|---------------|
| 方向键 | 预览 | 确认移动/攻击 |
| `.` | 预览 | 确认等待 |
| `1-4` | 预览 | 确认技能 |

### 操作一览

| 按键 | 功能 |
|------|------|
| `↑↓←→` | 移动 / 攻击（双击确认） |
| `1-4` | 技能（双击确认） |
| `.` | 等待（双击确认） |
| `e` | 背包（双栏界面） |
| `g` | 拾取脚下物品 |
| `>` | 下楼 |
| `F5` | 存档 |
| `F9` | 读档 |
| `q` / `Esc` | 退出 |

### 背包操作

| 按键 | 功能 |
|------|------|
| `←` `→` | 切换左右栏（背包/地面） |
| `↑` `↓` | 移动选中项 |
| `Enter` | 查看详情 |
| `0-9` `a-z` | 快捷选中 |
| `e` | 装备（详情页） |
| `d` | 丢弃（详情页） |
| `u` | 卸载装备（详情页） |
| `g` | 拾取地面物品 |
| `Esc` | 返回 / 关闭 |

## 物品系统

参考 **Minecraft 1.16+** 的 Registry + ItemStack + LootTable 设计。

- **ItemRegistry**：`assets/items.json` 定义，`OnceLock` 全局单例
- **ItemStack**：`(item_id, count)`，自动堆叠至 max_stack
- **Equipment**：直接持有 ItemStack，不占背包空间
- **LootTable**：怪物组件，死亡时独立概率掷骰

详细数值见 [GAME.md](GAME.md)。

## 视野记忆

`VisibleMemory` 资源记录实体的最后已知位置。视野外的实体在已探索区域以灰色显示。死亡实体自动清理。

## 全局 World

使用 `OnceLock<RwLock<World>>` + `world!()` 宏。

```rust
// 读
let w = world!();
w.resource::<T>();
w.get::<T>(entity);

// 写
let mut w = world!(mut);
w.resource_mut::<T>();
w.get_mut::<T>(entity);
```

**注意**：`RwLock` 不可重入，持有锁时不能调另一个需要锁的函数。

### 构建

```bash
cargo run
cargo test -p dungeon-core -- --test-threads=1
```

## 设计参考

- Minecraft 1.16+：Registry + ItemStack + LootTable 物品系统
- FFX CTB（Conditional Turn-Based）：队列增量模型
- DCSS aut 系统：事件式时间片推进
- Dota 2 / Overwatch Ability Component：行动即组件模式
