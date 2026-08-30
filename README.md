# Dungeon MVP

Rust 终端 Roguelike，基于 `ratatui` + `crossterm` + `bevy_ecs`（0.16）。

## 架构（5 crate 拆分）

```
terrain-forge/            ← 程序化地图生成引擎（被 dungeon-core 使用）

dungeon-core/             ← 纯数据 + 工具函数（被所有其他 crate 依赖）
  action_types.rs          ← 行动系统类型：ActionKindV3、ActionQueue、PlayerPreview、CanMove/Chase/…
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

dungeon-render/           ← 渲染层（依赖 core + action，仅行动类型；**不依赖 world**）
  color.rs                 ← (u8,u8,u8) → ratatui::Color 转换
  timeline.rs              ← build_timeline（行动轴面板）
  ui.rs                    ← render_ui + build_stats_panel（含 VisibleMemory 灰色渲染）
  title.rs                 ← draw_title（标题画面）

src/main.rs               ← 应用层：入口 + 标题画面 + 独立输入线程 + 主循环
src/pages/                ← 页栈按键处理器，按页拆分（Game / Look / ThrowSelect / ThrowAim / Inventory / Dialog）
src/keymap.rs             ← 声明式键位绑定表（按键 → PlayerAction）
src/throw.rs              ← 投掷辅助（弹道预览 update_throw_path / 自动装填 auto_equip_throwable / confirm_throw）
```

### 实际依赖链

```
core ← action ← world
  ↕        ↗
render ───╯（依赖 core + action——仅行动类型引用，不依赖 world）
```

核心 crate 不依赖渲染或世界生命周期，渲染 crate 直接从 ECS World 查询组件。这意味着修改渲染逻辑不需要重新编译其他 crate，但 render 与 core 的组件布局存在隐式耦合。render 对 action 的依赖仅限类型引用（`timeline.rs` 使用 `ActionQueue`/`ActionKindV3`/`PlayerPreview`），不依赖行动执行逻辑——若未来行动类型提取为独立 crate，render 可切换依赖取消对 action 的耦合（Dsn1）。

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
│   33ms 同键去重                  │
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

**游戏页：**

| 按键 | 功能 |
|------|------|
| `↑↓←→` `Home` `End` `PgUp` `PgDn` | 移动 / 攻击（8 方向，tap-tap 双击确认） |
| `1-4` | 技能（双击确认；需先学习卷轴） |
| `.` | 等待（双击确认） |
| `e` | 背包（双栏界面） |
| `x` | 查看模式（方向键移动光标，`x`/`Esc` 退出） |
| `t` | 投掷（副手石子 → 瞄准 → `Enter` 投掷，`x`/`Esc` 取消） |
| `g` | 拾取脚下物品 |
| `>` | 下楼（需站在楼梯上） |
| `F5` / `F9` | 存档 / 读档 |
| `q` / `Esc` | 退出 |

**背包页：**

| 按键 | 功能 |
|------|------|
| `←` `→` | 切换左右栏（装备+背包 / 地面） |
| `↑` `↓` | 移动选中项 |
| `Enter` | 查看详情 |
| `0-9` `a-z` | 快捷选中 |
| 详情页 `e` | 装备（仅可装备物品） |
| 详情页 `r` | 使用 / 学习（仅卷轴等可用物品） |
| 详情页 `d` | 丢弃（背包物品） |
| 详情页 `u` | 卸载（装备槽） |
| 详情页 `g` | 拾取该地面物品（地面栏详情） |
| `Esc` | 返回 / 关闭 |

> 详情页操作提示（`e:装备`、`r:使用/学习` 等）由共享判定生成，与按键处理器一致——**提示什么就能按什么**。

### 技能与投掷

- **技能**：技能卷轴拾取后，背包详情页按 `r` 学习 → 按 `1-4` 施放（第一次按键预览，同键第二次确认）。重复学习同卷轴提升熟练度；技能消耗 MP，部分技能依赖法术精通属性。
- **投掷**：`t` 进入瞄准。副手已有石子直接瞄准，否则自动从背包装填（副手若持有木盾等非投掷物会先放回背包）。方向键移动光标（红=无效，蓝=有效），`Enter` 投掷，`x`/`Esc` 取消。射程 5 格（切比雪夫距离），弹道任一格阻挡视线则无法投掷；命中消耗 1 颗副手石子。

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
- **多类型（Dsn24）**：`MapKind` 三类型（标准洞穴/繁茂洞穴/地海），由 `map_kind_for(seed, floor)` 确定性派生（F1 固定标准洞穴）；环境修饰差异化（水域规模、障碍种类、装饰方块）
- **生成管线**：terrain-forge → detect_cave_regions → generate_water → carve_expand → generate_obstacles → generate_terrain_decor → ensure_connectivity
- **Tile 种类**：`Wall` `Floor` `ShallowWater`(可行走) `DeepWater`(不可行走) `Stalactite`(#黄) + 繁茂（`Mycelium` `FungalPatch` `HangingVine`）+ 地海（`Sand` `Seagrass` `CoralReef`）
- **水体生成**：种子率按类型（洞穴 2‰ / 地海 20‰）→ 深水扩散 → 浅水扩散
- **通道挖掘**：墙挖成 Floor，深水变涉水浅水（保留水域且保证连通）
- **房间检测**：BFS flood-fill 找出连通 Floor 区域（max 12），按大小排序
- **连通性保障**：2 格宽醉汉游走通道连接孤立区域
- **出生点安全**：`ensure_spawn_accessible` — 检查 8 方向可达性，被困则醉汉游走打破
- **楼梯位置**：距出生房间（rooms[0]）曼哈顿距离最远的房间中心，落点兜底到最近可行走格（G22）
- **随机化**：每局使用随机种子（`MapSeed` 资源），存档保存种子保证下楼一致性；地图类型同样由种子确定性重建

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
