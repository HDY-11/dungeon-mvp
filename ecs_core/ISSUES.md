> **⚠️ 修改前必须阅读或回忆 [RULE.md](../RULE.md)——它定义了本文档的维护规则和更新时机。**

# 发现的问题记录 —— ecs_core

**归属范围：** 领域规则引擎：组件、事件、资源、行动链路、地图、战斗、怪物、结算系统。

**编号：** `ECS1`、`ECS2`… 每个 crate 独立编号，见 [RULE.md](../RULE.md) 与 [REFACTOR.md](../REFACTOR.md) §13。
根目录只记**跨 crate 协同 / 工具链与流程 / 迁移动因**；单 crate 的问题记在对应 crate 的 ISSUES.md 里。

**优先级：** 🔴 高（影响正确性或游戏体验） / 🟡 中（维护性或功能缺口） / 🟢 低（整洁或边缘情况）

## 待处理

### ECS1 — 行动实体 + 速度组件：AV 系统与 `ActionKind` 的替代设计（草案）

**问题：** 当前行动系统由 `ActionKind` 中央 enum + actor 上的 ZST 行动组件 + exclusive `&mut World` 系统组成；AV 计时器没有参与执行门禁，事件生命周期也不正确。继续加行动/行为会继续增加中央 match 和耦合。

**决策草案：** 按 REFACTOR.md §3.6 改为 action 实体方案：

- 一个行动 = 一个 actor 的子实体（`ChildOf` + `ActionPriority` + `ActionTimer` + ZST/payload + `Candidate/ActiveAction/Ready`）；
- `Can*` 保持 actor 上的 ZST 组件，不子实体化；
- 生成系统只 spawn 候选；仲裁系统唯一写入 actor 行动状态；执行系统按行动类型专用 query，零中央 match；completion 系统消费 `ActionSucceeded/FailedEvent`；
- 删除 `ActionKind` 与 `mount_action` 中央 match；
- 速度按 REFACTOR.md §2.6 改为 `MoveSpeed` / `AttackSpeed`，删除 `Agility`。

**状态：** ✅已落地。① 行动即实体：Phase B/C 完成；② 速度组件：Phase D 完成（`Agility` 已删除，改为 `MoveSpeed`/`AttackSpeed` 倍率）。详见 [DESIGN.md](DESIGN.md) DsnE8 与 [REFACTOR.md](../REFACTOR.md) §11.3。

**关联：** REFACTOR.md §2.6 / §3.6 / §10.6；ISSUES ECS2 / ECS3 / ECS25、ECS8 / ECS9、ECS7。

---


### ECS2 — 玩法系统用 exclusive `&mut World` 代替正常系统

**问题：** `action/execution/mod.rs` 的 tick/执行系统、`action/generation/ai.rs::decide_monster_actions`、`action/mod.rs::mount_action/finish_action_*`、`world/loop_.rs` 的推进函数都收 `&mut World`，手动 query/改实体/发事件；`run_action_cycle` 手动顺序调用系统。玩法逻辑被写成过程式代码，无法用 `Query`/`Commands`/`EventWriter` 组合，也无法并行。

**影响：** 🟡 中高 — 阻断 action 实体方案（REFACTOR §3.6）的落地；每次新增行动/行为都要改多个 exclusive 函数。

**位置：** `core/src/action/execution/mod.rs`、`core/src/action/generation/ai.rs`、`core/src/action/mod.rs`、`core/src/world/loop_.rs`、`core/src/combat/mod.rs`（直接执行辅助）

**状态：** ✅已修复（Phase C 完成）。行动链路里已无 exclusive 系统——执行器全部是普通参数化系统，
原先「多实体读写无法用 `Query` 表达」的判断是复用 `movement::execute_move(&mut World, ...)`
造成的假象（`Query<&mut T>` 只保证 per-entity 唯一可变访问，驱动实体与被写实体不同即无冲突）。
剩余 `&mut World` 只在 `world/loop_.rs` 的应用层入口，属 DsnX2 的有意选择（命令驱动回合的对外 API）。

**位置：** 原 `action/execution/mod.rs`、`action/generation/ai.rs`、`action/mod.rs` 已随 Phase C 删除；
现存唯一 `&mut World` 是 `ecs_core/src/world/loop_.rs`。

**关联：** REFACTOR.md §3.6 / §8.1；ISSUES ECS3、ECS8、ECS9。

---


### ECS3 — 删除 `ActionKind`：行动改为 action 子实体

**问题：** `ActionKind` 是中央分派 enum，`mount_action` 对它做 match；新增行动要改 enum + 中央 match + 生成/执行分支。`REFACTOR.md §3.2–3.5` 描述的 `ActionIntent + 仲裁` 方案从未落地，当前 `ai.rs::choose_action` 是独占的 `if/else` 函数。

**影响：** 🟡 中 — 扩展成本高；与 `Can*` + ZST + 专用 query 的 ECS 方向不一致。

**决策：** 按 REFACTOR.md §3.6 改为 action 实体方案；`Can*` 保持组件；不再需要 `ActionKind`。

**位置：** `core/src/action/mod.rs`、`core/src/action/generation/ai.rs`、`core/src/action/generation/player.rs`、`core/src/world/loop_.rs`

**状态：** ✅已修复（Phase C 全量迁移完成）— 六个行动（Wait/Move/BasicAttack/Wander/Chase/Flee）全部迁到 action 子实体，`ActionKind` 与配套的中央分派/独占执行系统已删除，全库无残留引用；主循环（`world/loop_.rs`）已切换到新链路，`cargo test -p core` 61 passed、`cargo test --workspace` 25 个目标全绿。

**修复后：**

- 生成：`wait_/wander_/flee_/chase_generation_system`（AI，只 spawn 候选）+ `player_action_generation_system`（玩家，直接产 active action）；
- 仲裁：`action_arbitration_system` 全序 `(ActionPriority, to_bits())`，loser 立即 despawn；
- tick：`tick_action_timers_system` 只推进 `With<ActiveAction>`，归零加 `Ready`；
- 执行：六个按行动类型拆分的参数化系统，零中央 match；只发 `ActionSucceeded/FailedEvent`；
- completion：消费事件 → despawn action 实体 → actor 回 `Idle`/`Failure`（互斥，I91）；
- 删除：`ActionKind`、`mount_action`、`finish_action_success/failure`、
  `decide_monster_actions`、`choose_action`、`run_action_cycle`、旧 `execute_*_system`、
  actor 上的行动 ZST 挂载路径、旧 `PlayerActionRequest`；
- parity：六行动各一个受控场景 + 共同残留不变量（`entity_tests.rs::parity_*`）。

**A41 范围收窄（同批完成）：** 执行器不再需要 exclusive `&mut World`。原先「action 实体 → actor 位置的多实体读写无法用普通 `Query` 表达」的判断，是「复用 `movement::execute_move(&mut World, ...)`」造成的假象，不是 ECS 限制——`Query<&mut T>` 只保证 per-entity 唯一可变访问，驱动实体与被写实体不同即无冲突。落点计算抽成纯函数 `movement::moved_position` 后，`execute_move_system` 已是普通系统（parity 测试 + 与 `CoreSettleSchedule` 同调度共存测试佐证）。剩余 exclusive 代码只剩 `world/loop_.rs` 的应用层入口（`&mut World` 参数传递，属 DsnX2 的有意选择）。

**关联：** D29、REFACTOR.md §3.6 / §11.3 Phase C。

---


### ECS4 — `finish_action_failure` 不清 `Idle`：actor 可同时持有 `Idle` + `Failure`

**问题：** `finish_action_failure` 只走 `clear_action_state`（清 `Active`/`ActionTimer`/`Ready`），然后 `insert(Failure)`，**没有 `remove::<Idle>()`**。于是「行动失败」的 actor 会同时持有 `Idle` 和 `Failure` 两个互斥状态组件。`finish_action_success` 有同样的对称问题（不清 `Failure`），只是成功路径上通常已经不在 `Failure`。

**影响：** 🟢 低（发现时无实际行为后果）——当前所有消费方（`ai.rs::decide_monster_actions`、`world/query.rs`、TUI 状态栏）都用 `Or<(With<Idle>, With<Failure>)>` 或「取其中一个」，冗余的第二个标记不会改变判定。但状态互斥被破坏，`Idle`/`Failure` 的 debug 断言与未来的状态机重构都会踩到它。

**位置：** `core/src/action/mod.rs:71-81`（`finish_action_success` / `finish_action_failure`）

**状态：** ✅已修复（Phase C5 迁移对照测试时发现；新链路的 `action_completion_system` 一开始也照抄了这个疏漏，已同批修正）。修复：`finish_action_failure` 先 `remove::<Idle>()`，`finish_action_success` 先 `remove::<Failure>()`，恢复「两者互斥」不变式；回归测试 `idle_and_failure_are_mutually_exclusive`。

**关联：** REFACTOR.md §11.3 Phase C（C5）


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


## ✅ 已修复

### ECS7 — 删除 `Agility`，改为 `MoveSpeed` / `AttackSpeed` ✅已修复

**修复前：** `Agility` 一个聚合数值同时承担「反应时」与「耗时修正」两件事：
`AV = max(100 - 敏捷×3, 20) + 耗时 × max(1 - 敏捷×0.02, 0.5)`。装备/防具/Buff
无法分别影响移动与攻击节奏；且 ECS8 修复前该公式对执行没有实际影响。

**修复后：** 速度拆成两个独立倍率组件，AV 只剩一个口径：

```
AV = base_duration ÷ clamp(速度, MIN_SPEED=0.25, MAX_SPEED=4.0)
```

- 行动类别 → 速度的映射在**挂载点**决定（`SpeedRule`：`Move` / `Attack` / `Fixed`）：
  移动/追击/逃跑/游荡读 `MoveSpeed`，近战攻击读 `AttackSpeed`，`Wait` 固定 800
  且不受任何速度影响（§11.6 第 4 项）；
- 已确认**删除反应时**（§11.6 第 3 项）；试玩若需要「出手前的固定延迟」，
  再统一加回一个常数项；
- 执行器完全不接触速度：AV 在生成时就固化进 `ActionTimer`，之后 tick/执行只看剩余值。
- 架构上顺带解耦：AV 计算变成纯函数
  （`SpeedRule::action_av(base_duration, ActorSpeeds)`，不需要 `World`），
  因此公式本身可以在无 ECS 的单测里逐条钉住。

**删除的符号：** `Agility`、`agility_to_reaction`、`agility_speed_factor`、
旧 `action_av(duration, agility)`、以及仅作迁移工具的 `agility_to_speed`。
`core/` + `tests/` 全量 grep 已无代码级 `Agility` 引用（仅 2 处测试名/注释中的
历史说明）；旧 `dungeon-*` crate 不在本轮范围（§10.5「不再维护」，
随 Phase G / R5 清理）。

**怪物与玩家的迁移口径（§11.6 第 5 项「先按旧敏捷保行为」）：**
速度 = **旧「耗时系数」的倒数**，因此新 AV 精确等于旧式的「`耗时 × 耗时系数`」
一项，丢掉的只有等量叠加的反应时常数项。**各角色之间的相对快慢与旧版完全一致**，
由 `template_speeds_preserve_legacy_agility_ordering` 钉住。

| 角色 | 旧敏捷 | 旧耗时系数 | 新速度 | 移动/攻击 AV（旧 → 新） | 游荡 AV（旧 → 新） |
|---|---|---|---|---|---|
| 玩家 | 10 | 0.80 | 1.25 | 310 → 240 | 470 → 400 |
| 老鼠 | 5 | 0.90 | 1.1111 | 355 → 270 | 535 → 450 |
| 蝎子 / 蘑菇傀儡 | 4 | 0.92 | 1.0870 | 364 → 276 | 548 → 460 |
| 哥布林 / 洞穴蟹 | 3 | 0.94 | 1.0638 | 373 → 282 | 561 → 470 |
| 孢子怪 | 8 | 0.84 | 1.1905 | 340 → 252 | 508 → 420 |
| 深鳗 | 10 | 0.80 | 1.25 | 310 → 240 | 470 → 400 |
| 洞穴鱼 | 14 | 0.72 | 1.3889 | 274 → 216 | 418 → 360 |

（旧 AV 已含各自的反应时：玩家 70 / 老鼠 85 / 蝎子 88 / 哥布林 91 /
孢子怪 76 / 洞穴鱼 58。）

**唯一的有意行为改动：** 删掉常数反应时后，AV 与 `base_duration` 变成严格成正比。
旧口径里「300ms 的攻击」实际要付 310 AV、而「500ms 的游荡」付 470 AV——
短行动被惩罚得更狠。这是设计取舍而非等价重构，已在 GAME.md 用 `[试调]`
标注全部速度值，并在 DsnE8 ②记录。

**新增测试（`cargo test -p core`：61 → 71）：**

- `av_is_inversely_proportional_to_speed_and_linear_in_duration`：`AV × 速度 ==
  base_duration` 恒成立、速度单调、AV 之比只由 base_duration 决定；
- `clamp_speed_bounds_both_ends_and_non_finite_inputs`：两端夹紧，NaN/±inf 兜成
  `MIN_SPEED`（防 AV 变 NaN 导致 `active_action_timer_delta` 的比较器 panic）；
- `player_initial_speed_is_the_migration_value`：1.25 是迁移产物而非设计值；
- `every_monster_template_has_usable_speeds` / `stats_expose_template_speeds`：
  八种怪速度齐全、有限、在 clamp 区间内，且真的走到 `stats()` 输出；
- `template_speeds_preserve_legacy_agility_ordering`：迁移保排序；
- `generated_actions_read_their_category_speed`（移动 1.6 / 攻击 0.8 刻意不同）
  与 `player_actions_read_their_category_speed`：接线正确，`Wait` 不受速度影响；
- `faster_monster_gets_its_action_ready_first`：真实生成 + 仲裁 + tick 的回合顺序。

**顺带修复的脆弱测试：** `system::tests::fov_memory_and_occupancy_update` 原先
隐含依赖「玩家选定的目标格在推进期间保持空闲」，而 `apply_player_command` 会推进
世界直到玩家行动做完，期间怪物可能游荡到该格、把命令改判成「走向怪物＝攻击」。
速度组件改变执行轮次 → 怪物消耗随机数的时机改变 → 这条"碰巧成立"的假设立刻失效
（实测怪物抢占了目标格）。改为先清场（`despawn_all::<Monster>/<Stairs>`）再断言。

**位置：** `core/src/components.rs`（`MoveSpeed`/`AttackSpeed`/`Speed`）、
`core/src/balance.rs`（`action_av`/`clamp_speed`/`MIN_SPEED`/`MAX_SPEED`/玩家初值）、
`core/src/action/entity.rs`（`SpeedRule`/`ActorSpeeds`/生成系统）、
`core/src/monster/mod.rs`（`MonsterSpeeds` 与八个模板）、
`core/src/world/init.rs`（玩家与怪物 spawn）、`core/src/test_util.rs`（helper）。

**关联：** D29、ECS8、REFACTOR.md §2.6 / §3.6.7 / §11.3 Phase D / §11.6；
DESIGN DsnE8 ②；GAME.md Gm1 / Gm4 / Gm7 / Gm8。
**教训见 LESSONS.md LSYN20**（「测试若依赖『没有别的实体碰巧动过』，它依赖的其实是执行顺序」）。

---


**原编号：** `G35`（迁移前）

### ECS8 — AV 门禁缺失：`ActionTimer` 未参与执行判断 ✅已修复



**问题：** `tick_action_timers_system` 把所有 `Active` 计时器减去最小正剩余 AV，只有最快的一个归零；但 `execute_*_system` 的查询只有 `With<Active>` + 行动组件，没有检查 `remaining_av <= 0`。结果所有 `Active` 行动每轮都会执行，AV/敏捷/速度不控制执行顺序或频率。



**影响：** 🔴 高 — 速度/AV 系统实际无效；REFACTOR.md / GAME.md 的 AV 描述与实现不符；在修复前更换速度公式没有意义。



**位置：** `core/src/action/execution/mod.rs:21-38`（tick）、`:42-303`（execute_*）、`:305-313`（run_action_cycle）；`core/src/world/loop_.rs:36-43`（advance_until_player_acted）



**修复：**



- 新增 `Ready` 组件（`core/src/components.rs`）；`tick_action_timers_system` 在 `remaining_av <= 0` 时插入 `Ready`；

- 所有 `execute_*_system` 查询加 `With<Ready>`；`mount_action` / `finish_action_*` / `player.rs::mount_player_action` 清理 `Ready`；

- `advance_until_player_acted` 每轮生成怪物行动 + tick/执行 + 结算，快怪可以在玩家行动期间执行多次；

- 回归测试：`action::execution::tests::av_gate_only_executes_ready_actions`、`zero_timer_is_marked_ready_without_positive_peers`。



**状态：** ✅已修复。



**关联：** A41、ECS3、ECS7。



---




**原编号：** `I89`（迁移前）

### ECS9 — 事件生命周期错误：`EventReader` 每轮重读历史事件 ✅已修复



**问题：** `build_core_schedule()` / `run_settle_systems()` 每次调用都新建 Schedule，`EventReader` 游标随之归零；同时 `insert_core_resources` 只注册 `Events<T>`，从未调用 `Events::update()`。因此每轮结算都会重读历史上所有 `AttackIntentEvent` / `AttackEvent`，旧伤害被反复结算，事件缓冲无限增长。



**影响：** 🔴 高 — 战斗结果不可信（伤害重复、死亡日志重复、内存增长）；任何基于事件的扩展点都不可靠。



**位置：** `core/src/system/mod.rs:257-282`（build_core_schedule/run_settle_systems）、`core/src/world/init.rs:70-76`（Events 注册）、`core/src/world/init.rs:393-416`（build_init_schedule/run_initialization）



**修复：**



- `insert_core_resources` 注册持久 Schedule（`CoreInitSchedule` / `CoreSettleSchedule`）；

- `build_core_schedule` 使用 `Schedule::new(CoreSettleSchedule)`，`run_settle_systems` 用 `world.run_schedule(CoreSettleSchedule)`，不再每轮重建；

- 新增 `update_events_system` 在结算末尾对 7 种事件调用 `Events::update()`；

- 回归测试：`system::tests::settle_does_not_reapply_old_events`。



**状态：** ✅已修复。



**关联：** A41、REFACTOR.md §3.6.8 / §5。



---



**原编号：** `I90`（迁移前）

### ECS10 — Gm4 玩家初始 HP 文档算术错误：28 vs 实现 33 ✅已修复

**修复前：** Gm4 标注 `HP = 20 + 等级×5 + 防御×2 = 28`，代码 `max_hp_for(1,4)=33`，文档漏加 `等级×5=5`。

**修复后：** Gm4 表改 `= 33（1级防4时）` 并展开计算过程 `20+5+8=33`，标注改 [⃞计算]。

**位置：** `GAME.md:152`

---


**原编号：** `D21`（迁移前）

### ECS11 — Gm6 熟练度表格与公式/代码错位一级 ✅已修复

**修复前：** 表格「熟练度 1」行写零加成，公式/代码熟练度 1 即有加成（治愈 +3、护盾/狂暴 +2）；Gm5 表同步错位。

**修复后：** Gm6 表格修正为熟练度 1 = `15+精通+3 / +7 / +7`，并加说明「熟练度 1 即有加成（熟练度×系数）」；Gm5 表护盾/狂暴熟练度 1 修正为 +7。

**位置：** `GAME.md:194-195`（Gm5）、`GAME.md:223-228`（Gm6）

---


**原编号：** `D22`（迁移前）

### ECS12 — Gm1 移动耗时 300ms 与 Gm7 武器攻速矛盾，CanMove.duration 成死字段 ✅已修复

**修复前：** Gm1 行动表写「移动 300ms」，I67 后耗时由主手武器 speed 决定。

**修复后：** Gm1 行动表改为「武器 speed（无武器 300）」并标注 [⃞计算]，交叉引用 Gm7；CanMove.duration 死字段注释同步（清理并入 A36）。

**位置：** `GAME.md:14`

---


**原编号：** `D26`（迁移前）

### ECS13 — Gm3 升级经验表 round vs 实现 trunc（每级差 1） ✅已修复

**修复前：** 表格按四舍五入，代码 `as u64` 截断（2→3=90 表写 91 等）。

**修复后：** 公式标注「结果向下取整（trunc）」，表格改为截断值（90/159/329/645/890）。

**位置：** `GAME.md:106-120`

---


**原编号：** `D27`（迁移前）

### ECS14 — Gm6 卷轴「每层 1-3 张」未记载深层增量 ✅已修复

**修复前：** 实现为 `1-3 + ⌊(楼层-1)/5⌋`，文档只写 1-3。

**修复后：** Gm6 掉落描述补充深层增量公式。

**位置：** `GAME.md:201`

---


**原编号：** `D28`（迁移前）

### ECS15 — 怪物种群补足循环无进展保证，深层/水域地图可死循环 ✅已修复

**修复前：** `while positions.len() < min_count` 无迭代上限、无候选格不足判定，可行走格不足时死循环卡死。

**修复后：** 迭代上限 40×期望数，未达期望时 log::warn 降级（不足比卡死好）。回归测试 1 个（全墙 6 格地图 + floor=20 不死循环）。

**位置：** `dungeon-world/src/population.rs`（generate_monster_population 补足段）

---


**原编号：** `I82`（迁移前）

### ECS16 — 单房间地面物品放置 random_range 空区间 panic ✅已修复

**修复前：** `random_range(2..r.w.saturating_sub(2))` 在房间 bounding box ≤4 时区间为空 panic。

**修复后：** 房间过小时跳过采样，充底到中心最近可行走格（与多房间路径语义一致）。

**位置：** `dungeon-world/src/init.rs`（place_ground_items 单房间分支）

---


**原编号：** `I83`（迁移前）

### ECS17 — pick_stair_pos 兜底坐标未钳制，可引发越界 panic ✅已修复

**修复前：** 螺旋搜索全失败时兜底 `(spx+15, spy)` 未 clamp，spx>64 时越界坐标传给 ensure_connection_between 索引越界 panic。

**修复后：** 兜底坐标 `saturating_add(15).min(MAP_WIDTH-1)` 钳制 + `nearest_walkable` 保证可行走。回归测试 1 个（spawn(75,59) 返回界内 walkable 坐标）。

**位置：** `dungeon-world/src/init.rs`（pick_stair_pos 兜底）

---


**原编号：** `I84`（迁移前）

### ECS18 — 护盾/狂暴实际时长约为文档宣称 3 倍，技能超模 ✅已修复

**修复前：** duration=3 → 3000 AV；玩家单次行动 AV 约 310ms，实际覆盖 7-13 次行动，远超「3 次行动」设计意图。

**修复后：** duration 3→1（1000 AV ≈ 3 次玩家行动），Gm2/Gm5 数值表同步更新（标注 [⃞试调] 调整轨迹）。回归测试 1 个（技能 duration=1 断言）。

**位置：** `dungeon-core/src/items.rs`（use_item 卷轴学习）、`GAME.md:98-99/194-195`

---


**原编号：** `G33`（迁移前）

### ECS19 — 玩家近战攻击无距离校验，可隔空命中已离开的怪物 ✅已修复

**修复前：** 玩家确认攻击后若怪逃跑离开，execute_attack 仍全额结算伤害；怪物侧攻击先判 8 方向邻接。check_condition 的 Attack 分支只查 target 仍是 Monster。

**修复后：** 新增 `adjacent_8` 距离判定，check_condition 的 Attack 分支与 execute_attack 执行入口双重校验（LECS20 执行层兜底），玩家与怪物规则对称。回归测试 1 个（非邻接目标攻击被取消）。

**位置：** `dungeon-action/src/execute.rs`（check_condition/execute_attack/adjacent_8）

---


**原编号：** `G29`（迁移前）

### ECS20 — 逃跑怪物永不回头：滞回未落地 + 卡墙角原地挨打 ✅已修复

**修复前：** 注释声称滞回（进入<25% 退出>30%）但实现仅 <25%；execute_flee 无路可逃时原地不动不反击。

**修复后：** 新增 `FLEE_HP_RATIO_EXIT=0.30`，保活检查用退出阈值（滞回落地：25%-30% 区间内已入队的逃跑继续有效，≥30% 取消）；提取共享 `monster_attack_player`，execute_flee 死角且邻接玩家（视野内）时兜底反击。回归测试 3 个（阈值关系 / 超阈值取消 / 死角反击）。

**位置：** `dungeon-core/src/ops.rs`（FLEE_HP_RATIO_EXIT）、`dungeon-action/src/execute.rs`（check_condition/execute_flee/monster_attack_player）

---


**原编号：** `G30`（迁移前）

### ECS21 — 对角穿墙（corner-cutting）：玩家与怪物均可斜穿墙角 ✅已修复

**修复前：** can_move_to 注释声称验证不穿墙角但实现没有；玩家入队与怪物 A* 均不检查。

**修复后：** can_move_to 增加对角约束（两侧正交格须可通行且未被占用）；`handle_player_direction` 入队前预检同规则；A* 8 方向遍历同步检查。回归测试 4 个（can_move_to 两侧墙/单侧墙/开放 + 玩家入队拒绝 + A* 不穿墙角 + A* 开放对角）。

**位置：** `dungeon-action/src/execute.rs`（can_move_to）、`dungeon-action/src/player.rs`、`dungeon-core/src/pathfinding.rs`（astar）

---


**原编号：** `G31`（迁移前）

### ECS22 — `ensure_connectivity` 直线收尾挖单格而非 2x2（G22 修复不完整） ✅已修复

**修复前：** G22 修复声明通道挖 2x2 块，但 `ensure_connectivity` 的 Bresenham 收尾用单格 `carve_channel`——区域间通道对角转折处仍可能 4 方向断裂（`ensure_connection_between` 已 2x2，两处不一致）。

**修复后：** 抽取共用 `carve_2x2`，`ensure_connectivity` 游走与收尾全部 2x2。回归测试 2 个（隔离区域连通 20 种子 + carve_2x2 挖 4 格）。

**位置：** `dungeon-core/src/map_gen.rs`（carve_2x2）

**提交：** `63866f2`


**原编号：** `G25`（迁移前）

### ECS23 — `place_ground_items` 单房间退路硬编码 `SmallRng::seed_from_u64(42)` ✅已修复

**修复前：** 单房间分支用固定种子 42 的独立 RNG，物品位置固定且违反 DsnE5。

**修复后：** `place_ground_items` 增加 `rng` 参数，单房间分支使用传入的楼层 RNG，setup_world/descend 两入口一致。

**位置：** `dungeon-world/src/init.rs`


**原编号：** `D18`（迁移前）

### ECS24 — `place_skill_scrolls` 缺少 exclude 参数 ✅已修复

**修复前：** 文档记录卷轴可能生成在楼梯/出生点上（ISSUES 开放区 D14）。

**修复后：** 核实代码：`place_skill_scrolls` 早已带 `exclude` 参数且 setup_world/descend 均传入（与 spawn_monsters/place_ground_items/scatter_stones 一致）。纯文档状态更新，无代码改动。

**位置：** `dungeon-world/src/init.rs:94`


**原编号：** `D14`（迁移前）

### ECS25 — 同类死抽象/重复表示清理 ✅已修复

**问题：** 与 `ActionKind` 同类的中央 token / 提前抽象 / 重复表示：

- 身份 ZST `Rat/Scorpion/...` 由 `MonsterKindId` match 后插入，但无任何读取方；与 `MonsterKindId` 重复；
- `CreatureKind` 无读取方；`EntityClass` 唯一读取是未迁移的 `Item` 判断；
- `ActionSucceededEvent` / `ActionFailedEvent` 无人发/读；`DeathEvent` / `LevelUpEvent` 写入但无消费者；`ThreatEvent` / `ThreatTable` 占位未接线；
- `BeAttacked` / `NeedRecordBeAttacked` 写入但无读取；`PendingExp` 是绕过 `DeathEvent` 的旁路；
- `MeleeResult` + `prepare_attack_event` / `resolve_melee` / `damage_entity` 是死代码/重复战斗路径；
- `MonsterStats` / `WorldInitConfig` 是低优先级中间层。

**影响：** 🟡 中 — 死抽象让文档/代码看起来比实际复杂，且容易误以为扩展点已存在。

**位置：** 见 REFACTOR.md §10.8 逐项清单。

**状态：** ✅已修复（Phase E 全量清理完成）。原则是「每个保留的抽象必须有真实读取方/消费者」，逐项**实测**后处置如下（两处与当初预判不同，已标注）：

| 项 | 实测 | 处置 |
|---|---|---|
| 身份 ZST `Rat/…/DeepEel` | 零 insert、零 query（比预判更死：连插入方都已随 Phase C 消失） | 删除 8 个 |
| `CreatureKind` | 只写不读 | 删除（含模板字段与 9 处赋值） |
| `EntityClass` | `EntityClass::Item` 判断恒假（全库从未插入过 `Item`） | 删除枚举；楼梯跳过改靠 `Stairs` 标记 |
| `DeathEvent` | 只写不读 | **接线**：事件携带 `reward`，由 `experience` 模块消费 |
| `LevelUpEvent` | 只写不读 | 保留：经验结算链路的 `EventLog` 是真实消费者，事件留给未来 UI/成就 |
| `PendingExp` | 绕过 `DeathEvent` 的旁路 | 删除资源；奖励改由事件携带 |
| `ThreatEvent` / `ThreatReason` / `ThreatTable` | 无生产者、无消费者，`ThreatTable` 三个方法也零调用 | 全部删除（S4 仇恨系统落地时重建） |
| `BeAttacked` / `NeedRecordBeAttacked` | 只写不读 | 删除组件与整条 `record_be_attacked_system`（从结算 Schedule 摘除） |
| `MeleeResult` | **是活的**：`compute_melee_damage` 返回它 | **保留**（与预判不同） |
| `prepare_attack_event` / `resolve_melee` / `damage_entity` / `can_attack` / `adjacent_8` | 零调用（整条链只被彼此调用） | 删除 5 个死函数 |
| `MonsterStats` / `WorldInitConfig` | 有真实调用方 | 保留 |
| `Idle/Active/Failure` | I91 已修 + 互斥测试在守 | 保留 ZST |

**顺带完成的结构对齐：** `system/mod.rs`（654 行）拆为
`mod.rs`(104) + `combat` + `perception` + `death` + `experience` + `occupancy` + `system_tests`；
`monster/mod.rs`（455 行）拆为 `mod.rs`(15) + `template`(物种数值) + `spawn`(出现概率) + `monster_tests`。
`mod.rs` 只留模块声明与重导出，与 `map/`・`spatial/`・`action/` 的形式一致。

**确认记录：** 2026-09 对话；§11.6 第 6 项。

**关联：** REFACTOR.md §10.8。

---


**原编号：** `A43`（迁移前）

### ECS26 — 楼梯/地面物品可能生成在不可行走格上 + 通道 4 方向断裂 ✅已修复

**成因链（四层）：**
1. `generate_stalactites` 在房间内每格 7% 概率放钟乳石（**含房间中心**），`generate_water` 扩散也可能波及
2. `pick_stair_pos` 多房间分支直接返回 `farthest_room_from`（最远房间中心）**不检查 walkable**（出生点 `spawn_point()` 有螺旋兜底，楼梯没有）→ 楼梯落在 Stalactite/DeepWater 上
3. `ensure_connection_between` 醉汉游走提前停止（距离<3 break）或 500 步耗尽 → 通道挖不到终点
4. **8 方向通道 4 方向断裂**：游走/Bresenham 路径可对角相邻，而玩家移动与 `has_path_between` 是 4 方向——单格宽通道在锯齿处断裂，玩家走不过去

**同类问题：** `place_ground_items` 多房间分支同样直接取房间中心（物品落不可走格捡不到）；`ensure_connectivity` 通道同样可能挖不到位且对角断裂。

**修复：**
- 治本：`generate_stalactites` 跳过房间中心（中心永远 Floor）
- 兜底：新增 `Map::nearest_walkable`（螺旋搜索），`pick_stair_pos`/`place_ground_items`（含 +1 偏移落点）/`persist.rs` 读档楼梯全部走兜底
- 收尾：`ensure_connection_between`/`ensure_connectivity` 游走后用 Bresenham 直线强制打通终点
- 连通性：通道改为挖 **2x2 块**（非单格），保证 4 方向连通

**回归测试：** `test_stalactites_skips_room_centers`（20 轮钟乳石生成中心仍 Floor）、`test_nearest_walkable_fallback`、`test_pick_stair_pos_always_walkable_and_reachable`（60 种子：落点 walkable + 完整流程后出生点→楼梯可达）

**位置：** `dungeon-world/src/init.rs`（pick_stair_pos/place_ground_items）、`dungeon-world/src/persist.rs`、`dungeon-core/src/map_gen.rs`（generate_stalactites/ensure_connection_between/ensure_connectivity）、`dungeon-core/src/lib.rs`（nearest_walkable）


**原编号：** `G22`（迁移前）
