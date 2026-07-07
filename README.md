# Dungeon MVP

Rust 终端 Roguelike，基于 `ratatui` + `crossterm` + `bevy_ecs`（0.16）。

## 架构（5 crate 拆分）

```
terrain-forge/            ← 程序化地图生成引擎（被 dungeon-core 使用）

dungeon-core/             ← 纯数据 + 工具函数（被所有其他 crate 依赖）
  action_types.rs          ← 行动系统类型：ActionKindV3、ActionQueue、Reaction、CanMove/Chase/…
  components.rs            ← ECS 组件（Stats, Buffs, Player, Monster, LootTable, …）
  resources.rs             ← ECS 资源（PendingExp, EventLog, VisibleMemory, TurnManager, …）
  items.rs                 ← ItemRegistry（OnceLock 单例）、ItemStack、Inventory、Equipment
  monster_def.rs           ← 怪物定义：MonsterKindId、属性公式、掉落、生成权重
  ops.rs                   ← 工具函数：碰撞图 rebuild、视野记忆、拾取、渲染收集、A* 寻路（8 方向）
  systems.rs               ← 基础 ECS System（FOV、死亡检测、经验应用、buff 衰减）
  api.rs                   ← 旧版 setup_world（仅测试使用）

dungeon-action/           ← 行动执行逻辑（依赖 core）
  execute.rs               ← advance_action_queue、保活检查、execute_entry（移动/攻击/技能/怪物 AI）
  monster.rs               ← 并行决策 system（chase / flee / wander → arbitration）
  player.rs                ← 玩家 tap-tap 行动处理（direction / wait / skill）
  tick.rs                  ← 串行编排：advance_until_player_acted

dungeon-world/            ← 世界生命周期 + 并行调度（依赖 action + core）
  init.rs                  ← setup_world（正式入口）、descend（下楼）
  persist.rs               ← GameSave（存档/读档，显式 &World 参数）
  systems.rs               ← 世界级 ECS System 包装（fov / death / buff / exp）
  tick.rs                  ← 并行 Schedule（advance_and_settle_parallel）

dungeon-render/           ← 渲染层（依赖 core + ratatui，**不依赖 world**）
  color.rs                 ← (u8,u8,u8) → ratatui::Color 转换
  timeline.rs              ← build_timeline（行动轴面板）
  ui.rs                    ← render_ui + build_stats_panel（含 VisibleMemory 灰色渲染）
  title.rs                 ← draw_title（标题画面）

src/main.rs               ← 应用层：独立输入线程 + 主循环 + tap-tap + 背包界面
```

### 实际依赖链

```
core ← action ← world
  ↕
render（只依赖 core，不依赖 world）
```

核心 crate 不依赖渲染或世界生命周期，渲染 crate 直接从 ECS World 查询组件。这意味着修改渲染逻辑不需要重新编译其他 crate，但 render 与 core 的组件布局存在隐式耦合。

## 行动系统 v3

采用 **AV 统一值 + 全局单队列 + 保活检查 + 持续推进** 模型。

### 核心概念

- **行动即组件**：`CanMove`、`CanChase`、`CanFlee`、`CanWander`、`CanWait`
- **AV = 反应时 + 耗时 × speed_factor**：单一值入队，av_remaining 递减至 0 自动执行
- **敏捷修正**：反应时 `max(100 - agi×3, 20)`，耗时系数 `max(1.0 - agi×0.02, 0.5)`
- **`ActionQueue` 全局单队列**：玩家与怪物混排，按 av 值决定顺序
- **8 方向移动**：玩家 Home↖ ↑ ↗ PgUp ← → End↙ ↓ ↘ PgDn，怪物 AI 使用 A* 寻路
- **事件式推进**：`next_event_distance()` → 同步推进 → `pop_ready()` → 保活检查 → 执行

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

80 × 60（4800 格）的洞穴地图由 **terrain-forge** 引擎按管线生成。

- **算法**：`room_accretion` — Brogue 风格有机洞穴，滑动房间直到贴合已有结构
- **生成管线**：terrain-forge → detect_cave_regions → generate_water → carve_expand → generate_stalactites → ensure_connectivity
- **Tile 种类**：`Wall` `Floor` `ShallowWater`(~浅蓝，可行走) `DeepWater`(≈深蓝，不可行走) `Stalactite`(#黄)
- **水体生成**：2% 噪声深水种子 → 深水扩散(25%-面积×2%) → 浅水扩散(10%)
- **房间检测**：BFS flood-fill 找出连通 Floor 区域（max 12），按大小排序
- **连通性保障**：2 格宽醉汉游走通道连接孤立区域
- **出生点安全**：`ensure_spawn_accessible` — 检查 8 方向可达性，被困则醉汉游走打破
- **楼梯位置**：距 rooms[0]（出生房间）曼哈顿距离最远的房间中心
- **随机化**：每局使用随机种子（`MapSeed` 资源），存档保存种子保证下楼一致性

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
- Brogue：有机洞穴 + vault 模板 + 细胞自动机 + 水体/环境格
- terrain-forge（EliasVahlberg）：room_accretion 算法、Grid<C> 泛型系统
- Dwarf Fortress：水体渲染（背景色为主、前景 glyph 为纹理）
- Cogmind / Brogue：A* 寻路 + 8 方向移动

## 项目文档

| 文档 | 用途 |
|------|------|
| [GAME.md](GAME.md) | 数值设计文档 — 行动耗时、战斗公式、属性、经验、掉落 |
| [ISSUES.md](ISSUES.md) | 问题追踪 — 设计/架构/实现/游戏逻辑层面的已知问题 |
| [LESSONS.md](LESSONS.md) | 抽象教训 — 从已修复问题中提炼的 Rust/ECS/游戏开发经验 |
| [RULE.md](RULE.md) | 操作规则 — 文档维护规范、工作流、设计模式、RNG 规范 |
