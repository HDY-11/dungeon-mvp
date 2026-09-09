# REFACTOR.md

> **本文件仅存在于 `refactor` 分支。**
>
> 它记录本次重构的目标架构、设计理由和行动系统规格。合并回主分支前，
> 本文件应被删除或改写成正式的 `DESIGN.md` 条目；不得作为长期设计文档进入 main。

---

## 1. 重构目标

旧架构以 `Stats` 聚合、`ActionQueue`、`ActionKindV3` 和大量 `&mut World` 工具函数为中心。
新架构将业务领域收敛到 `core`，并采用 ECS 原生模型：

- 组件只保存数据；规则在系统中。
- 实体范畴由 enum 表达；实体身份由 ZST marker 表达。
- 行动能力由 `Can*` 组件授权。
- 实体状态在 `Idle / Active / Failure` 之间轮转。
- 具体行动是挂在实体上的瞬态组件。
- 行动由**生成系统**产出意图，由**仲裁系统**选出唯一行动并挂载。
- 战斗结果通过事件传递：`AttackEvent -> Damage -> DeathEvent -> Exp/LevelUp`。
- 全局状态使用 `Resource`；`World` 继续显式参数传递，不恢复全局单例。

---

## 1.1 Crate 布局

重构目标下的 workspace 分层：

| crate | 职责 | 禁止 |
|---|---|---|
| `utils` | 无状态、无业务的通用工具与数据结构 | 依赖 bevy / ratatui / crossterm / 业务 crate |
| `core` | 唯一业务/领域层，ECS 组件、系统、地图、AI、战斗、初始化 | UI、OS 交互、纯工具函数 |
| `sys` | OS 交互：终端状态、输入线程、文件字节读写、文件日志 | 业务规则、ECS 组件、渲染 |
| `tui` | 渲染与 UI 状态：颜色转换、布局工具、Canvas、页面渲染 | 终端事件读取、文件 IO、业务规则 |
| `dungeon-app` | 根 crate / 应用装配：主循环、输入到 core、core 到 tui | 具体业务实现 |

`terrain-forge` 保持为外部地图生成引擎，不并入任何业务 crate。

### core 模块层级

```text
core/src/
  components.rs / entity_cls.rs / events.rs / resources.rs / balance.rs
  map/
    mod.rs / map_gen.rs
  spatial/
    fov.rs / pathfinding.rs / line.rs
  action/
    mod.rs
    execution/
      mod.rs / movement.rs
    generation/
      ai.rs / player.rs
  combat/
    mod.rs
  monster/
    mod.rs
  system/
    mod.rs
  world/
    init.rs / loop_.rs / query.rs
```

依赖方向：

```text
components/events/resources/balance
        ↓
map → spatial
        ↓
combat + action + monster
        ↓
system + world
```

---

## 2. 组件设计

### 2.1 实体范畴 enum

- `EntityClass`：最高层范畴，如 `Actor / Item / Buff / Projectile / Field`。
- `CreatureKind`：生物公式族，如 `Humanoid / Beast / Plant / MagicCreature / Construct / Aquatic`。

设计理由：

- 同一公式的实体共享同一范畴，避免按身份逐个 `match`。
- 物种级差异（如老鼠与蝎子的成长曲线）由 `MonsterKindId` 和 `MonsterTemplate` 表达。
- 范畴 enum 可以扩展，但只能追加变体，存档序列化不得重排。

### 2.2 实体身份 ZST

`Player / Monster / Stairs / Rat / Scorpion / Goblin / ...` 是零大小标记组件。

设计理由：

- 身份回答“这个实体是谁”，用于查询过滤和决定挂载哪些组件。
- 身份不承载数值，避免再次形成 `Stats` 式聚合。
- 身份与逻辑解耦：系统查询 `With<Player>`，而不是 `match kind { Player => ... }`。

### 2.3 数值组件

`Position / Health / Magic / Level / Experience / Attack / Defense / MagicMastery / Agility / CritRate / CritDamage` 均为细粒度组件，数值统一使用 `f64`。

设计理由：

- 细粒度组件让查询只读取需要的数据，是 ECS 缓存友好的前提。
- `f64` 为未来更精确的数值规划留出空间；旧的 i32/u32 数值不作为行为基准。

### 2.4 能力组件

`CanMove / CanWait / CanBasicAttack / CanChase / CanFlee / CanWander` 是纯标记组件。

设计理由：

- 能力表示“可以做什么”，不承载耗时、优先级等行动参数。
- 行动参数属于具体行动或行动配置，不应写回能力组件。
- 能力可以由装备、buff、状态授予或剥夺，因此必须可动态插入/移除。

### 2.5 状态组件

`Idle / Active / Failure` 表示实体的行动状态：

```text
Idle ──生成+仲裁──▶ Active ──执行成功──▶ Idle
  ▲                    │
  │                    └──保活失败──▶ Failure
  └────────────────────────────────────┘
```

- `Active` 实体同时持有 `ActionTimer` 和一个具体行动组件。
- `Failure` 是决策输入之一：失败实体会在下一轮重新进入生成系统。
- 具体行动组件是瞬态组件：`Wait / Move / BasicAttack / Chase / Flee / Wander`。

---

## 3. 行动系统

### 3.1 设计原则

行动系统由三段组成：

```text
行动生成系统（只读，产出 ActionIntent）
        ↓
行动仲裁系统（唯一写入方，选择并挂载行动）
        ↓
行动执行系统（AV 推进、保活检查、结算）
```

生成与仲裁分离的理由：

- 生成系统只读世界，可以并行、可以独立扩展。
- 仲裁系统是唯一修改行动状态的入口，保证“一个实体同一时间最多一个行动”。
- 避免旧架构中决策、优先级、条件散落在多个文件导致漂移。

### 3.2 ActionIntent

生成系统不直接改实体，只产出意图：

```rust
pub struct ActionIntent {
    pub entity: Entity,
    pub action: ActionKind,
    pub priority: u32,
    pub av: f64,
}
```

- `ActionKind`：`Wait / Move / BasicAttack / Chase / Flee / Wander`。
- `priority`：由生成系统根据行为类型赋予。
- `av`：由反应时 + 耗时 × 敏捷修正计算。

### 3.3 行动生成系统

每个行为一个生成系统，查询 `Idle` 或 `Failure` 且拥有对应 `Can*` 组件的实体：

| 生成系统 | 触发组件 | 生成条件 | 生成行动 | 优先级 |
|---|---|---|---|---|
| `flee_action_generation_system` | `CanFlee` | `Health.ratio() < FLEE_HP_RATIO` | `Flee` | 200 |
| `chase_action_generation_system` | `CanChase` | 玩家可见，或 `LastKnownPlayerPos` 有值 | `Chase` | 100 |
| `wander_action_generation_system` | `CanWander` | 无条件 | `Wander` | 50 |
| `wait_action_generation_system` | `CanWait` | 无条件兜底 | `Wait` | 0 |
| `player_action_generation_system` | 输入请求 | 玩家输入已确认 | `Move / BasicAttack / Wait` | 不参与 AI 仲裁 |

设计理由：

- 每个系统只表达一种行为，新增行为 = 新增一个生成系统 + 一个 `Can*`。
- 条件在生成时检查一次，执行前仍会进行保活检查。
- 玩家行动由输入系统直接授权，不经过 AI 仲裁，否则玩家操作可能被怪物行为覆盖。

### 3.4 行动仲裁系统

`action_arbitration_system` 收集本轮所有 `ActionIntent`：

1. 按实体分组。
2. 组内取优先级最高者。
3. 同优先级时按 `entity.to_bits()` 升序打破平局。
4. 对胜出意图调用统一的 `mount_action(entity, ActionKind, av)`。
5. 清理本轮所有意图，避免残留到下一轮。

排序必须满足**全序契约**：比较器只能依赖 `priority` 和 `entity.to_bits()`，
不得混入随机数。旧架构曾在 `sort_by` 比较器中调用 RNG，导致标准库检测到非全序比较而 panic。

### 3.5 优先级表

| 行动 | 优先级 | 理由 |
|---|---:|---|
| `Flee` | 200 | 生存优先于攻击；低血量时覆盖追击和游荡 |
| `Chase` | 100 | 主动敌对行为高于漫无目的移动 |
| `Wander` | 50 | 默认探索行为 |
| `Wait` | 0 | 最终兜底，保证有能力的实体不会无行动 |

玩家行动不进入此表。玩家输入确认后直接替换或取消玩家当前行动。

逃跑采用滞回阈值：

- 进入逃跑：`Health.ratio() < 0.25`
- 退出逃跑：`Health.ratio() >= 0.30`

---

## 4. 行动执行

- `ActionTimer` 随 AV 推进；归零后进入执行。
- 执行前做保活检查：
  - 攻击目标必须存活且 8 方向相邻。
  - 追击必须仍可见或仍有最后已知位置。
  - 逃跑必须仍处于低血量滞回区间。
  - 移动必须目标格可行走、未被占用、对角不穿墙。
- 执行成功：清除行动组件，`Active -> Idle`。
- 保活失败：清除行动组件，`Active -> Failure`，等待下一轮生成。

---

## 5. 战斗与成长事件链

```text
execute_basic_attack_system
        ↓ 写 AttackIntentEvent { attacker, target }
resolve_attack_system
        ↓ 查 Attack/Defense/Crit + RNG
        ↓ 写 AttackEvent { attacker, target, damage, is_crit }
apply_damage_system
        ↓
record_be_attacked_system
        ↓
check_death_system
        ↓ 写 DeathEvent
apply_exp_system
        ↓ 写 LevelUpEvent
```

设计理由：

- 攻击系统只负责保活校验和意图，不计算伤害。
- 伤害结算独立消费意图事件，未来远程/技能/DoT 可复用。
- 伤害应用系统只消费已结算结果，不读取攻击者属性。
- 死亡、经验、日志各自独立消费事件。

---

## 6. 威胁系统接口

本轮只预留接口：

- `ThreatEvent { source, target, amount, reason }`
- `ThreatReason`：`Damage / Sight / Noise / Heal / Proximity`
- `ThreatTable`：`entity -> (target -> threat)`

未来完整设计：

- 威胁事件累积到 `ThreatTable`。
- 达到进入阈值后，目标进入敌对目标队列。
- 队列按威胁值降序；单目标攻击取队首，多目标攻击按队列顺序取前 N。
- 目标死亡、离开视野或超过遗忘时间后从队列清除。

---

## 6.5 世界初始化系统

首次世界初始化由 `core::init` 中的系统链完成：

```text
generate_map_system
  ↓
spawn_player_system
  ↓
spawn_stairs_system
  ↓
spawn_monsters_system
  ↓
fov_system
  ↓
update_map_memory_system
  ↓
update_visible_memory_system
  ↓
rebuild_occupancy_system
```

- `insert_core_resources` 直接插入 `MapSeed / FloorNumber / Map / GameRng / ...`，不经过延迟 Commands。
- `generate_map_system` 读 `(MapSeed, FloorNumber)`，调用 `generate_map_from_seed`，地图结果完全确定。
- `spawn_player_system` 使用玩家工厂生成基础组件束。
- `spawn_stairs_system` 写入 `StairsPos`，并保证出生点与楼梯连通。
- `spawn_monsters_system` 使用噪声密度 + 元胞扩散生成怪物，排除出生点与楼梯。
- 系统链通过 `.chain()` 强制顺序；未加顺序约束时，Bevy 可能在地图生成前运行怪物系统。

---

## 6.6 最小可运行闭环

不依赖物品、背包、掉落、装备的最小版本已经可运行：

```text
sys::spawn_key_source()
  → dungeon-app 将 KeyCode 翻译为 PlayerCommand
  → core::player_action_generation_system 挂载玩家行动
  → core::advance_until_player_acted 推进并执行
  → core::decide_monster_actions 为怪物挂载下一轮行动
  → core::run_settle_systems 结算死亡/经验/FOV/记忆/占用图
  → tui::render_game 渲染
```

当前可用内容：地图生成、玩家移动/攻击/等待、怪物追击/逃跑/游荡、
近战伤害/暴击、玩家死亡、经验升级、FOV 与探索记忆、事件日志、游戏结束。

---

## 7. 装备与物品方向

旧架构只允许玩家持有装备。新架构方向：

1. 地面物品：独立实体。
2. 背包堆叠物：数据组件，不实体化。
3. 装备：`Equipment` 数据组件推广到任意实体。
4. 未来具有独立行为/耐久/冷却的独特物品：升级为子实体。

本轮不迁移物品、背包、装备和 buff。

---

## 8. 迁移状态

本轮已迁入 `core`：

- 细粒度数值组件、范畴与身份标记
- 地图、Tile、地图生成、FOV、A*、LOS、碰撞占用图
- 行动状态机、AV 推进、行动挂载与执行
- 行动执行已拆分为与行动组件一一对应的系统
- 攻击已拆分为 `execute -> resolve -> apply`
- 移动、近战、等待、追击、逃跑、游荡
- 战斗公式、暴击、经验、升级、死亡事件
- 怪物模板与生成权重
- 威胁系统接口
- 首次世界初始化系统链：地图、玩家、楼梯、怪物、FOV/记忆/占用图
- 玩家输入生成系统 `player_action_generation_system`
- 最小世界循环 `world_loop::new_game / apply_player_command`
- core 开发者日志：行动、战斗、AI、初始化
- 新增 `utils`：颜色、几何、Grid、文本工具
- 新增 `sys`：终端、输入线程、文件读写、文件日志 + 可捕获日志
- 新增 `tui`：颜色转换、布局工具、Canvas、UI 状态、新 core 场景渲染、Debug 日志面板
- 根 crate 更名为 `dungeon-app`

本轮明确不迁入：

- 旧渲染管线、旧页面处理器
- 物品、背包、装备
- buff 与技能效果
- 掉落表
- 下楼（descend）、存档读档

各模块的稳定等级、测试基线与冻结清单见 **§10 新代码稳定程度**。

---

## 9. 设计理由摘要

- **抛弃 `Stats`**：聚合组件破坏 ECS 查询粒度，所有系统被迫读取整块数据。
- **抛弃 `ActionQueue`**：队列是查询结果，不是数据模型；新模型直接查询 `Active + ActionTimer`。
- **生成/仲裁分离**：生成可并行、只读；仲裁唯一写入；比较器保持确定性全序。
- **事件驱动战斗**：意图与结果分离，死亡、经验、日志、威胁都能独立演化。
- **组件式授权**：`Can*` 表达能力，新增行为不修改统一决策流程。
- **不恢复全局 World**：继续显式 `&World / &mut World` 参数传递，由借用检查器防止重入与死锁。


---

## 10. 新代码稳定程度（进入渲染/插件重构前的基线）

> 本节只评价**新代码**：`core`、`utils`、`sys`、`tui`、`render-api`、根 crate `dungeon-app`。
> 旧 `dungeon-*` 与 `src/pages` 不在承诺范围内（见 §8：历史参考）。
> 基线：refactor 分支，`render-api` v1 落地后。

### 10.1 稳定度等级

| 等级 | 含义 | 变更政策 |
|---|---|---|
| **S0 冻结** | 已定型，只允许 bug 修复 | 破坏性改动需要迁移方案 + DESIGN 记录 |
| **S1 稳定** | 语义/API 基本确定 | 以追加为主；破坏性改动需 DESIGN + 测试更新 |
| **S2 可用但会变** | 当前可用，但已知下半重构会改 API | 调用方需预期迁移；不要在其上做深度封装 |
| **S3 过渡/临时** | 只服务当前闭环，计划替换/删除 | 禁止在其上构建新功能 |
| **S4 未迁移/占位** | 类型/接口存在但无实现或明确未迁 | 不得依赖；UI 页面不得假装它可用 |

### 10.2 总览

| 范围 | 等级 | 稳定的是 | 会变的是 | 测试基线 |
|---|---|---|---|---|
| `render-api` | **S1（契约）** | `SceneFrame` / `VisualKey` / `UiView` / `InputEvent` 的只读契约方向；`CONTRACT_VERSION = 1`；34 个测试 | 首个消费者（presentation/tui）落地前字段可能调整；破坏性改动必须递增版本 | ✅ 34 |
| `core` 领域模型 | **S1（语义）/ S2（API）** | 细粒度组件、`Can*`、`Idle/Active/Failure`、生成/仲裁/执行、事件链、`(seed, floor)` 地图确定性 | 公共 API 目前 `pub use *` 全暴露；调度入口可能被插件包装；物品/技能等会追加 | ⚠️ 0 单测 |
| `core::world` 应用入口 | **S2** | `new_game` / `apply_player_command` / `request_quit` 的“命令驱动回合”语义 | 可能被 `CorePlugin` 包装；`build_init_schedule` / `build_core_schedule` 会改成注册式 Schedule | ❌ doctest 失败 |
| `utils` | **S1** | 无业务依赖的通用工具 | 基本不变；缺测试 | ⚠️ 0 |
| `sys` | **S2** | 终端/文件/日志的 OS 封装可用 | 输入 API 从 `Receiver<KeyCode>` 改为 `InputQueue`；终端生命周期可能移入 `TuiPlugin`；`log/std` 依赖需显式声明 | ❌ 独立编译失败 |
| `tui` | **S3** | 当前能渲染最小闭环 | `scene.rs` 提取移到 `presentation`；`render.rs` 改为消费 `SceneFrame`；`state.rs` UI 状态移到 `presentation`；`render_game` / `extract_scene` 会删除 | ⚠️ 0 |
| `dungeon-app`（根） | **S3** | 当前 main 循环可跑通 | 将被 `App` + `Plugin` + `ScheduleRunnerPlugin` 替换；`keymap.rs` / `throw.rs` 是旧应用层代码；旧集成测试已失效 | ❌ 测试目标编译失败 |
| 旧 `dungeon-*` / `src/pages` | **S4** | 仅历史参考 | 不再扩展；迁移完成后删除/归档 | 旧测试不属于新代码 |
| `terrain-forge` | **S1（外部）** | 地图生成引擎接口 | 上游变更可能影响地图确定性；由 `(seed, floor)` 锁定行为 | 新 core 无专项测试 |

### 10.3 `core` 细分

| 模块 | 等级 | 说明 |
|---|---|---|
| `components.rs` | S1 | `Position/Health/Magic/Level/Experience/Attack/Defense/MagicMastery/Agility/CritRate/CritDamage` 等细粒度组件；数值 `f64`。组件名/语义是下半重构的冻结项；新增字段必须追加并考虑 serde。 |
| `entity_cls.rs` | S1 | `EntityClass` / `CreatureKind` 范畴 + `Player/Monster/Stairs` 等 ZST marker。`Item/Buff` 等范畴是 S4 占位。 |
| `events.rs` | S1 | `AttackIntentEvent -> AttackEvent -> DeathEvent -> LevelUpEvent` 事件链稳定；`ThreatEvent` 是 S4 接口占位。 |
| `resources.rs` | S1 | `GameRng`（确定性 xorshift64* + steps，回放/存档语义）、`MapMemory`、`VisibleMemory`、`OccupancyMap`、`EventLog`、`TurnManager`、`PendingExp`、`MapSeed`、`FloorNumber`。`ThreatTable` 是 S4。 |
| `map/` | S1（含 serde 兼容约束） | `Tile` 以 u8 0..10 序列化，**只能末尾追加**；`MapKind` 变体只能末尾追加；`Map/Room` 新字段用 `#[serde(default)]`。`Tile::glyph()` 属于显示数据，后续会移到 TUI catalog，不要当领域契约。 |
| `spatial/` | S1 | FOV / LOS / A* 纯函数；行为稳定，但新 core 无专项测试。 |
| `action/` | S2 | 生成/仲裁/执行三段式与 `Idle/Active/Failure` 状态机稳定；`ActionKind` 目前只有 `Wait/Move/BasicAttack/Chase/Flee/Wander`，`PlayerCommand` 只有 `Move/Wait`，后续会追加。 |
| `combat/` | S1 | 近战伤害/暴击公式；新增技能/远程会复用，不改现有公式语义。 |
| `monster/` | S1（serde 兼容约束） | `MonsterKindId` 变体只能末尾追加；`MonsterTemplate` 含 glyph/color（显示数据，后续移出）；数值公式可调但需 GAME.md 记录。 |
| `system/` | S2 | 系统本身稳定；`build_core_schedule()` 每次新建 Schedule，插件化后会改成注册式；调用方应优先用 `run_settle_systems` 包装。 |
| `world/init.rs` | S2 | `run_initialization` 是应用入口；`build_init_schedule()` 同上，插件化后改注册式。 |
| `world/loop_.rs` | S2 | 应用层入口 `new_game / apply_player_command / request_quit`；语义稳定（回合制、命令驱动），API 可能被 `CorePlugin` 包装。 |
| `world/query.rs` | S1 | 只读查询辅助函数。 |
| `balance.rs` | S1 | 数值公式；具体值可能随 GAME.md 调整，公式结构稳定。 |
| 物品/背包/装备/buff/技能/掉落/下楼/存档 | **S4** | 明确未迁移；UI 页面（Inventory/Throw 等）不得假装可用。 |

### 10.4 测试与构建基线（本轮实测）

| 命令 | 结果 | 说明 |
|---|---|---|
| `cargo check --workspace --offline` | ✅ | lib/bin 目标通过 |
| `cargo test -p render-api --offline` | ✅ 34 | 30 单测 + 4 集成 |
| `cargo clippy -p render-api --all-targets -- -D warnings` | ✅ | 0 警告 |
| `cargo test -p utils --offline` | ⚠️ 0 | 编译通过，无测试 |
| `cargo test -p tui --offline` | ⚠️ 0 | 编译通过，无测试 |
| `cargo test -p core --offline` | ❌ | 单测 0；doctest 因 `core::convert::Infallible` 失败（crate 名 `core` 与标准库 `core` 冲突） |
| `cargo test -p sys --offline` | ❌ | `log::set_boxed_logger` 被 `log/std` feature 门禁；workspace 构建因 feature 合并偶然通过 |
| `cargo test -p dungeon-app --offline --no-run` | ❌ | 旧集成测试 `tests/throw_test.rs` 引用不存在的 `dungeon_tui`；`tests/scenario_test.rs` 针对旧架构 |
| `cargo clippy -p core --all-targets -- -D warnings` | ❌ | 既有 `too_many_arguments` / `type_complexity`（rustc 1.95 / clippy 1.95，与本轮改动无关） |
| 旧 crate 测试（dungeon-core/action/world/render） | ✅（旧架构） | 只覆盖旧实现，不能作为新 `core` 的回归保障 |

结论：**新代码里只有 `render-api` 有可观的测试覆盖；`core` 是“迁移完成但未被测试保护”的状态。** 进入下半前，`core` 至少要有冒烟级回归，否则 presentation/tui 一旦改到 core 边界，没有自动信号。

### 10.5 进入下半前的冻结清单（presentation/tui 期间）

冻结（只允许追加/修 bug，不允许重命名/重排/改语义）：

- `core` 组件/资源/事件的**名称与语义**：`Position/Health/Magic/Level/Experience/Attack/Defense/MagicMastery/Agility/CritRate/CritDamage`；`Player/Monster/Stairs` marker；`Idle/Active/Failure`、`Can*`、`ActionTimer`、`Wait/Move/BasicAttack/Chase/Flee/Wander`。
- `core` 资源：`Map/MapSeed/FloorNumber/GameRng/MapMemory/VisibleMemory/OccupancyMap/EventLog/TurnManager/PendingExp`。
- `core` 事件：`AttackIntentEvent/AttackEvent/DeathEvent/LevelUpEvent`。
- serde 兼容：`Tile` u8 0..10、`MonsterKindId` 变体、`MapKind` 变体只能末尾追加；新字段 `#[serde(default)]`。
- `core::world_loop::{new_game, apply_player_command, request_quit}` 的调用语义（名字可保留兼容包装）。
- `render-api` v1 契约：`SceneFrame` / `VisualKey` / `UiView` / `InputEvent` 的字段可以追加；破坏性改动递增 `CONTRACT_VERSION`。
- 架构边界：`core` 不依赖 `bevy_app` / ratatui / wgpu / `render-api` / `presentation`；`tui` / `gpu` 不依赖 `core`。

允许/预期变更（不要在新代码里深度依赖）：

- `tui` 的 `scene.rs` / `render.rs` / `state.rs` 会被拆解/删除；
- `sys` 的 `Receiver<KeyCode>` 输入 API 会被 `InputQueue` 取代；
- 根 `main.rs` 会被 `App` / `Plugin` / runner 替换；
- `core` 的 `build_init_schedule` / `build_core_schedule` 会改成注册式 Schedule；
- `core` 的 `pub use *` 公共面可能收紧；
- 旧 `dungeon-*` / `src/pages` 不再维护。

### 10.6 进入下半前建议处理（按优先级）

1. **修复 `core` doctest 失败**：`core::convert::Infallible` → `std::convert::Infallible`（或 `::core::convert::Infallible`）；确认 `cargo test -p core` 至少 doctest 通过。长期考虑把 crate 改名为 `game-core` / `domain`，避免与标准库 `core` 同名。
2. **给 `core` 加冒烟回归**：地图生成确定性（同 seed 同结果）、玩家移动/攻击/等待、怪物行动推进、死亡→经验→升级、FOV/记忆/占用图。哪怕只有 5–8 个测试，也能给下半重构提供安全网。
3. **修复 `sys` 独立构建**：给 `sys` 的 `log` 依赖显式加 `features = ["std"]`（或 workspace `log` 统一声明），确保 `cargo test -p sys` 不依赖 feature 合并偶然通过；补输入/日志测试。
4. **处理失效的根集成测试**：`tests/scenario_test.rs` / `tests/throw_test.rs` 针对旧 crate；要么删除/归档，要么重写为新 `core` + `render-api` 的 headless 测试。不要让 `cargo test --workspace` 长期失败。
5. **建立 CI/本地门禁**：至少 `cargo check --workspace` + `cargo test -p render-api -p core -p utils -p tui`（修复后）+ `cargo clippy -p render-api -- -D warnings`；旧 crate 测试单独标记，不计入新代码门禁。
6. **`render-api` 消费验证**：presentation/tui 接上后，补 `SceneFrame` golden 测试 + `TestBackend` 渲染快照；确认 `VisualKey` payload 映射与 core 实际枚举一致。

### 10.7 兼容性与确定性规则

- **无存档承诺**：新 `core` 尚未迁移存档；旧 `dungeon-world` 的存档格式不兼容新组件。在存档迁移完成前，不要对外承诺“新 core 可读旧档”。
- **serde 只追加**：已 derive serde 的类型（`Tile` / `MapKind` / `MonsterKindId` / `Map` / `Room` 等）不得重排变体、不得复用旧判别值；新增字段用 `#[serde(default)]`。
- **确定性随机**：`GameRng` 的算法与 `steps` 语义是回放/存档基础；改变算法必须视为存档格式变更。
- **地图确定性**：`generate_map_from_seed` 对 `(MapSeed, FloorNumber)` 必须确定；修改地图生成算法会改变同 seed 地图，需要 DESIGN 记录 + golden seed 测试。
- **显示数据不属于领域**：`Tile::glyph`、`MonsterTemplate::glyph/color` 等是渲染数据，后续迁移到 `presentation` / `tui` catalog；不要把它们当 core 稳定契约。
