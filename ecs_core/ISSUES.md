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

### ECS31 — `Tile` 的 5 处中央 match：加一个地形变体要改 5 个地方 ✅已修复

**修复前：** 加一个 `Tile` 变体需要在 `glyph()` / `walkable()` / `blocking()` / `From<u8>` / `Into<u8>` **五处**同步修改，属中央分派债（与已删除的 `ActionKind` 同类）。且地形属性（如移动代价）没有存放位置，只能继续往这 5 处加。

**修复后（Phase H4）：** 新增 `ecs_core/src/map/tile.rs`，把种类与属性收进一张
`TILE_PROPS: &[TileProps]`（列：`tile` / `id` / `glyph` / `walkable` / `blocks_vision` /
`move_cost`）。五个读取点**全部读表**，没有一处 `match`：

| 读取点 | 修复前 | 修复后 |
|---|---|---|
| `glyph()` | `match self { Wall \| Stalactite => '#', .. }` | `self.props().glyph` |
| `walkable()` | `matches!(self, Floor \| ShallowWater \| ..)` | `self.props().walkable` |
| `blocks_vision()` | `matches!(self, Wall \| Stalactite \| ..)` | `self.props().blocks_vision` |
| `Serialize` | `match self { Wall => 0, .. }` | `serializer.serialize_u8(self.id())`，判别值取 `#[repr(u8)]` |
| `Deserialize` | `match v { 0 => Ok(Wall), .. }` | `Tile::from_id(v)`（`const fn` 查表） |

**加一个地形现在只需两处**：`Tile` 末尾加变体 + `TILE_PROPS` 末尾加一行。
（枚举本身无法自动派生——语言限制——`From<u8>` 已不再是其中之一。）

**配套纪律（LESSONS `LECS22`）：** 换表的代价是"漏加一项"从**编译期穷举检查**退化成
**运行期静默**，所以配了 4 条测试：`table_covers_every_variant_and_round_trips`（逐行枚举
全表 + 序列化往返 + 判别值唯一 + 越界不 panic）、`ids_match_the_pre_h4_serde_mapping`、
`properties_match_the_pre_h4_values`、`move_cost_is_still_a_reserved_slot`。

**顺带去掉了第二处必改点：** `presentation::catalog::tile_id` 原本自带一份 11 项穷尽
`match`（加地形要改的第六处），现在改为 `u16::from(tile.id())`；它原有的
`tile_ids_match_serde_discriminants` 用例保留，继续钉住"契约编号 = 存档判别值"。

**位置：** `ecs_core/src/map/tile.rs`（新）、`ecs_core/src/map/mod.rs`（改为转出）、
`presentation/src/catalog.rs`（`tile_id`）

**关联：** ECS27（地形代价维度——`move_cost` 列即该问题的预留位）、ECS28、DsnE13；
REFACTOR §11.3 Phase H（H4）。

### ECS35 — 行动终态无保证：执行器漏发事件 → actor 永久卡 `Active` + 子实体泄漏 ✅已修复

**修复前：** 每个执行器手写 `events.succeeded.write(...)` / `events.failed.write(...)`。若新增执行器漏写，**actor 会永久停在 `Active`**（生成系统被 `Without<Active>` 挡住，不再为它产出候选），且 action 子实体无终态事件可消费，永久泄漏。**编译器不会报错**，链路也没有兜底。

**修复后（Phase H1）：** 终态收成**唯一出口**——`ActionEvents` 的两个 `EventWriter` 改为**私有字段**，并新增两个方法：

```rust
pub struct ActionEvents<'w, 's> {
    succeeded: EventWriter<'w, ActionSucceededEvent>,  // 私有
    failed:    EventWriter<'w, ActionFailedEvent>,     // 私有
    commands:  Commands<'w, 's>,
}
impl ActionEvents<'_, '_> {
    pub fn succeed(&mut self, action: Entity, actor: Entity);
    pub fn fail(&mut self, action: Entity, actor: Entity);
}
```

于是「**恰好一个终态事件**」不再靠人记住，而是**类型层面的保证**：执行器拿不到
`EventWriter`，只能经这两个方法结束行动，而它们各自恰好发一个事件。
6 个执行器的 15 处裸 `events.*.write(...)` 全部改走新出口。

**判定：** 不需要"兜底检测"补丁——把出口收成一个，漏发事件在**编译期**就不可能发生。

**位置：** `ecs_core/src/action/entity.rs`（`ActionEvents` 及 6 个 `execute_*_system`）

**关联：** ECS36（同一根因的另一半）；REFACTOR §11.3 Phase H（H1）；DESIGN DsnE8。

---

### ECS36 — `Ready` 的清理散落多处：只有 `execute_move_system` 显式清理 ✅已修复

**修复前：** `execute_move_system` 显式 `remove::<Ready>()`，其余 5 个执行器**靠 completion `despawn` 实体顺带清理**——同一件清理有两条路径，且只有注释兜着。`components.rs` 声明"执行系统只处理 `With<Ready>` 的实体"，该守卫因此依赖"恰好有一方清理"。

**修复后（Phase H1）：** `Ready` 的清理并入终态出口——`ActionEvents::succeed` / `::fail`
**必然**清 `Ready`，与发事件是同一个原子动作。`execute_move_system` 原有的显式清理
已删除（不再需要），它也**不再需要 `Commands` 参数**；`action_completion_system` 里的
`remove::<Ready>()` 保留作防御（注释已说明它只剩防御意义）。

**H1 之前只有 `Move` 成立的断言，现在对全部 6 个行动成立**：`entity_tests.rs` 的
`run_one_action_roundtrip`（被 6 个 `parity_*` 用例共用）现在断言
「执行器跑完、completion 之前 `Ready` 已消失」。**变异验证：** 把终态出口里的
`remove::<Ready>()` 去掉后，**7 个用例立刻失败**（6 个 `parity_*` +
`parameterized_move_matches_world_based_move`），证明这条断言真的在守这条不变式。

**为什么不用"再跑一轮执行器、断言没有新事件"来测：** 终态事件是 completion 的输入，
`clear()` 掉它 completion 就收不回实体（断言会以"实体未被回收"假失败）；
先跑 completion 则实体已 despawn，断言退化为平凡真。见 `entity_tests.rs` 里的说明。

**位置：** `ecs_core/src/action/entity.rs`（`ActionEvents` / `execute_move_system` /
`action_completion_system`）、`ecs_core/src/components.rs`（`Ready` 的文档）、
`ecs_core/src/action/entity_tests.rs`（`run_one_action_roundtrip`）

**关联：** ECS35（同一根因）；REFACTOR §11.3 Phase H（H1）。

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

### ECS29 — `compute_melee_damage` 的签名无扩展位，返回类型是黑箱 ✅已修复

**修复前：** 签名是 5 个裸标量 `(attack, defense, crit_rate, crit_damage, crit_roll)`，**恰好表达"减一次防、乘一次暴击"**；每新增一个伤害修正都要改签名 + 改所有调用点。返回 `MeleeResult { damage, is_crit }` 只有最终值与是否暴击，**没有各因子分解**，因此日志无法解释"为什么是这个数"，也没有"按因子触发"的落点。

**修复后（Phase H2）：** 实现搬到 `ecs_core/src/rules/damage.rs`（数值类规则统一进 `rules`，
`combat` 只管位置/目标类规则），形状按 DESIGN DsnE9 定案：

| 决策 | 落地形态 |
|---|---|
| 输入按来源分组 | `MeleeInput { attack, defense, target_crit: CritProfile { rate, damage }, crit_roll }` |
| 返回因子分解 | `MeleeBreakdown { base, crit_multiplier, damage, is_crit }` + `recompute()` |
| 随机数仍在系统层 | `crit_roll` 仍由调用方从 `GameRng` 取，`rules::melee` 保持纯函数 |

`combat::compute_melee_damage` **保留为薄适配器**（内部转调 `rules::melee`），
因此唯一的调用点（`system/combat.rs`）不必为"改形状"而改动。

**验收（Phase H2 的两条线）：**

- **公式与数值不变**：`damage_floor_is_one`（下限 1）、
  `crit_threshold_is_strictly_greater`（`>` 而非 `>=`）、
  `crit_multiplier_is_neutral_or_one_plus_bonus`（负数加成吃 `max(0.0)`）；
  另加字段映射回归 `legacy_scalar_adapter_maps_fields_correctly`（用 `to_bits` 比较，正确处理 NaN）；
- **返回可复算**：`factors_reconstruct_the_final_damage` —— `base × crit_multiplier`
  逐位等于 `damage`。这条让"返回因子分解"成为**可断言的性质**，而不只是一堆字段。

**仍然待定（属 GAME.md，本轮一个都没填）：** 分区（有哪些因子）与系数（防御系数、
增伤叠加方式）。要改成加权形式时 `MeleeInput` 已是"按来源分组的输入"，加字段即可。

**位置：** `ecs_core/src/rules/damage.rs`（新）、`ecs_core/src/combat/mod.rs`（适配器）、
`ecs_core/src/system/combat.rs`（调用点不变）

**关联：** DESIGN DsnE9 / DsnE10（修正器与伤害输入是同一套"结构化输入"思路）；
REFACTOR §11.3 Phase H（H2）。

---

### ECS30 — 行动候选查询接受 actor 的**任意**子实体，持久子实体会污染仲裁 ✅已修复

**修复前：** `CandidateAction = (Entity, &ActionPriority, &ChildOf)`，候选查询是 `Query<CandidateAction, (With<Candidate>, Without<ActiveAction>)>`。该查询**不看实体类型**，因此 actor 的任意子实体只要带 `Candidate` 就会被纳入仲裁：**赢家会被 `insert(ActiveAction)` 认领、落选者会被 `despawn`**。实测 bevy 0.16 的 `Children` 是 `linked_spawn`，子实体语义同时承担"级联销毁"。

**影响：** 🟡 中高 — 每加一种"挂在 actor 下的持久实体"都会命中，而装备与地块效果的落地（DsnE12）都会加世界级实体；失败模式是**静默**（无报错、无日志）。

**修复后（Phase H / ECS41 一并处置）：** 行动归属改用**专用关系类型**
（`ecs_core/src/action/ownership.rs`）：

```rust
#[derive(Component)] #[relationship(relationship_target = ActionChildren)]
pub struct ActionOf(pub Entity);
#[derive(Component)] #[relationship_target(relationship = ActionOf, linked_spawn)]
pub struct ActionChildren(Vec<Entity>);
```

候选查询、`ReadyAction` 系列别名、仲裁/执行/completion 的归属读取全部改走 `ActionOf`，
**生产代码里不再有 `ChildOf`**。于是"这条查询只可能匹配到行动实体"从"filter 恰好写对"
升级为**类型保证**：别的子实体类型上根本没有 `ActionOf`，过滤条件写错也匹配不到。

**行为证据（新用例，且经变异验证）：**
`action_chain_never_claims_or_despawns_unrelated_children_of_an_actor` 刻意构造**最容易命中的情形**——
actor 名下放一个用**通用层级关系** `ChildOf` 挂着的旁观者，并给它 `Candidate` +
`ActionPriority(PRIORITY_FLEE)`（最高优先级），然后跑满 5 轮完整链路，断言它
**不被动、不被删、组件不被改写**。

> **变异验证的教训（值得记一笔）：** 这条用例的**第一版写弱了**——只断言"旁观者还活着"。
> 把候选查询改回 `ChildOf` 后它**照样通过**：因为旧口径下旁观者会**赢下仲裁**，
> 被 `insert(ActiveAction)` 认领，于是它当然还"活着"（只是已经变成行动实体了）。
> 补上 `ActiveAction` / `Candidate` 两条断言后，同一变异**立刻失败**。
> 教训：**"实体还在"不等于"没被动过"**——杀不死的 bug 常常是"被改写了"而不是"被删了"。

**位置：** `ecs_core/src/action/ownership.rs`（新）、`ecs_core/src/action/entity.rs`
（别名 + 仲裁 + 执行 + completion）、`ecs_core/src/action/entity_tests.rs`（归属构造与对照用例）

**关联：** ECS41（同一根因的另一半）；DESIGN DsnE12 第 1 条；REFACTOR §11.3 Phase H（H9/H11）。

---

### ECS41 — 效果/装备挂到 actor 名下时，`ChildOf` 既不安全也不够用 ✅已修复

**修复前：** 效果实体需要一个"挂在谁身上"的表达，而现成的 `ChildOf` 有两个缺陷：

1. **不安全（巧合式安全）**：行动链的候选查询
   `Query<(Entity, &ActionPriority, &ChildOf), (With<Candidate>, Without<ActiveAction>)>`
   **不看实体类型**——新挂在 actor 下的子实体只要带了这两个组件之一，就会被行动仲裁
   **认领或静默删除**（ECS30）。
2. **不够用**：`ChildOf` 一个父只有**一个** `Children` 列表，无法把"行动子实体"与
   "效果子实体"分成两个分组。

**修复后（Phase H）：** 行动侧先落地**专用关系**（`ActionOf` / `ActionChildren`，
`ecs_core/src/action/ownership.rs`），两者分工明确：

| 关系 | 谁用 | 级联销毁 | 参与行动仲裁 |
|---|---|---|---|
| `ActionOf` / `ActionChildren` | 行动实体 | ✅（`linked_spawn`，actor 死亡不留残留） | ✅ 唯一参与者 |
| `EffectOf` / `OwnedEffects`（H11 建） | 效果实体 | 按效果类别定（见 ECS42） | ❌ **结构上匹配不到** |
| `ChildOf` / `Children`（通用层级） | 不属于行动链的东西 | ✅ | ❌ |

`Relationship` / `relationship_target` 是 bevy 0.16 的公开派生
（`bevy_ecs-0.16.1/src/relationship/mod.rs:35-72`），**不需要自己写 unsafe**。

**位置：** `ecs_core/src/action/ownership.rs`（新）、`ecs_core/src/action/mod.rs`（转出）、
`ecs_core/src/action/entity.rs`

**关联：** ECS30（同一根因，同时修复）、ECS42（`linked_spawn` 对装备是错的）、
DESIGN DsnE12 第 1 条；REFACTOR §11.3 Phase H（H9/H11）。

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

**关联：** ECS41、DESIGN DsnE12 第 1 条与开放项；REFACTOR §11.3 Phase H（H9/H11）。

---

