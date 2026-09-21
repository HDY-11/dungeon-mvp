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
| `ecs_core`（原 `core`） | 唯一业务/领域层，ECS 组件、系统、地图、AI、战斗、初始化 | UI、OS 交互、纯工具函数 |
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

> **已迁移（Phase D，见 §2.6）：** `Agility` 已删除，速度由 `MoveSpeed` / `AttackSpeed` 两个倍率组件表达。

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

> **状态：** ✅ 已落地（Phase B/C）。实现记录见 §11.3 Phase B/C 与 DESIGN DsnE8 ①。

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

> **实现注记（Phase B）：** 实际落地为 `ActionPriority(i32)`，仲裁取 `(ActionPriority, to_bits())`
> 的**最小**者，因此 §3.5 的 200/100/50/0 单调映射为 `-200/-100/-50/0`（常量
> `PRIORITY_FLEE/CHASE/WANDER/WAIT`）。这样“优先级高 = 数值小”与“同优先级取小 bits”
> 共用同一个比较方向，避免两套方向混用出错。

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
- 保留生成/仲裁分离，不因当前规模合并为单函数（用户决策；这是 2.1 的 OCP 扩展点）。
- 普通怪 = 新模板 / Bundle（不同 `Can*`）；新增普通怪不需要修改 AI / 生成 / 执行代码。
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

### 8.1 行动系统目标重构（✅ 已落地）

> **进展（已过期描述已更正）：** AV 门禁（I89）与事件生命周期（I90）→ Phase A 落地；
> `ActionKind` 删除与 action 实体 PoC → **Phase B/C 全部落地**（commit `ac6623f`…`a8e2175`）；
> 速度组件迁移 → Phase D；死抽象清理 → Phase E。
> 下方清单是**当时的规划文本**，保留供追溯；逐项落地记录见 §11.3 Phase B/C/D/E。

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
| `ecs_core`（原 `core`）领域模型 | **S1（语义）/ S2（API）** | 细粒度组件、`Can*`、`Idle/Active/Failure`、生成/仲裁/执行、事件链、`(seed, floor)` 地图确定性 | `ActionKind` 与 `Agility` 已删除（§3.6 / §2.6 均已落地）；公共 API 目前 `pub use *` 全暴露；物品/技能等会追加 | ✅ 71 |
| `core::world` 应用入口 | **S2** | `new_game` / `apply_player_command` / `request_quit` 的“命令驱动回合”语义 | 可能被 `CorePlugin` 包装；`build_init_schedule` / `build_core_schedule` 已改持久 Schedule；事件生命周期已修（I90） | ✅ 含在 core 71 内 |
| 行动实体设计（§3.6） | **S1（已落地）** | action 子实体、`ActionPriority`、`ActionTimer`、`Ready`、completion 的语义 | 命名/字段已冻结；`Candidate`/`ActiveAction` 的池化策略未来可能调整 | ✅ parity 套件 |
| `utils` | **S1** | 无业务依赖的通用工具 | 基本不变；缺测试 | ⚠️ 0 |
| `sys` | **S2** | 终端/文件/日志的 OS 封装可用 | 输入 API 从 `Receiver<KeyCode>` 改为 `InputQueue`；终端生命周期可能移入 `TuiPlugin`；`log/std` 依赖需显式声明 | ❌ 独立编译失败 |
| `tui` | **S3** | 当前能渲染最小闭环 | `scene.rs` 提取移到 `presentation`；`render.rs` 改为消费 `SceneFrame`；`state.rs` UI 状态移到 `presentation`；`render_game` / `extract_scene` 会删除 | ⚠️ 0 |
| `dungeon-app`（根） | **S3** | 当前 main 循环可跑通 | 将被 `App` + `Plugin` + `ScheduleRunnerPlugin` 替换；`keymap.rs` / `throw.rs` 是旧应用层代码；旧集成测试已失效 | ❌ 测试目标编译失败 |
| 旧 `dungeon-*` / `src/pages` | **S4** | 仅历史参考 | 不再扩展；迁移完成后删除/归档 | 旧测试不属于新代码 |
| `terrain-forge` | **S1（外部）** | 地图生成引擎接口 | 上游变更可能影响地图确定性；由 `(seed, floor)` 锁定行为 | 新 core 无专项测试 |

### 10.3 `ecs_core` 细分

| 模块 | 等级 | 说明 |
|---|---|---|
| `components.rs` | S1 | `Position/Health/Magic/Level/Experience/Attack/Defense/MagicMastery/MoveSpeed/AttackSpeed/CritRate/CritDamage` 等细粒度组件（`Agility` 已在 Phase D 删除）。数值 `f64`；新增字段必须追加并考虑 serde。`Speed` 是 derive(Bundle) 打包件，不是新组件。 |
| `entity_cls.rs` | S1（待清理） | `Player/Monster/Stairs` 等查询用 ZST 稳定；`EntityClass` / `CreatureKind` 目前无实际读取方（§2.1），`Item/Buff` 是 S4 占位；身份 ZST `Rat/...` 与 `MonsterKindId` 重复（§2.2），待定去留。 |
| `events.rs` | S2 | 事件生命周期已修（I90，每轮 `Events::update()`）；`DeathEvent`/`LevelUpEvent` 仍无消费者、`ActionSucceeded/Failed` 未接线，按 §3.6.8 补齐后再冻结。 |
| `resources.rs` | S1 | `GameRng`（确定性 xorshift64* + steps，回放/存档语义）、`MapMemory`、`VisibleMemory`、`OccupancyMap`、`EventLog`、`TurnManager`、`MapSeed`、`FloorNumber`。`PendingExp` 可能在 `DeathEvent` 接上消费者后删除；`ThreatTable` 是 S4。 |
| `map/` | S1（含 serde 兼容约束） | `Tile` 以 u8 0..10 序列化，**只能末尾追加**；`MapKind` 变体只能末尾追加；`Map/Room` 新字段用 `#[serde(default)]`。`Tile::glyph()` 属于显示数据，后续会移到 TUI catalog，不要当领域契约。 |
| `spatial/` | S1 | FOV / LOS / A* 纯函数；行为稳定，但新 core 无专项测试。 |
| `action/` | S1（Phase B/C/D 已落地） | 生成/仲裁/执行三段式与 `Idle/Active/Failure` 状态机方向稳定；AV 门禁已修（I89，`Ready`）；`ActionKind` 已删除，行动改为 action 子实体（§3.6）；`Can*` 保留 actor 组件；速度经 `SpeedRule` 在挂载点解析（§2.6）。 |
| `combat/` | S1 | 近战伤害/暴击公式稳定；`prepare_attack_event` / `resolve_melee` / `damage_entity` 是死代码/重复路径（§10.6），建议删除。 |
| `monster/` | S1（serde 兼容约束） | `MonsterKindId` 变体只能末尾追加；`MonsterTemplate` 含 glyph/color（显示数据，后续移出）；速度改为字面值 `MonsterSpeeds`（§2.6，无「敏捷」字段）；数值公式可调但需 GAME.md 记录；身份 ZST 与 `MonsterKindId` 的重复见 §2.2 / §10.6。 |
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

- `core` 组件/资源/事件的**名称与语义**：`Position/Health/Magic/Level/Experience/Attack/Defense/MagicMastery/MoveSpeed/AttackSpeed/CritRate/CritDamage`；`Player/Monster/Stairs` marker；`Idle/Active/Failure`、`Can*`。`Agility` 与 `ActionKind` 已删除（Phase D / Phase C）；行动 ZST 名称（`Wait/Move/BasicAttack/Chase/Flee/Wander`）冻结，已从 actor 移到 action 子实体；`ActionTimer` 随之移动。
- `core` 资源：`Map/MapSeed/FloorNumber/GameRng/MapMemory/VisibleMemory/OccupancyMap/EventLog/TurnManager`（`PendingExp` 已在 Phase E 删除——奖励改由 `DeathEvent` 携带）。
- `core` 事件：`AttackIntentEvent/AttackEvent/DeathEvent/LevelUpEvent`（消费者/生命周期按 §3.6.8 补齐）。
- 行动实体新组件（`ActionPriority` / `ActionTimer` / `Candidate` / `ActiveAction` / `Ready` / `ChildOf` 归属）在 PoC 通过后再冻结；PoC 期间允许调整。
- serde 兼容：`Tile` u8 0..10、`MonsterKindId` 变体、`MapKind` 变体只能末尾追加；新字段 `#[serde(default)]`。
- `core::world_loop::{new_game, apply_player_command, request_quit}` 的调用语义（名字可保留兼容包装）。
- `render-api` v1 契约：`SceneFrame` / `VisualKey` / `UiView` / `InputEvent` 的字段可以追加；破坏性改动递增 `CONTRACT_VERSION`。
- 架构边界：`ecs_core` 不依赖 `bevy_app` / ratatui / wgpu / `render-api` / `presentation`；`tui` / `gpu` 不依赖 `ecs_core`。

允许/预期变更（不要在新代码里深度依赖）：

- `tui` 的 `scene.rs` / `render.rs` / `state.rs` 会被拆解/删除；
- `sys` 的 `Receiver<KeyCode>` 输入 API 会被 `InputQueue` 取代；
- 根 `main.rs` 会被 `App` / `Plugin` / runner 替换；
- `core` 的 `build_init_schedule` / `build_core_schedule` 会改成注册式 Schedule；
- `core` 的 `pub use *` 公共面可能收紧；
- 旧 `dungeon-*` / `src/pages` 不再维护。

### 10.6 进入下半前建议处理（按优先级）

1. **修复 `core` doctest 失败**：✅ 已修（I86）。`std::convert::Infallible` + `ScheduleLabel` 手写 impl。**crate 改名已落地（F5）**：`core` → `ecs_core`（目录同名），遮蔽问题从根上消除，见 DESIGN DsnX15。
2. **给 `core` 加冒烟回归**：✅ 已完成（Phase A）——`ecs_core` 现 **71 passed**，覆盖地图确定性、玩家移动/阻挡、攻击只结算一次、死亡→经验→升级、FOV/记忆/占用图、AV 门禁与事件生命周期。**注意 A6 的三个用例已随 Phase B/C 重构消失**（见 §11.4 的说明），"同等时间预算下快怪行动次数更多"这一行为声称**当前无直接断言**，后继用例 `faster_monster_gets_its_action_ready_first` 只钉住"快怪先拿 `Ready`"。
3. **修复 `sys` 独立构建**：给 `sys` 的 `log` 依赖显式加 `features = ["std"]`（或 workspace `log` 统一声明），确保 `cargo test -p sys` 不依赖 feature 合并偶然通过；补输入/日志测试。
4. **处理失效的根集成测试**：`tests/scenario_test.rs` / `tests/throw_test.rs` 针对旧 crate；要么删除/归档，要么重写为新 `core` + `render-api` 的 headless 测试。不要让 `cargo test --workspace` 长期失败。
5. **建立 CI/本地门禁**：至少 `cargo check --workspace` + `cargo test -p render-api -p core -p utils -p tui`（修复后）+ `cargo clippy -p render-api -- -D warnings`；旧 crate 测试单独标记，不计入新代码门禁。
6. **`render-api` 消费验证**：presentation/tui 接上后，补 `SceneFrame` golden 测试 + `TestBackend` 渲染快照；确认 `VisualKey` payload 映射与 core 实际枚举一致。
7. **行动实体 PoC（§3.6）**：先用一个 actor + `Wander` + `Move` 验证 generation → arbitration → tick(Ready) → execution → completion 全链路；通过后再迁移 Flee/Chase/Wait/BasicAttack，最后删除 `ActionKind` / `mount_action` 中央 match / `choose_action`。
8. **速度组件迁移（§2.6）**：✅ 已完成（Phase D1–D5）——`Agility` 与旧 AV 公式已删除，改为 `MoveSpeed`/`AttackSpeed` 倍率；GAME.md/DESIGN.md/ISSUES.md 已同步。

### 10.7 兼容性与确定性规则

- **无存档承诺**：新 `core` 尚未迁移存档；旧 `dungeon-world` 的存档格式不兼容新组件。在存档迁移完成前，不要对外承诺“新 core 可读旧档”。
- **serde 只追加**：已 derive serde 的类型（`Tile` / `MapKind` / `MonsterKindId` / `Map` / `Room` 等）不得重排变体、不得复用旧判别值；新增字段用 `#[serde(default)]`。
- **确定性随机**：`GameRng` 的算法与 `steps` 语义是回放/存档基础；改变算法必须视为存档格式变更。
- **地图确定性**：`generate_map_from_seed` 对 `(MapSeed, FloorNumber)` 必须确定；修改地图生成算法会改变同 seed 地图，需要 DESIGN 记录 + golden seed 测试。
- **显示数据不属于领域**：`Tile::glyph`、`MonsterTemplate::glyph/color` 等是渲染数据，后续迁移到 `presentation` / `tui` catalog；不要把它们当 core 稳定契约。
- **行动实体不存档**：action 实体是瞬态子实体；存档只存 actor 状态/速度/身份，读档后重新生成行动。action 实体的组件名不要作为存档格式的一部分。

### 10.8 同类死抽象/重复表示清理清单（报告，待判断）

与 `ActionKind` 同类的问题：**中央 token / 提前抽象 / 重复表示**。以下为清查结果，去留由用户判断：

> **用户决定（2026-09）：** 本清单先记录，Phase A/B/C 期间不执行删除；Phase E 开始前逐项确认是否删除/接线。对应 ISSUES A43。
>
> **状态（2026-09，Phase E 执行后）：** ✅ 已逐项实测并处置完毕。两处与当初预判不同——`MeleeResult` 其实是活的（被 `compute_melee_damage` 使用），身份 ZST 比预判更死（连插入方都没有）。下表的「判断/建议」列已更新为**实测处置**，逐项理由见 §11.3 Phase E。

| 项 | 证据 | 判断/建议 |
|---|---|---|
| `ActionKind` | `action/mod.rs`；`mount_action` 中央 match；`ai.rs` / `player.rs` / `world/loop_.rs` 引用 | ✅ Phase C 删除 |
| 身份 ZST `Rat/Scorpion/...` | `entity_cls.rs` 定义；无插入方、无读取方 | ✅ Phase E 删除；`MonsterKindId` 作为唯一数据键 |
| `EntityClass` | `system/mod.rs` 唯一读取是 `Item` 判断，而全库从未插入 `Item` → 恒假 | ✅ Phase E 删除 |
| `CreatureKind` | `monster/mod.rs` 模板字段 + spawn 插入；无读取方 | ✅ Phase E 删除 |
| `PlayerCommand` | `src/main.rs` 输入映射；`player.rs` 映射到 `ActionKind` | 保留为输入边界类型；去掉 `ActionKind` 后直接映射到 action 实体/Bundle |
| `AttackIntentEvent` | 唯一生产者 `execute_basic_attack_system`，唯一消费者 `resolve_attack_system` | 可合并进 `attack_system`；若保留作为技能/投射物扩展点，必须有未来消费者 |
| `ActionSucceededEvent` / `ActionFailedEvent` | 注册但无人发/读 | 按 §3.6.6 接 completion 系统，变成真实状态回转事件 |
| `DeathEvent` / `LevelUpEvent` | `DeathEvent` 原本只写不读 | ✅ Phase E：`DeathEvent` 携带 `reward` 并被 `experience` 消费；`LevelUpEvent` 保留待 UI 消费 |
| `ThreatEvent` / `ThreatReason` / `ThreatTable` | 注册/定义但无发送/读取 | ✅ Phase E 删除 |
| `BeAttacked` / `NeedRecordBeAttacked` | `record_be_attacked_system` 写入，无读取 | ✅ Phase E 删除组件与整条系统 |
| `PendingExp` | 死亡系统写、经验系统读的旁路 | ✅ Phase E 删除（奖励改由 `DeathEvent` 携带） |
| `MeleeResult` + `prepare_attack_event` / `resolve_melee` / `damage_entity` | `compute_melee_damage` 在用 `MeleeResult`；后两者无调用方 | ✅ Phase E：删三个死函数与 `can_attack`/`adjacent_8`，保留 `MeleeResult`（是活的） |
| `MonsterStats` | 仅 `monster_base_bundle` 使用的中间 DTO | 保留（有真实调用方；低优先级） |
| `WorldInitConfig` | 单字段 `map_seed` | 保留（有真实调用方；低优先级） |
| `Idle/Active/Failure` | 三个 ZST 表示一个状态机 | 保留 ZST + 互斥测试（I91；不做单一 `ActionState`） |
| `Can*` | 六个能力 ZST | 保留；这是 ECS 原生能力表达，不要子实体化 |
| `MonsterKindId` | 数据键 + 多处 match 表 | 保留 enum；可将 match 表改为 `&'static [MonsterTemplate]` 索引，减少样板 |
| `Tile` / `MapKind` / `RoomShape` | 领域/算法数据 enum | 保留；不是行为分派 token |
| `EventLevel` vs `render-api::LogLevel` | 跨层重复 | 故意分层；保留，映射集中一处 |


---

## 11. 实施计划（第 2 步起）

> **状态：** A–F 全部完成；G 完成 R1（`presentation` 57 / `tui` 25）与 **R5（旧 crate 归档）**，R2 部分（`Look`/`Dialog` 已落地），R3/R4 待续（R3 的原阻塞"环境无外网"**已消失**）；**H 设计输入已落地（commit `55d8086`），代码待开工**。
> **基线（本轮实测）：** `cargo test -p ecs_core` **71 passed**；`cargo test --workspace` **28 个测试目标全绿 / 0 failed**（含旧 `dungeon-*` 与 `terrain-forge`）；`scripts/gate.ps1` 9 步全绿（含 5 条 `cargo tree` 依赖边界）；`cargo check --workspace` 通过。
> **开放：** A41（exclusive `&mut World`，范围已随 Phase C 收窄至 `world/loop_.rs` 应用入口）、R2–R4、Phase H（H1–H14）。已关闭：A42（Phase C）、G35（Phase D）、A43（Phase E 逐项处置完毕）、F3/F4（core clippy、一键门禁）、I87/I88（Phase A 后）、SYN3（子模块配置）、R5（旧 crate 归档，本轮）。
> **不在门禁内：** 旧 `dungeon-*` / 根 `dungeon-app` 的历史集成测试——它们**能跑通但不代表新方向**（见 `PROTOCOLS.md` §五「为什么不覆盖全部」）。

### 11.1 目标与范围

**目标：** 在保持当前最小闭环可玩的前提下，依次完成：

1. 可靠的 core 冒烟测试基线（Phase A）；
2. action 实体方案 PoC（Phase B）；
3. 全部行动迁移到 action 实体（Phase C）；
4. `Agility` → `MoveSpeed` / `AttackSpeed`（Phase D）；
5. 同类死抽象/重复表示清理（Phase E）；
6. 构建/测试门禁修复（Phase F）；
7. 回到 `presentation` + `tui` 解耦（Phase G）；
8. **扩展性加固（Phase H）**：为下一批设计（战斗公式分层、地形代价、装备、技能）
   预留**形状**，不填值——见 §11.3 Phase H 与 DESIGN.md DsnX16。

**不做：** 物品/背包/装备、buff/技能、掉落表、下楼、存档读档；这些仍按 §8 保持 S4。

### 11.2 阶段总览

| 阶段 | 内容 | 依赖 | 主要产物 | 验收 |
|---|---|---|---|---|
| **A** | core 冒烟测试 | I89/I90 已修 | 地图确定性、移动/攻击、死亡/经验、FOV/记忆/占用图测试 | ✅ `cargo test -p core` 4 → 18 通过（commit 95449a9） |
| **B** | action 实体 PoC | A | actor + Wander + Move 全链路测试模块 | ✅ `core/src/action/entity.rs` + PoC 测试（commit ac6623f） |
| **C** | 全量行动迁移 | B | 生成/仲裁/Tick/执行/完成系统；删除 `ActionKind` | ✅ C1–C9 全部完成（commit 83700ff…a8e2175）：六行动 parity 通过、全库无 `ActionKind` 引用、主循环已切换 |
| **D** | 速度组件迁移 | C | `MoveSpeed`/`AttackSpeed`；删除 `Agility` 与旧公式 | ✅ D1–D5 全部完成（commit cff5940…）：`Agility`/旧公式零引用、AV 单调与 clamp 测试、怪物速度齐全、回合顺序场景通过 |
| **E** | 死抽象清理 | C/D | 身份 ZST、`EntityClass`/`CreatureKind`、死事件、`BeAttacked`、`PendingExp`、死 combat 函数等 | ✅ 全部完成：删除 6 类死抽象 + `DeathEvent` 接线（`PendingExp` 旁路随之消失）；`cargo clippy -p ecs_core -- -D warnings` 干净，71 测试通过 |
| **F** | 构建/测试门禁 | A 起可并行 | I87、I88、CI 本地门禁 | ✅ I87/I88 已在 Phase A 后修（commit 7d5b8e1）：`cargo test --workspace` 25 个目标全绿 |
| **G** | presentation + tui 解耦 | E/F | 提取 `render-api` 消费层、`TuiPlugin` | ⏳ R1 完成：新建 `presentation`（57 测试）；`tui` 去 `ecs_core` 依赖、改消费 `SceneFrame`（25 测试）；边界由 `scripts/gate.ps1` 的 `cargo tree` 步骤强制 |
| **H** | 扩展性加固（只碰形状，不碰值） | A–G | 行动终态单一出口、伤害输入结构体化、schedule 分组、`TileProps` 表、物种信息归位、规则修正器接口、技能三层骨架、世界级效果实体 | ⏳ 计划中：见 §11.3 Phase H；**不改数值、不改行为**（现有 71 测试须全绿） |

```text
A ──▶ B ──▶ C ──▶ D ──▶ E ──▶ G ──▶ H
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

**注意：** crate 已改名 `ecs_core`（F5 / DESIGN DsnX15），不再与标准库 `core` 同名；新增测试仍优先用单元测试（`#[cfg(test)]`），doctest 现在可用但不是主要形态。

#### Phase B — action 实体 PoC（✅ 已完成）

> **落地记录（commit `ac6623f`）：** `core/src/action/entity.rs` + 测试拆到 `core/src/action/entity_tests.rs`；
> 调度标签 `ActionPocSchedule`（`core/src/schedule.rs`），由 `build_action_poc_schedule()` 构建，**未接主循环**。

| 编号 | 任务 | 落地情况 |
|---|---|---|
| B1 | action 实体组件 | ✅ `ActionPriority(i32)`（取**最小**者胜出，与 §3.5 的 200/100/50/0 单调对应）、`ActionSource`、`Candidate`、`ActiveAction`、`ActionName`；`ActionTimer` / `Ready` 复用 `components.rs` |
| B2 | 生成系统 | ✅ `wander_generation_system` + `flee_generation_system`（后者让仲裁真的需要比较优先级）；两者只 spawn 候选，不写 actor 状态 |
| B3 | 仲裁系统 | ✅ 按 `ChildOf::parent()` 分组，赢家取 `(ActionPriority, action_entity.to_bits())` 最小者；先剔除“已有 `ActiveAction` 的 actor”的候选（`Without<ActiveAction>` 挡不住这种情况）；loser 一律 despawn |
| B4 | Tick | ✅ 只推进 `With<ActiveAction>`；与旧 tick 同一「最小正 AV」口径；归零加 `Ready` |
| B5 | 执行系统 | ✅ `execute_wander_system` 与 `execute_move_system` **都是普通参数化系统**（见下）；只发 `ActionSucceeded/FailedEvent`，不回收实体 |
| B6 | completion | ✅ 消费两个事件 → despawn 该 actor 的 action 实体 → actor 回 `Idle`/`Failure`；`ActionSucceeded/FailedEvent` 从死事件变成真实状态回转机制 |
| B7 | PoC 测试 | ✅ 11 个（全链路 / 优先级 / 平局 / 忙碌 actor / 生成不写状态 / 不重复生成 / 低血双候选 / 低血仲裁到 Flee / 玩家不被 AI 生成 / 参数化-旧 `World` 版 parity / 与 `CoreSettleSchedule` 共存）+ 1 个旧路径对照测试 |

**PoC 结论与遗留：**

- 链路可行：生成 → 仲裁 → tick → 执行 → completion 全通，无残留 action 子实体、无 actor 卡在 `Active`。
- **执行器全部是普通系统（修正记录）**：Phase B 初版把 `execute_move_system` 写成 exclusive
  `&mut World`，理由是「action 实体 → actor 位置的多实体读写无法用普通 `Query` 表达」。
  这个理由是**错的**——它只是**复用 `movement::execute_move(&mut World, ...)` 的后果**：
  那个签名把「读 `Position` + 读 `Map`/`OccupancyMap` + 写 `Position`」揉进一次 `&mut World` 调用。
  Bevy 的 `Query<&mut T>` 只保证 **per-entity** 唯一可变访问：驱动实体是 action 实体
  （`ActiveAction + Ready + Move` 都在它身上），被写的是**另一个实体** actor 的 `Position`，
  `ChildOf` 只作读——两处并无真冲突。
  **修法（已完成）**：落点计算抽成纯函数
  `movement::moved_position(map, occupancy, pos, dx, dy) -> Option<Position>`（`can_move_to` 不变），
  执行器改为 `Query<(Entity, &ChildOf, &Move), (With<ActiveAction>, With<Ready>)>` +
  `Query<&mut Position>` + `Res<Map>` + `Res<OccupancyMap>`。
  证据：`parameterized_move_matches_world_based_move`（与旧 `World` 版逐情形 parity）、
  `parameterized_move_executor_coexists_with_settle_systems`（追加进 `CoreSettleSchedule` 同调度不冲突）。
- **A41 范围因此收窄**：PoC 链路已无 exclusive 系统；剩余 exclusive 代码都在旧模型
  （`execution/mod.rs` 的 `execute_*_system`/tick、`ai.rs::decide_monster_actions`、`world/loop_.rs` 的推进），
  随 Phase C 删除旧链路一并消失。
- `ActiveAction` 在 actor 行动的那一轮内就被执行并回收，所以“一个 actor 至多一个 `ActiveAction`”
  只能在**仲裁之后、执行之前**断言（测试已按相位拆开）。
- `Move` 失败语义沿用旧实现：被挡/越界 → `ActionFailedEvent` → actor 进 `Failure`
  （等价旧 `execute_move_system` 的 `finish_action_failure`）；`Wander` 被挡仍算成功
  （等价旧 `execute_wander_system` 的无条件 `finish_action_success`）。Phase C parity 套件需保持这两个语义。
- 纪律提醒：这次是**注释/文档先给出了过强结论**（「必须 exclusive」）再被测试推翻。今后写
  「不可能 / 必须」类判断前，先写一个最小验证（一条 parity 测试就足够推翻它）。

**约束复核：** PoC 未接主循环、未删旧系统、未触碰 §10.8/A43 死抽象清单。

#### Phase C — 全量行动迁移（✅ 已完成）

> **落地记录（C1–C9，均已完成）：** `core/src/action/entity.rs` + `entity_tests.rs`；
> 调度 `ActionPocSchedule`（生成 → 仲裁 → tick → 执行 → completion，C7 起接主循环）
> 与 `PlayerMountSchedule`（仅生成 + 仲裁，用于判定玩家命令是否被接受）。
> 逐行动 commit：C1 `83700ff`、C2 `241e6d8`、C3 `6d4163a`、C4 `4d77bdb`、
> C5 `ecde0cc`、C7 `f0ea1a5`、C8 `4775b42`、C9 `a8e2175`。

| 顺序 | 行动 | 落地情况 |
|---|---|---|
| C1 | `Wait` | ✅ `wait_generation_system`（兜底，PRIORITY_WAIT）+ `execute_wait_system`（无条件成功） |
| C2 | `Move` | ✅ 玩家路径 `PlayerActionRequest` → 直接产出 active action；`execute_move_system` 参数化（executor 见 B 段修正记录） |
| C3 | `BasicAttack` | ✅ 走向怪物 = 攻击；执行器只发 `AttackIntentEvent`，伤害仍由结算链路负责（I90 保证不变） |
| C4 | `Wander` | ✅ 已有实现；对照测试钉住「先抽方向再判合法」的 RNG 步数契约 |
| C5 | `Chase` | ✅ 生成条件（可见或有记忆）+ 执行器（记忆写入/清空、A\*、相邻攻击） |
| C6 | `Flee` | ✅ 滞回保活、8 方向最远合法落点、被堵时相邻且可见则反咬 |
| C7 | 集成 | ✅ `world/loop_.rs` 重写为「先挂载、再推进」两段式；`decide_monster_actions` / `mount_action` / `run_action_cycle` 不再被调用 |
| C8 | 删除 | ✅ `ActionKind`、`mount_action` 中央 match、`finish_action_*`、actor 上的行动 ZST、旧独占执行系统全部删除；全库无残留引用 |
| C9 | Parity 套件 | ✅ 六行动各一个受控场景 + 共同残留不变量（见 `entity_tests.rs` 的 `parity_*`） |

**关键点（实现中的实际结论）：**

- 玩家行动直接 spawn `ActiveAction`，AI generation 过滤 `With<Monster>`，不覆盖玩家。
- 仲裁比较器只用 `(ActionPriority, action_entity.to_bits())`，无 RNG。
- 每轮结束必须清空候选：winner 转 `ActiveAction`，loser despawn。
- actor 已有 `ActiveAction` 时跳过仲裁；候选查询的 `Without<ActiveAction>` 不够。
- 用 `.chain()` / `ApplyDeferred` 保证 generation 的 `Commands` 在 arbitration 前落盘。
- **`Ready` 的清理有两条路**：`execute_move_system` 显式 `remove::<Ready>()`
  （它不 despawn 实体），其余执行器靠 completion despawn 实体顺带清掉。
  写测试时断言对象应是「完成/失败事件数」，不是「Ready 是否被移除」。
- **`Idle` / `Failure` 互斥**：completion 与（曾经的）`finish_action_*` 都必须显式清掉
  另一个再插入（I91）。
- **挂载与推进必须分两段**：`apply_player_command` 先跑 `PlayerMountSchedule` 判定
  「命令是否被接受」，再进推进循环；合并成一次调度会把「已经做完了」误判成「命令被拒绝」。

#### Phase D — `Agility` → `MoveSpeed` / `AttackSpeed`（✅ 已完成）

> **落地记录（D1–D5）：** `core/src/balance.rs`（`action_av` / `clamp_speed` /
> `MoveSpeed` / `AttackSpeed` 常量）、`core/src/components.rs`、`core/src/action/entity.rs`
> （`SpeedRule` / `ActorSpeeds`）、`core/src/monster/mod.rs`（`MonsterSpeeds`）。
> `cargo test -p core` 61 → 71。

| 编号 | 任务 | 落地情况 |
|---|---|---|
| D1 | 新增 `MoveSpeed(f64)` / `AttackSpeed(f64)` 与倍率 AV；保留 `Agility` 作对照 | ✅ `SpeedRule::action_av(base_duration, ActorSpeeds)` 纯函数；5 个新旧 AV 对比用例（commit cff5940） |
| D2 | 玩家/怪物模板与 spawn 迁移到速度组件；生成系统按行动类别取速度 | ✅ `MonsterSpeeds { move_speed, attack_speed }` 字面值取代模板的「敏捷」；生成系统只查自己那一个速度组件（commit 6a5453c） |
| D3 | 删除 `Agility`、`agility_to_reaction`、`agility_speed_factor`、旧 `action_av` | ✅ 连迁移工具 `agility_to_speed` 一并删除；`core/` + `tests/` 无代码级引用（commit cf70f90） |
| D4 | GAME.md 数值章节、DESIGN、ISSUES（G35）同步 | ✅ GAME.md Gm1/Gm4/Gm7/Gm8；DESIGN DsnE8 ②；ISSUES G35 移入 ✅ 已修复 |
| D5 | 速度测试：单调性、clamp、怪物速度齐全、回合顺序场景 | ✅ 6 个新用例，见 §11.4 |

**已确认语义（§11.6）：** 倍率 `AV = base_duration / clamp(速度, 0.25, 4.0)`；
**先删除反应时**（试玩需要再加统一常数）；`Wait` 固定 `WAIT_DURATION`；
怪物速度 = 旧耗时系数的倒数（保排序，GAME.md 用 `[⃞试调]` 重调）。

**落地时的两个实现细节（写代码前值得先读）：**

1. **元组 `Bundle` 只到 15 元**：bevy_ecs 0.16 的 `all_tuples!(tuple_impl, 0, 15, B)`
   意味着玩家基础束（原本 16 个元素）不能再往元组里塞组件。解法是
   `#[derive(Bundle)] struct Speed { move_speed, attack_speed }` —— 打包件不是新组件，
   实体上仍是两个独立组件，查询不变。
2. **不要用「跑一轮生成 → 挑一个候选 → 再跑一轮生成」写测试**：第二轮时 actor 已是
   `Active`，生成系统的 `Without<Active>` 会让它再也生成不出候选。
   同理 `tick_action_timers_system` 只推进 `ActiveAction`，候选必须先过仲裁才会被 tick。

#### Phase E — 死抽象/重复表示清理（✅ 已完成）

> **落地记录：** 逐项先查「读取方/消费者」再动手，实测结论与 §10.8 的预判有两处
> 出入（见下表），因此下面以**实测**为准。删除项均已确认零引用；
> 保留项写明理由。`cargo clippy -p ecs_core --all-targets -- -D warnings` 干净，
> `cargo test -p ecs_core` 71 passed。

| 项 | 实测状态 | 处置 |
|---|---|---|
| 身份 ZST `Rat/.../DeepEel` | **零 insert、零 query**（比 §10.8 说的「由 `MonsterKindId` match 后插入」更死——插入点已随 Phase C 消失） | ✅ 删除 8 个 ZST（`entity_cls.rs`） |
| `CreatureKind` | 只写不读（8 个模板 + 玩家都插入） | ✅ 删除（含 `MonsterTemplate.creature_kind` 字段与 9 处赋值） |
| `EntityClass` | `rebuild_occupancy_system` 检查 `EntityClass::Item`，但**全库从未插入过 `Item`** → 判断恒假；范畴本身也无读取者 | ✅ 删除枚举；楼梯跳过保留（靠 `Stairs` 标记） |
| `DeathEvent` | 只写不读（仅 `update_events_system` 刷缓冲） | ✅ **接线**：事件携带 `reward`，`experience` 模块消费；见下条 |
| `LevelUpEvent` | 同上 | ✅ 保留（经验结算链路里 `EventLog` 是真实消费者；事件留给未来的 UI/成就读，不再有「注册了永不用」的占位） |
| `PendingExp` | 死亡系统写、经验系统读的旁路 | ✅ 删除资源与 `insert_resource`；奖励改由 `DeathEvent` 携带——**实体在死亡系统里就被 despawn，奖励只有那一刻能拿到**，这正是旁路存在的原因，把奖励放进事件即可去掉两条路 |
| `ThreatEvent` / `ThreatReason` / `ThreatTable` | 无生产者、无消费者（`ThreatTable` 三个方法也零调用） | ✅ 全部删除；S4 仇恨系统落地时重建 |
| `BeAttacked` / `NeedRecordBeAttacked` | 只写不读（`record_be_attacked_system` 是唯一写入方） | ✅ 删除组件与整条系统（从结算 Schedule 摘除） |
| `MeleeResult` | **是活的**：`compute_melee_damage` 返回它，结算链路在用 | ✅ 保留（与 §10.8 预判不同） |
| `prepare_attack_event` / `resolve_melee` / `damage_entity` | `resolve_melee`/`damage_entity` 零调用；`prepare_attack_event` 只被 `resolve_melee` 调用 | ✅ 删除三个死函数 |
| `can_attack` / `adjacent_8` | 只被 `prepare_attack_event` 用 | ✅ 删除；`can_attack_positions` + `compute_melee_damage` 保留（前者是执行器在用的纯规则） |
| `MonsterStats` / `WorldInitConfig` | 有真实调用方，低优先级 | ✅ 保留 |
| `Idle/Active/Failure` | I91 已修 + `idle_and_failure_are_mutually_exclusive` 在守 | ✅ 保留 ZST（不做单一 `ActionState`） |

**顺带完成的结构对齐（用户要求「文件结构向 ecs_core 的质量看齐」）：**
`ecs_core` 里 `map/`、`spatial/`、`action/` 早已是「目录 + 子模块」，只剩
`system/mod.rs`（654 行）与 `monster/mod.rs`（455 行）还是「单文件 + mod.rs」。
本轮把它们拆成同样的形式，`mod.rs` 只留模块声明/重导出：

| 原文件 | 拆分后 |
|---|---|
| `system/mod.rs` 654 行 | `system/mod.rs` 104（声明 + 重导出 + Schedule）・`combat.rs` 81・`perception.rs` 60・`death.rs` 62・`experience.rs` 69・`occupancy.rs` 25・`system_tests.rs` 344 |
| `monster/mod.rs` 455 行 | `monster/mod.rs` 15・`template.rs` 295（物种数值）・`spawn.rs` 88（出现概率）・`monster_tests.rs` 94 |

测试沿用 `action/` 已有的「生产与测试分文件」形式（`#[path = "..._tests.rs"] mod tests;`）。

#### Phase F — 构建/测试门禁

| 编号 | 任务 | 验收 |
|---|---|---|
| F1 | I87：`sys` 的 `log` 依赖显式 `features = ["std"]` | `cargo test -p sys` 通过 |
| F2 | I88：删除/归档旧根集成测试；重写为新 core + render-api headless 测试 | `cargo test -p dungeon-app` 通过（或明确不纳入） |
| F3 | core clippy：`too_many_arguments`/`type_complexity`/`collapsible_if` 历史警告 | ✅ 21 → 0，`cargo clippy -p core --all-targets -- -D warnings` 通过（commit e751914） |
| F4 | CI/本地门禁：`cargo check --workspace` + `cargo test -p render-api -p core -p utils -p tui -p sys` + `cargo clippy -p render-api -- -D warnings` | ✅ `scripts/gate.ps1`（含 core clippy 共 4 步，全绿退出 0）；用法见 PROTOCOLS.md §五 |
| F5 | `core` crate 改名评估（I86 长期） | ✅ 改名为 `ecs_core`（目录同名，7 个文件引用全部同步）；决策记录见 DESIGN DsnX15。`scripts/gate.ps1` 的 `-p core` 已同步为 `-p ecs_core` |

#### Phase G — 回到 presentation + tui（R1 ✅ 完成）

按早前的渲染方案执行（DsnX14 的 R1–R5）：

- ✅ **R1-a 新建 `presentation`**：`ecs_core` → `SceneFrame` 提取、`VisualKey` 映射、
  相机、页栈、输入映射，共五个模块（57 测试）；
- ✅ **R1-b `tui` 去 `ecs_core` 依赖**：删掉 `tui/src/scene.rs`（那是"后端自己查 core"
  的最后一块），改为消费 `SceneFrame`；新增 `TuiCatalog`（glyph/颜色）与
  `TuiPlugin`；24 个测试含 `TestBackend` 全帧断言；
- ✅ **R1-c 边界门禁**：`scripts/gate.ps1` 新增 5 条 `cargo tree` 规则，
  违反即 FAIL（已用注入违规依赖的方式验证过会失败）；
- ✅ **R1-d 端到端可玩性验证**：`tests/mvp_loop_test.rs`（9 条）把 `crossterm::KeyCode`
  → 世界推进 → `SceneFrame` → **真实 ratatui 绘制**整条链路串起来断言；门禁新增
  「端到端 / 单元 / bin 构建」三步。详见下方「MVP 跑通的验证记录」；
- ⏳ **R2** 页栈 UI：`Look`/`Dialog` 已落地；`Inventory`/`Throw` 需要物品与投掷规则
  迁移（DsnX13 S4），当前给占位页；
- ⏳ **R3** `bevy_app` 宿主 + `ScheduleRunner`：**本轮未做**——工作区里没有
  `bevy_app`，且当时环境无外网。`TuiPlugin` 已按"插件"形状收敛，补 `impl Plugin`
  时调用点不变（`tui/src/plugin.rs` 顶部有说明）。
  > **阻塞已消失（后续实测）：** 网络已可用（crates.io / rsproxy 可达），
  > `bevy_app` 可直接引入；R3 因此改为"独立一轮"的计划项，排在 Phase H 的
  > H1–H4 之后（见 §11.9）。
- ⏳ **R4** GPU 后端：消费同一 `SceneFrame`，`ecs_core` 零改动；依赖 R3 的插件宿主。
- ✅ **R5** 旧 crate 清理：已归档 4 个旧 crate 与旧架构集成测试到 `archive/`
  （**仍保留为 workspace members 且仍可编译/可测**，理由与修补见 `archive/README.md`）；
  孤儿文件 `src/keymap.rs`、`src/throw.rs`、`src/pages/*`（不在模块树内、不参与编译）
  已删除；根 `Cargo.toml` 去掉 4 行旧依赖，`Cargo.lock` 相应减 50 行。
  `src/` 现在只有 `lib.rs` / `main.rs` / `keys.rs`。

**R1 的落地要点（写代码时的实际结论）：**

- **`presentation` 的依赖表就是它的边界**：`ecs_core` + `render-api` + `bevy_ecs`，
  没有终端库，所以"把终端细节漏进集成层"在编译期不可能。
- **相机夹取只能做一次**：世界 80×60、视口随终端变化。若让后端各自处理
  "视口比世界大 / 贴边越界"，两个后端必然裁得不一样。`camera::center_on_player`
  统一夹好：`view >= world` 时直接取世界中心，否则把中心夹进
  `[view/2, world - view/2]`，并把目标点取**格子中心**（`+0.5`）。
- **地形三态用并列位图，不用三种 tile 变体**：`MapView.tiles` 全量填地形，
  `visible`/`explored` 是两个并列 `Vec<bool>`。后端自己决定"可见 > 已探索 >
  未知"的画法（TUI 用 `Rgb::dim` 压暗）。
- **同格实体的绘制优先级不能靠遍历顺序**：`entities` 的顺序是 `presentation` 给的
  **稳定顺序**（按 layer/位置/id 排序，便于 golden 测试），不是绘制优先级。
  后端必须显式按 `VisualLayer` 取最大层，再让玩家压过同层——
  `tui/src/render` 里为此写了两条测试（Terrain 层的楼梯 vs Actor 层的怪物）。
- **`SceneFrame` 派生了 `PartialEq`**：golden 测试需要整帧比较（render-api 改动，
  字段都是普通数据，无语义风险）。
- **跨层测试的坑（写测试时踩到）**：`TestBackend` 的缓冲**不能拍平成一个字符串**
  再 `contains("中文")`——CJK/宽字符占多格，拍平后中文会被拆散；而且
  `Paragraph` 的 `Wrap { trim: true }` 会在 CJK 之间插空格。正确做法是逐行拼、
  断言前去空白。这条已写进 `tui/src/render/tests.rs` 的注释。

**MVP 跑通的验证记录（R1-d）：**

`tests/mvp_loop_test.rs` 用真实代码（不 mock）驱动整条链路：
`crossterm::KeyCode` → `dungeon_app::translate_key` → `App::handle` →
`ecs_core::apply_player_command` → 世界推进 → `App::refresh` →
`presentation::extract_scene_frame` → `TuiPlugin::draw` → `ratatui::TestBackend`。

| 用例 | 断言 |
|---|---|
| `mvp_starts_and_produces_a_renderable_frame` | 开局即有可渲染帧；画面上有玩家与标题 |
| `mvp_move_command_advances_the_world_and_the_frame` | 移动命令改变世界位置、帧同步、相机跟住玩家 |
| `mvp_rejected_command_does_not_advance_the_world` | 撞墙命令被拒且玩家仍 `Idle` |
| `mvp_wait_command_works` | 等待可用 |
| `mvp_quit_requires_confirmation` | `q` 只弹框、`Enter` 才退、`Esc` 能取消 |
| `mvp_overlay_swallows_movement_keys` | 覆盖页吃掉移动键，关闭后恢复 |
| `mvp_plays_a_lot_of_turns_without_panicking_or_leaking` | 连续 200 步不 panic、不卡死 |
| `mvp_terminal_keycode_drives_the_world_end_to_end` | 从 `KeyCode` 一路到世界状态与退出流程 |
| `mvp_viewport_resize_is_safe` | 视口从 1×1 到 200×60 都不 panic |

**这一步抓到一个真实设计缺陷（不是测试写错）：** 相机视口此前用的是**终端整体尺寸**，
而地图只占其中一部分（右侧侧栏 + 下方调试面板）。于是相机以为"视口比世界的一半还宽"，
放弃夹取，玩家一走出中心就滚出画面。修法是把布局收成一个函数
（`tui::frame_areas`）**只算一次**，相机视口 = 地图区去边框（`tui::map_viewport`），
并加两条回归断言（自洽 + 不可嵌套）。教训见 **LESSONS.md LTUI4**。

**装配层也因此重构：** `App` 从 `main.rs` 移进 `src/lib.rs`（bin 目标无法被测试导入，
放在 `main.rs` 里等于"主循环接线"永远没人测），`translate_key` 移进
`src/keys.rs`。`main.rs` 现在只剩终端生命周期与主循环，没有一条规则。

#### Phase H — 扩展性加固（⏳ 计划中）

> **动因：** 下一批设计（战斗公式分层、地形代价、装备、技能、危险程度/生物范畴）
> 会同时触及行动链路、怪物表、地图属性与规则层。它们**尚未定值**，但已能确定需要
> 哪些**形状**。本轮只把形状留出来，**不新增任何内容、不改任何数值、不改任何行为**。
>
> **归属纪律：** 形状进 DESIGN / 实现问题进 ISSUES / **数值与内容进 GAME.md（本轮完全不动）**。
> 判据见 **DESIGN.md DsnX16**。

**范围（只碰形状）**

| # | 任务 | 位置 | 内容影响 |
|---|---|---|---|
| H1 | 行动终态收成单一出口：统一 `Ready` 清理 + 保证"恰好一个终态事件" | `action/entity.rs` | 无 |
| H2 | 伤害计算输入结构体化 + 返回因子分解；**公式与数值一律不变** | `combat/mod.rs` | 无 |
| H3 | `build_action_poc_schedule()` 按角色分组（生成器组 / 执行器组各自独立） | `action/entity.rs` | 无 |
| H4 | `TileProps` 静态属性表：收敛 5 处 match；`Tile` 保留为种类键 | `map/mod.rs` | 无（值不变） |
| H5 | 怪物生成的随机源：**只记录 ISSUES（ECS34），本轮不改代码** | — | 无 |
| H6 | 物种信息归位：能力/技能作为**模板的列表字段** | `monster/template.rs`、`world/init.rs` | 无（只换存放位置） |
| H7 | `spawn_weight` 进模板，删除 `spawn.rs` 的三层 match（补完 `DsnE6`） | `monster/` | 无（同值换位置） |
| H8 | 规则修正器接口（`ActorSpeeds` 的推广）：预留"基础值修正 / 乘区修正"两类位 | `action/`、规则层 | 形状定，**填值待 GAME.md** |
| H9 | 格子属性统一读取入口 + 效果实体的位置索引（照 `OccupancyMap`） | `map/`、`resources.rs` | 无 |
| H10 | 技能三层骨架（激活 / 委派 / 行为）+ **一个 dummy 技能**走通 | 新模块 | 形状定，**无真技能** |
| H11 | 世界级效果实体模型 + 一个最小 `StatusEffect` | `ecs_core` | 形状定，**时长口径待定** |
| H12 | 扩展点判据成文（`LECS22`） | 文档 | — |
| H13 | 两类测试纪律：数据表配穷举测试、新组件配"真能被创建"测试 | 测试 | — |
| H14 | "格子身份"判据成文（`DsnE13`） | 文档 | — |

**明确不做（防范围蔓延）**

- **不改 `Map.tiles` 的网格表示**：实测地图在生成后**运行时零写入**，实体化会让每格读取
  从数组索引变成哈希/查询，且 `Tile` 的 u8 序列化是已冻结的存档契约。
- **不改 `Map` 是资源还是组件**：仅当要做多层/多地图时才有收益（见下方待定项）。
- **不写新伤害公式的分区与系数**：接口留位即可。
- **不写 `GAME.md`**：本轮不产生任何数值。

**验收**

| 验收项 | 判据 |
|---|---|
| 行为不变 | `scripts/gate.ps1` 全绿；`ecs_core` 现有 71 测试**全部保持通过**（不删不改断言） |
| H2 | 现有伤害测试全绿 + 新增"返回的因子分解可复算出最终值"测试 |
| H3 | 调度顺序语义不变（现有链路测试全绿），且分组后各是独立扩展点 |
| H4 | 加一个地形属性只改属性表；`Tile` 的 serde 判别值不变 |
| H7 | 权重与现有实现逐值一致（parity 测试） |
| **H10（关键）** | **加一个 dummy 技能，只改"新文件 + 执行器分组一行"**——超过则先修扩展点再写真技能 |
| H11 | 效果实体能被类型化 `Query` 发现；恰好一个 owner 系统负责终结；存档可枚举 |

**依赖顺序**

```text
H1  H3  H4（无依赖、零行为变化）→ H2 → H8 → H6 → H7
                                → H9 → H11 → H10 → H13
                                → H12  H14 成文
```

**待定决策（本阶段只记形状，值待内容阶段）**

| 待定项 | 触发条件 | 归属 |
|---|---|---|
| 伤害公式的分区与系数 | 内容阶段设计公式时 | **GAME.md** |
| 增伤/暴击等分区的叠加方式（加算或乘区） | 同上 | **GAME.md** |
| 地形减速的数值口径与系数 | 定 AV 口径时 | **GAME.md** |
| 危险程度各档的成长系数 | 内容阶段 | **GAME.md** |
| 生物范畴各族的固有性质取值 | 内容阶段 | **GAME.md** |
| 技能子实体是否存档 | 首个真技能时（默认不存，照 §10.7） | DESIGN |
| 状态效果的时长口径（AV 或秒） | 内容阶段 | **GAME.md** |
| 多层 / 多地图是否要做 | 真需要跨图时 | DESIGN |
| `Can*` 是否收窄到"内在能力" | 先实测 `CanBasicAttack` 与 `Attack` 是否一一对应 | DESIGN |

**风险与缓解**

| 风险 | 缓解 |
|---|---|
| 改接口时顺手改了数值 | 验收要求"现有测试不删不改断言全绿"；数值改动一律进 GAME.md |
| H10 骨架过度设计 | 只上 dummy 技能，真技能留给内容阶段 |
| H11 效果实体泄漏 | 每类恰好一个 owner 系统 + 存档可枚举 + 子实体数量断言 |
| 范围蔓延到内容 | 上方"明确不做"清单 + DsnX16 的三步判据 |

### 11.4 测试矩阵

| 测试 | 阶段 | 目的 |
|---|---|---|
| `map_generation_is_deterministic` | A | 同 seed 地图/怪位一致 |
| `player_move_and_blocked_move` | A/C | 移动规则与旧版一致 |
| `attack_applies_damage_once` | A/C | 伤害只结算一次 |
| `monster_death_rewards_exp_and_levels_up` | A | 死亡→经验→升级链路 |
| `fov_memory_occupancy_update` | A | 视野/记忆/占用图 |
| ~~`fast_actor_gets_more_actions`~~ | ~~A/C~~ | **已随 Phase B/C 消失**（全库无此用例，代码与文档均零匹配）。后继为 `faster_monster_gets_its_action_ready_first`（D），但它只断言"快怪先拿 `Ready`"，**不再断言"同等时间预算下行动次数更多"**——该行为声称当前无直接覆盖 |
| `action_entity_poc_round_trip` | B | 生成/仲裁/Tick/执行/完成 |
| `arbitration_priority_and_cleanup` | B/C | 优先级、loser despawn、无残留 |
| `player_action_not_overridden_by_ai` | C | 玩家路径独立 |
| `action_parity_wait/move/attack/chase/flee/wander` | C | 行为 parity |
| `av_is_inversely_proportional_to_speed_and_linear_in_duration` | D | 速度公式（`AV × speed == base_duration`、单调） |
| `clamp_speed_bounds_both_ends_and_non_finite_inputs` | D | clamp 与 NaN/±inf 兜底 |
| `every_monster_template_has_usable_speeds` / `stats_expose_template_speeds` | D | 怪物速度齐全且走到 `stats()` |
| `template_speeds_preserve_legacy_agility_ordering` | D | 迁移保排序（旧敏捷的相对快慢） |
| `generated_actions_read_their_category_speed` / `player_actions_read_their_category_speed` | D | 按行动类别取速度的接线 |
| `faster_monster_gets_its_action_ready_first` | D | 回合顺序场景 |
| `no_action_kind_references` / `no_agility_references` | C/D | 用 grep/脚本作为门禁 |
| `cargo test -p sys` / `cargo test --workspace` | F | 构建门禁 |

### 11.5 提交与文档策略

- 每个 Phase 至少一个 commit；Phase C 建议逐行动 commit，便于回滚。
- 修复 ISSUES 条目后：在条目内标 `✅已修复` + “修复前/修复后” + 位置；必要时加 LESSONS。
- 设计变化：追加 DESIGN.md（不删旧条目）；REFACTOR.md 只在本分支维护，合并前折叠进 DESIGN。
- GAME.md 数值改动用 `[⃞计算]` / `[⃞直觉]` / `[⃞试调]` 标注。
- 每个 commit 前跑对应测试门禁；Phase F 完成后跑全门禁。

### 11.6 决策确认（已按推荐执行）

> **确认记录（2026-09）：** 以下 8 项按推荐执行；第 6 项按用户要求“先记录，Phase E 前逐项确认”。

| # | 决策 | 决定 |
|---|---|---|
| 1 | Phase C 迁移顺序 | **逐行动**（Wait → Move → BasicAttack → Wander → Chase → Flee），每步 parity 测试 |
| 2 | Phase D 速度语义 | **倍率**：`AV = base_duration / speed`，clamp `[MIN, MAX]` |
| 3 | 反应时 | **先删除**；试玩需要时再加统一常数 `BASE_REACTION` |
| 4 | `Wait` | **固定 `WAIT_DURATION`**；后续再评估 `WaitSpeed` |
| 5 | 怪物速度映射 | **先按旧敏捷保行为映射**，再在 GAME.md 用 `[⃞试调]` 重调 |
| 6 | Phase E 死抽象 | **先记录，不删除**；Phase E 前逐项确认是否删除/接线（见 §10.8 / A43） |
| 7 | I87/I88 | **Phase A 后立即修**，恢复 `cargo test` 门禁 |
| 8 | `core` 改名 | **已执行（F5）**：改名 `ecs_core`，消除与标准库 `core` 的遮蔽；见 DESIGN DsnX15 |

**下一步：** Phase E 与 F3/F4 均已完成（见 §11.3 Phase E / F）；当前下一步是 **Phase H 的 H1 / H3 / H4**（见 §11.9）。

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

| 阶段 | 预估 | 实际 |
|---|---|---|
| A | 0.5–1 天 | ✅ 完成（commit `95449a9`） |
| B | 0.5–1 天 | ✅ 完成（commit `ac6623f`） |
| C | 2–3 天 | ✅ 完成（C1–C9，commit `83700ff`…`a8e2175`） |
| D | 1–2 天 | ✅ 完成（D1–D5，commit `cff5940`…） |
| E | 0.5–1 天 | ✅ 完成（含 `system/`、`monster/` 结构拆分） |
| F | 0.5–1 天 | ✅ 完成（F3/F4 clippy 清零 + `scripts/gate.ps1`；F5 改名 `ecs_core`） |
| G | 1–2 天（TUI 解耦） | ⏳ R1 完成、**R5 完成**；R2 部分；R3/R4 待续 |
| **H** | 2–3 天（只碰形状） | ⏳ 计划已落地（文档 `55d8086`），代码待开工 |
| **R5（旧 crate 归档）** | 0.5 天 | ✅ 已完成：见 §11.3 Phase G 的 R5 行与 `archive/README.md` |

### 11.9 下一步（Phase H 的开工顺序）

`H1` / `H3` / `H4` 之间无依赖且**零行为变化**，因此先做这三项；它们各自解掉一个
"下一个 `ActionKind`"（`ECS35`/`ECS36`、技能执行器挂载点、`ECS31`）：

```text
本轮：H1（行动终态单一出口）→ H3（schedule 分组）→ H4（TileProps 属性表）
      ↓ 每项收尾都跑 gate.ps1，且 ecs_core 71 测试不删不改断言全绿
下一轮：H2（伤害输入结构体化）→ H8（规则修正器）→ H6 / H7（物种信息归位）
      ↓
再下一轮：H9 → H11 → H10（dummy 技能，关键验收）→ H13 / H12 / H14 成文
      ↓
独立轮次：R3（bevy_app 宿主，网络已可用——原"环境无外网"的阻塞已消失）
```

---

## 12. 渲染后端插件化（摘要，详见 DESIGN DsnX14）

> **状态：** 草案；`render-api` v1 已落地。正式内容见 **DESIGN.md DsnX14**；本节只保留分支内摘要。

**依赖方向**

```text
core ──> presentation ──> render-api <── tui / gpu
                              ▲
                       dungeon-app（装配 + runner 选择）
```

- `render-api`：`SceneFrame` / `VisualKey` / `UiView` / `InputEvent` / `SurfaceInfo`；不依赖 core/ratatui/wgpu。
- `presentation`：core → `SceneFrame` 提取、`VisualKey` 映射、camera、`PageStack`/UI 状态、输入映射。
- `tui` / 未来 `gpu`：只消费 `render-api`，**不依赖 core**。
- `dungeon-app`：按 feature 添加 `TuiPlugins` 或 `GpuPlugins`，选择 runner。

**插件**

| 插件 | 职责 |
|---|---|
| `CorePlugin` | Startup 初始化；Update 消费 `PlayerCommand`、推进、结算 |
| `PresentationPlugin` | PostUpdate 提取 `SceneFrame` + camera；UI/日志状态 |
| `InputMapPlugin` | PreUpdate 输入路由；PageStack → UiAction / PlayerCommand；tap-tap |
| `SysInputPlugin` | sys 键盘线程 → `InputQueue` |
| `TuiPlugin` | 终端生命周期 + `TuiCatalog` + Last 绘制 |
| `GpuPlugin`（未来） | winit + wgpu/Bevy；消费同一 `SceneFrame` |
| runner | TUI: `ScheduleRunnerPlugin::run_loop(33ms)`；GPU: winit/自定义 runner；二者互斥 |

**迁移阶段**

| 阶段 | 内容 |
|---|---|
| R0（已完成） | `render-api` v1（34 tests） |
| R1 | `presentation` 提取层；`tui` 去 `core` 依赖；`TuiPlugin` 消费 `SceneFrame` |
| R2 | 页栈 UI（Game/Dialog/Look 优先；Inventory/Throw 等 core 迁移） |
| R3 | `bevy_app` 宿主 + `ScheduleRunner`；替换 main 循环 |
| R4 | GPU 后端；同一 `SceneFrame`，core 零改动 |
| R5 | 清理旧 `dungeon-*` / `src/pages`；文档同步 |

**开放决策**

- `bevy_app` 0.16 真实依赖 vs 离线 shim；
- GPU 路线：完整 Bevy renderer vs 独立 `wgpu`；
- UI 模型粒度：页面级 `UiView` 先行；
- `tui` / `sys` 边界；输入轮询保持在 `sys`/`SysInputPlugin`；
- 编译期 feature vs 运行时后端选择；
- crate 命名。

**执行时机：** ✅ R1 与 **R5** 已落地（Phase G）；R2–R4 待续。详见 §11.3 Phase G。

> 原则：**每个保留的抽象必须有真实读取方/消费者；否则就是下一个 `ActionKind`。**
---

## 13. 文档体系：按 crate 拆分（编号公约）

`ISSUES.md` / `LESSONS.md` / `DESIGN.md` 三份文档**每个 crate 各一份**，根目录保留一份
记协同内容（见 [RULE.md](RULE.md) §六）。

### 13.1 为什么拆

三份文档原本是单文件，随项目增长变成"全局大杂烩"：查一个 crate 的问题要先在 2300 行里
找，而**改 A crate 的人被迫读 B/C/D crate 的历史**。拆开之后：

- 每个 crate 的三份文档**就是它自己的维护清单**，与代码同目录，改代码时顺手看得到；
- 旧架构（`dungeon-*` / `src/pages`）的大批历史条目随重构被清理，不再稀释有效信息；
- 编号自带来源：看到 `ECS7` 就知道去 `ecs_core/ISSUES.md` 找。

### 13.2 编号公约

**每个 crate 的每份文档独立编号，前缀是该 crate 的三字母缩写**：

| 位置 | 缩写 | 示例 |
|---|---|---|
| 根目录（协同） | `SYN` / `LSYN` / `DsnX` | `SYN1`（协同问题）、`LSYN1`（通用教训）、`DsnX1`（跨 crate 决策） |
| `ecs_core/` | `ECS` / `LECS` / `DsnE` | `ECS7`、`LECS21`、`DsnE8` |
| `presentation/` | `PRE` / `LPRE` / `DsnP` | `PRE1`、`LPRE1`、`DsnP1` |
| `render-api/` | `API` / `LAPI` / `DsnA` | `API1`、`LAPI1`、`DsnA1` |
| `tui/` | `TUI` / `LTUI` / `DsnT` | `TUI1`、`LTUI4`、`DsnT1` |
| `sys/` | `SYS` / `LSYS` / `DsnS` | `SYS1`、`LSYS2`、`DsnS1` |
| `utils/` | `UTL` / `LUTL` / `DsnU` | `UTL1`、`LUTL4`、`DsnU1` |

规则：

- **序号只增不改**；条目被删除后其编号**不回收**（避免历史 commit / 对话里的引用指错）。
- 跨文件引用要**带前缀**（`见 LECS21`），不要写裸 `L21`——后者在另一个 crate 里可能是别的意思。
- 每份文档里保留一条 `**原编号：**`（迁移前的全局编号），供追溯历史 commit 与旧对话。

### 13.3 归属判据

| 文档 | 判据 |
|---|---|
| `ISSUES.md` | 问题**发生在哪个 crate 的代码里**。跨 crate 协同、工具链/流程问题、**迁移动因**留根目录。 |
| `LESSONS.md` | 教训的读者是 AI：**在任何 crate 都适用**的原则（语言、工具链、流程、测试方法论、通用 ECS 用法）留根目录；只在某个 crate 的代码里有落点的归该 crate。 |
| `DESIGN.md` | 决策的**落点在哪个 crate 的代码/接口**里就归哪里；跨多个 crate 的分层/契约/迁移路线决策留根目录。 |

### 13.4 迁移记录

拆分于 Phase G 之后一次性完成（commit 见 `git log --grep="文档按 crate 拆分"`）：

| 文档 | 迁移前 | 迁移后 |
|---|---|---|
| `ISSUES.md` | 226 条（A/D/G/I/P/R 六前缀混编） | 44 条保留（`SYN` 13 + `ECS` 26 + `TUI` 2 + `SYS` 3），**168 条删除**（旧 `dungeon-*` / `src/pages` 专属，代码将随 R5 移除，问题不会重现） |
| `LESSONS.md` | 51 条（L1–L51） | 51 条全部保留（`LSYN` 20 + `LECS` 21 + `LTUI` 4 + `LSYS` 2 + `LUTL` 4） |
| `DESIGN.md` | 29 条（Dsn1–Dsn29） | 29 条全部保留（`DsnX` 15 + `DsnE` 8 + `DsnP` 2 + `DsnS` 2 + `DsnA` 1 + `DsnT` 1） |

**删除的判据（ISSUES）：** 条目描述的代码已不存在、且**不会重新出现**——旧 UI 页
（`src/pages/*`、`dungeon-render` 的渲染/投掷/背包链路）、旧领域模型（`Stats` / `ActionKindV3` /
`ActionQueue` / `ModalKind` / `Reaction` / `InputBuffer` / `ActiveCooldowns` / `SavedStats` 等）。
它们的问题在重构后由新架构的对应条目覆盖（例如「存档静默丢字段」类 → `SYN9` 与 `DsnX10` 的兼容性规则）。

**教训的归属调整：** 少数教训的归属与直觉不同，值得说明——
`LTUI4`（布局函数不幂等）虽然是一般性教训，但落点在 `tui::frame_areas`，所以归 `tui`；
`LSYN1`（`query` vs `try_query`）虽是 Bevy 用法，但它约束的是**领域层**的查询写法，归 `ecs_core`。