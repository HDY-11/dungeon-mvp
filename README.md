# Dungeon MVP

Rust 终端 Roguelike，基于 `ratatui` + `crossterm` + `bevy_ecs`（0.16）。

## 当前状态（refactor）

本分支正在向 **业务领域只存在于 `core`** 的方向重构。

- `core/` 是新的唯一业务/领域层，完全采用 ECS 范式。
- 旧代码（`dungeon-core/`、`dungeon-action/`、`dungeon-world/`、`dungeon-render/`、`src/`）视为历史/过渡实现，参考价值有限。
- 旧组件体系（`Stats`、`ActionKindV3`、`ActionQueue` 等）不再作为新功能基础。

## 架构（目标形态）

```
core/                     ← 唯一业务/领域层，完全 ECS
  components.rs           ← 领域组件：Position、Health、Magic、Level、Experience、Attack、Defense、…
  entity_cls.rs           ← 实体类型 marker：Player、Rat、…
  events.rs               ← 领域事件：AttackEvent、…
  resources.rs            ← 领域资源（规划中）
  system.rs               ← 领域系统：死亡、受击记录、普攻执行、…
  map_gen.rs              ← 地图生成（规划中）
```

旧 crate 在迁移完成前暂时保留：

```
dungeon-core/             ← 旧领域数据/工具（历史参考）
dungeon-action/           ← 旧行动执行（历史参考）
dungeon-world/            ← 旧世界生命周期（历史参考）
dungeon-render/           ← 旧渲染（历史参考）
src/                      ← 旧应用层（历史参考）
terrain-forge/            ← 地图生成子模块（按需保留）
```

渲染契约（新增，见 [DESIGN.md Dsn26](DESIGN.md)）：

```
render-api/               ← 后端无关的只读数据契约
  scene.rs                ← SceneFrame：每帧从 ECS 提取的场景快照
  visual.rs               ← VisualKey / VisualLayer：语义外观键与渲染层级
  ui.rs                   ← UiView：页面级视图模型
  input.rs                ← InputEvent / InputQueue / SurfaceInfo：后端无关输入与表面尺寸
```

- `tui` / 未来的 `gpu` 只依赖 `render-api`，不依赖 `core`；
- `presentation`（下一步）负责把 `core` 提取成 `SceneFrame`；
- 完整渲染后端插件化方案（presentation / TuiPlugin / bevy_app / GPU 切换）见 [DESIGN.md Dsn28](DESIGN.md) 与 [REFACTOR.md §12](REFACTOR.md)。

## 行动模型（core 方向）

采用 **ECS 原生组件模型**，不再使用全局 `ActionQueue`：

- 实体用 `Idle` / `Active` / `Failure` 表达行动状态
- `Can*` 组件表达行动能力
- 具体 Action 组件表达当前正在执行什么（如 `BasicAttack { target }`）
- 领域事件（如 `AttackEvent`）在系统间传递意图/结果
- 时间/推进由 ECS 资源与系统管理，按最小剩余 AV 推进

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
cargo check -p core
cargo run
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
