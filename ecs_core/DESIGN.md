> **⚠️ 修改前必须阅读或回忆 [RULE.md](../RULE.md)——它定义了本文档的维护规则和更新时机。**
>
> 每条记载一个设计取舍，含两个小节：
> - **决策**：当前选的方向和理由（罗盘）
> - **背景**：舍弃的方案和演化过程（护卫）
>
> 数值和游戏规则见 [GAME.md](../GAME.md)。

# 设计决策记录 —— ecs_core

**归属范围：** 领域规则引擎的决策：ECS 模型、行动/速度、地图、怪物、结算链路。

**编号：** `DsnE1`、`DsnE2`… 每个 crate 独立编号。
判据：决策**落点在哪个 crate 的代码/接口**里就归哪里；跨多个 crate 的分层/契约/路线决策留根目录。

---

## 主题索引（语义概览，便于按主题跳转）

> 条目正文按编号顺序排列（编号是稳定 ID，不复用）；本表按**主题**归类，
> 与 `&ID` 标记配合使用：`grep '^<!-- &'` 可一次列出全部条目及其一句话结论。

| 主题 | 条目 |
|---|---|
| 调度与并行 | DsnE1 |
| 行动系统 | DsnE4（已并入）· DsnE8 · DsnE9 |
| 规则与效果 | DsnE10 · DsnE11 · DsnE12 |
| 空间与地图 | DsnE2 · DsnE3 · DsnE7 · DsnE13 |
| 数据与随机 | DsnE5 · DsnE6 |


<!-- &DsnE1  怪物决策走并行 Schedule，不串行遍历；可调试性优先于整洁 -->
### DsnE1 并行怪物决策（Schedule）

**决策**

三种怪物行为（追击、逃跑、游荡）的检查条件**没有数据竞争**，通过 bevy Schedule 并行执行：

```
chase_decision_system  ─┐
flee_decision_system   ─┤ 并发 → arbitration_system → ActionQueue
wander_decision_system ─┘
```

适当的并行度是 3（三种行为）+ 1（仲裁）。太少浪费 CPU，太多调度开销超过收益。

**意图缓冲区模式**：三个决策 system 各自写入独立的 `ChaseIntents`/`FleeIntents`/`WanderIntents` 资源，仲裁 system 串行合并。生产者-消费者模式无并发写冲突，数据流显式可追踪。

**背景**

旧设计 `run_monster_decision()` 串行遍历所有怪物，每个怪物检查条件→仲裁→入队，O(n) 且无法并行。保留串行兼容入口供调试——"可调试性比代码整洁更重要"。

---

**原编号：** `DsnE1`（迁移前）
<!-- &/DsnE1 -->

<!-- &DsnE2  占用图是独立资源，不进 Map；每轮重建 -->
### DsnE2 碰撞图（Occupancy Map）独立于 Map

**决策**

两个关注点变化频率不同，分离为独立结构：
- `Map`：存 Tile 地形（`Wall`/`Floor`）——楼层生成后基本不变
- `OccupancyMap`：存每个格子被哪个实体占据——每次移动/攻击/死亡后都变化

合并到一个结构意味着每次更新都要复制地形数据。

**背景**

每次行动后全量重建 `rebuild_occupancy()` O(n) ≈ 800 格（40×20），开销 <1μs。增量更新容易漏边界条件（实体死亡、下楼、传送），维护正确性的心智负担远高于全量重建的成本。

---

**原编号：** `DsnE2`（迁移前）
<!-- &/DsnE2 -->

<!-- &DsnE3  视野记忆拆两个结构：已探索(MapMemory) 与 此刻可见(VisibleMemory) -->
### DsnE3 视野记忆双结构（MapMemory + VisibleMemory）

**决策**

两个独立的记忆结构，因为它们的数据类型、更新频率、生命周期不同：
- **MapMemory**：记录哪些格子曾经被看到过（boolean 数组）——渲染已探索区域的灰色墙壁/地板。只增不减（探索过的格子不会"遗忘"）
- **VisibleMemory**：记录最后看到的实体（glyph/color/位置）——渲染视野外但已知的实体。需清理已死亡实体（否则显示幽灵）

**背景**

如果合并为一个结构，cleanup 逻辑需要区分"地图记忆（不清除）"和"实体记忆（需清除）"，增加复杂性。视野外的实体在已探索区域以灰色显示。死亡实体自动清理。

---

## 二、核心机制

**原编号：** `DsnE3`（迁移前）
<!-- &/DsnE3 -->

<!-- &DsnE4  已并入 DsnE8；保留编号供历史引用解析 -->
### DsnE4 行动系统设计哲学

> **本条目已并入 DsnE8「行动系统：设计哲学 + 行动即实体 + 速度组件」（2026-09 合并）。**
>
> 理由：DsnE4（哲学）与 DsnE8（行动即实体 + 速度组件）是同一个子系统的两半——分开写时，
> 读者要先读 DsnE4 的"行动队列 + 反应时 + 敏捷"再读 DsnE8 的"行动子实体 + 倍率速度"，
> 而前者有两处已被 Phase D 推翻（反应时删除、`Agility` 删除），却没有一句"已被取代"的标记。
> 合并后 DsnE8 是行动系统的**唯一权威条目**：设计哲学 → 具体决策 → 落地记录。
>
> **原文完整保留在 DsnE8 的「设计哲学（原 DsnE4）」一节**（RULE §五：不删旧记录），
> 并就地更正了被实现推翻的两处。本编号保留、不回收，供历史 commit 与旧对话引用解析。

---

**原编号：** `DsnE4`（迁移前）
<!-- &/DsnE4 -->

<!-- &DsnE5  GameRng 是唯一随机源；派生 RNG 必须对 (seed,floor) 确定 -->
### DsnE5 线程局部 RNG → GameRng 统一

**决策**

`GameRng` 成为唯一随机源。所有随机操作（仲裁、暴击、游荡、掉落、地图生成）统一走 `GameRng` 或基于 `MapSeed` 的派生 RNG。

**背景**

曾经有一个线程局部的 `RefCell<SmallRng>` 与 `GameRng` 并存，用于仲裁 system 中的随机选择——因为当时仲裁 system 无法访问 `GameRng` 资源。已由 ISSUES D1 解决。

---

## 四、输入与渲染

**原编号：** `DsnE5`（迁移前）
<!-- &/DsnE5 -->

<!-- &DsnE6  物种数值收进模板表；能力与生成权重仍待归位(ECS32，含原 ECS33 的 DTO 问题) -->
### DsnE6 MonsterTemplate 结构体统一 — 设计中，实验方向

**决策（暂缓执行）**

将 `monster_def.rs` 中 7 个分散的 match 函数（`monster_glyph`、`monster_color`、`monster_name`、`monster_attack_name`、`monster_stats`、`monster_loot`、`monster_spawn_weight`）合并为一个 `MonsterTemplate` 结构体 + 一个查表函数。

```rust
pub struct MonsterTemplate {
    glyph: char,
    color: (u8, u8, u8),
    name: &'static str,
    attack_name: &'static str,
    // 系数——不存最终值，存公式参数
    hp_base: i32, hp_per_floor: i32,
    atk_base: u32, atk_per_floor: f64, atk_max: u32,
    def: u32, agi: u32, magic_mastery: u32,
    exp_base: f64, exp_per_floor: f64,
    loot: &'static [LootEntry],
    spawn_weight_base: f32, spawn_weight_per_floor: f32,
    spawn_weight_min: f32, spawn_weight_max: f32,
}

impl MonsterTemplate {
    pub fn stats(&self, floor: u32) -> Stats { /* 统一公式 */ }
    pub fn spawn_weight(&self, floor: u32) -> f32 { /* 统一公式 */ }
    // glyph/color/name 等退化为字段直接访问
}
```

收益：
- 加新怪物从改 7 个 match 变为加 1 行数据
- 对外的 `monster_glyph(kind)` 等函数退化为 `template(kind).glyph` 等字段访问
- 未来切换到 JSON 配置驱动只需改 `fn template()` 的加载源，调用方不变

**当前状态：** 暂缓。当前 `monster_stats` 等函数中的公式是直接赋值（裸数值），尚未抽象为统一的系数+公式体系。GAME.md 的数值标注体系（`[⃞计算]/[⃞直觉]/[⃞试调]`）也未完成。在公式体系设计就绪前，硬套系数反而引入硬编码的假灵活性。先记录方向，等 GAME.md 完成后再实施。

**关联：** GAME.md Gm8（怪物设计）、LESSONS.md（无直接关联）

---

**原编号：** `DsnE6`（迁移前）
<!-- &/DsnE6 -->

<!-- &DsnE7  三类型地图共用同一骨架，只做环境修饰；取值在 GAME Gm10 -->
### DsnE7 多类型地图：只做环境修饰，不引入新算法（内容已迁 GAME）

> **归属变更（2026-09）：** 本条原本把**环境参数、新方块属性、生态对应与掉落**都写在
> DESIGN 里——那违反 RULE §五「DESIGN 不包含具体游戏数值」。内容已按 DsnX16 的三步判据
> 迁到 **GAME.md**：
>
> | 原内容 | 现在的位置 |
> |---|---|
> | 环境参数表（`MapEnvParams` 六个参数 × 三类型） | GAME.md **Gm10**「地图类型与环境参数」 |
> | 新方块 6 种的 glyph/颜色/可走/挡视线 | GAME.md **Gm10**「环境 Tile」（11 行完整表） |
> | 生态对应（新怪 5 种的 HP/攻/掉落） | GAME.md **Gm8**「基础属性」/「掉落表」（本来就有，此处不再复制） |
> | 地形消耗品 2 种（蘑菇/海藻） | GAME.md **Gm7**「消耗品」 |
> | 通道挖掘的水域保留规则 | GAME.md **Gm10**「地图类型与环境参数」（属规则，保留为文字） |
>
> DESIGN 只保留**决策**（下面三条），不再持有数值。

**决策**

1. **多类型地图不引入新生成算法。** 三类型共用同一个 `room_accretion` 洞穴骨架，
   只做**环境修饰差异化**（水域规模、障碍种类、装饰方块）。理由：新算法意味着第二套
   连通性/出生点/楼梯保证要各自证明一遍，而玩家能感知的差异几乎全在环境修饰上。
2. **类型由 `(MapSeed, floor)` 确定性派生。** 存档只存种子，读档后当前层与下楼结果一致
   ——**存档零改动**（DESIGN DsnX17 的确定性约束）。派生公式的**取值与分布**属 GAME.md。
3. **`Tile` 只追加、不重排。** 新增方块走 serde 判别值末尾追加（DESIGN DsnX17）；
   `Tile` 保留为种类键，属性收敛在 `TILE_PROPS` 静态表（Phase H4 / DsnE13）。

**明确不做 / 暂缓**

- 多分支楼梯（树状分支）暂缓：等类型系统稳定后再规划"类型分配"。
- 不为某类型单独做地形算法、单独做连通性保证。

**关联：** GAME.md Gm10（环境参数/方块/生成管线）、Gm7、Gm8 | ISSUES ECS15（种群补足）/ ECS22（通道 4 方向）/ ECS26（楼梯落点） | REFACTOR §1（Phase H4） | DsnE13（格子属性身份判据）

**状态：** 已定 — 已落地（三类型 + 6 新方块 + 5 新怪 + 8 新掉落）；**内容已迁 GAME.md**，本条只保留决策。

---

**原编号：** `DsnE7`（迁移前）
<!-- &/DsnE7 -->

<!-- &DsnE8  行动=actor 瞬态子实体；AV 走倍率；终态单一出口；保活覆盖缺口=ECS43 -->
### DsnE8 行动系统：设计哲学 + 行动即实体 + 速度组件

> **行动系统的唯一权威条目**（2026-09 合并了原 DsnE4）。
> 结构：设计哲学 → 决策（① 行动即实体 / ② 速度组件）→ 落地记录 → 必须守住的不变式。

#### 设计哲学（原 DsnE4，已就地更正被实现推翻的两处）

**组件式行动授权：** 能力由 `Can*` 组件赋予。系统遍历检查哪种行动的条件满足；
加新行为 = 加新组件 + 加新 system，**不修改决策流程**。

**AV 是单一数值，没有独立冷却系统**：~~`AV = 反应时 + 耗时 × 修正系数`~~
→ **已被 Phase D 推翻**：`Agility` 与反应时一并删除，现行唯一口径是
`AV = base_duration / clamp(速度倍率, 0.25, 4.0)`（见 ② 与 GAME.md Gm1）。

**事件驱动推进：** 每轮只推进「最小正 AV」，而不是固定帧 tick。

**保活检查：** 生成时检查一次条件，且**必须对所有待执行行动持续生效**——失效的行动会继续
占着 actor 唯一的 `ActiveAction` 槽位，让 actor 空转若干回合。
**现状只在执行前检查（每个执行器各一份）→ 这是缺口，见 ISSUES ECS43。**

**优先级仲裁：** 每个行动自带 `ActionPriority`（越小越优先：Flee 200 > Chase 100 >
Wander 50 > Wait 0），比较器是 `(ActionPriority, entity.to_bits())` 的**全序**，不含随机数。

**无前摇 / 无后摇：** 反应时删除后，出手前的固定延迟目前不存在；若试玩需要，加回一个
统一常数，**不要恢复按敏捷分档**。

**无预测轴 / 无锁定轴：** 预测需要模拟未来世界状态（克隆 World 或回滚日志），收益不抵成本。

**行动系统的三个问题**（本质不是"谁先执行的顺序器"）：**选什么行动**（生成 → 仲裁）、
**何时做**（AV 倒计时）、**做多久**（AV 组成）。

**被否掉的方案**

| 方案 | 为什么否掉 |
|---|---|
| 独立冷却计时器 | 与队列推进构成两个时间维度，必须保持同步（历史上多次"冷却空转"fix）；AV 合一后冷却作为独立概念消失——需要冷却的效果改调行动耗时 |
| 行为树（脑链） | 一次只执行一条链、顺序判断；组件式条件是并行的，可批量检查、完全只读 |
| 独立决策组件 | 把分散在各行动组件里的决策权收回一个中心，降低可扩展性 |
| 状态机 | 状态把"状态"与"行动"耦合（"进入追击模式 → 执行 CanChase"），决策权从组件被抢到状态机系统 |
| 预测轴 / 锁定轴 | 与组件式决策不兼容：系统只能知道"此刻条件满足"，无法知道"下一帧是否仍满足" |

#### 决策

**① 行动即实体（替代 `ActionKind`）**

- 一个行动 = 一个 actor 的**瞬态子实体**，不再用中央 `ActionKind` enum 分派。
- 组件：归属关系 `ActionOf(actor)`（专用关系，**不是**通用 `ChildOf`，见 DsnE12 / ECS41）、
  `ActionPriority`、`ActionTimer`、`ActionSource`、生命周期标记 `Candidate` / `ActiveAction` /
  `Ready`，以及具体行动 ZST/payload（`Wait` / `Move { dx, dy }` / `BasicAttack { target }` /
  `Chase` / `Flee` / `Wander`）。
- `Can*` **保持 actor 上的 ZST 组件**，不子实体化：能力回答"能不能做"，action 实体回答
  "正在考虑/执行什么"。
- 保留生成/仲裁分离（OCP 扩展点）；普通怪 = 新模板/Bundle，加怪不改 AI/生成/执行代码。
- 流程：生成只 spawn 候选 → 仲裁唯一写 actor 行动状态 →
  tick 推进 AV 并加 `Ready` → 执行按行动类型专用 query（零中央 match）→
  completion 消费终态事件并回收 action 实体。
- 玩家行动直接生成 active action，不进入 AI 仲裁。

**② 速度组件（替代 `Agility`）**

- 删除 `Agility`；`MoveSpeed(f64)` / `AttackSpeed(f64)` 是**倍率**（1.0 基准，越高越快）。
- `Move/Chase/Flee/Wander → MoveSpeed`；`BasicAttack → AttackSpeed`；
  `Wait` 固定 `WAIT_DURATION`（不吃速度）。
- 反应时与 `BASE_REACTION` 一律不保留；试玩需要时再加统一常数。
- 未来武器速度 → `AttackSpeed`；重甲/地形 → `MoveSpeed`（走修正器，见 DsnE10）；
  Buff/装备可动态增删组件。

**背景**

`ActionKind` 中央 enum + `mount_action` match 让新增行动要改多处；`ActionTimer` 从未参与
执行门禁（I89）、事件因 Schedule 重建 + 缺 `Events::update()` 被重复读取（I90）——
这两项先修，否则速度组件不会真正影响行为。

**代价**

- action 实体带来每轮候选 spawn/despawn 的 churn；规模上去可改持久 `ActionSlot` 或池化。
- 归属关系是 `linked_spawn`：actor 被 despawn 时会级联回收行动实体——**这是我们要的**
  （actor 死了不留残留），但也意味着不能把"要活过拥有者"的东西挂成子实体（ECS42）。
- 每个事件都必须有真实消费者；注册了没人读的事件就是下一个死抽象（ECS25）。

#### 落地记录

| 阶段 | 结论 | 提交/证据 |
|---|---|---|
| A 冒烟测试 | 地图确定性 / 移动 / 攻击只结算一次 / 死亡→经验→升级 / FOV·记忆·占用图 | `95449a9` |
| B PoC | 生成→仲裁→tick→执行→completion 全链路可行，无残留子实体 | `ac6623f` |
| C 全量迁移 | 六行动逐行动 parity；`ActionKind` / `mount_action` / 旧独占执行系统全删；主循环改「先挂载、再推进」两段式 | `83700ff`…`a8e2175` |
| D 速度组件 | 倍率口径落地；`AV × 速度 == base_duration` 恒成立；迁移保排序由 `template_speeds_preserve_legacy_agility_ordering` 钉住 | `cff5940`… |
| H1 终态出口 | `ActionEvents::succeed/fail` 字段私有 →「恰好一个终态事件」从约定变类型保证；变异验证（去掉 `remove::<Ready>()`）→ 7 个用例立刻失败 | `f1f6898` |
| H3 调度分组 | `ActionPhase` 五角色（Generate/Arbitrate/Tick/Execute/Complete），加执行器 = 加一行 | `5a9ea90` |

#### 后续工作必须守住的不变式

- **挂载与推进必须两段调度**（`PlayerMountSchedule` → 推进循环）：合成一次会把
  "本轮已执行完"误判成"命令被拒绝"。
- **`Idle` 与 `Failure` 必须互斥**（I91）：任何写终态的地方都要先清掉另一个。
- **执行器是普通参数化系统**：落点规则是纯函数（`moved_position`），不需要 exclusive
  `&mut World`——"必须 exclusive"那个判断是错的（写测试时踩过的坑见 LECS3 / LSYN20）。
- **元组 `Bundle` 上限 15 元**：打包成 `#[derive(Bundle)] struct Speed`，
  **打包不是新增组件**（见 LECS3）。
- **执行器不读速度**：AV 在挂载时就固化进 `ActionTimer`，速度只由 `MoveSpeed`/`AttackSpeed`
  经 `SpeedRule` 在挂载点决定。
- **终态只经一个出口**：执行器不得自己 `despawn` 实体或写 `Idle`/`Failure`，
  那是 `action_completion_system` 的职责。

**关联：** DsnE9（伤害输入形状）/ DsnE10（修正器求值侧）/ DsnE12（效果实体与归属关系）
| DESIGN DsnX17（冻结清单）/ DsnX18（路线表）
| ISSUES ECS41（`ChildOf` → 专用关系）、ECS43（保活覆盖范围，未闭合）
| LESSONS LECS3（Bundle 上限）、LECS22（扩展点判据）、LSYN20（测试依赖执行顺序）

**状态：** 已定 — ①② 与 H1/H3 均已落地；**唯一未闭合的设计缺口是保活检查的覆盖范围（ECS43）**。

**原编号：** `DsnE8`（迁移前）
<!-- &/DsnE8 -->

<!-- &DsnE9  伤害改成结构化输入 + 因子分解返回；公式与数值不变 -->
### DsnE9 伤害计算的输入接口：从标量参数组到结构化输入

**决策**

`combat::compute_melee_damage` 的**签名结构**改为结构化输入，**但公式与数值一律不变**（仍是 `max(攻击 − 防御, 1)` 与现有暴击逻辑）。本条目只定**接口形状**，不产生任何游戏数值。

**背景**

当前签名是：

```rust
pub fn compute_melee_damage(
    attack: f64, defense: f64, crit_rate: f64, crit_damage: f64, crit_roll: f64,
) -> MeleeResult   // { damage, is_crit }
```

这个形状**恰好表达"减一次防、乘一次暴击"**：每新增一个伤害修正都要改签名 + 改所有调用点，而且返回类型是黑箱（只有最终值与是否暴击，没有各因子的分解）。

**决策内容（只定形状）**

1. **输入分组**：参与伤害计算的量按"来源"分组传入，而不是一串裸标量。至少要能区分：**攻击方数值**、**受击方数值**、**随机输入**（`crit_roll` 一类）。
2. **返回分解**：返回结果携带**各因子的值与最终值**，而不只是最终值。理由有三个真实消费者：战斗日志要能解释"为什么是这个数"、调试面板、以及未来"按因子触发"的效果。
3. **随机数仍在系统层取**：`crit_roll` 由调用方从 `GameRng` 取（沿用现有做法），公式保持纯函数、可单测。

**明确不做**

- **不新增、不修改任何公式与系数**。分区（有哪些因子、各因子叫什么）与系数（防御系数等）**全部待定，见 GAME.md**——本条目只保证这些因子**有位置可放**。
- 不引入"伤害类型"等新概念；那属于内容设计。

**为什么现在做**

伤害公式是**承重层**：装备、Buff、技能、生物范畴的固有性质最终都要落到这个函数的输入或因子位置上。先让形状能容纳"多因子"，后续内容设计就不必再改接口。

**关联：** ISSUES ECS29（现签名无扩展位） | REFACTOR §1（Phase H2） | GAME.md（分区与系数，待定）

**状态：** 已定 — ✅ **已落地**（Phase H2）。实现落在 **`ecs_core/src/rules/damage.rs`**（不是 `combat/`）
——`combat` 现在只管**位置与目标**类规则（能不能打、8 方向相邻、目标存活），
数值类规则统一进 `rules`：

| 决策 | 落地形态 |
|---|---|
| 输入按来源分组 | `MeleeInput { attack, defense, target_crit: CritProfile { rate, damage }, crit_roll }` |
| 返回因子分解 | `MeleeBreakdown { base, crit_multiplier, damage, is_crit }` + `recompute()` |
| 随机数仍在系统层 | `crit_roll` 参数不变；`rules::melee` 保持纯函数 |

**兼容与验收：**

- `combat::compute_melee_damage` 保留为**薄适配器**（内部转调 `rules::melee`）：
  结算链路（`system/combat.rs`，唯一调用点）不必为改形状而改动；
- 「公式与数值不变」由三条口径用例钉住：`damage_floor_is_one`、
  `crit_threshold_is_strictly_greater`（`>` 而非 `>=`）、
  `crit_multiplier_is_neutral_or_one_plus_bonus`（负数加成吃 `max(0.0)`）；
- 适配器另有一条字段映射回归（`legacy_scalar_adapter_maps_fields_correctly`，
  用 `to_bits` 比较以正确处理 NaN）；
- `factors_reconstruct_the_final_damage`：`base × crit_multiplier` 逐位等于 `damage`
  ——这条让"返回因子分解"成为**可断言的性质**，而不是一堆字段。

**仍然待定（属 GAME.md，本阶段一个都没填）：** 分区（有哪些因子）、系数（防御系数、
增伤叠加方式）。要改成加权形式时 `MeleeInput` 已是"按来源分组的输入"，加字段即可，
不必再动签名与调用点。

---

**原编号：** `DsnE9`（迁移前）
<!-- &/DsnE9 -->

<!-- &DsnE10  效果载体是实体；求值侧五步：收桶→过滤→折叠→纯函数 -->
### DsnE10 规则修正器：装备/状态/地形如何影响纯函数规则

> **本条目经历了一次改写（2026-09 讨论，尚未落地）。** 初版只把修正器定义为
> 「`ActorSpeeds` 的推广」——**只覆盖"给某个数值算一个修正"**，因此碰到
> "无视某一类来源"时只能**在求值点写特例**，违反 DsnE12 第 4 条。
> 改写后：修正器的**载体是效果实体**（DsnE12），本条目只管**求值侧的形状**——
> 「怎么把效果折算成纯函数的输入」。原版的四条（求值位置、两类位、pull、`Can*` 例外）
> 全部保留，只是从"整个设计"降为"求值侧的四条实现约束"。

**决策**

规则修正分**两侧**，两侧都有明确归属，不混在一起：

| 侧 | 回答什么 | 机制 | 归属 |
|---|---|---|---|
| **载体侧** | "这个效果**存在**于哪里、由谁终结" | 效果实体 + 挂载关系 / 格索引 | **DsnE12** |
| **求值侧** | "效果**折算成**纯函数的什么输入" | 分桶收集 → 过滤 → 折叠 → 纯函数 | **本条目** |

**求值侧的形状（五步，顺序即语义）**

```text
① base       ：actor 上的权威数值（如 MoveSpeed），不是"最终值"
② 收桶       ：actor 名下的效果子实体  +  当前格索引里的效果  →  两个来源，一份列表
③ 过滤       ：丢掉 `ignored` 里列出的来源类别        ← "无视某类减速"只作用在这一步
④ 折叠       ：先基础值修正（加/减），再乘区修正（独立因子相乘）
⑤ 纯函数     ：规则只吃第 ④ 步的结果（既有范例 ActionSpeeds / action_av）
```

**四条实现约束（初版原样保留）**

1. **修正器在系统层求值，纯函数只吃结果。** 既有范例是 `ActorSpeeds`：
   `actor_speeds()` 在系统层查 `MoveSpeed`/`AttackSpeed`，`action_av` 只吃一个纯数据
   `ActorSpeeds`。**新修正器一律照此形态**——第 ①②③④ 步都在系统层完成，
   第 ⑤ 步的函数保持"位置无关、可单测"。
2. **修正器预留两类位**（形状，不是内容）：**基础值修正**（改变数值本身）与
   **乘区修正**（作为独立因子相乘）。第 ④ 步的顺序（先加后乘）是**形状**；
   具体哪类效果进哪一位、系数多少属 GAME.md（待定）。
3. **读取方式是 pull，不是 push。** 效果在**规则求值时查询**，而不是在装备/生效时
   往 actor 上插组件。理由：push 要求"穿上/脱下/丢弃/被偷/过期"每条改变途径都对称地
   插入与移除组件，**漏一条就是静默残留**；pull 只付一次查询。
   > **与效果实体的关系**：这条正是"效果用实体承载"的另一半理由——实体可以被
   > **恰好一个 owner 系统**终结（DsnE12 第 3 条），而 push 的组件必须由每个改变途径
   > 各自负责移除。
4. **内在能力仍用 push（`Can*`）。** `Can*` 是实体固有属性，生命周期与实体一致，
   不存在不对称问题——**两条路并存，判据是"能力是固有的还是可授予的"**。

**"无视某类减速"的正确定形**

它是第 ③ 步的**过滤**，不是"实体与实体之间的父子关系"，也**不是**规则里的特例分支：

```rust
// 纯函数：位置无关、可单测。规则里永远不出现"若带了某装备则……"。
fn apply_modifiers(base: f64, mods: &[Modifier], ignored: &[EffectSource]) -> f64

struct Modifier {
    source: EffectSource,   // 形状：来源桶（下表）
    base_delta: f64,        // 基础值修正位
    multiplier: f64,        // 乘区位
}
```

**为什么这条必须在形状里定死**：`EntityClass::Item` 的恒假判断（已删除）、
以及"在结算链路里写『若是火焰且是某效果则删除』"（DsnE12 第 4 条禁止）都是同一个
失败模式——**把"某一类东西"写进规则的分支，而不是写成可寻靶的数据**。
"无视"只是"丢掉一个来源桶"，因此无视类效果的测试也变成纯数据的：
**同 base、同效果集合，只改 `ignored` → 结果按预期变**。

**来源桶的**形状** vs **内容**（红线）**

| 属形状（本条目定） | 属内容（GAME.md，待定） |
|---|---|
| 有没有"来源"这个维度 | 各来源的**系数/取值** |
| 求值顺序（先基础值、后乘区） | 各分区的**叠加口径**（加算还是各自成区） |
| `ignored` 是"丢桶"而不是特例分支 | 哪些效果属于哪个桶（内容设计时定） |
| 初始桶集合：`Intrinsic` / `Equipment` / `Status` / `Terrain`（**初稿，待确认**） | 桶是否还要细分 |

**地形减速的归属**

`DsnE8` 已记「重甲/地形 → `MoveSpeed`」，本条目确认该方向：**地形减速作为速度修正
进入 `MoveSpeed` 那条链**（第 ④ 步），而不是给 `ActionTimer` 加独立代价项。
这样与 Phase D 的"倍率速度"模型一致，且"无视某类地形减速"退化为第 ③ 步的丢桶。
地形效果**存在格子上**（格索引，见 DsnE12 / DsnE13），**不是 actor 的子实体**：
actor 走开效果还在，两个 actor 站在同一格受同一个效果。

**最小闭环（H11 的验收对象）**

只做**一对**：**地形减速**（格子侧）× **一件无视地形的装备**（actor 侧）。
这一对同时压到两侧的每种机制：

- 载体侧：格索引 + actor 子实体挂载关系两种落点；
- 求值侧：两个来源桶合并、一个 `ignored` 过滤、两类修正位。

**证伪条件（先验，否则就是过度设计）**：若加"第 2 类效果"（如状态减速）必须改
**求值入口的结构**，则说明形状错了，**先修扩展点再写真内容**——判据即 `LECS22`
（"加第 N 个要改几处"）。

**明确不做**

- **不新增、不修改任何公式与系数**；不填任何效果的数值（属 GAME.md）。
- 不定义具体效果内容（状态效果/生成器/地块效果都只是"位"）。
- 不决定时长单位口径（AV 或秒）——见 GAME.md 待定。

**关联：** ISSUES ECS27（地形代价维度）/ ECS28（求值入口缺失）/ ECS30（挂载关系）
/ ECS41（挂载关系用专用 `Relationship`）| REFACTOR §2.1（H9/H11 未完成）
| DsnE8（地形 → `MoveSpeed`、Phase H4 的 `move_cost` 预留列）| DsnE12（载体侧）
| DsnE13（索引还是实体的判据）| LESSONS LECS22

**状态：** 已定 — 已改写定案（形状），**求值侧的 ③④⑤ 步已落地**（Phase H8：
`ecs_core/src/rules/modifier.rs`）；具体修正来源与数值随内容阶段补齐。

- ✅ **求值侧 ③④⑤ 已实现**：`Modifier { source: EffectSource, base_delta, multiplier }`、
  `apply_modifiers(base, mods, ignored)`、`evaluate_modifiers(...) -> ModifierOutcome`
  （附参与/被无视的条数，供日志与调试解释"为什么是这个数"）；
  初始来源桶：`Intrinsic` / `Equipment` / `Status` / `Terrain`（`#[non_exhaustive]`，可增补）；
- ✅ **接线已就位且零行为变化**：`SpeedRule::action_av` 现在先经
  `SpeedModifiers::effective_speed`（两个桶合并 → 过滤 → 折叠）再交给
  `balance::action_av` 做 clamp 与除法；`Wait`（`Fixed`）**不读修正器**
  （让不吃速度的行动受地形影响是另一条设计决策，不该顺手决定）。
  当前所有生成/挂载路径都传 `SpeedModifiers::EMPTY`，因此 AV 逐值等于接上之前
  （`empty_speed_modifiers_keep_the_previous_av_exactly` 钉住）；
- ✅ **"无视某类"已可执行**：`ignoring_the_terrain_bucket_changes_the_av_without_any_special_case`
  ——同一份修正，只因为 `ignored` 不同就得到不同 AV，规则里没有任何特例分支；
- ⏳ **第 ①② 步（收桶）待 H9/H11**：格索引与效果实体落地后，系统层才会往两个桶里放东西。
  在此之前 `SpeedModifiers::EMPTY` 就是全部调用路径。

---

**原编号：** `DsnE10`（迁移前）
<!-- &/DsnE10 -->

<!-- &DsnE11  技能三层：激活/委派/行为；技能负责点火，不负责烧多久 -->
### DsnE11 技能的三层结构：激活 / 委派 / 行为

**决策**

技能**不是**"一个技能一种系统"，也**不是**"一套数据框架描述全部效果"，而是三层，每层用不同机制：

| 层 | 承担什么 | 机制 |
|---|---|---|
| **① 激活** | 触发条件（范围计数、阈值、状态旗标） | 数据；闭合的小词汇表 |
| **② 委派** | 这一次施法立刻发生什么 | **每个技能一处代码**（生成 + 执行同处） |
| **③ 行为** | 需要持续发生的事 | **委派给独立实体 + 它自己的系统** |

**背景与理由**

- **为什么不是"一个技能一种系统"**：每加一个技能要写组件 + 生成器 + 执行器 + 在中央 schedule 注册，成本随技能数线性增长，且中央注册列表会变成新的 `ActionKind`。
- **为什么不是"一套数据框架"**：曾考虑用闭合操作词汇表（`Damage`/`Heal`/`Buff`/`Spawn`…）描述全部技能效果。该方案**在"持续行为"上失效**——例如"自身死亡并把格子变成随时间逐个生成生物的区块"，无法用数据描述，因为它的效果一部分是**之后持续发生的事**。
- **为什么③必须委派**：持续行为有自己的时间轴、生命周期与终止条件（可被特定效果解除）。把它塞进技能数据表就会把数据表变成行为解释器。

**关键原则**

> **技能负责"点火"，不负责"烧多久"。** 复杂效果的载体是**实体**，不是技能数据里的字段。

**技能作为子实体**

技能实例（这一次施法）挂在**瞬态子实体**上，复用行动实体的链路与生命周期：归属（玩家/怪物/装备授予）由"挂在谁身上"决定，用完即 `despawn`，中间状态自动消失。

**与 `Can*` 的关系（待定）**

`Can*` 目前是 actor 上的 ZST 能力标记，且**能力散在 spawn 代码、数值在模板**（见 ISSUES ECS34）。技能属"可授予"（卷轴学习、装备授予）还是"固有"，决定技能走 `Can*` 组件集合还是技能表 + 统一读取入口——**待查 `CanBasicAttack` 与 `Attack` 是否一一对应后再定**（见 REFACTOR.md Phase H 待定项）。

**关联：** ISSUES ECS31/ECS34 | REFACTOR §2.1（H10/H12 未完成） | DsnE12（长期效果的载体）

**状态：** 已定 — 待落地（Phase H）。本轮只搭骨架 + 一个 dummy 技能走通，**不含任何真技能**。

---

**原编号：** `DsnE11`（迁移前）
<!-- &/DsnE11 -->

<!-- &DsnE12  长期效果用世界级实体；归属用专用关系 EffectOf，不用 ChildOf -->
### DsnE12 世界级效果实体：长期效果的载体

**决策**

需要"活一段时间"的效果（状态效果、逐次生成的区块、可被特定手段解除的地块效果）**spawn 为独立的世界级实体**，由**类型化 `Query` 发现**、由**恰好一个 owner 系统**管理终结。

**背景**

技能三层结构（DsnE11）的第③层需要载体。实测证据：`ChildOf` 在 bevy 0.16 是 `linked_spawn`（`#[relationship_target(relationship = ChildOf, linked_spawn)]`，`bevy_ecs-0.16.1/src/hierarchy.rs:156`），**父实体 despawn 会级联 despawn 子实体**。因此"自杀并把地块变成生成器"这类技能，其效果实体**不能挂在施法者名下**——否则会在生成的同一瞬间被级联删除。（这就是下表里"需要活过拥有者"那一行的来源；它也是 `linked_spawn` 对装备是**错的**同一个原因。）

**决策内容**

1. **挂载用"专用关系类型"，不用 `ChildOf`；落点按"谁拥有它"二分。**
   `ChildOf` 在 bevy 0.16 是 `linked_spawn`（`bevy_ecs-0.16.1/src/hierarchy.rs:156`），
   **父实体 despawn 会级联 despawn 子实体**。更关键的是：**`ChildOf` 这一个分组已经被
   行动链路占用**——`action_arbitration_system` 的候选查询
   `Query<(Entity, &ActionPriority, &ChildOf), (With<Candidate>, Without<ActiveAction>)>`
   **不看实体类型**，落选者一律 `despawn`（ISSUES ECS30；今天不误伤只是因为 filter 恰好含
   `With<Candidate> + &ActionPriority`，属**巧合式安全**）。而 `ChildOf` 一个父只有**一个**
   `Children` 列表，无法同时承载"行动子实体"与"效果子实体"两个分组。
   因此：**挂在谁身上，用专用关系类型表达**，让"效果不参与行动仲裁"成为**类型保证**
   （`Relationship` / `relationship_target` 是 bevy 0.16 的公开派生，
   `bevy_ecs-0.16.1/src/relationship/mod.rs:35-72`）：

   ```rust
   #[derive(Component)]
   #[relationship(relationship_target = OwnedEffects)]
   pub struct EffectOf(pub Entity);          // 效果 → 拥有者

   #[derive(Component)]
   #[relationship_target(relationship = EffectOf, linked_spawn)]
   pub struct OwnedEffects(Vec<Entity>);
   ```

   > **落地进展（Phase H）：** 行动侧那一半**已经落地**——`ActionOf` / `ActionChildren`
   > （`ecs_core/src/action/ownership.rs`）取代了行动链里的 `ChildOf`，
   > 候选查询因此**结构上**匹配不到非行动实体（ISSUES ECS30 / ECS41 已修复）。
   > 效果侧那一半（`EffectOf` / `OwnedEffects`）随 H11 落地；
   > 届时不变量是：**两套关系互不可见**，所以"效果不能参与行动仲裁"由编译期保证。

   | 效果落点 | 归属 | 读取方式 | 级联语义 |
   |---|---|---|---|
   | actor 拥有的（buff、技能授予的持续效果） | actor 的 `EffectOf` 子实体 | 类型化 `Query<&EffectOf>` | `linked_spawn`：拥有者没了效果也没了 |
   | **格子**拥有的（地形减速、燃烧地面、冻结块） | **格索引**（第 2 条） | 统一格子属性入口 | 与 actor 无父子关系 |
   | 需要**在拥有者死后存活**的（掉落在地上的装备） | **开放项**（见下） | 位置 + 组件表达 | **不能用 `linked_spawn`** |

   > **`linked_spawn` 的语义对不同效果是相反的**（ISSUES ECS42）：状态效果"人死了效果消失"
   > 是对的，**装备"人死了掉出来"**用 `linked_spawn` 就是错的（战利品随尸体一起消失）。
   > 所以装备的落点必须单独决定，**不能默认套用本条的挂载**。
2. **位置索引照 `OccupancyMap` 的形态**：`[[Option<Entity>; MAP_WIDTH]; MAP_HEIGHT]` + 一个维护系统（既有范例 `rebuild_occupancy_system`）。理由：整张地图实体化会让每格读取从数组索引退化成哈希/查询，而"格子属性"只需一个**统一读取入口**，不需要统一存储。
   > **格索引不等于"没有身份"**：**效果**可以有身份（"这一格被冻住了"能被单独解除）；
   > 有身份的是**那个效果**，不是**格子本身**（判据见 DsnE13）。
   > 反过来说，**地形减速**这种"种类数据"根本不需要实体——它已经在 H4 的
   > `TILE_PROPS.move_cost` 里有位置了；**只有"这一格发生了额外的事"才需要效果实体**。
3. **求值入口在 DsnE10**：actor 侧与格子侧**两个数据来源，一份列表**，由 DsnE10 的五步求值折算成纯函数的输入。本条目只管"效果活在哪里、由谁终结"，不管它怎么被折算。
4. **每类效果恰好一个 owner 系统**负责它的全部终结路径（过期、被解除、被替换）。其他系统只能改它的数据，不能 `despawn` 它。既有范例：`action_completion_system` 是**唯一**允许回收 action 实体的地方。
5. **效果必须可寻靶**：效果实体携带 tag，解除类效果（如火焰）以"对范围内的 `<tag>` 生效"表达，**不得在结算链路里写"若是火焰且是某效果则删除"的特例分支**（同类教训见 `EntityClass::Item` 的恒假判断）。
6. **存档可发现**：持久效果必须能被存档统一枚举写出——这是"放世界实体空间"而非"各自存在子系统私有结构里"的主要理由。**技能实例（瞬态子实体）不存档**，照 DESIGN DsnX17 ⑥「瞬态子实体不存档」。
7. **消费随机数的效果系统必须显式排序**：`GameRng` 的确定性与 `steps` 语义是回放/存档基础，Bevy 系统顺序默认未指定。

**开放项（待用户决定；本轮只记形状）**

| 开放项 | 选项 | 决定的依据 |
|---|---|---|
| **装备的落点** | ① `EffectOf` 子实体 + **关闭 `linked_spawn`**；② 不用关系，用"位置 + `Equipment` 组件"表达 | 按 DsnE13：装备是否需要**独立身份**（可被偷/单独销毁/单独耐久）——需要才用实体与关系 |
| **格子效果的粒度** | ① 每格一个效果实体；② 一个世界级区域实体 + 范围查询 | "查这一格有什么效果"是索引命中还是范围扫描；后者省实体数但不省查询 |
| **来源桶的初始集合** | `Intrinsic` / `Equipment` / `Status` / `Terrain`（DsnE10 的初稿） | 形状可现在定；"哪些效果进哪个桶"属 GAME.md |

**明确不做**

- 不定义任何具体效果类型（状态效果/生成器/地块效果都只是**位**）。
- 不决定时长单位口径（AV 或秒）——见 GAME.md 待定。

**关联：** ISSUES ECS30 / ECS41（挂载关系）/ ECS42（`linked_spawn` 语义相反）
| REFACTOR §2.1（Phase H 剩余）（H9/H11） | DsnE11（三层结构第③层）
| **DsnE10（求值侧）** | DsnE13（索引还是实体的判据）| `OccupancyMap`（既有范例）

**状态：** 已定 — 已改写定案（形状），**待落地**（H9 格索引、H11 最小闭环）。
本轮只做一个最小效果走通，验证机制；三个开放项待用户决定。

---

**原编号：** `DsnE12`（迁移前）
<!-- &/DsnE12 -->

<!-- &DsnE13  格子身份判据：需要独立身份→实体，否则→索引；地形属性进静态表 -->
### DsnE13 格子属性的身份判据：索引还是实体

**决策**

判断某个"格子相关的东西"该用**索引**还是**实体**，判据是**它是否需要独立身份**：

> **需要独立身份**（能被单独引用、单独寻靶、单独销毁、能被玩家指认为"这一个"）→ **实体**
> **不需要**（同种类的格子可以互换）→ **索引**（数组/网格）

**背景**

讨论中出现过"把地块实体化、用组件表达概念"的方案。该方案**原理可行**（不少引擎如此），但对本项目不划算，因为实测事实是：

- 地图网格是**承重的**：`can_move_to`、`rebuild_occupancy_system`、`map_gen.rs`、`spatial/`（FOV/LOS/A*）、`presentation` 的 `extract_scene_frame` 都按数组索引读它；
- `Tile` 的 u8 序列化（0..10）**是已冻结的存档契约**（只能末尾追加）；
- 全地图 80×60 = **4800 格**，其中绝大多数是同种类的可互换格子。将"种类数据"复制 4800 份属于重复表示（同类问题见已删除的身份 ZST `Rat/Scorpion/...`）。

**决策内容**

1. **地形种类 = 数据**。`Tile` 枚举**保留为种类键**，其属性（可走/挡视线/glyph/移动代价）收敛到**一张静态属性表**，经单一查表函数访问——照 `MonsterTemplate` 的既有形态。**理由**：H4 之前加一个 `Tile` 变体要改 5 处 match（`glyph`/`walkable`/`blocking`/`From<u8>`/`Into<u8>`），属中央分派债（ISSUES ECS31）。
2. **格子级运行期效果 = 实体**（DsnE12）。"浅水"是 11 种之一（需要一份定义），"这一格被冻结了"才是格子独立状态（需要一个实体）。
3. **格子属性有统一读取入口**。地形属性与效果实体是**两个数据来源，一个答案**——由单一函数给出"这个格子的属性是什么"，规则层不必知道数据在哪。
4. **属性表让"网格是否改成组件"这一决定保持便宜**：一旦属性与枚举解耦，将来若要支持多层/多地图，只动存储与访问层，不必再动属性定义。

**明确不做**

- 不改 `Map` 的网格表示；不改 `Map` 是资源还是组件（**仅当要做多层/多地图时才有收益**，见 REFACTOR.md Phase H 待定项）。
- 不新增任何具体地形属性（移动代价的数值属于 GAME.md）。

**关联：** ISSUES ECS27/ECS28/ECS32 | REFACTOR §2.1（H9/H14 未完成） | DsnE12（效果实体）

#### H4 落地记录（原误放在 DsnE8）

地形的种类与属性收进 `map/tile.rs` 的一张表（`TILE_PROPS: &[TileProps]`），
`glyph` / `walkable` / `blocks_vision` / `Serialize` / `Deserialize` **全部读表**，
判别值改由 `#[repr(u8)]` 决定——**没有一处 `match` 在描述地形**。

| | H4 之前 | H4 之后 |
|---|---|---|
| 加一个地形要改 | `glyph` / `walkable` / `blocking` / `From<u8>` / `Into<u8>` /（以及 `presentation::tile_id`）**六处** | `Tile` 末尾 + `TILE_PROPS` 末尾，**两处**（枚举无法自动派生） |
| 新属性（移动代价）的落点 | 没有，只能继续往 5 个 match 里加 | `TileProps` 加一列 |
| 漏加一项的报错时机 | 编译期（穷尽 match） | **运行期** → 由穷举测试补回 |

**换表的代价必须用测试补回来**（`LECS22` 的配套纪律）：`table_covers_every_variant_and_round_trips`
逐行枚举全表并断言"表行 = 枚举顺序 = 判别值 = 序列化往返"，另加
`ids_match_the_pre_h4_serde_mapping` / `properties_match_the_pre_h4_values`
逐值钉住搬迁前的口径（防止抄错一格）。

**`move_cost` 是 H8 的预留位**：本轮只加列、恒为 1.0、**不接线**——
移动 AV 仍只由 `MoveSpeed` 决定。`move_cost_is_still_a_reserved_slot` 守住这一点，
防止"顺手填值"绕过 GAME.md（数值口径待定，见 DESIGN DsnX16）。

**变异验证：** 交换 `Tile::Wall` 与 `Tile::Floor` 的声明顺序 → 20+ 个用例失败
（地图/移动/视野都读地形身份）。这既是"判别值受保护"的证据，也说明
**枚举声明顺序是领域事实**，不是可以随手整理的排版。

**关联：** REFACTOR §1（Phase H4）；ISSUES ECS31（已修复）、ECS27（`move_cost` 的来处）。

**状态：** 已定 — 第 1 项（静态属性表）**已落地**（Phase H4，见 DsnE8「Phase H4 进展」）；第 2–4 项待落地（H9 / H11）。

- ✅ **第 1 项已实现**：`ecs_core/src/map/tile.rs` 的 `TILE_PROPS` 就是本决策说的那张表，
  `Tile::props()` 是唯一查表入口；`glyph` / `walkable` / `blocks_vision` / serde 读写全部读表，
  加一个地形从"改六处"变成"改两处"（ISSUES ECS31 已修复）。
- ⏳ **第 2–3 项待做（H9）**："格子属性统一读取入口"要从"地形属性"扩展到"地形 + 效果实体"，
  届时规则层调用的仍是同一个入口，只是背后多一个数据来源。
- ✅ **第 4 项的前提已就位**：属性表已经与枚举解耦，将来改网格存储不必再动属性定义。
<!-- &/DsnE13 -->

