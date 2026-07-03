# Dungeon MVP

Rust 终端 Roguelike，基于 `ratatui` + `crossterm` + `bevy_ecs`。

## 架构

```
dungeon-core/         ← 纯逻辑（无渲染依赖）
  action.rs           ← 行动系统 v3：Action 组件 + ActionQueue + 决策 + 执行引擎
  components.rs       ← ECS 组件（Stats, Buffs, Player, Monster, …）
  resources.rs        ← ECS 资源（GamePacing, EventLog, …）
  systems.rs          ← 基础系统（movement, FOV, 死亡检测, 技能, …）
  items.rs            ← 物品/装备
  pathfinding.rs      ← A* 寻路
  api.rs              ← setup_world, descend, FOV, 工具函数
  save.rs             ← 存档/读档
  global.rs           ← 全局 World（OnceLock<RwLock<World>> + world!() 宏）
  tests.rs            ← 8 个单元测试

dungeon-render/       ← 渲染层（依赖 core + ratatui）
  color.rs            ← (u8,u8,u8) → ratatui::Color 转换
  timeline.rs         ← build_timeline（行动表）
  ui.rs               ← render_ui + build_stats_panel
  title.rs            ← draw_title + draw_level_up

src/main.rs           ← 应用层：主循环、输入处理、模式切换
```

## 行动系统 v3

采用 **组件化行动 + 全局队列 + 统一 tap-tap 输入** 模型。

### 核心概念

- **每个行动是一个 Component**：`CanMove`、`CanChase`、`CanFlee`、`CanWander`、`CanWait`
- **反应时是实体属性**：`Reaction` 组件，由敏捷派生
- **耗时为行动属性**：`duration`，每个 Action 组件自己的常量
- **全局 `ActionQueue`**：所有实体的已确认待执行行动
- **事件式推进**：找到最近的执行点，一次性推进到该点，执行，继续

### 行动流程

```
玩家: tap → 预览 → tap(同方向) → 入队 ActionQueue
怪物: run_monster_decision() → 条件检查 → 仲裁(priority) → 入队

advance_action_queue():
  next_event_distance() → 同步推进所有条目 → pop_ready() → execute_*()
  → 返回推进量 → tick_all_cooldowns(推进量) 同步冷却
```

### 输入模型

统一 tap-tap，不再区分探索/战斗模式：

```
第一次 tap → 预览（显示待确认）
第二次 tap → 确认（入行动队列）
不同行动  → 替换预览
无效(墙)  → 丢弃
```

方向键语义识别：前方有敌人→Attack，是墙→丢弃，地板→Move。

### 操作

| 按键 | 功能 |
|------|------|
| `h/j/k/l` 或 `↑↓←→` | 移动 / 攻击（tap-tap） |
| `1-4` | 技能 |
| `e` | 背包 |
| `>` | 下楼 |
| `.` | 等待 |
| `F5` | 存档 |
| `F9` | 读档 |
| `q` / `Esc` | 退出 |

### 构建

```bash
cargo run
cargo test -p dungeon-core -- --test-threads=1
```

## 全局 World

使用 `OnceLock<RwLock<World>>` + `world!()` 宏，所有公共函数不再传 `&mut World` 参数。

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

## 设计参考

- FFX CTB（Conditional Turn-Based）：队列增量模型
- DCSS aut 系统：事件式时间片推进
- Dota 2 / Overwatch Ability Component：行动即组件模式
