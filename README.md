# Dungeon MVP

Rust 终端 Roguelike，基于 `ratatui` + `crossterm` + `bevy_ecs`（0.16）。

## 架构（5 crate 拆分）

```
terrain-forge/            ← 程序化地图生成引擎（15 种算法）
  src/algorithms/           ← room_accretion（Brogue 风格）、cellular、bsp、maze……
  src/grid.rs              ← Grid<C> 核心：泛型 Cell 特质、BFS 连通区分析

dungeon-core/             ← 纯数据 + 工具函数（无渲染 / 无执行依赖）
  action_types.rs          ← 行动系统类型：ActionKindV3、ActionQueue、Reaction、CanMove/Chase/…
  components.rs            ← ECS 组件（Stats, Buffs, Player, Monster, LootTable, …）
  resources.rs             ← ECS 资源（PendingExp, EventLog, VisibleMemory, TurnManager, …）
  items.rs                 ← ItemRegistry（OnceLock 单例）、ItemStack、Inventory、Equipment
  monster_def.rs           ← 怪物定义：MonsterKindId、属性公式、掉落、生成权重
  ops.rs                   ← 工具函数：碰撞图 rebuild、视野记忆、拾取、渲染收集
  systems.rs               ← 基础 ECS System（FOV、死亡检测、经验应用、buff 衰减）
  api.rs                   ← 旧版 setup_world（仅测试使用）

dungeon-action/           ← 行动执行逻辑（依赖 core）
  execute.rs               ← advance_action_queue、保活检查、execute_entry（移动/攻击/技能/怪物 AI）
  monster.rs               ← 并行决策 system（chase / flee / wander → arbitration）
  player.rs                ← 玩家 tap-tap 行动处理（direction / wait / skill）
  tick.rs                  ← 串行编排：advance_until_player_acted + advance_and_settle（旧版兼容）

dungeon-world/            ← 世界生命周期 + 并行调度（依赖 action + core）
  init.rs                  ← setup_world（正式入口）、descend（下楼）
  persist.rs               ← GameSave（存档/读档，显式 &World 参数）
  systems.rs               ← 世界级 ECS System 包装（fov / death / buff / exp）
  tick.rs                  ← 并行 Schedule（advance_and_settle_parallel）
  fov.rs                   ← 视野计算（对称阴影投射）
  loot.rs                  ← 怪物掉落表定义

dungeon-render/           ← 渲染层（依赖 core + ratatui）
  color.rs                 ← (u8,u8,u8) → ratatui::Color 转换
  timeline.rs              ← build_timeline（行动轴面板）
  ui.rs                    ← render_ui + build_stats_panel（含 VisibleMemory 灰色渲染）
  title.rs                 ← draw_title（标题画面）

src/main.rs               ← 应用层：独立输入线程 + 主循环 + tap-tap + 背包界面
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
怪物: 并行 Schedule（chase∥flee∥wander → arbitration）→ enqueue → 等待推进

advance_action_queue():
  next_event_distance() → 推进所有条目 av_remaining → pop_ready()
  → 逐个 check_condition() 保活检查 → execute_entry()
  → 每条目执行后 rebuild_occupancy()（防重叠）

advance_until_player_acted():
  循环 advance_action_queue 直到玩家行动被消费（或队列空）
  怪物决策只在每帧开始跑一次（由 Schedule 编排）
```

### 并行怪物决策（Schedule）

```rust
Schedule: (
  chase_decision_system,   // 追击条件检查
  flee_decision_system,    // 逃跑条件检查
  wander_decision_system,  // 游荡条件检查（无 chase/flee 时才生成）
) → arbitration_system     // 合并意图，按优先级入队
```

三个条件检查并行执行（数据无竞争），各自写入独立的意图缓冲区，仲裁 system 串行合并。

旧串行入口 `run_monster_decision()` 保留兼容。

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
│   ├ 确认(true) → advance_and_settle_parallel()
│   └ 非行动键 → 即时执行         │
│   无按键 → sleep(1ms)            │
│   render_ui()                    │
│   check TurnManager.wants_quit   │
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

## 地图生成

80 × 40 的洞穴地图由 **terrain-forge** 引擎生成。

- **算法**：`room_accretion` — Brogue 风格有机洞穴，滑动房间直到贴合已有结构
- **房间检测**：BFS flood-fill 找出连通 Floor 区域，按大小排序作为房间列表
- **随机化**：每局使用随机种子（`MapSeed` 资源），存档保存种子保证下楼一致性
- **扩展预留**：可按 biome 切换算法（bsp/ cellular/ room_accretion）

## 视野记忆

`VisibleMemory` 资源记录实体的最后已知位置。视野外的实体在已探索区域以灰色显示。死亡实体自动清理。

## World 传递模式

**不再使用全局 `OnceLock<RwLock<World>>`。** 所有函数改为显式接收 `&World` / `&mut World` 参数。

```rust
// 读（任意函数签名）
fn read_something(world: &World) { ... }

// 写
fn write_something(world: &mut World) { ... }
```

这避免了 RwLock 死锁问题，且使数据流更清晰。

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
- Brogue：有机洞穴 + vault 模板 + 细胞自动机
- terrain-forge（EliasVahlberg）：room_accretion 算法、Grid<C> 泛型系统
