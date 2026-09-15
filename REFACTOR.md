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

> **待清理（见 §10.6）：** 当前 `CreatureKind` 没有任何读取方；`EntityClass` 唯一的读取是 `rebuild_occupancy_system` 中对尚未迁移的 `Item` 的判断。是否保留这两个范畴、还是改为 ZST 标记 + 专用 query，待定。

### 2.2 实体身份 ZST

`Player / Monster / Stairs / Rat / Scorpion / Goblin / ...` 是零大小标记组件。

设计理由：

- 身份回答“这个实体是谁”，用于查询过滤和决定挂载哪些组件。
- 身份不承载数值，避免再次形成 `Stats` 式聚合。
- 身份与逻辑解耦：系统查询 `With<Player>`，而不是 `match kind { Player => ... }`。

> **待清理（见 §10.6）：** 当前 `Rat/Scorpion/...` 身份 ZST 由 `world/init.rs` 里的 `MonsterKindId -> ZST` match 插入，但没有任何系统读取它们；同时实体上还挂着 `MonsterKindId` 组件，属于重复身份表示。保留哪一种表示待定。

### 2.3 数值组件

`Position / Health / Magic / Level / Experience / Attack / Defense / MagicMastery / Agility / CritRate / CritDamage` 均为细粒度组件，数值统一使用 `f64`。

设计理由：

- 细粒度组件让查询只读取需要的数据，是 ECS 缓存友好的前提。
- `f64` 为未来更精确的数值规划留出空间；旧的 i32/u32 数值不作为行为基准。

> **待迁移（见 §2.6）：** `Agility` 计划由 `MoveSpeed` / `AttackSpeed` 两个速度组件取代。

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

> **更新（§3.6）：** 行动组件与 `ActionTimer` 不再挂在 actor 上，而是挂在 action 子实体上；actor 只保留 `Idle/Active/Failure` 状态（或后续的 `ActionState`）。

### 2.6 速度组件（替代 `Agility`，目标设计）

**决策：** 删除 `Agility`；速度由 `MoveSpeed(f64)` 和 `AttackSpeed(f64)` 两个组件表达。两者都是倍率，`1.0` 为基准，越高越快。

- `AV = base_duration / speed.clamp(MIN_SPEED, MAX_SPEED)`。
- 行动类别映射：
  - `Move / Chase / Flee / Wander` → `MoveSpeed`；
  - `BasicAttack` → `AttackSpeed`；
  - `Wait` → 固定 `WAIT_DURATION`（或后续 `WaitSpeed`，待定）。
- 删除 `agility_to_reaction` / `agility_speed_factor` / `action_av(duration, agility)`；是否保留统一的常数 `BASE_REACTION` 待定。
- 玩家初始：`MoveSpeed(1.0)` / `AttackSpeed(1.0)`；怪物按旧敏捷映射出初值（例如洞穴鱼偏移动速度、蘑菇傀儡偏攻击速度），再按 GAME.md `[⃞试调]` 重新校准。
- **前提：** 先修好 AV 门禁（§3.6.7）。当前实现里 `ActionTimer` 从未参与执行判断，换任何速度公式都不会改变实际行为。
- 文档同步：GAME.md 的“反应时/耗时修正”章节、玩家/怪物敏捷表、武器速度章节；DESIGN.md 需要新增决策条目；ISSUES.md 在动代码前记录逻辑改动。

---

## 3. 行动系统

> **状态（当前）：** 3.2–3.5 是最早的 `ActionIntent + ActionKind` 方案；它已被 **§3.6 行动实体方案**取代。下方内容保留为演化背景，实现以 §3.6 为准。

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

### 3.6 行动实体方案（当前采用，替代 `ActionKind`）

> **状态：** 目标设计；尚未落地。实现顺序见 §10.6。

#### 3.6.1 核心决策

去掉 `ActionKind` 这个中央分派 enum；**一个行动 = 一个 action 实体**（actor 的子实体）。
行动类型由 action 实体上挂载的具体 ZST/payload 组件表达：

- `Wait` / `Flee` / `Chase` / `Wander`：ZST 组件。
- `Move { dx, dy }` / `BasicAttack { target }`：带 payload 的组件。
- 不再有 `ActionKind`，也不再需要 `mount_action` 的中央 match。

`Can*` **不子实体化**，继续作为 actor 上的 ZST 组件；它们回答“能不能做”，action 实体回答“正在考虑/执行什么”。

#### 3.6.2 数据模型

Actor 实体：

```text
Monster / Player
Position, Health, MoveSpeed, AttackSpeed, ...
CanMove, CanWait, CanBasicAttack, CanChase, CanFlee, CanWander
Idle / Active / Failure        ← 行动状态（或单一 ActionState 枚举，待定）
```

Action 实体（actor 的子实体，瞬态）：

```text
ChildOf(actor)                 ← 归属；Bevy 0.16 关系组件
ActionPriority(u32)            ← 仲裁排序
ActionTimer { remaining_av }   ← AV；由生成系统按行动类别与速度计算
ActionSource { Ai | Player }   ← 可选：调试/仲裁策略
Candidate / ActiveAction / Ready  ← 生命周期标记
Move { dx, dy } / BasicAttack { target } / Flee / Chase / Wander / Wait
```

- action 实体是“意图”和“正在执行的行动”的统一表示：
  - `Candidate`：生成系统产出、等待仲裁；
  - `ActiveAction`：仲裁选中、等待/正在执行；
  - `Ready`：`ActionTimer` 归零、允许执行。
- 一个 actor 同一时间最多一个 `ActiveAction` 子实体。

#### 3.6.3 系统流程

```text
Generation（每个行为一个系统，只 spawn 候选，不改 actor 状态）
    ↓ Commands / ApplyDeferred
Arbitration（唯一写入 actor 行动状态的系统）
    ↓
Tick（推进所有 ActiveAction 的 ActionTimer，归零加 Ready）
    ↓
Execution（每个行动一个专用 query 系统；执行 + 发 ActionSucceeded/FailedEvent）
    ↓
Completion（消费事件：despawn action 实体，actor 回 Idle/Failure）
    ↓
Events::update（每轮一次；或由 bevy_app 的 event_update_system 负责）
```

循环直到玩家行动结束；速度快、AV 先归零的 actor 可以在一次玩家行动期间执行多次。

#### 3.6.4 生成系统

每个行为一个生成系统，只读 actor 状态，通过 `Commands` spawn 候选 action 实体：

```rust
fn flee_generation_system(
    mut commands: Commands,
    actors: Query<
        (Entity, &Health, &MoveSpeed),
        (With<Monster>, With<CanFlee>, Without<Active>, Or<(With<Idle>, With<Failure>)>),
    >,
) {
    for (actor, health, speed) in &actors {
        if health.ratio() < FLEE_HP_RATIO {
            commands.spawn((
                ChildOf(actor),
                ActionPriority(200),
                ActionTimer { remaining_av: action_av(FLEE_DURATION, speed.0) },
                ActionSource::Ai,
                Flee,
            ));
        }
    }
}
```

- 生成系统之间相互独立，不需要知道其他行为；
- 新增行为 = 新增一个生成系统 + `Can*`，不需要改仲裁/执行/已有生成系统；
- 生成系统只 spawn 候选，不修改 actor 的 `Active/Idle/Failure`；仲裁才是唯一写入方；
- 玩家路径：`player_action_generation_system` 直接 spawn 一个带 `ActiveAction` 的 action 实体（或带保留优先级），AI 生成系统用 `With<Monster>` 过滤，不会覆盖玩家。

#### 3.6.5 仲裁系统

```rust
fn action_arbitration_system(
    mut commands: Commands,
    candidates: Query<(Entity, &ChildOf, &ActionPriority), (With<Candidate>, Without<ActiveAction>)>,
) {
    // 1. 按 ChildOf.parent() 分组
    // 2. 每组取 (ActionPriority, entity.to_bits()) 最大者
    // 3. winner: remove Candidate, insert ActiveAction；actor: insert Active
    // 4. loser: despawn
}
```

- 仲裁是 actor 行动状态的唯一写入方；
- 比较器只依赖 `ActionPriority` 和 `action_entity.to_bits()`，保持全序、无 RNG；
- 仲裁结束必须清空所有候选：winner 转 `ActiveAction`，loser despawn；
- actor 已有 `Active`/`ActiveAction` 时跳过；
- 实际实现时，候选查询的 `Without<ActiveAction>` 只能保证候选自身未激活；仲裁前还需过滤掉 actor 已有 `ActiveAction` 子实体（或 actor 上有 `Active`）的情况，避免同一 actor 同时存在多个 active action。

#### 3.6.6 执行与完成

- `tick_action_timers_system`：查询 `With<ActiveAction>` 的 `ActionTimer`，同步推进；归零加 `Ready`。
- 执行系统按行动类型专用 query，执行层零 match：

```rust
fn execute_move_system(
    mut commands: Commands,
    actions: Query<(Entity, &ChildOf, &Move), (With<ActiveAction>, With<Ready>)>,
    // ...
) { /* ... */ }
```

- 执行成功/保活失败：发 `ActionSucceededEvent { actor, action }` / `ActionFailedEvent { actor, action }`；
- `action_completion_system` 消费事件：despawn action 实体，移除 actor 的 `Active`，插入 `Idle`/`Failure`；
- 这样 `ActionSucceededEvent` / `ActionFailedEvent` 从“死事件”变成真实的状态回转机制。

#### 3.6.7 与 AV 修复的关系

> **进展：** AV 门禁已在当前 actor-component 模型落地（I89：`Ready` + 每轮生成/结算）；action 实体方案沿用同一语义。

当前实现里 `ActionTimer` 从未被用于执行门禁，所有 `Active` 行动每轮都会执行。行动实体方案必须同时修复：

- 只有 `Ready` 的 action 才执行；
- 外层循环每轮都运行 Generation/Arbitration/Tick/Execution，而不是等玩家行动结束后才生成一次；
- 这样 `MoveSpeed` / `AttackSpeed` 才真正决定“谁先动、谁动得多”。

#### 3.6.8 事件与生命周期

> **进展：** 事件生命周期已修（I90：持久 Schedule + `update_events_system` 每轮 `Events::update()`）；消费者接线仍待做。

- `ActionSucceededEvent` / `ActionFailedEvent`：由 completion 系统消费；
- `AttackIntentEvent` / `AttackEvent`：保留为战斗扩展点（技能/投射物），但必须有真实消费者；
- `DeathEvent` / `LevelUpEvent`：要么由经验/掉落/UI 消费，要么删除；
- `ThreatEvent` / `ThreatTable`：S4 占位，未接线前不要假装可用；
- **所有事件每轮必须 `Events::update()`**；引入 `bevy_app` 后由其 `event_update_system` 负责。

#### 3.6.9 代价与缓解

- **实体 churn**：每轮每个 actor 可能 spawn/despawn 多个候选。缓解：
  - 只为 `Idle`/`Failure` 且 `Without<Active>` 的 actor 生成；
  - 仲裁立即 despawn loser；
  - 未来可改为每个 actor 一个持久 `ActionSlot`，或每个行为一个持久行为实体（池化），但当前规模不需要。
- **层级查询**：仲裁需要 `ChildOf` 分组；可接受。
- **清理**：Bevy 0.16 的 `Children` 是 `linked_spawn`，父实体 despawn 会联动 despawn 子实体；action 完成时的主动 despawn 仍由 completion 系统负责。
- **调试**：action 实体是瞬态的，建议加 `ActionName`/`ActionSource` 便于日志。

#### 3.6.10 开放项

- `Idle/Active/Failure` 三 ZST vs 单一 `ActionState` enum：待定；行动实体方案下 actor 状态仍可用 ZST。
- action 实体是否需要额外的 `ActionOwner(Entity)` 组件，还是只用 `ChildOf`：待定。
- 候选 action 实体是否要持久化/池化：先不做，按性能数据决定。
- `ActionSource` / `ActionName` 的具体字段：实现时定。

---

## 4. 行动执行

> **实现以 §3.6 为准。** 行动组件不再挂在 actor 上，而是挂在 action 子实体上；`ActionTimer` 也随 action 实体移动。

- `ActionTimer` 随 AV 推进；归零后给 action 实体加 `Ready`，只有 `Ready` 才执行。
- 执行前做保活检查：
  - 攻击目标必须存活且 8 方向相邻。
  - 追击必须仍可见或仍有最后已知位置。
  - 逃跑必须仍处于低血量滞回区间。
  - 移动必须目标格可行走、未被占用、对角不穿墙。
- 执行成功/失败：发 `ActionSucceededEvent` / `ActionFailedEvent`，由 completion 系统统一 despawn action 实体并回转 actor 的 `Idle` / `Failure`。

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

> **事件生命周期修正（必须）：** 当前 `run_settle_systems` 每次重建 Schedule，且从未调用 `Events::update()`，导致 `EventReader` 每轮重读历史事件（旧攻击/伤害被反复结算）。引入 action 实体后，事件必须走**持久 Schedule + 每轮 `Events::update()`**（或由 `bevy_app` 的 `event_update_system` 负责）；每个保留的事件必须有真实消费者（见 §3.6.8）。

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

> **目标流程（§3.6）：** 当前最小闭环仍是 `decide_monster_actions` + `mount_action`；action 实体方案落地后改为：
> generation（spawn 候选 action 实体）→ arbitration（选一个）→ tick（Ready）→ execution（专用 query）→ completion（事件回转）→ `Events::update`。
> 下方旧流程保留为当前实现说明。

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

### 8.1 行动系统目标重构（未落地）

> **进展：** AV 门禁（I89）与事件生命周期（I90）已落地；`ActionKind` 删除 / action 实体 PoC 仍待做。

以 §3.6 行动实体方案为准，下一步需要：

- 删除 `ActionKind` 与 `mount_action` 的中央 match；
- 行动候选/执行改为 action 子实体（`ChildOf` + `ActionPriority` + `ActionTimer` + ZST/payload + `Candidate/ActiveAction/Ready`）；
- 生成系统只 spawn 候选；仲裁系统唯一写入 actor 行动状态；执行系统专用 query；completion 系统消费 `ActionSucceeded/FailedEvent`；
- ~~修复 AV 门禁：只有 `Ready` 的 action 执行，且每轮都运行 generation/arbitration/tick/execution~~ ✅（I89）；
- ~~修复事件生命周期：持久 Schedule + 每轮 `Events::update()`~~ ✅（I90）；
- 清理同类死抽象：未读取的 `Rat/Scorpion/...` 身份 ZST、`EntityClass`/`CreatureKind`、无消费者事件、`BeAttacked`、`PendingExp`、死 combat 函数等（见 §10.6）；
- `Can*` 保持 actor 上的 ZST 组件，不子实体化。

各模块的稳定等级、测试基线与冻结清单见 **§10 新代码稳定程度**。

---

## 9. 设计理由摘要

- **抛弃 `Stats`**：聚合组件破坏 ECS 查询粒度，所有系统被迫读取整块数据。
- **抛弃 `ActionQueue`**：队列是查询结果，不是数据模型；新模型直接查询 `Active + ActionTimer`。
- **生成/仲裁分离**：生成只产出候选；仲裁唯一写入 actor 行动状态；比较器保持确定性全序。
- **行动即实体（§3.6）**：行动候选/执行用 action 子实体表达，不再需要 `ActionKind` 中央 enum；行动类型由 ZST/payload 组件表达，执行层专用 query 零 match。
- **能力保持组件（§3.6）**：`Can*` 是纯能力标记，继续做 actor 上的 ZST 组件，不子实体化。
- **事件驱动 + 生命周期**：意图与结果分离；每个事件必须有真实消费者，且每轮 `Events::update()`（或由 `bevy_app` 负责）。
- **组件式授权**：`Can*` 表达能力，新增行为不修改统一决策流程。
- **不恢复全局 World**：继续显式 `&World / &mut World` 参数传递，但玩法系统改用 `Query` / `Commands` / 持久 Schedule，不再用 exclusive `&mut World` 代替系统。


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
| `core` 领域模型 | **S1（语义）/ S2（API）** | 细粒度组件、`Can*`、`Idle/Active/Failure`、生成/仲裁/执行、事件链、`(seed, floor)` 地图确定性 | `Agility` 将被 `MoveSpeed/AttackSpeed` 取代；`ActionKind` 计划删除（见 §3.6）；公共 API 目前 `pub use *` 全暴露；物品/技能等会追加 | ✅ 4 |
| `core::world` 应用入口 | **S2** | `new_game` / `apply_player_command` / `request_quit` 的“命令驱动回合”语义 | 可能被 `CorePlugin` 包装；`build_init_schedule` / `build_core_schedule` 已改持久 Schedule；事件生命周期已修（I90） | ✅ 4 |
| 行动实体设计（§3.6） | **S3（目标设计，未落地）** | action 子实体、`ActionPriority`、`ActionTimer`、`Ready`、completion 的语义 | 命名/字段/池化策略在 PoC 后可能调整；`ActionKind` 将被删除 | ⚠️ 0 |
| `utils` | **S1** | 无业务依赖的通用工具 | 基本不变；缺测试 | ⚠️ 0 |
| `sys` | **S2** | 终端/文件/日志的 OS 封装可用 | 输入 API 从 `Receiver<KeyCode>` 改为 `InputQueue`；终端生命周期可能移入 `TuiPlugin`；`log/std` 依赖需显式声明 | ❌ 独立编译失败 |
| `tui` | **S3** | 当前能渲染最小闭环 | `scene.rs` 提取移到 `presentation`；`render.rs` 改为消费 `SceneFrame`；`state.rs` UI 状态移到 `presentation`；`render_game` / `extract_scene` 会删除 | ⚠️ 0 |
| `dungeon-app`（根） | **S3** | 当前 main 循环可跑通 | 将被 `App` + `Plugin` + `ScheduleRunnerPlugin` 替换；`keymap.rs` / `throw.rs` 是旧应用层代码；旧集成测试已失效 | ❌ 测试目标编译失败 |
| 旧 `dungeon-*` / `src/pages` | **S4** | 仅历史参考 | 不再扩展；迁移完成后删除/归档 | 旧测试不属于新代码 |
| `terrain-forge` | **S1（外部）** | 地图生成引擎接口 | 上游变更可能影响地图确定性；由 `(seed, floor)` 锁定行为 | 新 core 无专项测试 |

### 10.3 `core` 细分

| 模块 | 等级 | 说明 |
|---|---|---|
| `components.rs` | S1 | `Position/Health/Magic/Level/Experience/Attack/Defense/MagicMastery/CritRate/CritDamage` 等细粒度组件；`Agility` 计划由 `MoveSpeed`/`AttackSpeed` 取代（§2.6）。数值 `f64`；新增字段必须追加并考虑 serde。 |
| `entity_cls.rs` | S1（待清理） | `Player/Monster/Stairs` 等查询用 ZST 稳定；`EntityClass` / `CreatureKind` 目前无实际读取方（§2.1），`Item/Buff` 是 S4 占位；身份 ZST `Rat/...` 与 `MonsterKindId` 重复（§2.2），待定去留。 |
| `events.rs` | S2 | 事件生命周期已修（I90，每轮 `Events::update()`）；`DeathEvent`/`LevelUpEvent` 仍无消费者、`ActionSucceeded/Failed` 未接线，按 §3.6.8 补齐后再冻结。 |
| `resources.rs` | S1 | `GameRng`（确定性 xorshift64* + steps，回放/存档语义）、`MapMemory`、`VisibleMemory`、`OccupancyMap`、`EventLog`、`TurnManager`、`MapSeed`、`FloorNumber`。`PendingExp` 可能在 `DeathEvent` 接上消费者后删除；`ThreatTable` 是 S4。 |
| `map/` | S1（含 serde 兼容约束） | `Tile` 以 u8 0..10 序列化，**只能末尾追加**；`MapKind` 变体只能末尾追加；`Map/Room` 新字段用 `#[serde(default)]`。`Tile::glyph()` 属于显示数据，后续会移到 TUI catalog，不要当领域契约。 |
| `spatial/` | S1 | FOV / LOS / A* 纯函数；行为稳定，但新 core 无专项测试。 |
| `action/` | S2（重构中） | 生成/仲裁/执行三段式与 `Idle/Active/Failure` 状态机方向稳定；AV 门禁已修（I89，`Ready`）；目标设计见 §3.6：删除 `ActionKind`，行动改为 action 子实体；`Can*` 保留 actor 组件。 |
| `combat/` | S1 | 近战伤害/暴击公式稳定；`prepare_attack_event` / `resolve_melee` / `damage_entity` 是死代码/重复路径（§10.6），建议删除。 |
| `monster/` | S1（serde 兼容约束） | `MonsterKindId` 变体只能末尾追加；`MonsterTemplate` 含 glyph/color（显示数据，后续移出）；数值公式可调但需 GAME.md 记录；身份 ZST 与 `MonsterKindId` 的重复见 §2.2 / §10.6。 |
| `system/` | S2 | 系统本身稳定；`build_core_schedule()` 已改为持久 Schedule（I90），插件化后由 App 管理；调用方继续用 `run_settle_systems` 包装。 |
| `world/init.rs` | S2 | `run_initialization` 是应用入口；`build_init_schedule()` 已注册为持久 Schedule（I90）。 |
| `world/loop_.rs` | S2 | 应用层入口 `new_game / apply_player_command / request_quit`；语义稳定（回合制、命令驱动），API 可能被 `CorePlugin` 包装。 |
| `world/query.rs` | S1（待清理） | 只读查询辅助函数；`update_visible_memory` / `rebuild_occupancy` 与 `system/` 重复且无调用方（§10.6），建议删除。 |
| `balance.rs` | S1 | 数值公式；具体值可能随 GAME.md 调整；`agility_to_reaction` / `agility_speed_factor` 计划随 §2.6 删除。 |
| 物品/背包/装备/buff/技能/掉落/下楼/存档 | **S4** | 明确未迁移；UI 页面（Inventory/Throw 等）不得假装可用。 |

### 10.4 测试与构建基线（本轮实测）

| 命令 | 结果 | 说明 |
|---|---|---|
| `cargo check --workspace --offline` | ✅ | lib/bin 目标通过 |
| `cargo test -p render-api --offline` | ✅ 34 | 30 单测 + 4 集成 |
| `cargo clippy -p render-api --all-targets -- -D warnings` | ✅ | 0 警告 |
| `cargo test -p utils --offline` | ⚠️ 0 | 编译通过，无测试 |
| `cargo test -p tui --offline` | ⚠️ 0 | 编译通过，无测试 |
| `cargo test -p core --offline` | ✅ 4 | AV 门禁 2 + 事件生命周期 1 + 闭环 1；doctest 已修（I86） |
| `cargo test -p sys --offline` | ❌ | `log::set_boxed_logger` 被 `log/std` feature 门禁；workspace 构建因 feature 合并偶然通过 |
| `cargo test -p dungeon-app --offline --no-run` | ❌ | 旧集成测试 `tests/throw_test.rs` 引用不存在的 `dungeon_tui`；`tests/scenario_test.rs` 针对旧架构 |
| `cargo clippy -p core --all-targets -- -D warnings` | ❌ | 既有 `too_many_arguments` / `type_complexity`（rustc 1.95 / clippy 1.95，与本轮改动无关） |
| 旧 crate 测试（dungeon-core/action/world/render） | ✅（旧架构） | 只覆盖旧实现，不能作为新 `core` 的回归保障 |

结论：**`render-api` 有可观测试覆盖；`core` 已补上 AV 门禁 / 事件生命周期 4 个回归（I89/I90），但地图确定性、战斗、AI 等仍缺冒烟测试。** 进入下半前，至少把 §10.6 第 2 项补齐。

### 10.5 进入下半前的冻结清单（presentation/tui 期间）

冻结（只允许追加/修 bug，不允许重命名/重排/改语义）：

- `core` 组件/资源/事件的**名称与语义**：`Position/Health/Magic/Level/Experience/Attack/Defense/MagicMastery/CritRate/CritDamage`；`Player/Monster/Stairs` marker；`Idle/Active/Failure`、`Can*`。`Agility` 计划删除；`ActionKind` 计划删除（§3.6）；行动 ZST 名称（`Wait/Move/BasicAttack/Chase/Flee/Wander`）冻结，但会从 actor 移到 action 子实体；`ActionTimer` 随之移动。
- `core` 资源：`Map/MapSeed/FloorNumber/GameRng/MapMemory/VisibleMemory/OccupancyMap/EventLog/TurnManager/PendingExp`。
- `core` 事件：`AttackIntentEvent/AttackEvent/DeathEvent/LevelUpEvent`（消费者/生命周期按 §3.6.8 补齐）。
- 行动实体新组件（`ActionPriority` / `ActionTimer` / `Candidate` / `ActiveAction` / `Ready` / `ChildOf` 归属）在 PoC 通过后再冻结；PoC 期间允许调整。
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

1. **修复 `core` doctest 失败**：✅ 已修（I86）。`std::convert::Infallible` + `ScheduleLabel` 手写 impl；`cargo test -p core` 通过（4 单测 + doctest）。长期仍建议评估 crate 改名（`game-core` / `domain`），避免与标准库 `core` 同名。
2. **给 `core` 加冒烟回归**：进行中 — 已补 AV 门禁（快怪多动/慢怪等待）与事件只结算一次（I89/I90，共 4 个测试）；地图生成确定性、玩家移动/攻击、死亡→经验→升级、FOV/记忆/占用图仍待补。
3. **修复 `sys` 独立构建**：给 `sys` 的 `log` 依赖显式加 `features = ["std"]`（或 workspace `log` 统一声明），确保 `cargo test -p sys` 不依赖 feature 合并偶然通过；补输入/日志测试。
4. **处理失效的根集成测试**：`tests/scenario_test.rs` / `tests/throw_test.rs` 针对旧 crate；要么删除/归档，要么重写为新 `core` + `render-api` 的 headless 测试。不要让 `cargo test --workspace` 长期失败。
5. **建立 CI/本地门禁**：至少 `cargo check --workspace` + `cargo test -p render-api -p core -p utils -p tui`（修复后）+ `cargo clippy -p render-api -- -D warnings`；旧 crate 测试单独标记，不计入新代码门禁。
6. **`render-api` 消费验证**：presentation/tui 接上后，补 `SceneFrame` golden 测试 + `TestBackend` 渲染快照；确认 `VisualKey` payload 映射与 core 实际枚举一致。
7. **行动实体 PoC（§3.6）**：先用一个 actor + `Wander` + `Move` 验证 generation → arbitration → tick(Ready) → execution → completion 全链路；通过后再迁移 Flee/Chase/Wait/BasicAttack，最后删除 `ActionKind` / `mount_action` 中央 match / `choose_action`。
8. **速度组件迁移（§2.6）**：在 AV 门禁修复后，把 `Agility` 换成 `MoveSpeed`/`AttackSpeed`，同步 GAME.md/DESIGN.md/ISSUES.md。

### 10.7 兼容性与确定性规则

- **无存档承诺**：新 `core` 尚未迁移存档；旧 `dungeon-world` 的存档格式不兼容新组件。在存档迁移完成前，不要对外承诺“新 core 可读旧档”。
- **serde 只追加**：已 derive serde 的类型（`Tile` / `MapKind` / `MonsterKindId` / `Map` / `Room` 等）不得重排变体、不得复用旧判别值；新增字段用 `#[serde(default)]`。
- **确定性随机**：`GameRng` 的算法与 `steps` 语义是回放/存档基础；改变算法必须视为存档格式变更。
- **地图确定性**：`generate_map_from_seed` 对 `(MapSeed, FloorNumber)` 必须确定；修改地图生成算法会改变同 seed 地图，需要 DESIGN 记录 + golden seed 测试。
- **显示数据不属于领域**：`Tile::glyph`、`MonsterTemplate::glyph/color` 等是渲染数据，后续迁移到 `presentation` / `tui` catalog；不要把它们当 core 稳定契约。
- **行动实体不存档**：action 实体是瞬态子实体；存档只存 actor 状态/速度/身份，读档后重新生成行动。action 实体的组件名不要作为存档格式的一部分。

### 10.8 同类死抽象/重复表示清理清单（报告，待判断）

与 `ActionKind` 同类的问题：**中央 token / 提前抽象 / 重复表示**。以下为清查结果，去留由用户判断：

| 项 | 证据 | 判断/建议 |
|---|---|---|
| `ActionKind` | `action/mod.rs`；`mount_action` 中央 match；`ai.rs` / `player.rs` / `world/loop_.rs` 引用 | 按 §3.6 删除 |
| 身份 ZST `Rat/Scorpion/...` | `entity_cls.rs` 定义；`world/init.rs:363-386` 由 `MonsterKindId` match 后插入；无读取方 | 与 `MonsterKindId` 重复；建议保留 `MonsterKindId` 作为数据键，删 ZST，等需要 `With<Rat>` 时再加 |
| `EntityClass` | `system/mod.rs` 唯一读取是未迁移的 `Item` 判断 | 占位；建议改为 ZST 标记或删除 |
| `CreatureKind` | `monster/mod.rs` 模板字段 + spawn 插入；无读取方 | 占位；等有公式分支时再引入 |
| `PlayerCommand` | `src/main.rs` 输入映射；`player.rs` 映射到 `ActionKind` | 保留为输入边界类型；去掉 `ActionKind` 后直接映射到 action 实体/Bundle |
| `AttackIntentEvent` | 唯一生产者 `execute_basic_attack_system`，唯一消费者 `resolve_attack_system` | 可合并进 `attack_system`；若保留作为技能/投射物扩展点，必须有未来消费者 |
| `ActionSucceededEvent` / `ActionFailedEvent` | 注册但无人发/读 | 按 §3.6.6 接 completion 系统，变成真实状态回转事件 |
| `DeathEvent` / `LevelUpEvent` | 写入但无消费者 | 接经验/掉落/UI 消费者，或删除 |
| `ThreatEvent` / `ThreatReason` / `ThreatTable` | 注册/定义但无发送/读取 | S4 占位；未接线前明确标注或删除 |
| `BeAttacked` / `NeedRecordBeAttacked` | `record_be_attacked_system` 写入，无读取 | 接威胁/AI 或删除 |
| `PendingExp` | 死亡系统写、经验系统读的旁路 | `DeathEvent` 接消费者后删除 |
| `MeleeResult` + `prepare_attack_event` / `resolve_melee` / `damage_entity` | `combat/mod.rs`；后两者无调用方 | 死代码/重复路径；删除，保留纯函数 |
| `MonsterStats` | 仅 `monster_base_bundle` 使用的中间 DTO | 可选：折成 `MonsterTemplate::spawn_bundle(floor)`；保留也可 |
| `WorldInitConfig` | 单字段 `map_seed` | 可内联为 `u64`；低优先级 |
| `Idle/Active/Failure` | 三个 ZST 表示一个状态机 | 查询友好，但互斥靠手动维护；可考虑单一 `ActionState`，或保留 ZST + debug 断言 |
| `Can*` | 六个能力 ZST | 保留；这是 ECS 原生能力表达，不要子实体化 |
| `MonsterKindId` | 数据键 + 多处 match 表 | 保留 enum；可将 match 表改为 `&'static [MonsterTemplate]` 索引，减少样板 |
| `Tile` / `MapKind` / `RoomShape` | 领域/算法数据 enum | 保留；不是行为分派 token |
| `EventLevel` vs `render-api::LogLevel` | 跨层重复 | 故意分层；保留，映射集中一处 |


---

## 11. 实施计划（第 2 步起）

> **状态：** 计划稿，执行前先确认 §11.6 的开放决策。
> **基线：** I89（AV 门禁）/ I90（事件生命周期）/ I86（core doctest）已修；`cargo test -p core` 4 个测试通过。
> **开放：** A41（exclusive `&mut World`）、A42（ActionKind → action 实体）、A43（死抽象清理）、G35（Agility → 速度组件）、I87（sys `log/std`）、I88（根集成测试）。

### 11.1 目标与范围

**目标：** 在保持当前最小闭环可玩的前提下，依次完成：

1. 可靠的 core 冒烟测试基线（Phase A）；
2. action 实体方案 PoC（Phase B）；
3. 全部行动迁移到 action 实体（Phase C）；
4. `Agility` → `MoveSpeed` / `AttackSpeed`（Phase D）；
5. 同类死抽象/重复表示清理（Phase E）；
6. 构建/测试门禁修复（Phase F）；
7. 回到 `presentation` + `tui` 解耦（Phase G）。

**不做：** 物品/背包/装备、buff/技能、掉落表、下楼、存档读档；这些仍按 §8 保持 S4。

### 11.2 阶段总览

| 阶段 | 内容 | 依赖 | 主要产物 | 验收 |
|---|---|---|---|---|
| **A** | core 冒烟测试 | I89/I90 已修 | 地图确定性、移动/攻击、死亡/经验、FOV/记忆/占用图测试 | `cargo test -p core` ≥ 10 通过 |
| **B** | action 实体 PoC | A | actor + Wander + Move 全链路测试模块 | PoC 测试通过；不接主循环 |
| **C** | 全量行动迁移 | B | 生成/仲裁/Tick/执行/完成系统；删除 `ActionKind` | 行为 parity 测试通过；无 `ActionKind` 引用 |
| **D** | 速度组件迁移 | C | `MoveSpeed`/`AttackSpeed`；删除 `Agility` 与旧公式 | 无 `Agility` 引用；AV 单调/clamp 测试通过 |
| **E** | 死抽象清理 | C/D | 身份 ZST、`EntityClass`/`CreatureKind`、死事件、`BeAttacked`、`PendingExp`、死 combat 函数等 | 每个保留抽象有真实读取方/消费者 |
| **F** | 构建/测试门禁 | A 起可并行 | I87、I88、CI 本地门禁 | `cargo test --workspace` 通过或明确排除 |
| **G** | presentation + tui 解耦 | E/F | 提取 `render-api` 消费层、`TuiPlugin` | 见早前渲染方案 |

```text
A ──▶ B ──▶ C ──▶ D ──▶ E ──▶ G
A ──▶ F（并行）
```

### 11.3 详细任务

#### Phase A — core 冒烟测试

| 编号 | 任务 | 位置/测试 | 验收 |
|---|---|---|---|
| A1 | 地图生成确定性：同 seed 两次 `new_game`，比较 `Map.tiles`、`rooms`、`StairsPos`、`PlayerSpawn`、怪物位置/种类 | `core/src/world/init.rs` 测试或 `core/src/world/loop_.rs` | 两次完全相等 |
| A2 | 玩家移动：可控 World（`insert_core_resources` + 手 spawn 玩家，或 `new_game` 后清怪）分别验证合法移动与撞墙/越界 | `core/src/world/loop_.rs` 测试 | `Position` 正确变化/保持不变 |
| A3 | 攻击与伤害：手 spawn attacker/target，发 `AttackIntentEvent`，settle 后 HP 只扣一次 | `core/src/system/mod.rs` 测试（扩展已有） | HP 精确减少；重复 settle 不再扣 |
| A4 | 死亡→经验→升级：怪物 HP 归零后 despawn、玩家获得 `Experience`，跨阈值时 `Level` 提升、HP/MP 重算 | `core/src/system/mod.rs` 测试 | 经验/等级/属性断言通过 |
| A5 | FOV/记忆/占用图：settle 后 `Viewshed.visible_tiles` 非空、`MapMemory.explored` 有增量、`OccupancyMap` 含玩家与怪物；移动后占用图更新 | `core/src/system/mod.rs` 测试 | 计数/包含关系断言通过 |
| A6 | 快怪多动：两个 actor 不同 AV，连续 `run_action_cycle`，AV 小的执行次数更多 | `core/src/action/execution/mod.rs` 测试 | 执行计数符合 AV |
| A7 | 测试辅助：统一的 `test_world()`/`spawn_test_actor()` helper，避免每个测试重复搭 World | `core/src/test_util.rs`（`#[cfg(test)]`） | helper 编译且被复用 |

**注意：** `core` crate 名与标准库 `core` 同名；新增测试优先用单元测试（`#[cfg(test)]`），不新增依赖 `core::` 的 doctest。

#### Phase B — action 实体 PoC

| 编号 | 任务 | 位置 | 验收 |
|---|---|---|---|
| B1 | 定义 action 实体组件：`ActionPriority(u32)`、`ActionTimer`（复用）、`Candidate`、`ActiveAction`、`Ready`（复用） | `core/src/action/entity.rs`（新模块） | 组件可插入/查询 |
| B2 | `wander_generation_system`：actor 满足 `CanWander`、`Idle/Failure`、`Without<Active>` 时 `Commands::spawn((ChildOf(actor), ActionPriority(50), ActionTimer{...}, Candidate, Wander))` | 同上 | 只 spawn 候选，不改 actor 状态 |
| B3 | `action_arbitration_system`：按 `ChildOf.parent()` 分组，选 `(ActionPriority, entity.to_bits())` 最大者，winner 去 `Candidate` 加 `ActiveAction`，actor 加 `Active`，loser despawn | 同上 | 每个 actor 至多一个 `ActiveAction` |
| B4 | `tick_action_timers_system`（action 实体版）：只推进 `With<ActiveAction>`，归零加 `Ready` | 同上 | 未归零不执行 |
| B5 | `execute_move_system`（action 实体版）：`Query<(Entity, &ChildOf, &Move), (With<ActiveAction>, With<Ready>)>`，移动 actor，成功/失败发 `ActionSucceeded/FailedEvent` | 同上 | actor `Position` 只变一次 |
| B6 | `action_completion_system`：消费事件，despawn action 实体，actor 回 `Idle`/`Failure` | 同上 | 无残留 `ActiveAction`/候选子实体 |
| B7 | PoC 测试：手建 actor + `CanWander`，跑上述系统链若干轮，断言“生成 → 仲裁 → Ready → 移动 → 完成”，并检查 loser despawn | `core/src/action/entity.rs` 测试 | 全链路通过 |

**约束：** PoC 不接主循环、不删旧系统；先证明 action 实体方案可行。

#### Phase C — 全量行动迁移

推荐**逐行动迁移**，每步保持可编译、可测试：

| 顺序 | 行动 | 迁移内容 | parity 测试 |
|---|---|---|---|
| C1 | `Wait` | `wait_generation_system` + action 实体执行 + completion | 等待后 actor 回 Idle，AV 正确 |
| C2 | `Move` | `move_generation_system`（玩家路径直接 active action） + 移动执行 | 合法/阻挡/对角规则与旧版一致 |
| C3 | `BasicAttack` | `basic_attack_generation_system` + 执行；保留 `AttackIntentEvent` 或合并 | 伤害/暴击/死亡链路一致 |
| C4 | `Wander` | `wander_generation_system` + 执行 | 随机方向、碰撞行为一致 |
| C5 | `Chase` | `chase_generation_system` + 执行 | 视野/LastKnownPlayerPos/相邻攻击一致 |
| C6 | `Flee` | `flee_generation_system` + 执行 | 低血滞回、逃跑方向一致 |
| C7 | 集成 | `advance_until_player_acted` 每轮 generation → arbitration → tick → execution → completion → settle；删除 `decide_monster_actions`/`choose_action`/`run_action_cycle` | 闭环测试、场景测试通过 |
| C8 | 删除 | `ActionKind`、`mount_action` 中央 match、actor 上的行动 ZST（移到 action 实体）；`ActionSucceeded/Failed` 成为 completion 的真实输入 | 全库无 `ActionKind` 引用 |
| C9 | Parity 套件 | Wait/Move/Attack/Chase/Flee/Wander 各一个受控场景，断言位置/HP/状态/子实体数量 | 全部通过 |

**关键点：**

- 玩家行动直接 spawn `ActiveAction`（或保留最高优先级），AI generation 过滤 `With<Monster>`，不得覆盖玩家。
- 仲裁比较器只用 `(ActionPriority, action_entity.to_bits())`，无 RNG。
- 每轮结束必须清空候选：winner 转 `ActiveAction`，loser despawn。
- actor 已有 `ActiveAction` 时跳过仲裁；候选查询的 `Without<ActiveAction>` 不够。
- 用 `.chain()` / `ApplyDeferred` 保证 generation 的 `Commands` 在 arbitration 前落盘。

#### Phase D — `Agility` → `MoveSpeed` / `AttackSpeed`

| 编号 | 任务 | 验收 |
|---|---|---|
| D1 | 新增 `MoveSpeed(f64)` / `AttackSpeed(f64)`；新增 `action_av_for(kind, move_speed, attack_speed)`；保留 `Agility` 作对照 | 新旧 AV 对比测试通过 |
| D2 | 玩家/怪物模板与 spawn 迁移到速度组件；生成系统按行动类别取速度 | 无新代码读取 `Agility` |
| D3 | 删除 `Agility`、`agility_to_reaction`、`agility_speed_factor`、旧 `action_av` | 全库无 `Agility` 引用 |
| D4 | GAME.md 反应时/耗时章节、玩家/怪物数值表、武器速度章节同步；DESIGN/ISSUES 更新 | 文档与公式一致 |
| D5 | 速度测试：单调性、clamp、怪物速度组件存在、回合顺序场景 | 测试通过 |

**待确认语义：** 倍率 `AV = base_duration / speed`；是否保留 `BASE_REACTION`；`Wait` 固定 duration 还是用 `MoveSpeed`；旧敏捷→新速度映射表（见 §2.6/§11.6）。

#### Phase E — 死抽象/重复表示清理

按 §10.8 清单逐项处理，每项先确认“读取方/消费者”：

| 项 | 建议 | 验证 |
|---|---|---|
| 身份 ZST `Rat/...` | 保留 `MonsterKindId` 作为数据键，删除无读取方 ZST；需要 `With<Rat>` 时再加 | 编译通过，怪物功能不变 |
| `EntityClass` / `CreatureKind` | 无读者则删除；需要时改为 ZST 标记 + 专用 query | 无未使用范畴 |
| `DeathEvent` / `LevelUpEvent` | 接线到经验/掉落/UI，或删除 | 每个事件有 reader |
| `ThreatEvent` / `ThreatTable` | 保留为 S4 占位或删除；不允许“注册但永不用” | 明确状态 |
| `BeAttacked` / `NeedRecordBeAttacked` | 接威胁/AI 或删除 | 无 write-only 组件 |
| `PendingExp` | `DeathEvent` 接消费者后删除 | 经验链路仍通过 |
| `MeleeResult` + dead combat helpers | 删除，保留纯函数 | 无零调用方函数 |
| `MonsterStats` / `WorldInitConfig` | 可选折叠/内联；低优先级 | 无多余 DTO |
| `Idle/Active/Failure` | 保留 ZST + debug 断言，或评估单一 `ActionState` | 状态互斥测试 |

#### Phase F — 构建/测试门禁

| 编号 | 任务 | 验收 |
|---|---|---|
| F1 | I87：`sys` 的 `log` 依赖显式 `features = ["std"]` | `cargo test -p sys` 通过 |
| F2 | I88：删除/归档旧根集成测试；重写为新 core + render-api headless 测试 | `cargo test -p dungeon-app` 通过（或明确不纳入） |
| F3 | core clippy：`too_many_arguments`/`type_complexity`/`collapsible_if` 历史警告 | `cargo clippy -p core --all-targets -- -D warnings` 通过（可选） |
| F4 | CI/本地门禁：`cargo check --workspace` + `cargo test -p render-api -p core -p utils -p tui -p sys` + `cargo clippy -p render-api -- -D warnings` | 一条命令可跑 |
| F5 | `core` crate 改名评估（I86 长期） | 记录决策，不阻塞本轮 |

#### Phase G — 回到 presentation + tui

按早前的渲染方案执行：

- 新建 `presentation`：`core` → `SceneFrame` 提取 + `VisualKey` 映射 + UI 状态/页栈 + 输入映射；
- `tui` 去掉 `core` 依赖，改为 `TuiPlugin` + `SceneFrame` 消费 + `TestBackend` 测试；
- 然后才是 `bevy_app` 插件宿主与未来 GPU 后端。

### 11.4 测试矩阵

| 测试 | 阶段 | 目的 |
|---|---|---|
| `map_generation_is_deterministic` | A | 同 seed 地图/怪位一致 |
| `player_move_and_blocked_move` | A/C | 移动规则与旧版一致 |
| `attack_applies_damage_once` | A/C | 伤害只结算一次 |
| `monster_death_rewards_exp_and_levels_up` | A | 死亡→经验→升级链路 |
| `fov_memory_occupancy_update` | A | 视野/记忆/占用图 |
| `fast_actor_gets_more_actions` | A/C | AV 门禁与多动 |
| `action_entity_poc_round_trip` | B | 生成/仲裁/Tick/执行/完成 |
| `arbitration_priority_and_cleanup` | B/C | 优先级、loser despawn、无残留 |
| `player_action_not_overridden_by_ai` | C | 玩家路径独立 |
| `action_parity_wait/move/attack/chase/flee/wander` | C | 行为 parity |
| `speed_av_monotonic_and_clamped` | D | 速度公式 |
| `monster_speed_components_present` | D | 模板迁移完整 |
| `no_action_kind_references` / `no_agility_references` | C/D | 用 grep/脚本作为门禁 |
| `cargo test -p sys` / `cargo test --workspace` | F | 构建门禁 |

### 11.5 提交与文档策略

- 每个 Phase 至少一个 commit；Phase C 建议逐行动 commit，便于回滚。
- 修复 ISSUES 条目后：在条目内标 `✅已修复` + “修复前/修复后” + 位置；必要时加 LESSONS。
- 设计变化：追加 DESIGN.md（不删旧条目）；REFACTOR.md 只在本分支维护，合并前折叠进 DESIGN。
- GAME.md 数值改动用 `[⃞计算]` / `[⃞直觉]` / `[⃞试调]` 标注。
- 每个 commit 前跑对应测试门禁；Phase F 完成后跑全门禁。

### 11.6 开放决策（执行前确认）

| # | 决策 | 推荐 |
|---|---|---|
| 1 | Phase C 迁移顺序：逐行动 vs 一次性 | 逐行动（Wait → Move → BasicAttack → Wander → Chase → Flee），每步 parity 测试 |
| 2 | Phase D 速度语义：倍率 vs AV 消耗 | 倍率：`AV = base_duration / speed`，clamp `[MIN, MAX]` |
| 3 | 反应时：删除 vs 常数 `BASE_REACTION` | 先删除（公式最简）；若试玩觉得先手感不足，再加统一常数 |
| 4 | `Wait`：固定 vs `MoveSpeed` | 固定 `WAIT_DURATION`；试玩后再决定是否引入 `WaitSpeed` |
| 5 | 怪物速度映射：保行为 vs 重新设计 | 先按旧敏捷映射出初值（保行为），再在 GAME.md 中 `[⃞试调]` 重调 |
| 6 | Phase E 死抽象：删除 vs 保留占位 | 无真实读取方/消费者就删除；S4 占位必须显式标注 |
| 7 | I87/I88：先修 vs 最后统一修 | 建议 Phase A 后立即修，恢复 `cargo test` 门禁 |
| 8 | `core` 改名（I86 长期） | 本轮不改；记录为独立决策 |

### 11.7 风险与缓解

| 风险 | 缓解 |
|---|---|
| action 实体引入行为漂移 | Phase A 测试 + Phase C 逐行动 parity + 旧代码保留到对应 commit 通过 |
| 实体 churn（每轮候选 spawn/despawn） | 只为 `Idle/Failure` 且 `Without<Active>` 生成；仲裁立即 despawn loser；必要时持久 `ActionSlot`/池化 |
| 子实体生命周期泄漏 | Bevy 0.16 `Children` 是 `linked_spawn`；completion 主动 despawn；测试断言子实体数量 |
| 事件消费者缺失/重复 | 每个事件必须有 reader；每轮 `Events::update()`（已修 I90）；`ActionSucceeded/Failed` 由 completion 消费 |
| 玩家被 AI 覆盖 | 玩家路径直接 active action / 保留最高优先级；AI 过滤 `With<Monster>` |
| 仲裁不确定 | `(ActionPriority, entity.to_bits())` 全序；比较器无 RNG |
| 速度迁移破坏平衡 | D1 双轨对比、D5 场景测试、GAME.md 试调标注 |
| 测试基础设施缺失 | Phase A 先补 helper；I87/I88 先修 |
| 范围蔓延 | 每阶段 timebox；Phase E 只做 §10.8 清单 |

### 11.8 预估与下一步

| 阶段 | 预估 |
|---|---|
| A | 0.5–1 天 |
| B | 0.5–1 天 |
| C | 2–3 天 |
| D | 1–2 天 |
| E | 0.5–1 天 |
| F | 0.5–1 天 |
| G | 1–2 天（TUI 解耦） |

**下一步：** 确认 §11.6 的开放决策后，从 Phase A 开始执行。建议先用 A1–A5 建立安全网，再进入 action 实体 PoC。

> 原则：**每个保留的抽象必须有真实读取方/消费者；否则就是下一个 `ActionKind`。**
