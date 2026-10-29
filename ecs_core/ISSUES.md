> **⚠️ 修改前必须阅读或回忆 [RULE.md](../RULE.md)——它定义了本文档的维护规则和更新时机。**

# 发现的问题记录 —— ecs_core

**归属范围：** 领域规则引擎：组件、事件、资源、行动链路、地图、战斗、怪物、结算系统。

**编号：** `ECS1`、`ECS2`… 每个 crate 独立编号，见 [RULE.md](../RULE.md) §七。
根目录只记**跨 crate 协同 / 工具链与流程 / 迁移动因**；单 crate 的问题记在对应 crate 的 ISSUES.md 里。

**优先级：** 🔴 高（影响正确性或游戏体验） / 🟡 中（维护性或功能缺口） / 🟢 低（整洁或边缘情况）

## 待处理



<!-- &ECS43  保活只覆盖即将执行的行动：失效行动会占槽位让 actor 空转 -->
### ECS43 — 保活检查只覆盖"即将执行"的行动，待执行期间的失效检测缺失 🟡

**问题：** 保活（keep-alive）原先在**每个执行器内部**各写一份，只在行动 AV 归零、轮到自己执行
的那一刻检查：追击查"仍可见或仍有记忆"，逃跑查"血量仍在滞回区间内"（2026-09 已搬进校验器，
见下「进展」，但**检查时机没变**）。而一个行动从**仲裁授予 `ActiveAction`** 到**被执行**之间
可能隔若干回合——`tick_action_timers_system` 每轮只推进「最小正 AV」，所以慢行动要等其他行动
先到点。

在这段等待期内，世界会继续变化（目标跑远、血量回升、最后已知位置被清空），但**没有任何东西
会重新检查这个行动的成立条件**。后果是一个串行的两头堵：

- 失效的行动继续占着 actor **唯一的 `ActiveAction` 槽位**；
- 而生成系统被 `Without<Active>` 挡住（`AiActor<C>` 的 filter），**因此不会为它产出新候选**；
- actor 于是空转若干回合，直到轮到自己执行时才失败，再花一轮重新生成。

**影响：** 🟡 中高 — 不是崩溃，而是**玩家可见的行为钝感**：怪物该从"逃"切到"追"时会僵住；
行动越慢、场上行动越多，僵住越久。且它属于"设计意图与实现不一致"（DESIGN DsnE8 的设计哲学
写明保活应持续生效，实现只在执行前），失败模式是静默（无报错、无日志）。

**位置：** `ecs_core/src/action/entity.rs`（`validate_chase_system` / `validate_flee_system`
的保活分支，2026-09 已从执行器搬进校验器）、`tick_action_timers_system`、`AiActor` 过滤别名；
文档侧：DESIGN DsnE8「设计哲学 → 保活检查」、GAME.md Gm1「行动推进」/ Gm8「怪物行为」

**进展（2026-09）：前置形状已落地。** `ActionPhase::Validate` 已加进链路（`Generate →
Arbitrate → Tick → **Validate** → Execute → Complete`），保活分支也随之从执行器搬进了校验器
（`action/validation.rs`），终态仍走 `ActionEvents::fail`——**"每个行动一个校验器"这个形状
已经在了**，加一类保活检查只是往校验器里加几行。

**仍未完成的是"范围"，而范围是行为决策：** 现在的校验器只对 `Ready`（已到期）的行动运行，
所以"排队等 AV 期间失效"这个原始问题**依然存在**。要修它得把校验器的过滤器从
`(With<ActiveAction>, With<Ready>)` 放开到 `With<ActiveAction>`——那会带来两个可观察变化：

1. 提前取消会改变 `GameRng` 消耗时序与行动顺序（六行动 parity 套件要跟着改）；
2. "失效"与"还没轮到"的边界要定死（例如逃跑血量回升到 30% 以上时，是**立即**取消，
   还是允许它做完这一步——现状是做完）。

按 RULE §八 ③，这类玩家可见行为的判定权在用户。**待决策项**：要不要放开到全部 `ActiveAction`。

**关联：** DESIGN DsnE8（设计哲学 → 保活检查）/ DsnE14（Validate 阶段本身就是按本条描述的形状
落地的）；GAME.md Gm1 / Gm8；ECS35（终态出口，H1 已修复，本条的取消路径复用它）；
REFACTOR §1（Phase H1/H3 提供了 Phase 分组与终态出口这两块前置）。
<!-- &/ECS43 -->

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
| ECS2 | 玩法系统用 exclusive `&mut World` 代替正常系统 | `core/src/action/execution/mod.rs`、`core/src/action/generation/ai.rs`、`core/src/action/mod.rs` | DsnE8 / ECS3 / ECS8 |
| ECS3 | 删除 `ActionKind`：行动改为 action 子实体 | `core/src/action/mod.rs`、`core/src/action/generation/ai.rs`、`core/src/action/generation/player.rs` | DsnE8 |
| ECS4 | `finish_action_failure` 不清 `Idle`：actor 可同时持有 `Idle` + `Failure` | `core/src/action/mod.rs:71-81`（`finish_action_success` / `finish_action_failure`） |  |
| ECS7 | 删除 `Agility`，改为 `MoveSpeed` / `AttackSpeed` | `core/src/components.rs`（`MoveSpeed`/`AttackSpeed`/`Speed`） | DsnE8 / ECS8 |
| ECS8 | AV 门禁缺失：`ActionTimer` 未参与执行判断 | `core/src/action/execution/mod.rs:21-38`（tick）、`:42-303`（execute_*）、`:305-313`（run_action_cycle）；`core/src/world/loop_.rs:36-43`（advance_until_player_acted） | ECS3 / ECS7 |
| ECS9 | 事件生命周期错误：`EventReader` 每轮重读历史事件 | `core/src/system/mod.rs:257-282`（build_core_schedule/run_settle_systems）、`core/src/world/init.rs:70-76`（Events 注册）、`core/src/world/init.rs:393-416`（build_init_schedule/run_initialization） | DsnE8 |
| ECS15 | 怪物种群补足循环无进展保证，深层/水域地图可死循环 | `dungeon-world/src/population.rs`（generate_monster_population 补足段） |  |
| ECS19 | 玩家近战攻击无距离校验，可隔空命中已离开的怪物 | `dungeon-action/src/execute.rs`（check_condition/execute_attack/adjacent_8） | LECS20 / ECS20 |
| ECS20 | 逃跑怪物永不回头：滞回未落地 + 卡墙角原地挨打 | `dungeon-core/src/ops.rs`（FLEE_HP_RATIO_EXIT）、`dungeon-action/src/execute.rs`（check_condition/execute_flee/monster_attack_player） |  |
| ECS21 | 对角穿墙（corner-cutting）：玩家与怪物均可斜穿墙角 | `dungeon-action/src/execute.rs`（can_move_to）、`dungeon-action/src/player.rs`、`dungeon-core/src/pathfinding.rs`（astar） |  |
| ECS22 | `ensure_connectivity` 直线收尾挖单格而非 2x2（G22 修复不完整） | `dungeon-core/src/map_gen.rs`（carve_2x2） |  |
| ECS25 | 同类死抽象/重复表示清理 | 清单已归档（`docs-pre-governance` tag） |  |
| ECS26 | 楼梯/地面物品可能生成在不可行走格上 + 通道 4 方向断裂 | `dungeon-world/src/init.rs`（pick_stair_pos/place_ground_items）、`dungeon-world/src/persist.rs`、`dungeon-core/src/map_gen.rs`（generate_stalactites/ensure_connection_between/ensure_connectivity） |  |
| ECS27 | 移动与格子都有"代价"维度：`TILE_PROPS.move_cost` 接进移动 AV，格子效果提供 `Terrain` 桶乘区 | `ecs_core/src/effect/cell.rs`、`effect/collect.rs`、`map/tile.rs` | DsnE13 / DsnE10 / DsnE12 |
| ECS28 | 规则层扩展点：`EffectContext::collect` → `CollectedModifiers`，规则仍只吃纯数据 | `ecs_core/src/effect/collect.rs`、`action/entity.rs`（`SpeedRule::action_av_ignoring`） | DsnE10 / DsnE12 |
| ECS29 | `compute_melee_damage` 的签名无扩展位，返回类型是黑箱 | `ecs_core/src/rules/damage.rs`（新）、`ecs_core/src/combat/mod.rs`（适配器） | DsnE9 / DsnE10 |
| ECS30 | 行动候选查询接受 actor 的**任意**子实体，持久子实体会污染仲裁 | `ecs_core/src/action/ownership.rs`（新）、`ecs_core/src/action/entity.rs` | DsnE12 / ECS41 |
| ECS31 | `Tile` 的 5 处中央 match：加一个地形变体要改 5 个地方 | `ecs_core/src/map/tile.rs`（新）、`ecs_core/src/map/mod.rs`（改为转出） | LECS22 / DsnE13 / ECS22 |
| ECS32 | 物种信息散三处 → 全部收进模板一行（能力列表 + 按地图的生成权重），去掉恒 0 的 `Magic` 占位（原 ECS33） | `ecs_core/src/monster/template.rs`、`spawn.rs`、`world/init.rs` | DsnE6 / DsnE11 |
| ECS35 | 行动终态无保证：执行器漏发事件 → actor 永久卡 `Active` + 子实体泄漏 | `ecs_core/src/action/entity.rs`（`ActionEvents` 及 6 个 `execute_*_system`） | DsnE8 / ECS36 |
| ECS36 | `Ready` 的清理散落多处：只有 `execute_move_system` 显式清理 | `ecs_core/src/action/entity.rs` | ECS35 |
| ECS41 | 效果/装备挂到 actor 名下时，`ChildOf` 既不安全也不够用 | `ecs_core/src/action/ownership.rs`（新）、`ecs_core/src/action/mod.rs`（转出） | DsnE12 / ECS30 / ECS42 |
| ECS42 | 级联语义相反的两类效果用两个关系类型分开：`EffectOf`（级联）/ `EquippedBy`（不级联） | `ecs_core/src/effect/ownership.rs`（新） | DsnE12 |
| ECS44 | 确定性：仲裁的授予/回收顺序改为**显式排序**（`HashMap` 迭代顺序每进程种子不同 → 同种子可能不同结果）；四个 Schedule 经 `core_schedule()` 构建并强制 `ambiguity_detection = Error` | `ecs_core/src/action/entity.rs`（`ordered_grants` / `losers.sort`）、`ecs_core/src/schedule.rs`（`core_schedule`） | DsnX17 ③ / DsnE8 |
| ECS46 | 状态效果改按 **AV 时间轴**计时（`WorldClock`）：中毒不再随"结算轮次/怪物密度"漂移 | `ecs_core/src/effect/status.rs`、`resources.rs`（`WorldClock`）、`action/entity.rs`（tick 推进时钟） | GAME.md Gm5
| ECS47 | 怪物数按区间取目标值（不再恒为上限）；泥潭/地面物品改为撒在出生点附近 | `ecs_core/src/world/init.rs`（`MUD_SPAWN_RADIUS` / `ITEM_SPAWN_RADIUS`） | GAME.md Gm8/Gm10
| ECS45 | 同一轮内两个移动抢同一格 → 判定搬进 `ActionPhase::Validate`（串行、判定通过即**预留**目标格），落地收进**一个**系统 `settle_action_system`（唯一写 `Position`）；六个执行器 → 一个 | `ecs_core/src/action/{validation,settle}.rs`（新）、`system/occupancy.rs`（占格改用 `Blocks`）；中间形态 `execution/apply_move.rs` 已删除 | DsnE14 / DsnE2 / LECS23 / LECS24 |
| ECS34 | 构造期随机源收敛为单一入口：`DerivePurpose` + `derived_rng`（四种用途的偏移集中、只能追加），golden 断言 + 门禁禁止第二处 `SmallRng::seed_from_u64` | `ecs_core/src/resources.rs`（新）、`world/init.rs`（四处改走入口）、`scripts/docs_check.py` | DsnE5 / DsnE15 |
