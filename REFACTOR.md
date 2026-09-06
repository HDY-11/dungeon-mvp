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

---

## 9. 设计理由摘要

- **抛弃 `Stats`**：聚合组件破坏 ECS 查询粒度，所有系统被迫读取整块数据。
- **抛弃 `ActionQueue`**：队列是查询结果，不是数据模型；新模型直接查询 `Active + ActionTimer`。
- **生成/仲裁分离**：生成可并行、只读；仲裁唯一写入；比较器保持确定性全序。
- **事件驱动战斗**：意图与结果分离，死亡、经验、日志、威胁都能独立演化。
- **组件式授权**：`Can*` 表达能力，新增行为不修改统一决策流程。
- **不恢复全局 World**：继续显式 `&World / &mut World` 参数传递，由借用检查器防止重入与死锁。
