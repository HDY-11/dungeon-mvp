> **⚠️ 修改前必须阅读或回忆 [RULE.md](../RULE.md)——它定义了本文档的维护规则和更新时机。**

# 发现的问题记录 —— ecs_core

**归属范围：** 领域规则引擎：组件、事件、资源、行动链路、地图、战斗、怪物、结算系统。

**编号：** `ECS1`、`ECS2`… 每个 crate 独立编号，见 [RULE.md](../RULE.md) §七。
根目录只记**跨 crate 协同 / 工具链与流程 / 迁移动因**；单 crate 的问题记在对应 crate 的 ISSUES.md 里。

**优先级：** 🔴 高（影响正确性或游戏体验） / 🟡 中（维护性或功能缺口） / 🟢 低（整洁或边缘情况）

## 待处理

### ECS5 — 材料物品无消耗渠道

**问题：** 生物血肉（id=10）、破布（11）、坚硬木棍（12）、染血兽牙（13）、黑色甲壳（14）五种材料物品只能拾取和堆积，没有任何消耗途径。背包 36 格在 4-5 层后会被材料大量占用，玩家被迫在"拾取所有材料"和"留空间给有用物品"之间做无趣的选择。

```rust
// 当前材料的全部用途：占背包格
// 没有任何合成/升级/交换/消耗机制消费它们
```

**影响：** 🟡 中 — 材料的存在感为零。玩家的理性选择是"忽略所有材料掉落"。

**方案：** 模板碎片系统（DESIGN.md DsnX12）。碎片作为一次性消耗品，用材料合成指定物品。Phase 1 材料开始有出口，Phase 2 引入核心→完整模板。渐进实现。

**状态：** 部分修复 — Phase 1 已落地（模板碎片作为独立消耗品掉落和使用，材料已有消耗渠道）。Phase 2（模板核心）与 Phase 3（实验级碎片）保持 Deferred。**注意：** 物品系统尚未迁移到 `ecs_core`（[DESIGN.md](DESIGN.md) DsnX13 S4），本条在物品迁移后需要重新评估。

**位置：** `assets/items.json` items 10-14

### ECS6 — 战斗公式缺乏层次深度（Won't Fix — MVP 范围决策）

**表现：** 当前 `max(攻击 - 防御, 1)` 的差值公式完全线性，1 点攻击永远对应 1 点伤害。无穿甲穿透、无元素属性/抗性、无距离衰减、无背后/侧击加成。装备增强集中在 +攻击/+防御 两个维度。

**影响：** 🟢 低 — MVP 阶段可以接受。但扩展到 8+ 种怪物、3+ 种武器类型时，所有战斗都会感觉"差不多"——只有数值差异，没有策略差异。当需要设计"抗高攻怪"和"抗高防怪"两种不同策略时，当前公式无法提供区分度。

**状态：** Won't Fix — MVP 范围决策。触发条件：怪物种类 ≥8 或武器类型 ≥3 时重新评估。

**位置：** `dungeon-action/src/execute.rs:285-310`（execute_attack）

---

### ECS27 — 移动规则与 `Tile` API 都没有"地形代价"维度，地形类装备效果无落点 🟡

**问题：** `Tile` 的公开 API 只有 `glyph()` / `walkable()` / `blocking()`，全部是二值；`can_move_to` 只判"能否走 + 是否被占用"；移动 AV = `base_duration / clamp(speed)`，`base_duration` 是常量，**不随目标地块变化**。因此"地形减速"这不是"有但无视不了"，而是**该能力根本不存在**——"无视某类地形减速"这类装备效果无处落地。

**影响：** 🟡 中 — 阻断"地形影响移动"这一整类设计（地形代价、地形类装备、地形类状态），而 `DsnE8` 已记「地形 → `MoveSpeed`」的方向，说明这是既定设计方向的实现缺口。

**位置：** `ecs_core/src/map/mod.rs`（`Tile` 的方法）、`ecs_core/src/action/execution/movement.rs:32-66`（`can_move_to`）、`ecs_core/src/balance.rs:79`（`action_av`）

**备注：** **记录在案，Phase H 逐项确认**。修法见 DESIGN DsnE10 / DsnE13；具体代价数值属 GAME.md。

---

### ECS28 — 规则层扩展点缺失：规则无法获知"执行者身上有什么" 🟡

**问题：** `movement.rs` 的分层卖点是**纯规则不碰 `World`**，但装备/状态类效果要求规则知道"移动者是谁、带了什么"。直接给纯函数加 `&World` 参数会退回 `ECS2` 所批评的形态（把读资源与读组件揉进一次调用），并丢掉可单测性。项目现有正确范例是 `ActorSpeeds`（系统层查询 → 纯数据输入给 `action_av`），但**没有推广成通用形态**，因此每加一类"规则修正"都要重新决定怎么接。

**影响：** 🟡 中 — 装备、状态、地形三类影响都卡在这里；不推广则每类各写一套接法。

**位置：** `ecs_core/src/action/entity.rs`（`ActorSpeeds` / `actor_speeds`，现有范例）、`ecs_core/src/action/execution/movement.rs`

**备注：** **记录在案，Phase H 逐项确认**。形态见 DESIGN DsnE10（修正器在系统层求值、纯函数只吃结果、pull 读取）。

---

### ECS32 — 物种信息散在三处：`Can*` 能力在 spawn 代码、数值在模板、权重在 `spawn.rs` 🟡

**问题：** `MonsterTemplate` 只有数值字段；**能力（`Can*`）在 `world/init.rs` 的 spawn 代码里插入**；**生成权重在 `monster/spawn.rs`**。于是"这只怪能做什么"要读三个文件，加一只怪要改多处。`DsnE6` 已把 `spawn_weight` 记为模板字段的方向，但**未落地**。

**影响：** 🟡 中 — 阻碍"加一只怪 = 加一行数据"这个目标；设计已定方向，属未落地。

**位置：** `ecs_core/src/world/init.rs`（`spawn_monsters_system` 的能力插入）、`ecs_core/src/monster/template.rs`、`ecs_core/src/monster/spawn.rs`

**备注：** **记录在案，Phase H 逐项确认**。修法：能力/技能作为**模板的列表字段**（每物种固定但非全物种共有 → 列表，不展开成全字段 DTO）。

---

### ECS33 — `MonsterTemplate`/`MonsterStats` 是全字段 DTO，`Magic` 是恒 `0.0` 占位 🟢

**问题：** `template.rs` 的 `stats()` 无条件产出 `MonsterStats` 的全部字段，`monster_base_bundle` 再逐字段搬进实体。其中 `magic: Magic::new(0.0)` 对所有怪物恒为占位值，而 `stats.magic` 的**唯一消费者是玩家**的构建——即"怪物从来不用，却作为字段强制声明"。新增按物种可选的数据（抗性/技能等）会继续以占位值形态堆进这个 DTO。

**影响：** 🟢 低 — 不写不读、无行为后果；但它是"新增按需字段"的第一道阻力。

**位置：** `ecs_core/src/monster/template.rs:85-121`（`MonsterStats` / `stats()`）、`ecs_core/src/world/init.rs:310-334`（`monster_base_bundle`）

**备注：** **记录在案，Phase H 逐项确认**。修法：模板只装 100% 需要的字段；按需数据改用组件在 spawn 时添加。**中间 DTO 与模板同样受此约束**，否则只是把占位值从模板挪到 DTO。

---

### ECS34 — 怪物生成用 `SmallRng` 而非 `GameRng`，与"唯一随机源"决策并存两套状态 🟡

**问题：** `spawn_monsters_system` 用 `rand::rngs::SmallRng::seed_from_u64(seed)`，其中 `seed` 是 `MapSeed` 按楼层派生。严格说这落在 `DsnE5`（"统一走 `GameRng` **或基于 `MapSeed` 的派生 RNG**"）的第二种形态内，**当前确定性是成立的**；但它与 `GameRng` 是**两套独立状态**，而 `GameRng` 的 `state`/`steps` 是回放与存档的基础。若将来该派生种子的算法变动，同 `(seed, floor)` 的怪物布局会改变，而这类改动**不会体现在 `GameRng.steps` 上**。

**影响：** 🟡 中 — 不立即致命，但新增的生成/掉落类系统会继续复制这一先例，形成"第二随机源"惯例，削弱回放与 SL 防护的可验证性。

**位置：** `ecs_core/src/world/init.rs:346`

**备注：** **记录在案，Phase H 逐项确认**。处置二选一：① 统一到 `GameRng`；② 明确把它记为"派生 RNG"的合法例外，并加断言钉住"同 `(seed, floor)` 布局不变"。

---

### ECS42 — `linked_spawn` 的语义对不同效果是相反的：状态效果要级联，装备不能级联 🟡

**问题：** 用关系挂载效果时，"拥有者 `despawn` 是否级联 `despawn` 效果"必须逐类决定，
而两类效果要的答案**相反**：

| 效果 | 期望 | 用 `linked_spawn` |
|---|---|---|
| 状态效果（中毒、护盾、加速） | 人死了效果消失 | ✅ 对 |
| 技能授予的持续效果 | 施法者没了效果没了 | ✅ 对（但"自杀并把地块变成生成器"这类**不属于此类**，见 DsnE12） |
| **装备（掉落物）** | 人死了**掉出来**给玩家拾取 | ❌ **错**：战利品随尸体一起消失 |

即：把"效果"当成一个统一的类别去套同一个级联语义，会得到**一个静默的数据丢失 bug**
（怪物死了、装备没了），而且它只在"玩家击杀带装备的怪物"时才显现。

**影响：** 🟡 中 — 影响掉落与装备整条链路；失败模式是静默消失（无报错、无日志）。

**位置：** 尚无代码落点（H9/H11 落地时才会出现）；设计落点 DESIGN DsnE12 第 1 条的落点表。

**修法：** 装备的落点**单独决定**（DsnE12 的开放项）——按 `DsnE13` 的判据：装备若需要
**独立身份**（可被偷、单独销毁、单独耐久），就用实体关系但**关闭 `linked_spawn`**；
若不需要，则用"位置 + `Equipment` 组件"表达，"掉在地上"就是换了位置。

**关联：** ECS41、DESIGN DsnE12 第 1 条与开放项；REFACTOR §2.1（H9/H11 未完成）。

---

### ECS43 — 保活检查只覆盖"即将执行"的行动，待执行期间的失效检测缺失 🟡

**问题：** 保活（keep-alive）目前在**每个执行器内部**各写一份，只在行动 AV 归零、轮到自己执行
的那一刻检查：`execute_chase_system` 查"仍可见或仍有记忆"，`execute_flee_system` 查
"血量仍在滞回区间内"。而一个行动从**仲裁授予 `ActiveAction`** 到**被执行**之间可能隔若干回合
——`tick_action_timers_system` 每轮只推进「最小正 AV」，所以慢行动要等其他行动先到点。

在这段等待期内，世界会继续变化（目标跑远、血量回升、最后已知位置被清空），但**没有任何东西
会重新检查这个行动的成立条件**。后果是一个串行的两头堵：

- 失效的行动继续占着 actor **唯一的 `ActiveAction` 槽位**；
- 而生成系统被 `Without<Active>` 挡住（`AiActor<C>` 的 filter），**因此不会为它产出新候选**；
- actor 于是空转若干回合，直到轮到自己执行时才失败，再花一轮重新生成。

**影响：** 🟡 中高 — 不是崩溃，而是**玩家可见的行为钝感**：怪物该从"逃"切到"追"时会僵住；
行动越慢、场上行动越多，僵住越久。且它属于"设计意图与实现不一致"（DESIGN DsnE8 的设计哲学
写明保活应持续生效，实现只在执行前），失败模式是静默（无报错、无日志）。

**位置：** `ecs_core/src/action/entity.rs`（`execute_chase_system` / `execute_flee_system`
的保活分支）、`tick_action_timers_system`、`AiActor` 过滤别名；
文档侧：DESIGN DsnE8「设计哲学 → 保活检查」、GAME.md Gm1「行动推进」/ Gm8「怪物行为」

**修法形状（待确认后实现）：** 新增 `ActionPhase::Validate`，放在 `Tick` 与 `Execute` 之间，
**每个行动一个校验系统**——与执行器对称（执行器回答"做成了吗"，校验器回答"还该做吗"）。
一回合内对所有 `ActiveAction` 跑一遍，失效即刻发终态事件（走 `ActionEvents::fail`，
H1 之后终态只有一个出口），执行器里的保活分支随之删除（只保留"执行瞬间的最终防线"）。
判据是 `LECS22`：**加一个行动要改几处**——按此形状是"执行器 + 校验器"各一处，两者都在
`ActionPhase::Execute` / `ActionPhase::Validate` 的列表里各占一行，**不引入中央 match**。

**为什么不当场改：**

1. **它是行为改动**：提前取消会改变 `GameRng` 的消耗时序与行动顺序，进而影响六行动 parity
   套件的断言。按 RULE §八 ③，这类玩家可见行为的判定权在用户，不在 AI。
2. 需要先决定一个口径：**"失效"与"还没轮到"的边界**——例如逃跑血量回升到 30% 以上时，
   是立即取消（本条目主张），还是允许它做完这一步（现状）。这条口径会写进 GAME.md。

**关联：** DESIGN DsnE8（设计哲学 → 保活检查，已标注本缺口）/ DsnE4（已合并进 DsnE8）；
GAME.md Gm1 / Gm8；ECS35（终态出口，H1 已修复，本条的取消路径复用它）；
REFACTOR §1（Phase H1/H3 提供了 Phase 分组与终态出口这两块前置）。

---

## ✅ 已修复（索引表）

> **2026-09 文档治理：** 本区由「逐条 修复前/修复后」压缩成索引表——明细读的人少、
> 查的人多。**原文完整保留**在 `docs-pre-governance` tag（取回方式见
> `archive/docs-trash-manifest.md`）。表格保留 id / 结论 / 位置 / 参考四项，
> 需要「为什么这么修」的细节时按 id 去 tag 里查。
>
> **位置列是修复当时的路径**：`core/` 后来改名 `ecs_core/`（DESIGN DsnX15），
> 且部分文件已随重构删除（`action/execution/mod.rs`、旧 `dungeon-*` 等）。
> 查现状请看「参考」列的 DESIGN / LESSONS 条目。
>
> **规则偏差（待用户批准修 RULE §二）：** RULE §二 要求「已修复」条目保留
> 修复前/修复后正文。本条按 2026-09 用户授权压缩；若长期采用，应把 RULE §二
> 改成「已修复区维护索引表，明细归档」。

| id | 修复后的行为（一句话） | 位置 | 参考 |
|---|---|---|---|
| ECS1 | 行动实体 + 速度组件：AV 系统与 `ActionKind` 的替代设计（草案） | — | DsnE8 / ECS2 / ECS3 |
| ECS2 | 玩法系统用 exclusive `&mut World` 代替正常系统 | `core/src/action/execution/mod.rs`、`core/src/action/generation/ai.rs`、`core/src/action/mod.rs` | DsnE8 / ECS3 / ECS8 |
| ECS3 | 删除 `ActionKind`：行动改为 action 子实体 | `core/src/action/mod.rs`、`core/src/action/generation/ai.rs`、`core/src/action/generation/player.rs` | DsnE8 |
| ECS4 | `finish_action_failure` 不清 `Idle`：actor 可同时持有 `Idle` + `Failure` | `core/src/action/mod.rs:71-81`（`finish_action_success` / `finish_action_failure`） |  |
| ECS7 | 删除 `Agility`，改为 `MoveSpeed` / `AttackSpeed` | `core/src/components.rs`（`MoveSpeed`/`AttackSpeed`/`Speed`） | DsnE8 / ECS8 |
| ECS8 | AV 门禁缺失：`ActionTimer` 未参与执行判断 | `core/src/action/execution/mod.rs:21-38`（tick）、`:42-303`（execute_*）、`:305-313`（run_action_cycle）；`core/src/world/loop_.rs:36-43`（advance_until_player_acted） | ECS3 / ECS7 |
| ECS9 | 事件生命周期错误：`EventReader` 每轮重读历史事件 | `core/src/system/mod.rs:257-282`（build_core_schedule/run_settle_systems）、`core/src/world/init.rs:70-76`（Events 注册）、`core/src/world/init.rs:393-416`（build_init_schedule/run_initialization） | DsnE8 |
| ECS10 | Gm4 玩家初始 HP 文档算术错误：28 vs 实现 33 | `GAME.md:152` |  |
| ECS11 | Gm6 熟练度表格与公式/代码错位一级 | `GAME.md:194-195`（Gm5）、`GAME.md:223-228`（Gm6） |  |
| ECS12 | Gm1 移动耗时 300ms 与 Gm7 武器攻速矛盾，CanMove.duration 成死字段 | `GAME.md:14` |  |
| ECS13 | Gm3 升级经验表 round vs 实现 trunc（每级差 1） | `GAME.md:106-120` |  |
| ECS14 | Gm6 卷轴「每层 1-3 张」未记载深层增量 | `GAME.md:201` |  |
| ECS15 | 怪物种群补足循环无进展保证，深层/水域地图可死循环 | `dungeon-world/src/population.rs`（generate_monster_population 补足段） |  |
| ECS16 | 单房间地面物品放置 random_range 空区间 panic | `dungeon-world/src/init.rs`（place_ground_items 单房间分支） |  |
| ECS17 | pick_stair_pos 兜底坐标未钳制，可引发越界 panic | `dungeon-world/src/init.rs`（pick_stair_pos 兜底） |  |
| ECS18 | 护盾/狂暴实际时长约为文档宣称 3 倍，技能超模 | `dungeon-core/src/items.rs`（use_item 卷轴学习）、`GAME.md:98-99/194-195` |  |
| ECS19 | 玩家近战攻击无距离校验，可隔空命中已离开的怪物 | `dungeon-action/src/execute.rs`（check_condition/execute_attack/adjacent_8） | LECS20 / ECS20 |
| ECS20 | 逃跑怪物永不回头：滞回未落地 + 卡墙角原地挨打 | `dungeon-core/src/ops.rs`（FLEE_HP_RATIO_EXIT）、`dungeon-action/src/execute.rs`（check_condition/execute_flee/monster_attack_player） |  |
| ECS21 | 对角穿墙（corner-cutting）：玩家与怪物均可斜穿墙角 | `dungeon-action/src/execute.rs`（can_move_to）、`dungeon-action/src/player.rs`、`dungeon-core/src/pathfinding.rs`（astar） |  |
| ECS22 | `ensure_connectivity` 直线收尾挖单格而非 2x2（G22 修复不完整） | `dungeon-core/src/map_gen.rs`（carve_2x2） |  |
| ECS23 | `place_ground_items` 单房间退路硬编码 `SmallRng::seed_from_u64(42)` | `dungeon-world/src/init.rs` | DsnE5 |
| ECS24 | `place_skill_scrolls` 缺少 exclude 参数 | `dungeon-world/src/init.rs:94` |  |
| ECS25 | 同类死抽象/重复表示清理 | 清单已归档（`docs-pre-governance` tag） |  |
| ECS26 | 楼梯/地面物品可能生成在不可行走格上 + 通道 4 方向断裂 | `dungeon-world/src/init.rs`（pick_stair_pos/place_ground_items）、`dungeon-world/src/persist.rs`、`dungeon-core/src/map_gen.rs`（generate_stalactites/ensure_connection_between/ensure_connectivity） |  |
| ECS29 | `compute_melee_damage` 的签名无扩展位，返回类型是黑箱 | `ecs_core/src/rules/damage.rs`（新）、`ecs_core/src/combat/mod.rs`（适配器） | DsnE9 / DsnE10 |
| ECS30 | 行动候选查询接受 actor 的**任意**子实体，持久子实体会污染仲裁 | `ecs_core/src/action/ownership.rs`（新）、`ecs_core/src/action/entity.rs` | DsnE12 / ECS41 |
| ECS31 | `Tile` 的 5 处中央 match：加一个地形变体要改 5 个地方 | `ecs_core/src/map/tile.rs`（新）、`ecs_core/src/map/mod.rs`（改为转出） | LECS22 / DsnE13 / ECS22 |
| ECS35 | 行动终态无保证：执行器漏发事件 → actor 永久卡 `Active` + 子实体泄漏 | `ecs_core/src/action/entity.rs`（`ActionEvents` 及 6 个 `execute_*_system`） | DsnE8 / ECS36 |
| ECS36 | `Ready` 的清理散落多处：只有 `execute_move_system` 显式清理 | `ecs_core/src/action/entity.rs` | ECS35 |
| ECS41 | 效果/装备挂到 actor 名下时，`ChildOf` 既不安全也不够用 | `ecs_core/src/action/ownership.rs`（新）、`ecs_core/src/action/mod.rs`（转出） | DsnE12 / ECS30 / ECS42 |
