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
2. **重建之外还有"移动时写穿"**：`apply_move_system` 成功移动后**立刻**写出旧格、
   写入新格。所以占用图在**回合内部**也是准确的，而不只是回合边界上准确。
   这条是"同一回合两个 actor 挤进同一格"（ECS45）的根治：判定与占位发生在同一次调用里。

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
**未落地**：装备侧的实际物品（物品系统未迁移，R2/S4）；格子效果的时长/解除路径（尚无第二个效果，
owner 系统按 DsnE12 第 4 条届时一并加）。

---

**原编号：** `DsnE12`（迁移前）
<!-- &/DsnE12 -->

<!-- &DsnE13  格子身份判据：需要独立身份→实体，否则→索引；地形属性进静态表 -->
<!-- &DsnE14  移动=意图->判定->唯一写入点；规则纯函数、写 Position 只有一个系统 -->
### DsnE14 移动的唯一写入点：意图 → 判定 → 应用

**决策**

移动拆成三步，**只有第三步写 `Position`**：

```text
execute_move / chase / flee / wander   ──MoveRequest{actor, dx, dy}──▶  apply_move_system
（决定"这一步该算成功还是失败"）                                        （判定 + 唯一写位置 + 写穿占用图）
                                                                              │
                                                              plan_step（纯规则，带 BlockReason）
```

- **规则是纯函数**：[`plan_step`] 返回 `MovePlan::Allowed(Position)` 或
  `Blocked(BlockReason)`，四种原因（越界/墙/占用/对角穿墙）都是**数据**，
  玩家的"为什么没动"直接由它生成，不再由各调用点各写一句。
  `can_move_to` / `moved_position` 退化为它的 `bool` / `Option` 视图。
- **写入只有一个系统**：`apply_move_system` 是全项目唯一写 `Position` 的地方
  （生成期 spawn 除外），由 `docs_check.py` 的"写 Position 白名单"在门禁里强制。

**背景与被否方案**

- **被否：只把四份实现合并成一个函数（`plan_move` + `commit_move`）。** 那是把
  "四份实现"变成"一份实现 + 四个调用点"——**统一入口靠约定**。下次再加第五条移动路径
  照样会漏（这正是 ECS45 两次没修净的原因：每次要手工种四处守卫，形状还各不相同：
  `if claim(..)` / `if claim(..) && let ..` / `&& claim(..)` / 压根没有）。
  改成"一个系统拥有写权"之后，漏写不是"忘了调用"，而是**根本没地方写**。
- **被否：规则整体搬进系统。** 那样 `movement.rs` 的 5 条无 `World` 单测就没了，
  而"规则能脱离 World 测"是 ECS28/ECS2 换来的红利（纯函数层与 ECS 层分离）。
  所以选择**两者都要**：规则纯、写入单点。
- **被否：保留 `RoundClaims`（批内预留表）。** 判定与写入既然在同一次调用里，
  "检查"与"占位"之间没有窗口，第二个派生状态就没有存在理由——
  两个派生状态收敛成一个（占用图本身，移动时写穿）。

**代价**

1. **多一个阶段与一份瞬态事件**（同一 tick 内完成，不影响玩家节奏）。
2. **执行器仍在调 `plan_step`/`moved_position`**：它用这份判定决定"行动算成功还是失败"，
   应用系统用它决定"能不能落下去"。两处不是重复——前者属于行动生命周期，后者属于状态写入；
   但**读同一规则**是必须的，否则两处口径会漂（`the_three_entry_points_agree` 钉住这一点）。
3. **`Position` 仍是公开组件**，"只有一个系统写"是**约定 + 门禁**，不是类型保证。
   真正的类型保证要把 `Position` 的写权封进专用 API，成本远高于收益。

**关联：** ISSUES ECS45（已修复）| DsnE2（占用图：占格能力 + 写穿）| DsnE8（行动子系统）| `LECS23`

**状态：** 已定 — 已落地（`action/execution/apply_move.rs` + 四个执行器改发意图）。
<!-- &/DsnE14 -->

---

<!-- &DsnE15  共享状态的写入者清单：哪些是唯一入口、哪些是有意的多写者 -->
### DsnE15 共享状态的写入者清单（"第二入口"审计）

**决策**

对每一个"会被多个系统改"的状态，**要么只有一个入口，要么在文档里写清为什么可以有多个**。
这张表就是那个"写清"：

| 状态 | 写入者 | 判定 |
|---|---|---|
| `Position` | [`apply_move_system`]（唯一）+ 生成期 spawn | **唯一入口**，由 `docs_check.py` 白名单强制（ISSUES ECS45） |
| `OccupancyMap` | `rebuild_occupancy_system`（每轮）+ `apply_move_system`（移动写穿） | **有意的两个**：一个是"对账"，一个是"事务内更新"；都写同一套规则（`With<Blocks>`） |
| `Active` / `Idle` / `Failure` | `begin_action` / `finish_action`（各一份实现，共 3 个调用点） | **单一实现**：挂载（玩家）、仲裁（AI）、completion（收尾）语义必须一致 |
| `Health` | `Health::damage(..)` 一份实现；调用点：攻击结算、中毒 tick、升级重算 | **单一实现**：三个调用点是三种**不同原因**，共用一个方法 |
| `Ready` | `tick_action_timers_system`（置位）+ `ActionEvents::{succeed,fail}`（清理） | **有意的两个**：谁置位、谁清理各一处；执行器拿不到 `EventWriter`（H1） |
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

