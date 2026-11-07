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
| 移动与写入权 | DsnE14 · DsnE15 |
| 行动系统 | DsnE4（已并入）· DsnE8 · DsnE9 |
| 规则与效果 | DsnE10 · DsnE11 · DsnE12 |
| 物品与装备 | DsnE16 · DsnE18 · DsnE20 · DsnE22 |
| 生命/法力与品质 | DsnE23 |
| 战斗与武器成长 | DsnE24 |
| 存档与读档 | DsnE19 |
| 事件时间轴与界面 | DsnE21 |
| 楼层与推进 | DsnE17 |
| 空间与地图 | DsnE2 · DsnE3 · DsnE7 · DsnE13 |
| 怪物与物种 | DsnE6 |
| 数据与随机 | DsnE5 |


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

**2026-09 两处修订（ECS45 之后）：**

1. **"占格"是实体自己声明的能力**（`components::Blocks`），不是"有 `Position` 就占"。
   占用图查询是 `With<Blocks>`，楼梯只是不挂它——以前那句写死的
   `if stairs.is_some() { continue }` 已删。加一类"在格子上但不占格"的实体
   （地面物品、格子效果）不再需要发明私有位置组件去躲这条查询。
2. **重建之外还有"回合内写穿"**（2026-09 修订，`ActionPhase::Validate` 之后）：
   - 校验阶段（串行）判定通过就**立刻 `occupancy.set` 预留**目标格；
   - 落地阶段（`action/settle.rs`）把 actor 写进新格、腾出旧格。
   所以占用图在**回合内部**也是准确的，而不只是回合边界上准确。这条是"同一回合两个
   actor 挤进同一格"（ECS45）的根治：判定与占位仍发生在同一次调用里（判定通过=当场占住）。

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

`GameRng` 成为**运行期**唯一随机源。所有随机操作统一走两种形态之一：

| 形态 | 用在哪 | 入口 | 是否进 `GameRng.steps` |
|---|---|---|---|
| `GameRng` | 游戏进行中（仲裁、暴击、游荡、掉落） | `ResMut<GameRng>` | ✅ |
| **派生 RNG** | **世界构造期**（地图 / 楼梯 / 怪物种群 / 格子效果 / 地面物品） | `resources::derived_rng(seed, floor, DerivePurpose)` | ❌（由 `(seed,floor)` 决定） |

**2026-09 修订（ISSUES ECS34）：** 派生 RNG 从"默许的第二种写法"变成**受管的一种形态**：

1. **唯一入口**：所有派生种子集中在 `DerivePurpose` 枚举里，四种用途各一个偏移
   （以前是 `init.rs` 里四处 `wrapping_add(777/999/1234/4321)`，改一处无人知道）；
2. **偏移是契约**：只能追加、不能改值或重排——`derive_purpose_offsets_are_pinned` 与
   `world_layout_is_pinned_for_a_known_seed` 钉住；
3. **门禁**：`docs_check.py` 禁止 `ecs_core` 里出现第二处 `SmallRng::seed_from_u64`
   （唯一例外是 `map_gen.rs` 内部为"子阶段互不干扰"重建的局部流，那是地图算法的实现细节）。

为什么世界构造**不**走 `GameRng`：`GameRng` 的 `state`/`steps` 是回放与存档的基础
（DsnX17 ③），它是"游戏进行中"的流；把构造塞进去会让"世界长什么样"与"玩家怎么打"
共用一条链，改动任一方都会平移另一方。代价是派生种子不出现在 `steps` 里——
所以用上面 2、3 两条把"改动必须显式"补回来。

**背景**

曾经有一个线程局部的 `RefCell<SmallRng>` 与 `GameRng` 并存，用于仲裁 system 中的随机选择——因为当时仲裁 system 无法访问 `GameRng` 资源。已由 ISSUES D1 解决。

---

## 三、核心机制（续：怪物 · 行动 · 规则 · 效果 · 地形）

> 本节标题原为「四、输入与渲染」——自 `presentation` / `tui` 拆出后，本文件已不再持有
> 任何输入与渲染内容，标题却留了下来（2026-09 核对文档陈述时更正）。

**原编号：** `DsnE5`（迁移前）
<!-- &/DsnE5 -->

<!-- &DsnE6  一个物种=一行数据：数值+能力+按地图的生成权重同住模板 -->
### DsnE6 物种模板：一个物种一行数据（已落地）

**决策**

一个物种 = 一条 `MonsterTemplate` 常量，**三类信息都在这一行里**：

| 字段 | 回答的问题 | 归位前住在哪 |
|---|---|---|
| 数值（`hp_base` … `exp_per_floor`） | 这只怪多强 | 本条目（Phase C 起已收敛） |
| `abilities: &'static [Ability]` | 这只怪能做什么 | `world/init.rs` 的 spawn 代码逐条 `insert` |
| `spawn: &'static [SpawnWeightFor]` | 这只怪多常见（**按地图类型**） | `monster/spawn.rs` 的三层 `match` |

于是"加一只怪" = 加一条常量 + 在种类池里排一次位。查询退化为查表/字段访问：
`monster_template(kind)` → `&'static MonsterTemplate`；`template.stats(floor)`；
`template.spawn_weight(map_kind, floor)`（表里没有该地图的条目 = `0.0` = 不生成）。

**背景与被否方案**

- **被取代的旧计划（2026-09 作废）：** 本条曾停在"设计中，实验方向 / 暂缓"，
  理由是"公式体系未就绪，硬套系数只会引入假灵活性"。H6 时该理由不再成立——
  数值本来就是系数 + 公式，而**能力与权重是散落**（不是未抽象）：继续等只是让
  "加一只怪要改三个文件"固化（ISSUES ECS32）。
- **被否：把 `Can*` ZST 直接放进模板的列表字段。** 模板是 `Copy` 字面量**数据**，
  `Can*` 是 bevy 组件；混进一张表会让"数据表"变成"组件清单"，加一行数据都要引用
  `components`。中间留一层 `Ability` 枚举作 `Ability → Can*` 的**唯一**映射点
  （`spawn::attach_abilities`），加一种能力由编译器逼着补那一支。
- **被否：权重留在 `spawn.rs`。** "这只怪多常见"与"这只怪多强"是同一物种的两个属性，
  分居两个文件的代价在 ECS32 里已经显形（读一处不够）。
- **被否：按枚举顺序自动派生种类池**（池 = 有权重条目的物种，更整洁）。池序是
  **RNG 敏感**的——`roll_one_kind` 按池序累加权重，派生会把 `LushCavern` 的池从
  `[孢子怪, 蘑菇傀儡, 鼠, 哥布林]` 变成 `[鼠, 哥布林, 孢子怪, 蘑菇傀儡]`，行为静默改变。
  池因此保持**显式数组**，顺序由 `kind_pools_keep_their_declared_order` 钉死。
- **被否：`spawn_weight` 用 4 个裸 f32 字段**（旧草图的 `_base`/`_per_floor`/`_min`/`_max`）。
  10 条曲线每条只用到一侧截断，裸字段会让"没用上的那侧填什么"变成暗规则。改用一个
  `SpawnWeight`（`rising` / `falling` 两个构造器 + `clamp` 求值），没用上的一侧填无穷
  ——于是 `.max(底)` 与 `.clamp(底, INF)` 在所有有限输入上**逐位相等**，搬迁不产生数值差异。

**代价**

1. **权重与池分居两个文件**：加物种要改两处（模板常量 + 种类池）。前者是数据、后者是顺序，
   由 `pools_and_weights_cover_the_same_species` 维持"池 ⟺ 有权重条目"。不合并成一处是
   有意的——合并只能靠"按枚举派生"，而那正是上面被否的那条。
2. **位级比较绑住了公式形态**：`assert_eq!` 对 f64 是位级比较，任何**数学等价但舍入顺序不同**
   的改写都可能被拒绝。这是刻意的：权重是行为，浮点差异会平移 `GameRng` 的消费。
3. **`MonsterStats` 少了一个字段**：怪物不再自带恒为 `0.0` 的 `Magic` 占位
   （原 ECS33 并入 ECS32 的那半条）。将来法系怪要蓝条，就在 spawn 时按需 `insert`，
   而不是"每只怪都带一个 0"。

**落地记录（H6 / H7）**

| | 归位前 | 归位后 | 加一只怪要改 |
|---|---|---|---|
| 能力 | `spawn_monsters_system` 里四条 `insert` | 模板 `abilities` 列（`Ability` 枚举） | 常量里写一行 |
| 权重 | `spawn.rs` 三层 `match`、10 条裸表达式 | 模板 `spawn` 列（`SpawnWeightFor` × 图表） | 常量里写一行 |
| 种类池 | `spawn.rs` 三个显式数组 | **不变**（顺序是行为） | 池里排一次位 |

- `spawn_weights_match_the_legacy_match_arms`：把归位前三层 `match` **原样抄进测试**，
  3 地图 × 8 物种 × 6 楼层逐点 `assert_eq!`（f64 位级相等）。
- `kind_pools_keep_their_declared_order` / `pools_and_weights_cover_the_same_species`：锁顺序、锁一致性。
- `every_template_grants_the_legacy_ability_set` / `attach_abilities_maps_each_ability_to_its_marker`：锁能力集合。
- **变异验证：** ① 把 `LUSH_KINDS` 的"鼠"提到首位 → `kind_pools_keep_their_declared_order` 失败；
  ② 把鼠的繁茂下限 `0.5` 改成 `0.4` → `spawn_weights_match_the_legacy_match_arms` 失败。
  两处都被**对应用例**接住，其余用例不受影响。

**状态：** 已定 — H6/H7 已落地（`ecs_core/src/monster/template.rs`、`spawn.rs`，2026-09）。

**关联：** ISSUES ECS32（已修复，含原 ECS33）| GAME.md Gm8（怪物数值）/ Gm10（按地图类型的权重口径）| DsnE13（静态属性表的同形做法）| DsnE11（能力/技能的三层结构，H10 将接上 `abilities`）

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

<!-- &DsnE8  行动=actor 瞬态子实体；AV 走倍率；终态单一出口；保活覆盖全部待执行行动 -->
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

**保活检查：** 生成时检查一次条件，且**对所有待执行行动持续生效**——失效的行动会继续
占着 actor 唯一的 `ActiveAction` 槽位，让 actor 空转若干回合。
**已落地（2026-09，ISSUES ECS43）**：保活在 `ActionPhase::Validate` 里，对**所有**待执行行动
（`PendingAction<A>`，不带 `With<Ready>`）每轮扫一遍，失效即刻取消；**定案**（选方向 + 预留格子）
仍只对**到点**的行动做——提前定案会拿到过期的落点，还会白占一格。生成条件与保活条件用**滞回**
的两个阈值（逃跑 0.25 进 / 0.30 出），所以不会"生成 → 立刻取消 → 再生成"。

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
| M1 判定/落地分阶段 | `ActionPhase` 六角色（新增 **Validate**）；判定收进校验器（`action/validation.rs`），落地收进**一个**系统（`action/settle.rs`）；六个执行器 → 一个 | DsnE14 |

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
| ISSUES ECS41（`ChildOf` → 专用关系）、ECS43（保活覆盖范围，**已闭合**：见上文「保活检查」）
| LESSONS LECS3（Bundle 上限）、LECS22（扩展点判据）、LSYN20（测试依赖执行顺序）

**状态：** 已定 — ①②、H1/H3 与保活覆盖范围（ECS43，2026-09）均已落地。

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

> **本条目经历了一次改写（2026-09 讨论；求值侧随后已落地）。** 初版只把修正器定义为
> 「`ActorSpeeds` 的推广」——**只覆盖"给某个数值算一个修正"**，因此碰到
> "无视某一类来源"时只能**在求值点写特例**，违反 DsnE12 第 4 条。
> 改写后：修正器的**载体是效果实体**（DsnE12），本条目只管**求值侧的形状**——
> 「怎么把效果折算成纯函数的输入」。原版的四条（求值位置、两类位、pull、`Can*` 例外）
> 全部保留，只是从"整个设计"降为"求值侧的四条实现约束"。
> 2026-11 的 DsnE22 ②a 又把 `Stat` 闭枚举换成运行时词表 `StatId`，并把"两类位"
> 拆成 `Add / MultiplyBase / MultiplyTotal` 三个算子；求值五步本身没变。

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
   `actor_speeds()` 在系统层从 `Stats` 读 `MOVE_SPEED`/`ATTACK_SPEED`，`action_av` 只吃
   一个纯数据 `ActorSpeeds`。**新修正器一律照此形态**——第 ①②③④ 步都在系统层完成，
   第 ⑤ 步的函数保持"位置无关、可单测"。
2. **修正器预留两类位（现在是三个算子）**：**基础值修正**（`Add`）与**乘区修正**
   （`MultiplyBase` / `MultiplyTotal`，各自独立因子相乘）。第 ④ 步的顺序
   `(base + ΣAdd) × ΠMulBase × ΠMulTotal` 是**形状**；具体哪类效果进哪一位、
   系数多少属 GAME.md。
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
fn apply_modifiers(
    stat: StatId,
    base: f64,
    mods: &[Modifier],
    ignored: &[EffectSource],
) -> f64

struct Modifier {
    stat: StatId,           // 作用项：属性词表里的哪一项（DsnE22 ②a）
    source: EffectSource,   // 形状：来源桶（下表）
    op: ModifierOp,         // Add / MultiplyBase / MultiplyTotal
    value: f64,
}
```

> 当前代码里 `apply_modifiers` 还有第 0 个参数 `stat`，只保留 `stat` 匹配的修正；
> `StatId` 词表见 DsnE22 阶段 ②a。两种乘区各自乘成一个独立因子（先加后乘的形状未变）。

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
| 初始桶集合：`Intrinsic` / `Equipment` / `Status` / `Terrain`（**已定案**，DsnE12 开放项） | 桶是否还要细分 |

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
   > 效果侧那一半**也已落地（H11）**：`EffectOf` / `OwnedEffects`（级联）与
   > `EquippedBy` / `EquippedItems`（**不**级联）在 `ecs_core/src/effect/ownership.rs`。
   > 不变量成立：**两套关系互不可见**，所以"效果不能参与行动仲裁"由编译期保证。

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

**开放项（2026-09 定案 2/3，第 3 项待定）**

| 开放项 | 结论 | 依据 |
|---|---|---|
| **装备的落点** | **已定 ①**：两个关系类型——`EffectOf` / `OwnedEffects`（**带** `linked_spawn`，状态与技能授予的持续效果）+ `EquippedBy` / `EquippedItems`（**不带** `linked_spawn`，装备） | 两种级联语义**相反**（ISSUES ECS42），所以用**两个类型**把它写进类型，而不是在 owner 系统里按种类分支。"装备掉在地上" = `EquippedItems` 的 owner 系统把它换成"位置 + 组件"表达，而不是靠 `despawn` 级联 |
| **格子效果的粒度** | **已定 ①**：每格一个效果实体 + 格索引（`[[Option<Entity>; MAP_WIDTH]; MAP_HEIGHT]`，照 `OccupancyMap` 的形态与维护系统） | 「查这一格有什么效果」是 O(1) 索引命中；且"这一格被冻住了"能被**单独**解除（DsnE13 的独立身份判据）。大范围效果用"一个生成器实体 → 逐格 spawn"表达，**不**把存储改成范围查询（那会让每次读格子属性退化成扫区域） |
| **来源桶的初始集合** | **已定 ①**：维持四项 `Intrinsic` / `Equipment` / `Status` / `Terrain` 不变（H8 已按此落地，`#[non_exhaustive]`） | 桶集合回答的是"玩家词汇里有哪些『无视 X』"。现状对应三句可理解的玩家词汇（无视地形 / 装备 / 状态）+ 一个固有修正位；拆得更细（`Terrain` 分成地形种类与格子效果、`Equipment` 分到槽位）的收益只在"已确定要做『只无视其中一部分』的设计"时兑现，而现在**没有这样的设计**，拆了就是没人使用的桶。加变体的代码成本≈0，到期再拆 |

> **来源桶权衡（2026-09 定案：维持四项，即第三档不拆）：**
>
> 1. **桶在链路里只有一个作用**：`apply_modifiers` 用 `ignored: &[EffectSource]` 做一次
>    `contains` 过滤（`rules/modifier.rs`）。因此桶集合**只**决定"无视类能力能表达什么"，
>    不影响折叠口径、不影响数值、不影响性能。
> 2. **粒度是一条连续的轴**，不是四选一：一个桶（无视减速 = 无视全部来源）→ 现在四桶
>    （地形 / 装备 / 状态 三句玩家词汇）→ 更细（把 `Terrain` 拆成"地形种类代价"与
>    "格子效果"，或把 `Equipment` 拆到槽位级）。**只有当设计里确实存在"只无视其中一部分"
>    且玩家能理解的差别时，才拆**——否则拆出来的是没人使用的桶。
> 3. **成本不对称**：加桶的代码成本 ≈ 0（`#[non_exhaustive]` + 加变体 + 调用方 `ignored`
>    列表多一项，`apply_modifiers` 一行不动）；但一旦有 N 个效果在 GAME.md 里分好桶，
>    拆桶就意味着重新归属 + 重新平衡 + 现有"无视某桶"的测试重写。**现在没有内容，
>    所以"加桶便宜"这一侧还完整可用；等有了内容，这个期权就贬值了**——这是选 A 的主因。
> 4. **`Intrinsic` 是唯一目前没有内容的桶**（基准数值由 `apply_modifiers` 的 `base` 参数承载）。
>    留它的理由：让"物种天赋/等级带来的固有修正"与增益、减益走**同一条链路**，从而也能被
>    "无视固有"表达；删它则这类需求会退化成在规则里写分支——正是 DsnE10 要避免的失败模式。
>    留的代价是它必须守一条纪律：**`Intrinsic` 只装 actor 自身的固有修正，不充当杂项桶**。
> 5. **被否：自由 tag（每条修正带任意 tag 集合）**。表达力更强（一件装备可同时是
>    `Equipment` + `Curse`），但 tag 写错是**静默失效**（"无视"永远不生效且不报错）——
>    本项目已被静默失效咬过两次（`EntityClass::Item` 恒假判断、`linked_spawn` 对装备语义相反）。
>    闭集枚举的编译期穷尽是最便宜的护栏；真需要多类别时，加第五个桶比引入 tag 词表便宜。

**明确不做**

- 不定义任何具体效果类型（状态效果/生成器/地块效果都只是**位**）。
- 不决定时长单位口径（AV 或秒）——见 GAME.md 待定。

**关联：** ISSUES ECS30 / ECS41（挂载关系）/ ECS42（`linked_spawn` 语义相反）
| REFACTOR §2.1（Phase H 剩余）（H9/H11） | DsnE11（三层结构第③层）
| **DsnE10（求值侧）** | DsnE13（索引还是实体的判据）| `OccupancyMap`（既有范例）

**落地记录（H9 / H11，2026-09）**

`ecs_core/src/effect/` 三个文件，对应本条的三个落点与一条读取入口：

| 落点 | 落地物 | 级联 |
|---|---|---|
| actor 拥有的持续效果 | `ownership::EffectOf` / `OwnedEffects` | ✅ `linked_spawn` |
| 装备 | `ownership::EquippedBy` / `EquippedItems` | ❌ 不级联（**实测**：拥有者消失后装备存活，`EquippedBy` 被关系机制自动清掉 → 正是"无主待认领"的状态） |
| 格子效果 | `cell::CellEffectAt` + `CellEffects`（`[[Option<Entity>; W]; H]`，照 `OccupancyMap`）+ 每轮重建的 `rebuild_cell_effects_system` | 与 actor 无父子关系 |

- **统一读取入口 = `cell_attributes(&TileProps, Option<&CellMoveCost>) -> CellAttributes`**：
  地形表与格子效果合成一个可解释的结构（`walkable` / `blocks_vision` / `move_cost` / `has_effect`）。
  取 `&TileProps` 而不是 `Tile`，是为了让"读表"这件事能被单测钉住（合成一个 `move_cost: 2.5`
  的属性即可断言，不必改 `TILE_PROPS` 的数值）。
- **收桶 = `EffectContext::collect(actor, cell) -> CollectedModifiers`**：这是 ECS28 要的那条接缝，
  系统层一次查询拿齐"脚下这一格的修正 + actor 名下效果声明的无视来源"，
  纯规则仍然只吃 `&[Modifier]` + `&[EffectSource]`。
- **最小闭环一对**：`cell::CellMoveCost`（格子侧，贡献 `Terrain` 桶的速度乘区）与
  `IgnoresSource`（actor 侧，让求值第 ③ 步丢掉该桶）。验收在 `effect_tests.rs`：
  同一格 `2.0` 代价 → 移动 AV 是平地两倍；给 actor 挂 `IgnoresSource::TERRAIN` → 回到平地值；
  挂 `IgnoresSource(Status)` → **仍**是两倍（无视是**按桶**生效，退化成一个布尔会当场失败）。
  **规则层（`rules/modifier.rs`）一行没改。**

**用哪一格：脚下，而不是"要去的那一格"**

AV 在**生成期**固化（`SpeedRule` 的既有约定）。玩家的 `Move` 在生成期知道方向，
但 AI 的 `Chase`/`Flee`/`Wander` **不知道**——方向是执行期才在 `execute_*_system` 里定的。
所以"目标格"口径要求把 AI 的 AV 计算整体推到执行期，那会推翻"AV 挂载时固化"。
取脚下这一格则始终是生成期确定的事实。**代价**：踏入减速格的**这一步本身不变慢**（下一步才慢）；
要做"进入即变慢"是一次独立换道。

**`CellEffectAt` 为什么不复用 `Position`**

`rebuild_occupancy_system` 的查询是 `Query<(Entity, &Position, Option<&Stairs>)>`——**任何**
带 `Position` 的实体都会被登记进占用图。格子效果若用 `Position`，立刻变成"占着格子"的实体：
玩家走不进一块本来能走的泥潭，且无任何报错。这是本项目第三次踩同一类问题
（`ChildOf` 的候选查询、`Position` 的占用图、这次），测试 `cell_effects_do_not_enter_the_occupancy_map` 把它钉住。

**变异验证：** 把格子修正的来源桶从 `Terrain` 改成 `Status` → **4 条用例失败**
（最小闭环两条 + 收桶断言 + "无视是按桶生效"那条），而"两倍代价变慢"那条**仍然通过**
——正好证明这几条测试区分的是"变慢了"与"在正确的桶下变慢"。

**状态：** 已定 — 形状与 H9/H11 **均已落地**；三个开放项全部定案（2026-09）。
**未落地**：格子效果的时长/解除路径（尚无第二个效果，owner 系统按 DsnE12 第 4 条届时一并加）。
**已补齐（2026-11）**：装备侧的实际物品后由 DsnE16 / DsnE18 落地；无视来源不再由装备实体
硬编码，写在蓝图 `behaviors`（`Equipped` + `IgnoreSource`，DsnE22 阶段 ③），求值侧仍只做第 ③ 步过滤。

---

**原编号：** `DsnE12`（迁移前）
<!-- &/DsnE12 -->

<!-- &DsnE13  格子身份判据：需要独立身份→实体，否则→索引；地形属性进静态表 -->
<!-- &DsnE14  判定一个阶段、落地一个系统：Validate 定案并预留，settle 只写结果 -->
### DsnE14 移动的判定与落地：`ActionPhase::Validate` + 单一落地系统

**决策**

行动链路从五阶段扩成**六阶段**，把"判定"提成一个有名字的阶段：

```text
Generate → Arbitrate → Tick → [Validate] ──▶ Execute → Complete
                                  │              │
          每个行动一个校验器：      一个通用落地器：只写结果
          保活（所有排队中的行动）  （写 Position / 发攻击意图）
          + 定案（只对到点的行动）
          并**立刻预留**目标格（串行，所以可靠）
```

- **判定只有一处**：`action/validation.rs` 里每个行动恰好一个校验器
  （`validate_move_system` / `validate_basic_attack_system` / `validate_chase_system` /
  `validate_flee_system` / `validate_wander_system`）。它做两件事：**定案**
  （`MoveTarget` / `AttackTarget` / 什么都不加 = 原地）和**预留**（`occupancy.set`）。
- **一个校验器管两件范围不同的事**（ISSUES ECS43）：**保活**（"还该做吗"）对**所有**待执行
  行动生效——包括还在排队、AV 没归零的；**定案 + 预留**只对**到点**的行动做。所以查询过滤器
  是 `PendingAction<A>`（不带 `With<Ready>`），"到没到点"由数据里的 `Has<Ready>` 决定。
  提前定案会拿到过期落点、白占一格；而保活提前生效正是这个阶段存在的另一半理由。
- **落地只有一处**：`action/settle.rs::settle_action_system` 是全项目唯一写 `Position` 的
  系统（另一个写入点是生成期出生点），由 `docs_check.py` 的"写 Position 白名单"在门禁里
  强制。它**不判定**：没有 `MoveTarget` 就没有移动。
- **六个执行器 → 一个**：判定被抽走之后，每个行动的落地动作完全同构（写位置 / 发意图 /
  什么都不做），再拆成六份就是六份相同的代码。**扩展点搬到校验器那一侧**：
  加一个行动 = 加一个生成器 + 加一个校验器（各一行）。
- **规则仍是纯函数**：[`plan_step`] 返回 `MovePlan::Allowed(Position)` 或
  `Blocked(BlockReason)`，四种原因（越界/墙/占用/对角穿墙）都是**数据**，玩家的
  "为什么没动"直接由它生成。校验器只多做一件"合法就写进占用图"。
- **预留的生命周期上限是一轮**：占用图每轮整轮重建，没被兑现的预留活不过这一轮。

**为什么"判定"必须是阶段，而不是藏在执行器里**

ECS45 的第一次修法是"唯一写入点 + 写穿占用图"。它**解决了**同轮抢格，但判定仍散在四个
执行器内部：读代码的人要钻进每一个执行器才知道"什么情况下这一步会被拒"，而这条判定被
四个执行器各写一遍（ECS45 两次没修净的根源）。分阶段之后，"谁有权判定"与"谁有权写"各有
一个答案，而且对称：**一个行动 = 一个校验器 + 一个落地动作**。

**被否方案**

- **被否：由执行器校验（判定留在 Execute 阶段）。** 那只是把"判定散在四处"从生成期挪到
  执行期，形状不变；同轮抢格依旧依赖"执行器按顺序互相看见对方刚写的占用图"，而"为什么
  被拒"也依然藏在系统体里。等于回到 ECS45 之前。
- **被否：由生成器校验。** 生成与判定**不是同一时刻**：候选生成后还要仲裁、还要排 AV，
  真正执行时世界已经变了（目标格可能已被占、目标可能已死）。生成期判定必然过期，而
  过期判定比没有判定更危险——它看起来是对的。**判定必须贴着落地发生**。
- **被否：只把四份判定合并成一个函数（`plan_move` + `commit_move`）。** 那是把"四份实现"
  变成"一份实现 + 四个调用点"——**统一入口靠约定**。下次再加第五条移动路径照样会漏
  （ECS45 两次没修净就是这个形状：每次要手工种四处守卫，写法还各不相同：
  `if claim(..)` / `if claim(..) && let ..` / `&& claim(..)` / 压根没有）。
- **被否：规则整体搬进系统。** 那样 `movement.rs` 的 5 条无 `World` 单测就没了，而
  "规则能脱离 World 测"是 ECS28/ECS2 换来的红利（纯函数层与 ECS 层分离）。所以选择
  **两者都要**：规则纯、判定单阶段、落地单系统。
- **被否：保留 `RoundClaims`（批内预留表）。** 判定发生时才占格，占用图本身就是预留表，
  第二个派生状态没有存在理由（两个派生状态收敛成一个）。

**代价**

1. **多一个阶段与两个瞬态组件**（`MoveTarget` / `AttackTarget`，同一 tick 内被消费）。
   阶段数 5 → 6，调度里多一次 `ApplyDeferred`——这是"阶段可见"的必要代价。
2. **校验期一个人合法占两格**（原格 + 预留格），落地才腾出原格。所以"一人一格"是对
   **落地结果**的不变式，不是对校验中间态的不变式（断言必须写在落地之后）。
3. **试算必须无副作用**：逃跑要在八个方向里挑最远的，而 `plan_step` 的穿角检查会读占用图，
   所以"边试边预留"会让先试的方向把自己后面的方向判死——`Reservation` 因此拆成
   `probe`（只判）/ `claim`（只占）两步。这是落地时**实测踩到**的（`LECS24`）。
4. **`Position` 仍是公开组件**，"只有一个系统写"是**门禁 + 文件边界**，不是类型保证；
   白名单窄才有意义，所以落地系统单独一个文件（`action/settle.rs`）。
5. **单行动语义不变，整局结果会变**（这两句要分开说，否则会误以为"同种子同结果"）：
   - **不变**：`Move` 被挡仍算失败，`Chase`/`Flee`/`Wander` 被挡仍算成功，`Wait` 没有
     校验器就是"无条件有效"；`GameRng` 仍是每游荡行动一轮一次、顺序也在同一位置；
     六行动 parity 套件与 13 条校验器用例原样通过。
   - **会变**：`fuzz`（`examples/playthrough.rs`，同种子 400 回合）改动前后的输出**不再逐字
     相同**。两个原因，都是分阶段的**目的**而非偏差：
     1. **预留对同一轮后面的校验器可见**。以前六个执行器都是在 `Execute` 里**先各自判定、
     最后**才由 `apply_move_system` 按顺序写入，所以它们看到的是"本轮开始时"的占用图——
     两个怪物可以同时选中同一格，后一个在落地时被拒；现在后一个能看到前一个的预留，
     **换一条路**而不是原地不动（`Chase`/`Flee` 因此表现更好）。
     2. **`Move` 被同轮预订抢走时现在算失败**。旧实现是"执行器先算成功 → 落地时才被拒"，
     结果是"行动成功但没动"；现在校验阶段就判失败（也正是
     `the_second_claimant_of_a_cell_loses_before_anyone_moves` 钉住的那条）。
     
     所以任何"跨版本的整局指纹"（种子 → 结果）都会变，跨版本对比只能比**不变式**，不能比
     输出。两组 fuzz 的不变式（占用图与 `Position` 一致 / HP 边界 / 空闲玩家不留
     `ActiveAction` 子实体）在改动前后都是"异常：无"。
6. **保活放开到排队中的行动（ECS43）后，行动顺序会再变一次**：失效的排队行动现在会被
   **提前**取消，actor 立刻回到 `Idle`/`Failure` 并可以在下一轮重新生成候选，而不是拖到
   到点才失败。实测 6 颗种子里有 1 颗（400 回合）因此整局走向不同，其余 5 颗没触发
   （普通怪物的 AV 决定"排队窗口"只有一两轮，条件很少在窗口里翻转）。
   这也是为什么这条必须由用户拍板（RULE §八 ③）：它是玩家可见的行为改动，而不是重构。

**关联：** ISSUES ECS45（同轮抢格）/ ECS43（保活对所有待执行行动生效）| DsnE2（占用图：占格能力 + 回合内写穿）| DsnE8（行动子系统）| DsnE15（写入者清单）| `LECS23` / `LECS24`

**状态：** 已定 — 已落地（`action/validation.rs` 校验器 + `action/settle.rs` 唯一落地；
`execution/apply_move.rs` 与 `MoveRequest` 已删除）。
<!-- &/DsnE14 -->

---

<!-- &DsnE15  共享状态的写入者清单：哪些是唯一入口、哪些是有意的多写者 -->
### DsnE15 共享状态的写入者清单（"第二入口"审计）

**决策**

对每一个"会被多个系统改"的状态，**要么只有一个入口，要么在文档里写清为什么可以有多个**。
这张表就是那个"写清"：

| 状态 | 写入者 | 判定 |
|---|---|---|
| `Position` | `settle_action_system`（唯一）+ 生成期 spawn | **唯一入口**，由 `docs_check.py` 白名单强制（ISSUES ECS45）；白名单只有 `action/settle.rs` 与 `world/init.rs` |
| `OccupancyMap` | `rebuild_occupancy_system`（每轮整轮重建）+ 校验阶段**预留**（`Reservation::claim`）+ 落地阶段腾旧格（`clear_at`） | **有意的三个，只有一套规则**：重建是"对账"，预留是"判定通过即占位"，腾格是"兑现预留"。三者都只认 `With<Blocks>` |
| `Active` / `Idle` / `Failure` | `begin_action` / `finish_action`（各一份实现，共 3 个调用点） | **单一实现**：挂载（玩家）、仲裁（AI）、completion（收尾）语义必须一致 |
| `Health` | `Health::damage(..)` 一份实现；调用点：攻击结算、中毒 tick、升级重算 | **单一实现**：三个调用点是三种**不同原因**，共用一个方法 |
| `Ready` | `tick_action_timers_system`（置位）+ `ActionEvents::{succeed,fail}`（清理） | **有意的两个**：谁置位、谁清理各一处；校验器/落地器都拿不到 `EventWriter`（H1） |
| 事件缓冲 | `update_events_system`（唯一 `update()`） | **唯一入口**：新增事件必须登记进去，否则跨轮重读（I90） |
| 运行期随机 | `GameRng` | **唯一入口**（DsnE5） |
| 构造期随机 | `resources::derived_rng` + `DerivePurpose` | **唯一入口**，门禁强制（ISSUES ECS34） |
| `EventLog` | 多处（战斗/中毒/拾取/系统提示） | **有意的多写者**：日志天生多源；靠 `EventMessage.repeat` 合并重复 |

**本次审计删掉的一处**：`world/query.rs` 曾有两个 **World 版辅助函数**
`rebuild_occupancy(&mut World)` 与 `update_visible_memory(&mut World)`——Phase B 的产物，
被同名系统取代后一直没删。它们**没有调用者**，而且 `rebuild_occupancy` 用的是
**旧的占格规则**（`Option<&Stairs>` 过滤）：一旦有人顺手调用，就会得到与调度里
不同的占用图，且**没有任何报错**。这就是"第二入口"的典型形态——
同一个职责两份实现，其中一份还是过期的。已删除。

**判据（写新系统时自问）**

1. 这个状态是**单一真相**还是**派生数据**？派生数据允许多个写入者，但必须共用一套规则。
2. 如果有多份实现：**它们是否会在某天分叉？** 会分叉就必须收敛成一份实现
   （哪怕调用点仍是多个），或者收敛成一个入口 + 门禁。
3. 收敛之后加**门禁**：约定会在某次重构里被打破，白名单不会。

**关联：** ISSUES ECS45 / ECS34 / ECS35（终态出口）| DsnE14（移动单入口）| DsnE5（随机源）| DsnE2（占用图）

**状态：** 已定 — 清单与两处收敛均已落地；`Position` 与派生 RNG 各有门禁。
<!-- &/DsnE15 -->

---

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

**`move_cost` 的接线（H9）**：`TILE_PROPS.move_cost` 已**真的被消费**——移动类行动的 AV 由
`EffectContext::collect` 收进 `Terrain` 桶的乘区（`1/move_cost`）。但**表里没有非 1.0 的值**，
所以"地形变慢"目前没有内容、只有位置（数值口径属 GAME.md，见 DESIGN DsnX16）。
`terrain_move_cost_is_still_a_neutral_placeholder` 守住"代码里不许偷偷填数"，
`cell_attributes_consume_the_terrain_move_cost_column` 用一个合成的 `move_cost: 2.5` 证明这一列
**确实被读**（后者防的正是"接线了但没人读"这种空实现）。

**变异验证：** 交换 `Tile::Wall` 与 `Tile::Floor` 的声明顺序 → 20+ 个用例失败
（地图/移动/视野都读地形身份）。这既是"判别值受保护"的证据，也说明
**枚举声明顺序是领域事实**，不是可以随手整理的排版。

**关联：** REFACTOR §1（Phase H4 / H9）；ISSUES ECS31、ECS27（`move_cost` 的来处，均已修复）。

**状态：** 已定 — 四项**全部落地**（第 1 项 H4；第 2–3 项 H9）。

- ✅ **第 1 项已实现**：`ecs_core/src/map/tile.rs` 的 `TILE_PROPS` 就是本决策说的那张表，
  `Tile::props()` 是唯一查表入口；`glyph` / `walkable` / `blocks_vision` / serde 读写全部读表，
  加一个地形从"改六处"变成"改两处"（ISSUES ECS31 已修复）。
- ✅ **第 2–3 项已落地（H9）**：格子效果用世界级实体 + `CellEffects` 格索引（每格一个，见 DsnE12）；
  **统一读取入口**是 `effect::cell_attributes(&TileProps, Option<&CellMoveCost>) -> CellAttributes`
  ——地形表与格子效果两个数据来源、一个答案，规则层不必知道数据在哪。
- ✅ **第 4 项的前提已就位**：属性表已经与枚举解耦，将来改网格存储不必再动属性定义。
<!-- &/DsnE13 -->


---

<!-- &DsnE16  物品=静态表+ID；背包是数据、装备是实体；修正带 Stat 维度；掉落表进物种行 -->
### DsnE16 物品系统：注册表、背包、装备与掉落

**决策**

四件东西、四种存法，判据都是"它有没有独立身份 + 谁要读它"：

| 部件 | 存法 | 判据 |
|---|---|---|
| 物品定义 | **静态表**（`item/registry.rs` 的 `ITEMS: &[ItemDef]`，键是 `ItemId`） | 它是数据；关键是没有中央分派——**加物品 = 加一行** |
| 地面物品 | 实体 + `GroundItemAt`（专用位置组件） | 它占位置但不占格；用 `Position` 就会挡住玩家（第三次踩同一个坑） |
| 背包 | 组件里的定长数组（`Inventory`） | 没有身份、没有位置；做成实体是纯亏（遍历、排序、泄漏） |
| 装备 | 实体 + `EquippedBy` 关系（`EquippedItem { slot, id }`） | 求值侧要按关系读它；且 owner 死亡时要**掉出来**（DsnE12/E42） |

**为什么物品表是 Rust 常量而不是运行时注册表（否决了旧实现）**

旧实现（`archive/dungeon-core/src/items.rs`）用 `assets/items.json` + `OnceLock` 全局单例。
成本全在边界上：读文件失败、ID 指向不存在的物品、JSON 里写错槽位——每一条都要在运行期兜。
本项目的两张数据表（`TILE_PROPS`、`MonsterTemplate`）已经把"静态表 + 一条枚举全表的测试"
这条路走通了两遍，物品表照抄。**"数据驱动"不等于"数据在文件里"**：数值仍写在表里、代码里
没有 `match item_id { .. }`，这才是数据驱动的实质。

**装备的加成怎么生效（否决了"实例化效果实体"）**

C4 的第一版把装备的无视声明**实例化**成 `IgnoresSource` 子实体，于是"卸下"必须去找回
那些子实体——C4 干脆没做卸下。DsnE18 落地后装备是**身体容器里的槽位数据**
（`Container` 的槽 → 物品实体），求值侧（`effect::collect`）顺着身体槽位找到物品实体，
再去蓝图取修正与行为：

- 一件装备的数值只有**一处**（蓝图那一行），不会出现"实体上的加成与表里的不一致"；
- 卸下 = 把物品实体从槽里摘出来，加成**自动**消失，不需要"把授予的效果再找回来删掉"；
- 状态效果仍走 `EffectOf` 实体（它们有自己的时间轴与解除路径）——两条关系各管一摊（ECS42）。
- 旧的 `EquippedItem` / `EquippedBy` 关系已在 DsnE18 退役，这里不再讨论。

**修正器补上"作用于哪一项"的维度（DsnE10 的第 ④ 步，DsnE22 ②a 扩展）**

装备落地之前"一条修正作用于什么"是隐含的：唯一的调用点折算速度。一把剑的"攻击 +3"
如果被折进速度，玩家移动会凭白变快——**数值串味且不会报错**。两条路：给每项数值各配
一个组件（`AttackModifier`/`DefenseModifier`/…），或给修正加一个"作用于哪一项"的字段。
按 `LECS22`（加第 N 个要改几处）选后者；现在形状是
`Modifier { stat: StatId, source: EffectSource, op: ModifierOp, value }`，
`StatId` 与 `EffectSource` **正交**（`source` 回答谁带来的、`stat` 回答加到哪一项）。
DsnE22 ②a 把词表从封闭枚举改成了运行时词表（`STATS` / `StatId`），加属性只改词表一行。

**掉落表进物种行**

`MonsterTemplate.loot: &'static [LootEntry]`（口径同 `abilities` / `spawn`：一个物种一行）。
每条目**独立**掷概率，再掷数量；顺序固定为"概率 → 数量"，所以同种子永远同一串掉落。
武器与模板碎片只从怪物掉、材料只从怪物掉、地面只发装备与消耗品——
这样"刷图"与"打怪"各有产出，不会两个渠道发同一种东西。

**代价**

1. **`ItemId` 是 16 位数而不是类型化的枚举**：写 `ItemId(999)` 能编译，运行期 `def()` 才 panic。
   换来的是"号段与 GAME.md 表格一一对应、加物品不改类型"。DsnE22 阶段 ① 之后换成
   `assets/items.json` + 启动期加载校验（`def()` 对未知编号仍 panic）；校验对整张表做一遍。
2. **背包是"七部位 + 12 格行囊"而不是定长背包**（DsnE18 / DsnE21）：改容量只需改
   `ContainerDef` / 蓝图，不用改详情页翻页逻辑；身体部位本身就是固定槽位。
3. **装备可以改 HP/MP 上限**（DsnE23）：上限 = 等级曲线 + `max_health`/`max_mana` 修正，
   `apply_maxima` 是唯一写者（装备变化 / 升级 / 读档时重算）；早期"防御顺带加血"的口径已删除。
4. **消耗品不再是"拾取即自动用"**：地面拾取全显式（DsnE21 阶段 4），拿取时消耗品在对应资源
   未满时自动用 1 个（`take_from_ground`），也可在背包页按 `r` 使用。

**关联：** GAME.md Gm4 / Gm7 | DsnE10（求值五步，第 ④ 步补 `Stat`）| DsnE12 / ECS42（两条归属关系）|
DsnE13（身份判据：索引还是实体）| DsnE8（AV：武器改基础耗时）| DsnE18 / DsnE20 / DsnE22 / DsnE23 | `LECS22`

**状态：** 已定 — 规则层**已落地**（注册表/堆叠/容器/装备/掉落/消耗品/行为数据/耐久）。
**未落地**：合成（模板碎片）、技能与卷轴（H10）。
<!-- &/DsnE16 -->
---

<!-- &DsnE17  下楼=用既有初始化系统重建层内容；玩家保留；楼层内容由 (seed, floor) 完全决定 -->
### DsnE17 楼层推进：换层 = 保留玩家、重建其余

**决策**

下楼(`>`，`world/floor.rs`) 不是"再建一个世界"，也不是"在原世界叠加"，而是：

```text
清场（怪物/地面物品/楼梯/格子效果）
  → FloorNumber += 1
  → generate_map → spawn_stairs → spawn_cell_effects → spawn_ground_items → spawn_monsters
  → 玩家搬到新出生点 + 行动状态归零 + 视野/探索记忆重置 + 占用图/格索引重建
```

**复用既有初始化系统**而不是另写一套生成：避开出生点、避开楼梯、避开泥潭这些约束
都写在各自系统里，复制一份必然分叉（ISSUES ECS25 那类重复表示）。

**清场用白名单**（`With<Monster>` / `GroundItemAt` / `With<Corpse>`（含尸体容器里的物品）/
`Stairs` / `CellEffectAt`），**不用** `world.clear_entities()`：玩家、玩家身体容器与行囊里的
物品实体、它名下的持续效果（`EffectOf` 子实体）必须活着——"保留玩家"就是这条白名单。
尸体被清掉时其容器里的物品也要一起 despawn（否则会留下"没有位置也没有主人"的孤儿实体）。
旧的 `EquippedBy` 关系已退役（DsnE18：装备就是身体槽位里的物品实体），不在白名单讨论范围。

**被否方案**

- **被否：`new_game` 重开。** 那会把玩家一起重置（等级/背包/装备全丢）。
- **被否：原地增量替换**（只换地图，怪物逐只替换）。等价于在同一个世界里手工维护
  "哪些是本层内容"，而白名单清场已经把这件事说清楚了。
- **被否：把下楼做成一个行动**（有 AV、占 action 实体槽位）。换层不是某个 actor 做的事，
  给它 AV 会引出"下楼时其他行动还在排队怎么办"这种无意义的问题。

**取舍（都写进代码注释）**

1. **持续效果跨层保留**（中毒不会因为下楼而解）。要改成清空，得先回答"换层算不算经过
   时间"（AV 时间轴要不要跳），那是另一条设计决策。
2. **写 `Position` 的第三个文件**：`world/floor.rs` 把玩家搬到新出生点。它属于**构造期
   写入**（与 `world/init.rs` 的出生点同类），不是运行期的移动仲裁——运行期仍然只有
   `action/settle.rs` 一处。`docs_check.py` 的白名单按这个区分扩到三个文件。
3. **楼层内容由 `(seed, floor)` 完全决定**：同一颗种子下第 3 层永远一样，与你前两层做过
   什么无关。这是"存档只存 seed + floor + 玩家状态"的前提（DsnX17 的确定性口径）。

**关联：** GAME.md Gm12（玩家口径）| DsnE16（物品跨层保留）| DsnE2（占用图每轮重建）| DsnE8（行动系统）

**状态：** 已定 — 已落地（`ecs_core/src/world/floor.rs` + `PlayerCommand::Descend`；
7 条 `floor_tests` 覆盖"不在楼梯上不下楼/玩家与背包装备保留/层内容换新/派生数据重建"）。
<!-- &/DsnE17 -->
---

<!-- &DsnE18  容器=带过滤的槽位；物品=实体+实例组件；身体/行囊/尸体/箱子都是容器（评估稿，待拍板） -->
### DsnE18 容器与槽位：身体 / 行囊 / 尸体 / 箱子

> **状态：方向已定，落地前待拍板（见文末「开放决策」）。** 本条目是用户 2026-11 提出的物品/容器模型
> 的形式化 + 评估；**尚未改任何代码**。物品规则层现状见 DsnE16。

**用户提出的方向（原话要点）**

- 物品**很少直接掉到地上**；**尸体算容器**；箱子等多种容器。
- 物品一定会需要**自己的数据**（耐久/附魔…）；**物品实体化不算问题**。
- **身体（Body）就是一种背包**：固定槽位（头、胸…），每个部位只接受**带对应过滤的**物品，
  一个部位只放一件，放进去之后该槽位「满」。
- 行囊（如「旅者行囊」）也是背包，只是**没有过滤**，满时挂 `Full`。
- `Full` 是**相对的**（将来引入重量后语义会变）。

**形式化：每样东西该存在哪（按 DsnE13 的身份判据）**

| 概念 | 载体 | 判据 |
|---|---|---|
| 物品定义 | 静态表 `ItemDef`（已有） | 数据；无中央分派 |
| 物品实例 | **实体** + `Item { id }` + 0..n 个实例数据组件（如 `Durability`） | 需要身份与实例态，且要被容器引用 |
| 容器布局（几个槽、各接什么） | 静态 `ContainerDef { slots: &'static [SlotDef] }` | 内容：同类容器的布局相同 |
| 容器当前内容 | 组件 `Container { def, slots: Vec<Option<Entity>> }` | 运行时状态；不占格、不进渲染抽取 |
| 槽位过滤 | `ItemDef.tags` ∩ `SlotDef.accept` ≠ ∅ | 两侧必须共享同一份词汇 |
| 「满」 | **派生**：`free_slot_for(tags).is_none()` | 满与否取决于**要放进什么**，不是一个静态事实 |

**评估**

1. **物品实体化 —— 采纳。** 地面物品已经是实体，行囊是唯一还在用数据堆（`ItemStack{id,count}`）的地方。
   实例数据用**组件**表达（`Durability { current, max }`），不需要一个 `ItemData` 大结构：
   这是 ECS 的自然答案，代价只是存档时"要枚举哪些实例组件入档"（教程 ch11 的 `serialize_individually!`
   就是这件事）。**结论：把行囊从数据改成容器装实体，是这条方向的前提，不能只改一半**
   —— 否则"物品"有两种表示，其中一种（数据堆）无法携带实例数据。
2. **槽位存法 —— 建议「槽位 = 数据 + 静态 `ContainerDef`」，不建议「槽位 = 实体」。**
   - 一个 27 格箱子 = 27 个实体；身体 + 行囊 ≈ 20 个实体；而槽位除了"这个位置接什么 + 现在有谁"之外
     **没有独立身份**（DsnE13 的判据：不需要独立身份 → 索引）。
   - 槽位实体让存档变成"存一棵实体树"；数据槽位是一段可直接 serde 的数组。
   - 槽位实体唯一多给的是"给槽位挂组件"（按部位受伤等）；真需要时再加一份 `SlotState` 数组即可，
     不必现在为它付实体成本。
3. **过滤不能靠"组件类型"——这是对原设想的一处必要修正。** Rust/bevy 没有"把组件类型存成值再查询"的能力
   （除非引入 `Reflect`），所以"槽位带一个过滤组件、物品带同种组件"要么需要反射，要么退化成
   `match 槽位种类` → 中央分派，正是本项目一直在灭的东西。**正确形状是两份同源词汇求交**：
   `ItemDef.tags: &'static [ItemTag]` 与 `SlotDef.accept: Accept::Any | Accept::Only(&'static [ItemTag])`。
4. **`Full` 作为权威标记 —— 建议先不做。** 理由两条：
   - 它与"槽位里有谁"是**同一个事实的两种写法**，权威标记即第二个真相（ECS25 / ECS42 的老问题）；
   - "满"依赖**要放进什么**：有过滤的容器对头盔可能满、对靴子却空着，单个 `Full` 标记表达不了。
   需要"快速筛空容器"时（UI 灰显 / AI 搜刮），再做**由系统重建**的标记——本项目已有三处同类派生索引
   （`OccupancyMap`、`CellEffects`、`CellMemory`），照抄即可；那时 `Full` 的语义也可以从
   "无空格"改成"无槽能接这一类"，不必改类型。
5. **身体作为容器 ⇒ 与现有 `EquippedBy` 重叠 —— 本次最大的结构决定。**
   两条机制都能表达"物品挂在 actor 身上"，并存就是"第二入口"。建议**统一到容器**
   （装备 = 身体槽位被占用），但必须连带定两件事：
   - **owner 死亡时装备怎么办**：现在 `EquippedBy` 不级联是**类型级**保证（DsnE12/ECS42 的成果）；
     容器化之后要靠"尸体容器接管"或"槽位随容器消失 + 物品变散落"来保住同一语义，不能靠自觉；
   - `EquippedItem.slot` 字段退役：槽位由容器的 def 给出，不再由物品自己声明。
6. **槽位容量现在就要写成字段。** 即便今天恒为 1，`SlotDef { capacity: u32 }` 也让"引入重量/堆叠"时
   只改数据、不改形状；`Full` 的判据随之从"有谁"变成"占用 ≥ 容量"。
7. **尸体作为容器 ⇒ 掉落路径改变。** 怪物死亡不再直接把战利品撒在地上，而是造一个容器实体，
   把战利品（以及它身上的装备）放进去；"搜刮尸体"成为获取途径——内容感好，但需要拾取动作/UI。
   过渡期可以沿用现有口径（踩到该格即自动把容器内容转移进背包），UI 落地后再改成显式搜刮。
8. **地面不做容器。** 保留"散落物"（`GroundItemAt` + `Item` 实体）：格子是空间索引，把每格做成容器
   等于常驻 4800 个容器。地面散落物的正确定位是"容器链条的末端"，而不是另一个容器类型。
9. **必须防的三类新错误**（每条都要有测试）：
   - **环形包含**：把行囊塞进它自己（或它的子孙容器）→ 转移前做"目标不是自己的子孙"检查；
   - **容器内物品带 `Position`**：会让渲染抽取与占用图漂移 → 物品进容器时必须没有位置表达；
   - **两处记录同一条事实**：`Container.slots` 与物品上的 `ContainedIn` 关系谁是权威？只能选一个
     （建议：**容器格子是权威**，物品不挂反向关系；"我在哪个容器"用查询得到，不另存）。

**对现有代码的落点（不是现在的实现，是迁移清单）**

| 文件 | 改什么 |
|---|---|
| `item/registry.rs` | `ItemDef.tags`（过滤词汇）+ 逐行填 |
| `item/container.rs`（新） | `ItemTag` / `SlotAccept` / `SlotDef` / `ContainerDef` / `Container` + 原子操作（`insert`/`remove`/`free_slot_for`/`find_by_tag`）+ 环检查 + 测试 |
| `item/mod.rs` | `Item { id }` 取代 `GroundItem { stack }`；拾取改为"把实体移进容器" |
| `item/inventory.rs` | **删除**（被 `Container` 取代），`inventory_tests` 改写成容器用例 |
| `effect/collect.rs` | 装备加成从"身体容器的槽位"读，不再走 `EquippedBy` |
| `effect/ownership.rs` | `EquippedBy` / `EquippedItems` 退役（或改由尸体容器接管） |
| `system/death.rs` | 尸体容器 + 战利品入容器 |
| `world/init.rs` | 玩家挂身体容器；旅者行囊作为物品实体放在身体的「背包」槽 |
| `presentation/extract` | HUD 的"装备："与背包占用从容器读 |
| `examples/playthrough` | 背包峰值指标改成容器占用 |

**决策（已拍板，m00094 / m00401 / m03226）**

1. **装备并入身体槽位、退役 `EquippedBy`**：是。两条机制并存 = 第二入口，且这正是 ECS42 想根除的形态。
2. **尸体取代"掉落在地上"的档位**：(a) 战利品直接落地、尸体只装怪物身上的装备。
3. **槽位存法**：数据 + 静态 def（`Vec<Option<Entity>>`），不做槽位实体。
4. **`Full`**：派生（`free_slot_for`），不做权威标记。
5. **`ItemTag` 初版词汇**：`Head / Chest / Weapon / Shield / Feet / Ring / Pack`（后续只增不改）。
6. **身体槽位与行囊容量**：身体 = `头 / 躯干 / 主手 / 副手 / 脚 / 手指 / 背包`，旅者行囊 = 12 个无过滤格。
7. **拾取全显式**（m03226）：踩上不自动拿，背包页「地面」栏 `Enter` 拿；尸体同样显式搜刮。
8. **多具尸体分组**：同格多具尸体在「脚下」栏按尸体分组标题列出，物品行带尸体下标。

**状态：** 已定 — 三片全部落地：
① 词汇 + 容器骨架 + 身体/行囊（含装备统一，退役 `EquippedBy`）；
② 尸体容器与（显式）搜刮；
③ 实例数据与存档投影 —— 由 DsnE22 阶段 ④ 接替实现（耐久实例 + 武器成长 `WeaponProgress`，
   存档升到 V2 → V4，见 DsnE19）。箱类容器（放置/打开）仍未做。

**关联：** DsnE16（物品规则层现状）| DsnE13（身份判据：索引还是实体）| DsnE12 / ECS42（两条归属关系）|
DsnE17（层内容由 `(seed, floor)` 重建，决定了"存档只需存玩家容器树"）| `ecs_core/RESEARCH-item-systems.md`
**落地记录（2026-11，第①片）：**
- **物品实体化**：`Item { id }` + `Amount`（一堆一个实体），`spawn_ground_item` 是"物品出现在世界里"的唯一入口；
- **身体 = 容器**：玩家挂 `BODY`（七部位），旅者行囊作为**一件物品**塞在「背包」槽里、自带 12 格容器；
- **旧四槽位模型退役**：`EquipSlot` / `EquippedBy` / `EquippedItem` 删除，装备 = 身体槽位里的物品，
  求值侧（`effect::collect`）遍历身体槽位读注册表——"移出槽位 = 加成消失"不需要任何清理代码；
- **拾取路由**：装备进部位、材料/已占部位的装备进行囊（先并堆再开新格）、消耗品即时用 1 个、
  放不下就留在原地并记日志；
- 新增内容：皮帽（头，防御 +1）、旅者行囊（背包）；
- 验证：`cargo test -p ecs_core` 212 passed、workspace 全绿、`gate.ps1` 10/10、fuzz 6 种子无异常。
- **背包页（R2 只读版）**：`e` 开关；左栏身体（部位：物品）、右栏行囊（物品 ×数量 + 占用 n/12）；
  `↑/↓` 移光标。视图在 `presentation::extract::inventory_view` 每帧从**容器**重建，
  后端（tui）只负责画 `InventoryView`，因此界面与规则不可能分叉。
- **背包页操作（R2 完成）**：`Enter` = 身体行卸下 / 行囊行装上（部位已占则**交换**：旧装备收回行囊），
  `r` 使用消耗品、`d` 丢弃到脚下。核心动作在 `item::actions`（6 条用例：装、交换、卸下被拒、
  丢弃落地、使用消耗、三类错误），应用层再用一条端到端用例把「按键 → 页栈 → 世界动作」串起来。
  这些动作**不消耗回合**（换装不是行动）；将来若要耗时，是加一次世界推进而不是塞进行动链路。
- **仍未做**：实例数据（耐久等）——由 DsnE22 阶段 ④ 完成（`durability_max` + `Durability` 实例组件）。
  **落地记录（2026-11，第②片，尸体容器）：**
- **尸体 = 容器 + 世界对象**：`Corpse { kind, name }`（`monster` 模块）+ `Container::new(&CORPSE)`（4 个无过滤格），
  带 `Position`、**不带 `Blocks`**（不挡路）；渲染用独立外观 `VisualKey::Corpse`（`%` 灰色，契约 V2）、
  名字读作「哥布林的尸体」（`VisualLayer::Item`，探索过即画）。
- **战利品口径 (a)**（用户拍板）：模板 `loot` 仍落在地上；`MonsterTemplate.equipment`（新增字段）装它**身上的装备**，
  死亡时进尸体。当前内容：哥布林带匕首（id 24）、蘑菇傀儡带石锤（id 23），其余物种为空。
- **搜刮是显式动作**（玩家反馈：默认全拾取太粗暴）：踩上去什么都不发生；按 `e` 打开背包页，
  Nav2 选「尸体」页签、逐件 `Enter` 拿取（`item::actions::take_from_corpse(world, corpse, slot)`，
  失败回滚到尸体原位）。同格多具尸体时按尸体分组标题列出，物品行带该尸体的确定性下标
  （按实体编号升序，与核心动作同源）。
- **下楼清场**：`despawn_floor_contents` 把尸体**连同它装着的装备**一起清掉——只清尸体不清内容会留下
  "没有位置、没有主人"的物品实体。
- 验证：`cargo test -p ecs_core` 222 passed（+3 尸体用例、+1 下楼清场用例）；gate 见提交记录。
- **仍未做**：箱类容器（等放置/交互）；实例数据已由 DsnE22 阶段 ④ 完成。
<!-- &/DsnE18 -->

<!-- &DsnE19  存档 V4：bincode 完整快照 + 手写投影；只存 seed/floor/玩家状态/容器树/物品实例差异（耐久、武器等级经验）/当前生命法力；F5/F9 -->
### DsnE19 存档 V4：投影式快照（seed + floor + 玩家容器树 + 物品实例差异 + 当前生命法力）

**决策**

1. **只存"重开一局后无法重算"的东西。** 地图、怪物、楼梯、地面物品、格子效果都由
   `(map_seed, floor)` 完全决定（DsnE17 / DsnX17）——读档时重新生成，不写盘。存档因此只有
   几百字节，也不会把生成器的中间状态变成磁盘契约。
2. **bincode + 手写投影。** DsnX10 已定"bincode 完整快照、不做增量"；但"完整"不等于
   `derive(Serialize)` 整个世界：世界里有 `Entity` 引用与派生资源，derive 会把它们也变成契约。
   改用手写投影：

   ```text
   文件头（8 字节）：magic b"DNSV" + 版本 u32（小端）
   载荷：bincode(SaveV4)
   SaveV4 { items_schema, map_seed, floor, player: PlayerSave }
   PlayerSave { x, y, level, experience, health, mana, magic_mastery, body: [Option<ItemSave>] }  // health/mana 只存 current
   ItemSave   { id, count, overrides: ItemOverrides, slots: Option<[Option<ItemSave>]> } // 容器树递归
   ItemOverrides { durability: Option<f32>, weapon: Option<WeaponSave> }
   WeaponSave { level, exp }                   // 武器养成进度（DsnE24 阶段 ②）
   ```

   `items_schema` 是物品数据版本（`ecs_core/assets/items.json` 的 `schema_version`）：读档时
   与编译期内嵌的 `ITEMS_SCHEMA_VERSION` 不等就报错，避免把旧数据装配成新实体。

   `Entity` 不上盘；容器槽位本身是数据（DsnE18），树直接摊平。版本放在**文件头**而不是
   bincode 载荷里：bincode 1.3 不是自描述格式，结构一变旧档就解不出来——版本若在载荷里，
   读旧档时永远走不到它，只能报"编码失败"。头部先读、版本先判，才能报"版本 N 不认识（当前 M）"。
   V2 起 `items_schema` 也进载荷：物品数据版本与编进来的一致才读，否则明确报错（MVP 不做
   迁移，见 DsnE22 已拍板第 3 条）。
3. **不进档的东西（以及代价）。** 地面物品不入档：玩家丢在地上的东西读档后消失，地面回到
   `(seed, floor)` 的初始内容——这是 DsnE17 的直接推论。同理，**中途存档读回的是该层初始状态**
   （已杀的怪物会回来、已捡走的地面物品会重刷）；想要"保留中途进度"得等 V2 的地面/怪物差异段。
   另有两个有意的取舍：**不存 `GameRng`**（读档后随机流从该层开局重来，与"一直玩下去"会分叉，
   这也是"进层检查点"比"随时存档"更自然的另一个理由）。
   V2 只补存"玩家造成的物品实例差异"（目前是耐久当前值与武器等级/经验），地面物品与怪物
   差异仍然不入档。
   V3（DsnE23）起**生命/法力只存 current、不存上限**：上限是"等级曲线 + 装备修正"的派生
   数据，读档时按当前规则重算——否则改公式后旧档会带着旧上限继续跑，出现"同一件装备两个
   上限"。这条替代了 V2 的"上限按值存"取舍。
   V4（DsnE24 阶段 ②）把武器养成等级/经验放进 `ItemOverrides.weapon`：它们由玩家行为改变，
   无法重算；等级带来的攻击/力量上限同样是派生值，读档按蓝图重算。
4. **load 复用初始化与下楼代码，不另写生成管线。** `new_game(map_seed)` 起世界 →
   `FloorNumber = floor` 后走一次"重建层内容"（与 `descend` 共用的函数）→ 还原玩家与容器树。
   任何"重建层"的修正都同时修到下楼与读档两条路径。
5. **文件 I/O 在集成层，不在规则层。** `src/save.rs` 在根 crate（`bincode` 已是根依赖），
   `ecs_core` 不引序列化框架；`presentation` 的键位表只表达"按键 → 游戏命令/页意图"，
   存档是集成层能力，直接在 `App::handle` 拦 `Key::F(5)` / `Key::F(9)`。
6. **失败要玩家看得见。** `SaveError::{Io, Encode, UnsupportedVersion, NoPlayer, Corrupt}` +
   `describe()`，统一写 `EventLog`。失败静默 = 玩家以为存上了。
7. **死亡状态不可存。** `game_over` 时拒绝存档，避免存出"读档即死"的档。

**状态：** 实现（V4）。落地：`src/save.rs`（文件头 magic + 版本，载荷 `SaveV4`；手写投影、
`restore_to_world` 复用 `rebuild_floor`；物品实例差异 = `ItemOverrides { durability, weapon }`；
玩家 `health/mana` 只存 current，读档按 DsnE23 的单一上限公式重算后夹取）、统一装配桥
`spawn_item_assembled` 同时服务系统与读档（武器装配自动带 1 级 0 经验，读档再套等级/经验，
不合法进度报 `Corrupt`）、`App` 的 F5/F9 接线、`tests/save_test.rs`
11 条（roundtrip / 楼层重建 / 版本头 / 物品数据版本 / magic / 未知编号 / 缺文件 /
非法养成进度 / F5-F9 端到端 / 模板装配 == 存档装配）。`SAVE_VERSION = 4`；旧 V1/V2/V3
文件被版本头直接拒绝，`items_schema` 不一致报 `ContentVersion`（MVP 不迁移，DsnE22 已拍板第 3 条）。
**关联：** DsnX10（bincode 快照）| DsnE17 / DsnX17（层内容由 seed 重建）|
DsnE18（容器是数据，可直接摊平）| DsnE16（物品注册表）| DsnE22 阶段 ④（实例差异形状）|
DsnE24 阶段 ②（武器养成进度）|
`ecs_core/RESEARCH-containers-save.md` §3（投影 vs bevy_save/DynamicScene 的取舍、版本放文件头的原因）
<!-- &/DsnE19 -->

<!-- &DsnE20  重量：每单位重量的注册表数据 + 行囊载重上限；装得下 = 有空槽 且 重量不超；派生不存档 -->
### DsnE20 重量与行囊载重：容量是"槽位 × 载重"

**决策**

1. **重量是注册表数据，按单位计。** `ItemDef.weight: f32`（每单位），一堆重量 = 每单位 × 数量
   （`item::stack_weight`）。理由：堆叠物品共用一行注册表数据，按堆记重会让"拿 1 个还是 10 个"
   变成两套字段；注册表是唯一权威，判定与界面都从这一行读。
2. **只有行囊限重。** `ContainerDef.max_weight: Option<f32>`：旅者行囊 `Some(20.0)`，身体与尸体 `None`。
   身体不限重是因为"装备太重让人脱不下来"是纯粹的负体验；尸体不限重是因为玩家不需要背得动才能翻。
   将来重量参与移动速度时再按平衡表调数值——结构上已经能表达。
3. **"装得下" = 有空槽 且 重量不超。** 槽位判据（`free_slot_for`）与载重判据（`accepts_weight`）
   分开，两处都过才允许放进；拾取、从尸体拿取、卸下装备三处走同一条口径。
4. **超重与满格是两句话。** 玩家要知道"是没格子还是背不动"：日志分别记「行囊满了」「行囊超重」。
5. **重量是派生数据，不进存档。** 存档只存物品编号与数量（DsnE19），读档后用注册表重算；
   容器不缓存"当前载重"，避免第二个真相。
6. **界面只显示不判定。** 行囊栏 footer 显示 `n/12 格 · x/20.0 载重`、HUD 一行「行囊载重」、
   物品详情一行「重量」；核心层是唯一判据（界面算错也不影响能不能装）。

**为什么仍然不做"Full 标记"**（与 DsnE18 第 4 条同源）：现在"满"有两个维度（格子、载重），
做成标记就要在每次移动后维护它，而且它对不同物品可能不同；派生判据永远与容器状态一致。

**状态**
- 已落地：`ItemDef.weight`（23 行）、`ContainerDef.max_weight`、`Container::{max_weight,total_weight,accepts_weight}`、
  `item::stack_weight` / `container_weight`；拾取与 `equip_from_pack` / `unequip_to_pack` / `take_from_corpse`
  的载重校验（`ItemActionError::Overweight`）；背包页 / 详情 / HUD 显示。
- 未做：重量影响移动速度、按重量分档的容器（箱子/袋）、投掷物重量（投掷未迁移）。

**测试**：registry 全表重量为正、`stack_weight` 数量换算；container 只有行囊限重、`accepts_weight`
边界；item 超重留地并记日志；actions 卸下/搜刮超重原子失败；presentation 详情重量行、行囊载重、
HUD 载重行。

**关联：** DsnE18（容器/槽位模型）· DsnE16（注册表）· DsnE19（存档只存编号与数量）。
<!-- &/DsnE20 -->

<!-- &DsnE21  事件时间轴：Done 旧→新，Running 按剩余时间降序，底部=0 刻度；Running 由 ActiveAction 投影；背包格 Nav/Context/Detail/Tip -->
### DsnE21 事件时间轴与背包格：一列"截止现在"，一个三维网格

**背景（用户设计稿，m03220 / m03226 拍板）**

界面四栏（DsnA2）里的「事件」栏不是日志，而是一条**时间轴**：旧事件在上、
新事件在下；已经发生完的事实在上方，正在进行中的行动在下方，**最下面可以看成
0 刻度线**。同时背包页从"两栏列表"升级成前端式的 grid：

```text
Nav1          Nav2          Detail
Context1      Context2      Detail
Tip           Tip           Tip
```

被统一掉的语义：过去时（"你击中了老鼠"）与现在进行时（"哥布林举起了匕首"）
都读作**"截止现在这一刻它已经做了的那部分"**。玩家不需要分辨"这条是日志还是预告"。

**决策**

1. **一条时间轴，两段排序。** `Done`：发生时刻升序（旧在上、新在下）。
   `Running`：按**剩余时间降序**（剩 100 AV 的在剩 50 AV 的上面），所以越往下
   越接近"现在"，栏底就是 0 刻度。两段拼成一列交给后端，后端不重排。
2. **事实库在 core，进行中条目在投影层。** 已发生的事实仍由 `EventLog` 记
   （容量从 50 提到 **200**；连续相同消息合并计数）。`Running` 不另存一份状态：
   玩家可见（回合等待输入）时玩家的行动必然已完成，怪物名下的 `ActiveAction`
   还带着 `ActionTimer` 剩余 AV——它**本来就是**"已开始、还没落"的唯一权威。
   投影在 `presentation::extract` 里从 `(ActionOf, ActionName, ActionTimer,
   ActiveAction)` 生成，因此不会出现"事件库说在跑、行动实体已经没了"的第二真相。
3. **"条目可变"表现为同一行动的行在下一帧剩余时间变小。** 行动的终态由规则层
   已有的完成/失败日志落地成 Done 事实；进行中行没有独立 id 与生命周期，
   所以不需要清理、也不进存档（DsnE19 的"只存重开无法重算的东西"）。
4. **事件栏 V1 只滚动、不选中。** 自动贴底（显示最新）；玩家上滚时暂停自动贴底，
   回到栏底恢复。事件内容不参与焦点操作——焦点轮转仍是 DsnA2 的四栏。
   （已落地：`PageStack.events_scroll` + `UiIntent::ScrollEvents` + `SceneFrame.events_scroll`，
   滚动语义见 DsnA6。）
5. **背包页是 3×3 网格。** Nav1 = 身体、行囊（身上）；Nav2 = 尸体、地面（脚下）；
   Context = 当前 tab 对应容器的内容；Detail = 选中物品的详情；Tip = 按键与效果。
   焦点只有 **4 个**：Nav1 → Nav2 → Context2 → Context1（顺时针），Nav 聚焦时
   `←/→` 切 tab，Context 聚焦时 `↑/↓` 选行 + `Enter/r/d`；Detail 跟随"最近一次
   Context 选中"，Tab 不落到 Detail 上（用户拍板）。
6. **地面物品全显式。** 踩上去不再自动拾取（`pickup_system` 退役）；「地面」栏
   列出玩家同格的地面物品，`Enter` 拿一件。理由：随物品变多（重量、实例数据、
   容器）自动拾取会变成"一踩就替玩家做决定"；显式化后拾取错误可以只撤销一次操作。
7. **「地面」是合成视图，不是容器。** 它把同格 `GroundItemAt` 物品拼成一个列表，
   不引入"地面容器"实体（延续 DsnE18 第 7 条：地面不做容器）。尸体是真实容器
   （CORPSE 4 格），同格多具尸体时按分组标题列出。
8. **图鉴（Codex）后置。** 设计稿里的图鉴页需要解锁条件与条目库，本轮不做；
   先把四栏、事件、背包 grid 立住。

**状态**

- 已落地（阶段 1）：四栏几何与焦点进契约（DsnA2 / Gm14）。
- 已落地（阶段 2）：事件行契约（DsnA3，`EventLine{running}`）+ `EventLog` 容量 200 +
  事实/进行中合并排序（presentation 投影，`ActionName::phrase` 出文案）+ tui 事件栏
  渲染（进行中暗色）；进行中只列玩家视野内的行动。
- 已落地（阶段 3）：`Observable` 标记（怪物/楼梯/地面物品/尸体；玩家与容器内
  物品不标）、信息栏 `InfoView` 契约（DsnA4）、presentation 按「近→远、同距方位
  顺时针」排序（正上=0）、`PageStack.info_cursor` 选行与地图反色高亮。
- 已落地（阶段 4a，地面全显式拾取）：`pickup_system` 退役出结算链（踩上不再自动捡）、
  新核心动作 `take_from_ground(world, index)`（路由沿用旧拾取：空部位装备 / 消耗品在资源
  不满时用 1 个 / 其余进行囊并堆后开格；超重与满格分别报 `Overweight`/`NoRoom` 并留原地）、
  背包页新增「地面」栏（`InventoryTarget::Ground { index }`，行序=同格地面物品按实体编号
  升序，与动作同源）、`Enter` 拿取；尸体仍走 `take_from_corpse`。
- 已落地（阶段 4b，背包 3×3 grid）：契约 `InventoryView` 换成
  Nav1/Nav2/Context1/Context2/Detail/Tip（DsnA5，契约版本 6）；Nav1=身体/行囊、
  Nav2=尸体/地面（按脚下情况出现）；4 个焦点位顺时针
  Nav1→Nav2→Context2→Context1、Nav2 不存在时跳过其两格；导航栏 `←/→` 切页签、
  内容栏 `↑/↓` 选行；每条 Context 保留各自光标；tui 网格渲染（窄屏两条 Context
  上下堆叠、详情先右侧后底部）。
- 已落地（补齐，2026-11）：
  - **事件栏滚动**（决策 4）：`PageStack.events_scroll` + `UiIntent::ScrollEvents`
    （↑=看更早、↓=回最新，夹到 0）+ `SceneFrame.events_scroll`（DsnA6，契约版本 8）；
    tui 按偏移裁窗口、标题标 `↑N`，偏移超过可滚上限时按可见高度夹取。
  - **多具尸体分组**（决策 7）：`take_from_corpse(world, corpse, slot)` 按同格尸体
    的实体编号升序取下标；「尸体」页签下每具尸体一个分组标题（不可操作），物品行
    带 `Corpse { corpse, slot }`；`take_from_ground` 拆堆也走唯一装配桥
    （`spawn_item_assembled`），不再手工拼物品实体。
- 未做：图鉴页（放一放）。

**测试**

- 事件：Done 事件旧→新、Running 按剩余时间降序排在 Done 之后；玩家自己空闲时
  不出现玩家 Running 行；行动实体消失后对应 Running 行不残留。
- 背包 grid：焦点顺序与 Nav2 跳格、←/→ 切页签、每 tab 的 Context 保留各自选中、
  地面物品不再因踩踏自动消失、Enter 从地面拿一件（超重/满格留在原地）、
  键盘端到端（装/卸、拿地面、搜尸体）。
- 渲染：事件栏显示 Running 行且带剩余 AV、贴底行为、0 刻度在栏底。

**关联：** DsnA2（四栏布局与焦点）· DsnE18（容器/槽位/地面不是容器）·
DsnE19（存档边界）· GAME.md Gm14（界面）
<!-- &/DsnE21 -->

<!-- &DsnE22  物品系统重构：蓝图（数据）→ 唯一装配桥 → ECS 实体（组件）；动态在数据与实例，组件类型保持静态 -->
### DsnE22 物品系统重构：蓝图、装配与实例组件

**背景**

用户判断（m05071 / m05072）：物品是本作主要机制与玩法载体，属性结构必然是
"长尾专用多、高频通用少"；当前 `ItemDef` 静态表 + `ItemKind` 枚举 + `Stat`
四变体无法负载未来内容，必然要动一次物品系统。约束两条：与 ECS 相适应
（主体逻辑在系统中，与数据紧耦合的逻辑可留在方法里）、与丰富属性相适应
（结构体加字段与"预制体静态表"本质相同，必须用动态类型）。用户提出的形态：
动态类型、序列化/反序列化、重建物品实体与属性组件、逻辑在系统；前两个是
"动态预制体库"，第三个是桥。

我的拆解（m05075）：封闭性有三层——`ItemKind` 枚举、`Stat` 词表、
`ITEMS` 编译进二进制。路线 A（bevy_reflect 运行时组件）能拿编辑器/Mod 级
灵活，但 ECS 查询仍是静态的、存档 Entity 重映射被 DsnX10 否掉；路线 C
（嵌入式脚本）调试、确定性、存档成本都高；选 **B：数据驱动定义 + 系统解释**。
用户 m05076 确认 B，并补充"组件按需从存档读取或从模板建立，用版本号保证一致性"。

外部参考（m05079 的判断，未另做探索式调研）：Minecraft Java 1.20.5+ 的
data components（物品 = 原型 + 组件补丁、组件类型代码注册而取值由数据供给、
`attribute_modifiers` 的 add / multiply_base / multiply_total 三档操作、
`!` 显式移除、未知组件报错）在**语义层**验证了"原型 + 补丁"与三档 op；
Bedrock 的 item components JSON 验证"配置只描述"。实现技术（DFU Codec、
网络同步、DataFixer）不借鉴。

**决策**

1. **模型固定为三段：蓝图（数据）→ 装配（唯一桥）→ ECS 实体（组件）。**
   蓝图只描述、不出现控制流；行为语义在系统；装配函数是唯一把蓝图翻译成
   组件的地方，模板与存档共用它——禁止第二套"从存档直接灌组件"的路径，
   否则两条路迟早分叉。
2. **动态性放在数据与实例，不放在 Rust 类型上。** 不做运行时注册的组件类型
   （那需要反射，且系统查询依旧静态）；加属性、加效果 = 改数据；加全新组件
   语义 = 加代码，并在数据里留 id 占位。
3. **属性用动态词表 + 稠密组件，不做"每属性一个组件"。**
   `StatId(u16)` + `StatRegistry`（`StatDef { key, label, default, min, max }`）；
   物品实体挂稳定的 `Stats`（按 StatId 稠密存放基础值）与
   `Modifiers(Vec<Modifier { stat, op: Add | MulBase | MulTotal, value }>)`。
   `ItemKind` 枚举退役，换成 `Traits(Vec<TraitId>)` + 结构组件（Container 等）。
   理由：`Query<&AttackBonus>` 式写法会让每加一个属性都改代码，且每个物品的
   archetype 数量爆炸。
4. **行为是声明式数据。** `BehaviorDef { id, trigger: OnUse | OnEquip | OnHit |
   OnTurn, effects: [Effect] }`；`Effect` 是白名单指令（加修正、伤害、治疗、
   给状态、生成实体、写事件、条件分支）。表达力上限：能用"触发 → 效果列表"
   表达则进数据，否则写系统并留数据 id 占位；解释器不做成小型脚本语言。
5. **装配时机是"进入世界那一刻"，不做惰性。** 开局、刷怪掉落、下楼重建、
   读档都立即装配好组件；惰性首次查询时建会让执行顺序影响结果，破坏 DsnX17
   的确定性。UI 缓存只能是缓存层。
6. **实例差异只存玩家造成的部分。** 稀疏实例组件（耐久、充能、附魔等）在装配
   时挂；存档存 `blueprint_id + overrides + save_schema_version`，基础数值永远
   跟蓝图走。读档后内容更新会让老物品按新蓝图解释——这是取舍，要写进文档。
7. **堆叠规则提前钉死。** 同蓝图且实例差异等价才可合并；默认"实例差异非空"
   的物品不堆叠（有耐久差异的武器不叠），需要时再按白名单放宽。否则容器、
   拾取、存档三处会各写一套合并逻辑（Minecraft 的"组件等价才叠"是同一结论）。
8. **版本号分两层，作用不同。** 数据 schema 版本（蓝图文件头；未知版本 =
   启动失败）；存档格式版本（`SAVE_VERSION` 升到 v2，读档时迁移或明确拒绝）。
   蓝图 id 只增不删，删除留墓碑——否则老存档引用会静默变义。
9. **加载期校验，失败即启动失败。** id 唯一、引用存在、数值有限且在
   `[min, max]`、trigger / effect 在白名单、容器蓝图无环；确定性要求数组而非
   HashMap 迭代、effect 保持声明顺序。
10. **不开放 `custom_data` 式自由 blob。** 长尾属性走词表、新数据加显式字段；
   自由 blob 等于给自己开一个绕过校验的后门（MC 需要它服务 Mod 生态，我们
   不需要）。

**形状**

```text
BlueprintRegistry (Resource，加载 + 校验)
  StatRegistry: StatId -> StatDef { key, label, default, min, max }
  BehaviorDef: id, trigger, effects: [Effect]
  ItemBlueprint: id, name, desc, tags, weight, max_stack,
                 stats: [(StatId, f32)], behaviors: [BehaviorId],
                 container: Option<ContainerDefId>

物品实体:
  BlueprintRef(BlueprintId) · Amount · Stats{ values } · Modifiers
  · Traits · Container(可选) · Durability / Charges…（稀疏实例组件）

装配: assemble(blueprint, overrides) -> 组件集合         // 唯一桥
存档: { blueprint_id, overrides, save_schema_version }   // 不存 Entity、不存全量组件
```

**分阶段迁移（每阶段独立过 gate）**

1. **数据外置**：`ITEMS` 常量 → 数据文件（实现用 `assets/items.json`：`serde_json` 已是依赖），
   默认 `include_str!` 内嵌 + 可选外置路径，补加载/校验/测试链路；`ItemId` 语义改
   `BlueprintId`；行为与数值保持完全不变。验收：全表加载、校验失败用例、现有测试全绿。
2. **动态属性**：`StatId` + `StatRegistry` + 稠密 `Stats` + `Modifier.op` 三档；
   迁移现有 4 个属性；求值、详情、HUD 全走注册表。验收：新增一条属性只改数据；
   未知 StatId 报错；三档 op 求值测试。拆成两片：**②a** 属性词表 + 算子（行为零
   变化，数据 schema 升 2）；**②b** 稠密 `Stats` 组件 + `[min,max]` 夹取。
3. **行为数据化**：先把蘑菇（Heal）、海藻（RestoreMagic）、踏水靴（ignores
   terrain）迁成 trigger → effects 三个切片；验收：与现有行为逐条等价，事件
   日志文案不变。
4. **实例组件 + SaveV2**：耐久等稀疏组件；存档 v2（blueprint + overrides +
   schema_version）。验收：不变量测试「模板装配 == 存档装配（无差异时）」、
   版本不符拒绝读（MVP 不做迁移）、蓝图 id 缺失时明确报错。

**已拍板（用户 m05120）**

1. 蓝图 id **只允许构建期注册**（运行时热加不做）。
2. 属性值只允许有限 f32，且在 `[min, max]` 内；加载期拒绝 NaN / ∞ / 越界。
3. **MVP 不承诺存档跨内容版本兼容。** 现在仍处原型阶段、内容格式版本尚未定过，因此
   MVP 不引入迁移框架、不讨论适配策略：存档随格式变化升 `SAVE_VERSION`，旧档不保证
   可读；读档时蓝图 id 缺失或当前数据校验失败 → 明确报错，不静默改写。

**状态：** 设计定稿（2026-11-06）。三点已拍板（m05120）。
阶段 ① 数据外置**已落地**：物品表从 `registry.rs` 的 Rust 字面量搬到
`ecs_core/assets/items.json`（`include_str!` 内嵌 + `LazyLock` 首次访问时加载），
加载器逐行校验（schema 版本、编号严格递增、名字/描述非空、重量有限且 `(0, 10]`、
堆叠上限、装备/容器不可堆叠、攻速与消耗品数值为正、Pack 布局存在、未知字段/
标签/类型直接报错）；`Stat`/`EffectSource`/`Modifier`/`ItemTag`/`ConsumeEffect` 加 serde。
阶段 ②a 属性词表**已落地**：新增 `rules/stat.rs`——`StatId(u16)` + 静态
[`STATS`] 词表（`StatDef { id, name, label, is_ratio, min, max, default }`，数据里用名字
序列化、未知名字加载期报错）；`Modifier` 从"两个数值位"改成**一条修正一个算子**
（`op: Add | MultiplyBase | MultiplyTotal` + `value`），折叠式
`(base + ΣAdd) × ΠMulBase × ΠMulTotal` 与旧口径逐值等价；数据 schema 升 2
（`{"stat":"attack","source":"equipment","op":"add","value":3.0}`）；加载器拒绝
未知属性、非有限修正值、非正乘区因子。当前词表仍在代码里（加属性 = 加一行表 +
数据引用），把词表迁进数据留给后续阶段。阶段 ②b 稠密属性表**已落地**：六个独立组件
（`Attack`/`Defense`/`MoveSpeed`/`AttackSpeed`/`CritRate`/`CritDamage`）并成一份
`Stats` 组件（按 `StatId` 下标稠密存放，`get`/`set`/`from_pairs`/`with`）；词表补
`crit_rate`（[0,1]）与 `crit_damage`（[0,10]，是暴击伤害**加成**）、每项加 `default`
（未装配基准）；`set` 是唯一的夹取缝（忽略 NaN，其余按 `[min,max]` clamp）。
生成点（玩家/怪物/测试/驱动器）与读取点（战斗、经验、行动速度、HUD）全部改走
`Stats`；物品的数值加成仍走 `Modifier`（阶段 ③/④ 再迁物品基础值）。
阶段 ③ 行为数据化**已落地**：`ItemDef` 增加 `behaviors: &'static [Behavior]`
（`Behavior { trigger: Use | Equipped, effects }`）；删掉 `ConsumeEffect` 与
`ItemKind::Equipment.ignores`，改由 `BehaviorEffect::{Heal, RestoreMagic,
IgnoreSource}` 表达全部三条现有行为（蘑菇/海藻的 `use` 回复、踏水靴的
`equipped` 无视地形）；加载器按**触发/效果矩阵**校验（`use` 只能出现在消耗品上且
只能带回复效果、`equipped` 只能出现在装备上且只能带 `ignore_source`、消耗品必须
至少有一条 `use` 行为、空行为与未知触发/效果直接拒绝），所以解释器不需要兜底分支。
求值侧（`EffectContext::ignored_sources`）与使用/拾取路由改为读行为表，
不再认识具体物品编号；数据 schema 升 3。
阶段 ④ 实例组件 + SaveV2 **已落地**：`ItemDef.durability_max`（数据 schema 升 4，只有装备
可声明、加载器校验为正有限数）、`Durability { current, max }` 稀疏组件（装配时满值、夹在
`[0, max]`）；唯一装配桥 `ItemSpawner` + `spawn_item_assembled`（系统走 `Commands`、读档走
`World`）；存档升 `SaveV2`（`items_schema` + `ItemSave.overrides`，只存玩家造成的耐久差异），
读档先按蓝图装配再套差异；`SAVE_VERSION = 2`，旧 V1 档被版本头拒绝、`items_schema` 不一致
报 `ContentVersion`（MVP 不迁移）。DsnE23 之后存档再升 V3：生命/法力只存 current，
装配/上限重算仍走同一读档管线。验收不变量测试「无 overrides 时模板装配 == 存档装配」已落地。
四阶段全部完成；后续长尾内容（附魔/充能/磨损规则等）走同一形状增量加，不改结构。

**关联：** DsnE16（现行物品注册表/背包/装备）· DsnE18（容器与槽位）·
DsnE20（重量）· DsnE19（存档边界，V2/V3 在这里扩展）· DsnE10（Modifier 求值）·
DsnX17（确定性）。
<!-- &/DsnE22 -->

<!-- &DsnE23  品质六档 + 生命/法力基线：等级曲线 + 装备修正、单一上限写者、只存 current -->
### DsnE23 品质与生命/法力基线

**背景。** 物品系统原本没有"品质"维度（所有物品平级），生命/法力上限是半成品：由
`max_hp_for(level, defense)` / `max_mp_for(level, mastery)` 直接算出，防御顺带加血、等级
曲线写死两个方向。用户拍板：生命/法力可以重做，不用考虑适配历史（原型阶段）。

**决策**

1. **品质是蓝图分类，不是数值表。** 内部 `Quality` 枚举六档、数据里写 snake_case 名字：
   `Coarse(粗劣) / Common(普通) / Fine(精良) / Rare(稀有) / Epic(史诗) / Legendary(传说)`；
   `Common` 为默认。中文名 + 固定颜色（灰/白/绿/蓝/紫/金）由后端映射，**不做罗马数字**
   （与武器等级撞轴）、**不做 I/I+ 细分**（武器等级已提供细粒度成长）。品质影响"预算/
   学派门槛/EXP 倍率"，不是唯一数值来源；实例不改品质（MVP 不做升品）。
2. **生命/法力是同一个通用池。** `Pool { current, max }`；`Health` 与 `Mana` 是它的
   newtype（`Deref` 暴露字段，既有代码 `health.current` 不动）；以后气力直接加第三个池。
   `Magic` 组件改名 `Mana`，避免"法力资源"与"魔法体系"在讨论里打架。
3. **上限 = 等级曲线 + 修正，防御不再加血。** `max_hp = 40 + 8×(等级-1) + max_health 修正`、
   `max_mp = 12 + 2×(等级-1) + max_mana 修正`；曲线是内容数值，可调。词表新增
   `max_health` / `max_mana`（默认 0、区间 [0, 9999]），装备/附魔/词条通过普通 `Modifier`
   加（木盾 +3、皮甲 +5 已作为真实内容）。防御只做减伤。
4. **单一上限写者。** `rules::vitals::apply_maxima` 是唯一写 max 的函数（升级、装备/卸下、
   每轮兜底、读档都走它）；普通刷新只把 `current` 夹到上限内、**不回血**（穿上再脱下
   +生命上限装备不能变成免费治疗）；升级时传 `restore = true` 回满（存活者，沿用 P1 修复：
   死亡不回血）。
5. **存档只存 current（V3）。** 上限是派生数据，读档后按当前规则重算再夹取。这样改曲线或
   加一件撑上限的装备，旧档不会带着旧上限继续跑。
6. **MP 自然回复延后。** 目前没有法术消耗出口，回复系统随后续法术阶段（法术/附魔/回魔，尚未立项）一起做；
   本阶段只做上限重算与存档口径。

**状态：** 已落地。`Quality` + 加载器（`items.json` schema 5，23 行品质）；`Pool`/`Health`/
`Mana` 与 `Magic → Mana` 改名；`STATS` 两项 + `rules/vitals.rs`（曲线、单一写者、每轮刷新）；
`experience` 升级回满、`init` 出生满池、装备动作后即时刷新；UI 物品行按品质着色、详情加
「品质：」行（契约 `UiTextStyle::Quality(u8)`，渲染契约版本 7）；存档 V3。测试：`vitals`
单测、`components` 池测试、注册表加载校验、存档 roundtrip/版本/装配等价。

**关联：** DsnE16 / DsnE22（物品蓝图与实例）· DsnE19（存档 V3）· DsnE20（重量）·
DsnX17（确定性）· Gm4 / Gm7 / Gm13。
<!-- &/DsnE23 -->

<!-- &DsnE24  物理伤害：攻击+力量加成，乘增伤/暴击/护甲(δ/(Def+δ))/抗性；武器蓝图声明力量上限 -->
### DsnE24 物理伤害管线与武器成长

**背景。** 旧的伤害口径是 `max(攻击 − 防御, 1)`：防御是**减法**，攻击与防御一过线就可能
把伤害压到 1；武器只有 `攻击 +N` 与攻速两个旋钮，成长空间只有"换一把"。用户 m06401 给出
分域公式（基础值 × 增伤域 × 暴击域 × 减伤域），m06409 拍板"物理/魔法各加独立抗性区"、
"武器等级改副词条与附带效果"、"EXP 归蓝图定义、实例保存数值"（武器成长见阶段 ②）。

**决策（物理伤害内容，DsnE24 阶段 ①）**

1. **基础值 = 攻击力 + 力量加成。**
   `base = 攻击力 + min(力量, 力量上限) × φ`，`φ = 0.5`。攻击力仍是既有 `attack` 属性
   （基础 + 装备 + Buff 修正）；**力量**新增词表 (`strength`)，**武器上限**是蓝图数据
   (`ItemDef::strength_cap`)，空手用 `UNARMED_STRENGTH_CAP = 4`。这样"力量吃多少"成为
   每把武器的旋钮，而不是给所有武器一个全局乘数。
2. **减伤是护甲曲线，不是减法。** `护甲域 = δ / (目标有效防御 + δ)`，`δ = 4`
   （GAME.md 已写"防御 4 → 减伤 50%"）。性质：防御 0 不减伤；每点防御让有效生命
   `(Def+δ)/δ` 线性增长；不需要额外的"防御 clamp"——`max(0)` 后公式自然有界。
3. **乘区分开写，顺序无关但语义分开。**
   `最终伤害 = max(1, base × 增伤域 × 暴击域 × 护甲域 × 抗性域)`；
   `增伤域 = 1 + damage_bonus`（新增属性，默认 0）；`暴击域 = 暴击 ? 1 + 暴击伤害 : 1`
   （沿用 `>` 判定与 `max(0)` 口径）；`抗性域 = 1 - 物理抗性`（新增属性，区间 `[0, 0.75]`，
   魔法抗性 `magic_resist` 同时进词表，法术阶段再接线）。
4. **下限在最终值上。** `max(1, …)` 放在五域乘完之后：高护甲/高抗性只把伤害压到 1，不会
   出现 0 伤害或负伤害；也不给"基础值"单独设下限，避免"防御越高反而打得更痛"这类反直觉。
5. **所有数值都能被 `Modifier` 修正。** 力量/伤害加成/物理抗性都走属性词表，装备与 Buff
   只写修正，战斗系统只折叠 `Stats`；`EffectContext` 只额外回答"主手武器的力量上限"，
   不参与其它数值。
6. **旧的 `combat::compute_melee_damage` 适配器移除。** DsnE9 的"新旧逐值一致"以
   "公式内容不变"为前提；本条目正是改公式内容，前提消失。结算链路本来就只有一个调用点
   （`system/combat.rs`），直接走 `rules::melee(MeleeInput)`；DsnE9 的**形状**决策
   （结构化输入 + 因子分解）继续有效，因子分解增加了护甲/抗性/力量项。
7. **武器成长（阶段 ②）。** 武器实例带 `WeaponProgress { level, exp }`（1 级 0 经验起），
   蓝图声明 `WeaponGrowth { max_level, attack_per_level, strength_cap_per_level, exp_base }`
   （数据 schema 升 7）；实例只存等级与经验这两个玩家造成的差异（沿用 DsnE22 的 overrides
   形状，存档升 V4）。等级带来的攻击/力量上限是**派生值**，装配时按蓝图重算；EXP 由
   "造成伤害 × 品质倍率"获得，升级门槛 `exp_base × 等级^1.5`；升级在事件栏记一条。
   断点触发的副词条/附带效果留给后续内容，不写在本次实现里。

**状态：** 阶段 ①（物理伤害管线）已落地：`rules::damage` 重写（`MeleeInput { attack,
strength, strength_cap, damage_bonus, defense, physical_resist, crit, crit_roll }`、
`MeleeBreakdown` 五域分解）、`STATS` 新增 `strength/damage_bonus/physical_resist/
magic_resist`、`ItemDef::strength_cap`（数据 schema 升 6，只有武器可声明且必须声明）、
`EffectContext::main_hand_weapon` 提供力量上限、战斗系统按新公式结算；旧标量适配器删除。

阶段 ②（武器成长）已落地：`ItemDef::weapon_growth`（`WeaponGrowth`，数据 schema 升 7，
只有武器可声明且必须声明，各值加载期校验）、`WeaponProgress` 实例组件（唯一装配桥
`ItemSpawner::insert_blueprint_parts` 插 1 级 0 经验）、`Quality::exp_factor` 品质倍率
（粗劣 1.25 → 传说 0.4）、`apply_damage_system` 按"伤害 × 倍率"记经验并在升级时写事件、
`EffectContext::collect_actor` 把等级攻击加成合成一条装备 `Modifier`、
`main_hand_strength_cap` 加上等级成长；存档 V4 用 `ItemOverrides.weapon = WeaponSave
{ level, exp }` 保存，读档先按蓝图装配再套进度，不合法进度报 `Corrupt`。
断点/副词条/附带效果仍属未做内容（后续内容阶段追加到 `WeaponGrowth` 形状）。

**关联：** DsnE9（结构化输入形状）· DsnE10（修正器求值）· DsnE22（蓝图/实例/存档边界）·
DsnE23（属性词表与品质）· DsnE19（存档 V4 记武器养成进度）· Gm2 / Gm4 / Gm7 / Gm8 / Gm13。
<!-- &/DsnE24 -->
