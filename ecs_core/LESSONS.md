> **⚠️ 修改前必须阅读或回忆 [RULE.md](../RULE.md)——它定义了本文档的维护规则和更新时机。**

# 经验教训 —— ecs_core

**归属范围：** 领域规则引擎的教训：ECS 用法、领域不变量、规则该住在哪一层。

**编号：** `LECS1`、`LECS2`… 每个 crate 独立编号。
判据：教训的读者是 AI。**在任何 crate 都适用**的原则留根目录；只在某个 crate 的代码里有落点的归该 crate。


### LECS1 — 区分 `query()` 和 `try_query()`

`World::query()` 要 `&mut self` 不是因为查询执行需要写，而是因为内部做了**组件懒注册**。`World::try_query()` 只要 `&self`。

项目中所有组件在 `setup_world` 时已注册，可安全用 `try_query().unwrap()` 替代 `query()`。

**原编号：** `LECS1`（迁移前）

---

### LECS2 — resource_mut() 返回的 Mut<T> 借用了 &mut World

```rust
// ❌ 错误：临时 &mut World 过早 drop
let memory = world.resource_mut::<MapMemory>();
// ✅ 正确
let mut world = world; // 绑定延长生命周期
let memory = world.resource_mut::<MapMemory>();
```

**原编号：** `LECS2`（迁移前）

---

### LECS3 — bevy_ecs 0.16 Bundle 上限 16 个组件

超过需用 `cmd.insert()` 链式。

**原编号：** `LECS3`（迁移前）

---

### LECS4 — 行动系统：AV 统一值 + 全局单队列

旧设计有"冷却计时器"和"队列推进"两个独立时间维度 → 需要同步、量纲对齐 → 移除冷却，全部由 AV 统一管理。

```rust
AV = reaction_time + duration        // 单一值入队
av_remaining -= next_event_distance() // 同步推进
av_remaining ≤ 0 → pop_ready()       // 执行
```

**原编号：** `LECS4`（迁移前）

---

### LECS5 — 新旧系统共存时，计算/集成层必须设排他开关，不可对两者求和

**问题背景：** I29 引入 ActiveBuffs（新 AV 制）时保留了旧 `Buffs`（旧回合制）。`execute_skill` 同时写入两者，`effective_attack` / `effective_defense` 对两者求和 → Buff 双倍叠加。

**参见 ISSUES.md #G8**

**错误做法：** 
```rust
// ❌ 两个系统各算各的，求和
if let Some(ab) = active_buffs { atk += berzerk_from_av; }
if let Some(b) = buffs { atk += berserk_from_turns; }
```

**正确做法：** 引入新系统时，在唯一切入点（计算层）设三态开关：
- **Phase 1（共存期）：** 新系统存在时**只读新系统**，旧系统仅作 fallback
- **Phase 2（验证期）：** 写双系统但**只读新系统**，旧系统作为异常检测（assert 两者一致）
- **Phase 3（完成期）：** 移除旧系统，代码中不再出现旧系统引用

```rust
// ✅ 新系统优先，旧系统仅作为 fallback
if let Some(ab) = active_buffs {
    atk += berserk_from_av;
} else if let Some(b) = buffs {
    atk += berserk_from_turns;
} // 绝不同时求和
```

**为什么更好：** 双系统求和不是一个"崩溃很快暴露"的错误——它不会 panic，不会断言失败，只会静默影响游戏平衡。此后无论谁改了 `effective_attack` 代码，都不会无意间恢复求和行为。

---

## 三、ECS 使用

**原编号：** `LECS5`（迁移前）

---

### LECS6 — 组件式行动授权优于行为树/状态机

行动能力由组件赋予（`CanMove`/`CanChase`/`CanFlee`/`CanWander`/`CanWait`），组件携带条件，系统批量检查，仲裁解决冲突。

**收益：** 并行检查、易扩展（加组件=加行为）、不需要修改决策流程。

**原编号：** `LECS6`（迁移前）

---

### LECS7 — Intent 缓冲区模式用于并行决策

三个决策 system 各自写入独立缓冲区（`ChaseIntents`/`FleeIntents`/`WanderIntents`），仲裁 system 串行合并。

**收益：** 无并发写冲突、数据流显式可追踪。

**原编号：** `LECS7`（迁移前）

---

### LECS8 — 状态变更后必须立即刷新依赖

- 死亡后跳过 `advance_and_settle`（否则死后仍推进）
- 下楼后跑 `fov_system` + `update_map_memory` + `update_visible_memory`（否则新楼层视野为空）

**原编号：** `LECS8`（迁移前）

---

### LECS9 — despawn→spawn 要维护所有组件的传递

`descend()` despawn 所有实体后重生玩家，重生时不能漏组件。

---

## 四、游戏逻辑

**原编号：** `LECS9`（迁移前）

---

### LECS10 — buff 必须参与伤害/防御计算

`effective_attack` 和 `effective_defense` 必须包含 `stats.attack/defense + equipment_bonus + buffs.berserk_atk/shield_def`。

**原编号：** `LECS10`（迁移前）

---

### LECS11 — 战斗公式的暴击/掉落必须走统一 RNG

`LootTable::roll()` 和 `execute_attack` 的暴击判定应接受 `&mut impl Rng` 参数，从 `GameRng` 取随机值，而非调用 `rand::random::<f32>()`（系统熵）。否则游戏行为不可复现。

**原编号：** `LECS11`（迁移前）

---

### LECS12 — 逃跑行为需要滞回区间

进入条件（`CanFlee::condition`）和退出条件（`check_condition`）不应相同。否则一旦逃跑永不回头。建议：

```rust
进入：hp_ratio < 0.25
退出：hp_ratio < 0.30   // 多 5% 的宽容窗口
```

**原编号：** `LECS12`（迁移前）

---

### LECS13 — 游荡/随机行为使用独立随机源

怪物游荡方向用 `rand::random::<u8>() % 8`（原设计用 `(FloorNumber + monster_count) % 8` 确定性计算）→ 所有怪物朝同一方向游荡，不合理。

---

## 五、物品/装备

**原编号：** `LECS13`（迁移前）

---

### LECS14 — 事件粒度的推进

`next_event_distance()` → `advance(dist)` → `pop_ready()` 的设计天然支持分批推进。可以单步执行到下一个事件点，让玩家在每个事件后做决策。

**不是所有行动都需要玩家介入：** 每个事件步执行一个条目（而非一个实体的所有条目），玩家在行动执行前可以切换方向。

---

## 八、地图与地形

**原编号：** `LECS14`（迁移前）

---

### LECS15 — 楼梯需要可达性保证

地图生成后 BFS 检查出生点到楼梯是否有路径，若无可使用加权醉汉游走（70% 指向楼梯方向，30% 随机）挖掘通道。

**原编号：** `LECS15`（迁移前）

---

### LECS16 — 环境修饰不应覆盖关键位置

水体/钟乳石生成时应远离房间中心（`is_away_from_rooms`），但保护距离不宜过大，否则水体偏少。

---

## 九、存档兼容性

**原编号：** `LECS16`（迁移前）

---

### LECS17 — 实体放置应始终排除关键位置

`generate_monster_population` 使用噪声密度 + 元胞扩散覆盖所有 walkable 格，没有排除楼梯/出生点/物品位置。这导致：
- 怪物站在楼梯格上 → 阻止玩家走回楼梯
- 怪物站在物品格上 → 将玩家的"拾取意图"变成了"攻击意图"

**教训：** 任何随机/噪声驱动的实体放置函数都应接受一个排除位置集合参数。怪物、陷阱、装饰物等不应生成在玩家必须交互的位置（楼梯、传送点、关键物品）。

**原编号：** `LECS17`（迁移前）

---

### LECS18 — ECS 查询必须包含最具体的类型约束

`on_stairs()` 判断玩家是否站在楼梯上，但查询写成了：

```rust
// ❌ 查询任意实体的位置
world.try_query::<&Position>().unwrap().iter(world).next()
```

这将返回迭代器中的第一个实体——可能是怪物、物品或楼梯本身，不保证是玩家。如果怪物恰好先被遍历到，下楼判定读取的是怪物的位置。

**教训：** 任何"判断玩家状态"的 ECS 查询必须显式加入 `&Player` 组件约束。使用 `&Position` 不加 `Player` 过滤器是一个类型系统无法捕获的逻辑错误——它编译通过、运行不 panic，只是行为随机。

**正确写法：**
```rust
world.try_query::<(&Player, &Position)>().unwrap()
    .iter(world).next().map(|(_, p)| *p)
```

**原编号：** `LECS18`（迁移前）

---

### LECS19 — bevy_ecs 的 Mut<T> 存活期间不得再对同一资源调用 resource_mut

**问题背景：** `open_throw_aim` 中 `world.resource_mut::<ThrowPreview>()` 返回 `Mut<ThrowPreview>`（`tp`），然后调用 `update_throw_path(world)`——后者内部再次调用 `world.resource_mut::<ThrowPreview>()`。`Mut<T>` 和第二次 `resource_mut` 的操作都在同一资源上。bevy_ecs 内部使用 `UnsafeCell` 实现运行时借用检测，检测到二次可变访问后直接 panic。

**错误做法：**
```rust
// ❌ tp 存活时调用了同一资源的 resource_mut
let mut tp = world.resource_mut::<ThrowPreview>();
tp.active = true;
update_throw_path(world);  // 内部 resource_mut::<ThrowPreview>() → panic
```

**正确做法：** 任何 `resource_mut::<T>()` 返回的 `Mut<T>` 应在再次调用 `resource_mut::<T>()` 前 drop：

```rust
// ✅ 提前结束作用域，drop 后再调 update_throw_path
{
    let mut tp = world.resource_mut::<ThrowPreview>();
    tp.active = true;
}
update_throw_path(world);  // 安全：前一个 Mut<T> 已 drop
```

**为什么更好：** bevy_ecs 在编译期用 `UnsafeCell` 绕过了 Rust 的借用检查器，但运行时有自己的检测机制。`Muts` 存活期内对同一资源的任何二次可变访问（包括通过函数调用间接访问）都会触发 panic。这个 panic 的信息可能不明显（表现为"系统崩溃"而非清晰的错误消息），因为 panic hook 只写文件。

**相同约束适用：**
- `world.resource_mut::<T>()` — 同一资源
- `world.get_mut::<T>(entity)` — **同一组件类型**（不同实体可以）
- `world.query::<Q>().iter_mut(world)` — 同一 Query 不会冲突，但 query + get_mut 对同一组件类型会冲突

**参见 ISSUES.md #I46**

**原编号：** `LECS19`（迁移前）

---

### LECS20 — 规则验证必须存在于执行入口，显示层的"有效性"只是提示不算规则

**问题背景：** 投掷的射程/视线检查只在 `update_throw_path` 计算 `valid_target`，渲染层据此画红/蓝轨迹（Gm9 的"目标不可选中"）。但 Enter 入队和 `execute_throw` 都不检查——玩家瞄准墙后/超射程目标按 Enter，石子穿墙命中（I59）。

**错误做法：** 认为"显示层阻止了无效操作"——红色轨迹只是提示，玩家仍可确认。显示层没有任何阻止能力。

**正确做法：** 规则验证写入执行入口（入队时 + 执行时双保险）：

```rust
// 入队前：UI 层检查（体验）
if !tp.valid_target { 推送提示; return; }
// 执行时：规则层检查（兜底，防状态变化/绕过 UI）
if let Err(reason) = validate_throw(world, attacker, tx, ty) { 取消; return; }
```

**为什么更好：** 显示层可能被绕过（直接入队、脚本调用、未来新 UI），执行层是唯一不可绕过的关卡。凡是"玩家能否做 X"的规则，验证必须在执行函数入口重复一次——显示层的检查只负责"提前告知"，不负责"阻止"。

**参见 ISSUES.md #I59**

---

**原编号：** `LECS20`（迁移前）

---

### LECS21 — Bevy 查询「静默返回空」：空结果既可能是没匹配，也可能是类型没注册

**问题背景：** 为迁移 `Wait` 写「新旧执行器语义对照」测试时，在 `test_world()` 里 spawn 一个 `(Active, Wait)` 实体后调用旧 `execute_wait_system(&mut World)`，断言它回到 `Idle`——失败了。可旧执行器的单元测试（Phase A 的 `av_gate_only_executes_ready_actions`）明明是通过的。

**根因（两层，都很隐蔽）：**

1. Bevy 的 `query_filtered` / `Query` 按 **archetype** 精确匹配：实体必须带上查询要求的**全部**组件。旧执行器查询的是 `(With<Active>, With<Wait>, With<Ready>)`，`Ready` 是 tick 系统后加的——只 spawn `(Active, Wait)` 永远匹配不上。
2. `test_world()` 只注册资源与事件；`Active` / `Wait` / `Ready` 这些**组件类型**在「某个系统第一次真正碰过它们」之前并未向 World 注册。而对从未注册的类型，Bevy 的查询**静默返回空**，不报错、不 panic。

于是「跑了旧代码，实体没变」有两种完全不同的解释：行为确实不同，或**根本没查到**。用它当对照基线，会得出错误结论。

**错误做法：** 把「查询没匹配」当成「行为差异」，据此改生产代码或改断言——最坏的情况是把真实的语义差异掩盖成"测试写错了"。

**正确做法：**

- 跑对照实验前，先确认**类型已注册 + archetype 齐备**：要么用已注册齐的测试世界（如本项目的 `poc_world()`），要么让实体带上查询要求的全部组件，显式写出来：

```rust
// ✅ 显式给出查询要求的全部组件，并保留一个"不应被误伤"的对照实体
let actor = world.spawn((Active, Wait, Ready, ActionTimer { remaining_av: 0.0 })).id();
legacy_execute_wait_system(&mut world);
assert!(world.get::<Idle>(actor).is_some());
assert!(world.get::<Idle>(bystander).is_some());   // 反面对照：没有 Wait 的实体不受影响
```

- 把「空查询不报错」这条行为本身钉成一条测试（本项目为
  `unregistered_component_query_returns_empty_without_panic`），下次有人怀疑时不用重新推。

**为什么更好：** ECS 查询的空结果是一个**没有信号**的失败——没有 panic、没有日志、没有类型错误。凡是拿「查询结果」当行为证据的测试或诊断，都必须先排除"类型/archetype 不齐"这一解释，否则会把测量误差当成被测对象的性质。这条与 LSYN9（测试需要独立的 World）互补：独立 World 解决了"互相污染"，但没解决"组件类型没注册"。

**参见 REFACTOR.md §11.3 Phase C（C1 迁移记录）**

**原编号：** `LECS21`（迁移前）

---

### LECS22 — 扩展点用"加第 N 个要改几处"度量，而不是用"是不是数据"判断

**问题背景：** 讨论新增行动/技能/怪物/地形时的扩展成本，容易把问题归结为"该用数据还是该用代码"。这个二分**没有预测力**——静态数据表和中央分派 enum 可以一样难扩展。

具体证据：本项目曾用静态表形态的 `EntityClass` 承载"未来可能有 `Item`/`Buff`/`Projectile`/`Field`"，它**是数据驱动的，照样死了**：`rebuild_occupancy_system` 检查 `EntityClass::Item` 而全库从未插入 `Item`，判断恒假，最终删除。反过来 `MonsterKindId` + `&'static [MonsterTemplate]` 也是数据，却是好扩展点。**两者都是"数据"，差别不在数据，在"分离点在哪里"。**

**错误做法：** 用"这是数据 / 这是枚举"来评估扩展性；或认为"上了 ECS 就没有中央点了"。

**正确做法：** 用一条可数的判据：

> **加第 N 个同类东西时，要改几处代码？**
> **0 处（只加数据行）** → 扩展点做好了
> **1 处（一个分组列表加一行）** → 可接受
> **N 处（中央 match / 中央列表逐个登记）** → 这是下一个 `ActionKind`

本项目的实测刻度：`monster_template(kind)` 加一只怪约 **1** 处（好）；`Tile` 加一个变体要改 **5** 处（`glyph`/`walkable`/`blocking`/`From<u8>`/`Into<u8>`，差）；`monster_spawn_weight` 要改 **3** 处三层 match（差）。

**为什么更好：** 这条判据不依赖审美，**能在同一份代码里横向比较**，并且能把"感觉不舒服"翻译成一个数字。它同时给出正确的行动：**不是把 enum 换成表，而是让"新增"这个动作不再需要改中央**。另外它揭示了 ECS 的边界——ECS 消除了**类型分发的中央点**（`With<A>` 取代 match），但**消除不了"顺序协调的中央点"**（schedule 里系统必须按序登记）和**全局策略的中央点**（仲裁规则）。把这三类分开，才知道哪些该修、哪些只能显式化。

**配套纪律（缺了它，数据表就是净亏）：** 数据表把"漏加一项"的错误从**编译期穷举检查**推到了**运行期静默**。所以每张数据表必须配一个**枚举全部行并断言有效**的测试（本项目范例：`every_monster_template_has_usable_speeds`、`stats_expose_template_speeds`），每个新组件配一个"**真的能被创建**"的测试（`With<A>` 写了但没人 spawn `A`，编译器不会告诉你）。

**参见 ISSUES.md ECS31 / ECS32 / ECS35 | DESIGN.md DsnE13 | REFACTOR.md §11.3 Phase H**

---
