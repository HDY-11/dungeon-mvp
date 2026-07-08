> **⚠️ 修改前必须阅读或回忆 [RULE.md](RULE.md)——它定义了本文档的维护规则和更新时机。**

# LESSONS.md — 从已修复问题中抽象的实现教训

本文档记录项目开发过程中从修复的问题中抽象出的**通用教训**。
这些教训适用于本项目及类似 Rust ECS 终端游戏项目。

---

## 一、Rust 语言层面

### L1 — match arm 是互斥的，没有 fallthrough

当 match arm 的 pattern 匹配了输入，后面的 arm 永远不会执行。

```rust
// ❌ 错误：KeyCode::Char('e') 永远收不到
KeyCode::Char(ch) if ch.is_ascii_lowercase() => { /* 热键 */ }
KeyCode::Char('e') => { /* 装备 — 不可达！ */ }
```

**教训：** 不要依赖 match arm 顺序 + if 内部跳过来实现"有条件的匹配"。

**解决方案：** 用 `Page` 枚举表示页面状态，`match (&page, key.code)` 二元组直接分派。

### L2 — 区分 `query()` 和 `try_query()`

`World::query()` 要 `&mut self` 不是因为查询执行需要写，而是因为内部做了**组件懒注册**。`World::try_query()` 只要 `&self`。

项目中所有组件在 `setup_world` 时已注册，可安全用 `try_query().unwrap()` 替代 `query()`。

### L3 — resource_mut() 返回的 Mut<T> 借用了 &mut World

```rust
// ❌ 错误：临时 &mut World 过早 drop
let memory = world.resource_mut::<MapMemory>();
// ✅ 正确
let mut world = world; // 绑定延长生命周期
let memory = world.resource_mut::<MapMemory>();
```

### L4 — bevy_ecs 0.16 Bundle 上限 16 个组件

超过需用 `cmd.insert()` 链式。

### L5 — `wrapping_add_signed` 在 usize 为 0 时回绕

`0usize.wrapping_add_signed(-1) = usize::MAX`。用于边界判断时需额外检查范围。

### L37 — `sort_by` 比较器必须满足全序契约，不能混入随机数

`arbitration_system` 中按 action priority 降序排序，同优先级用 `random_range` 做 tiebreaker。但 `sort_by` 要求比较器对同一对元素始终返回一致结果——即全序（total order）：`a < b` 和 `b < a` 不能同时成立。随机数每次生成不同值，破坏了这一契约。

标准库在 debug 和 release 模式下都可能检测到不一致并 panic。本项目中该 panic 在第 3 层固定触发（前两层的数据分布未触发边界条件）。

**做法：** 如果同优先级的排序顺序无关紧要（如仲裁器中跨实体顺序无意义），直接用 `sort_by(|a, b| a.cmp(b))` 即可——稳定排序保留插入顺序，不需要 tiebreaker。

**教训：** 混入随机数的比较器看似"公平"，实际是未定义行为。`sort_by` 不接受随机比较器。如果确实需要随机顺序，应先 shuffle 再 sort。

---

## 二、架构设计

### L6 — 移除全局状态，改为参数传递

全局 `OnceLock<RwLock<World>>` 引发两种死锁模式：
- `advance_action_queue` 持锁调用 `execute_*`
- `render_ui` 持锁调用子函数

**教训：** Rust 的借用检查器在编译期保证不会同时持有 `&mut` 引用——这比任何运行时锁策略都更强。函数签名 `fn foo(world: &World)` 和 `fn bar(world: &mut World)` 明确表达了读写意图，不需要文档约束。

### L7 — 四层 crate 拆分按关注点变化速度分层

```
core ← action ← world
  ↕
render（只依赖 core）
```

| 层 | 变化原因 |
|------|---------|
| core | 很少改动（数据类型、公式） |
| action | 添加新行动/技能时需要改动 |
| world | 添加新地图特征/怪物行为时 |
| render | TUI 库升级/换框架时需要改动 |

**教训：** 动渲染代码时不应有改坏战斗公式的风险。渲染 crate 直接从 ECS World 查询组件。

### L8 — 渲染层不应暴露游戏逻辑信息

**已修复（I11）：** `renderables` 遍历中 `else if explored[ey][ex]` 分支在已探索暗处画出了实体的实时位置，给玩家提供 X 射线透视。

**教训：** 渲染层只应画出玩家"应该看到"的信息。暗处实体位置依赖 `VisibleMemory`（记忆的上次位置），而非 `renderables`（实时位置）。任何在已探索暗处绘制实体当前位置的行为都是给玩家作弊。

### L9 — 订阅模式优于轮询（输入线程）

独立输入线程 + mpsc channel 比主线程直接 `event::read()` 阻塞更适合游戏循环：
- 主循环保持非阻塞
- 输入线程独立限流（16ms 轮询）
- 50ms 去重过滤 OS key-repeat
- 模态对话时用 AtomicBool 暂停输入线程，主线程直读 stdin

### L10 — 行动系统：AV 统一值 + 全局单队列

旧设计有"冷却计时器"和"队列推进"两个独立时间维度 → 需要同步、量纲对齐 → 移除冷却，全部由 AV 统一管理。

```rust
AV = reaction_time + duration        // 单一值入队
av_remaining -= next_event_distance() // 同步推进
av_remaining ≤ 0 → pop_ready()       // 执行
```

---

## 三、ECS 使用

### L11 — 组件式行动授权优于行为树/状态机

行动能力由组件赋予（`CanMove`/`CanChase`/`CanFlee`/`CanWander`/`CanWait`），组件携带条件，系统批量检查，仲裁解决冲突。

**收益：** 并行检查、易扩展（加组件=加行为）、不需要修改决策流程。

### L12 — Intent 缓冲区模式用于并行决策

三个决策 system 各自写入独立缓冲区（`ChaseIntents`/`FleeIntents`/`WanderIntents`），仲裁 system 串行合并。

**收益：** 无并发写冲突、数据流显式可追踪。

### L13 — 状态变更后必须立即刷新依赖

- 死亡后跳过 `advance_and_settle`（否则死后仍推进）
- 下楼后跑 `fov_system` + `update_map_memory` + `update_visible_memory`（否则新楼层视野为空）

### L14 — despawn→spawn 要维护所有组件的传递

`descend()` despawn 所有实体后重生玩家，重生时不能漏组件。

---

## 四、游戏逻辑

### L15 — buff 必须参与伤害/防御计算

`effective_attack` 和 `effective_defense` 必须包含 `stats.attack/defense + equipment_bonus + buffs.berserk_atk/shield_def`。

### L16 — 战斗公式的暴击/掉落必须走统一 RNG

`LootTable::roll()` 和 `execute_attack` 的暴击判定应接受 `&mut impl Rng` 参数，从 `GameRng` 取随机值，而非调用 `rand::random::<f32>()`（系统熵）。否则游戏行为不可复现。

### L17 — 逃跑行为需要滞回区间

进入条件（`CanFlee::condition`）和退出条件（`check_condition`）不应相同。否则一旦逃跑永不回头。建议：

```rust
进入：hp_ratio < 0.25
退出：hp_ratio < 0.30   // 多 5% 的宽容窗口
```

### L18 — 游荡/随机行为使用独立随机源

怪物游荡方向用 `rand::random::<u8>() % 8`（原设计用 `(FloorNumber + monster_count) % 8` 确定性计算）→ 所有怪物朝同一方向游荡，不合理。

---

## 五、物品/装备

### L19 — 装备操作需要原子语义

装备卸载应先预检背包容量（`Inventory::can_add()`），有空间再执行。避免部分添加后无法完全回滚。

### L20 — 未完成的游戏机制不应留在代码中

`PendingLevelUp` 累积属性点但没有任何消费路径。此类"悬空机制"应删除而非留下代码陷阱。

---

## 六、调试与测试

### L21 — 测试需要独立的 World

不再依赖全局 World。每个测试函数创建自己的 World，可并行测试（摆脱 `--test-threads=1`）。

### L22 — 场景测试（scenario_test）比单元测试更适合验证游戏循环

模拟按键→截取渲染帧→验证游戏状态。比孤立测试 action queue 推进更能发现集成问题。

---

## 七、事件帧模式（计划中，D5）

### L23 — 事件粒度的推进

`next_event_distance()` → `advance(dist)` → `pop_ready()` 的设计天然支持分批推进。可以单步执行到下一个事件点，让玩家在每个事件后做决策。

**不是所有行动都需要玩家介入：** 每个事件步执行一个条目（而非一个实体的所有条目），玩家在行动执行前可以切换方向。

---

## 八、地图与地形

### L24 — 楼梯需要可达性保证

地图生成后 BFS 检查出生点到楼梯是否有路径，若无可使用加权醉汉游走（70% 指向楼梯方向，30% 随机）挖掘通道。

### L25 — 环境修饰不应覆盖关键位置

水体/钟乳石生成时应远离房间中心（`is_away_from_rooms`），但保护距离不宜过大，否则水体偏少。

---

## 九、存档兼容性

### L26 — 新字段用 `#[serde(default)]` 兼容旧存档

每次新增字段时，如果存档结构发生变化，用 `#[serde(default)]` 保证旧存档可以反序列化。

### L27 — enum 的序列化合约由类型自身管理，而非调用方

用 `tile as u8` 将 enum 转为 u8 序列化依赖编译器分配的隐式判别值，且 restore 端需要重复 match 逻辑。改由类型实现自定义 `Serialize`/`Deserialize`：

```rust
impl serde::Serialize for Tile {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u8(match self {
            Tile::Wall => 0,
            Tile::Floor => 1,
            // ...
        })
    }
}
```

**收益：**
- 序列化合约与类型定义共处一处，不会 drift
- 调用方（capture/restore）只需 push/pull Tile，不需知道内部编码
- `Vec<Tile>` 与 `Vec<u8>` 二进制格式一致，无需迁移旧存档
- 新增变体时只需改这一处，编译器会提醒 match 未覆盖

---

## 十、实体放置与边缘情况

### L28 — 列表选择算法须处理单元素退化

当从列表中选取"离某点最远/最近"的元素时，单元素列表会退化为自身引用：

```rust
// rooms 只有一个元素时，max_by_key 返回 rooms[0]
m.rooms.iter()
    .map(|r| (r.center(), dist(r.center(), sp)))
    .max_by_key(|(_, d)| *d)
    .map(|(p, _)| p)
    .unwrap_or(m.rooms[0].center())
```

此时楼梯位置 = 玩家出生位置 = `rooms[0].center()`，导致重合。

**教训：** 任何"从 N 个候选中挑选与基准最远/最近"的选择逻辑，都必须显式处理 N=1 的退化情况。不要依赖 `max_by_key` / `min_by_key` 在 N=1 时的语义来帮你"自然避开"。

### L29 — 实体放置应始终排除关键位置

`generate_monster_population` 使用噪声密度 + 元胞扩散覆盖所有 walkable 格，没有排除楼梯/出生点/物品位置。这导致：
- 怪物站在楼梯格上 → 阻止玩家走回楼梯
- 怪物站在物品格上 → 将玩家的"拾取意图"变成了"攻击意图"

**教训：** 任何随机/噪声驱动的实体放置函数都应接受一个排除位置集合参数。怪物、陷阱、装饰物等不应生成在玩家必须交互的位置（楼梯、传送点、关键物品）。

### L30 — `.skip(1)` 与单元素集合产生空结果

```rust
// rooms.len() == 1 → room_centers 为空
let room_centers = rooms.iter().skip(1).map(|r| r.center()).collect();
let item_count = room_centers.len().min(8);  // 0
```

`.skip(1)` 在集合大小为 1 时返回空迭代器，是一个静默的"零结果"。这种错误不会触发 panic，只是什么都不生成。

**教训：** 任何"跳过第一个"的过滤操作，如果后续依赖非空结果，应当显式检查集合大小并为 N=1 提供 fallback。`.skip(N)` 返回空迭代器不是错误，但使用它的代码往往假设至少有 N+1 个元素。

---

## 十一、查询与状态判断

### L31 — ECS 查询必须包含最具体的类型约束

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

### L32 — 廉价操作的重复调用仍应消除，不是因为它贵，而是因为它混乱

`advance_and_settle_parallel` 中 `rebuild_occupancy` 在 `advance_until_player_acted`（内部每 action 后调用）和调度器运行后被重复调用。每趟 4800 格重建仅 <1μs，看似无害。

但重复调用制造了一种假象：似乎两处都"负责"维护碰撞图。当未来有人修改一处而忘记另一处时，这个重复就会从"无害冗余"变成"隐蔽 bug"。

**教训：** 消除重复操作的理由不是性能，而是职责清晰。每个副作用应该只有一个责任点。即使开销可忽略，重复也是债务。

### L33 — 多个 bug 共享根因时，应一次性修复并提取共享函数

G9（玩家与楼梯重合）、G10（怪物阻挡关键位置）、I16（单房间无物品）三个问题表面上不相关，但追溯到根因都是 **房间数为 1 时的退化行为**。各自的"修复方向"都指向 `init.rs` 中 `setup_world` 和 `descend` 的副本。

如果将三者分开修复，就得在两地各改三遍——6 次修改，遗漏风险高。一次性提取 `spawn_monsters`、`place_ground_items`、`pick_stair_pos` 三个共享函数，将修复逻辑写进函数内部，两地各调一次即可。

**教训：** 当多个 bug 的修复点落在同一区域的重复代码上时，**先提取共享函数，再修复**——而不是在副本上各修各的。这既消除了重复，又确保了修复对两个入口同时生效。

### L34 — `.expect()` 比 `.unwrap()` 更有信息量，且零成本

将 25 处 `try_query().unwrap()` 替换为 `try_query().expect("Player+Position registered at init")` 是一个纯粹的机械变换——不改变一行行为逻辑，不增加一条指令。但崩溃时前者输出 `called 'Option::unwrap()' on a None value`，后者输出 `"Player+Position registered at init"`。

**教训：** 任何时候你确信某个 unwrap 不会失败，都应该用 expect 记录你的理由。这个理由字符串在未来代码演化中比任何注释都可靠——它出现在崩溃堆栈中，而注释不会。

### L35 — 暂缓的问题应当记录明确的触发条件，而非"以后再说"

A9（ViewData 重构）和 D5（事件帧模式）都在评估后判定"现在不做"。如果不记录触发条件，几个月后有人翻到它们时无法判断应该做还是继续 defer。

**做法：** 在 ISSUES.md 已修复区标记 `Deferred`，正文写明具体的数字或行为条件：

```
触发条件：render 中 try_query 模式超过 15 种，或同一组件重组导致 render 连续改两次
触发条件：出现足够复杂的战斗逻辑（可打断吟唱、范围预警、状态倒计时）
```

**教训：** 任何 deferred 的问题都应该有一个**可验证的触发条件**，而不是"等我们做完了X再考虑"。前者是自动驾驶，后者需要人工判断。

### L36 — 修改 RULE.md 前必须重新征求明确同意，不能沿用上一次的授权

用户此前说"我允许你这一次可以修改 RULE"，这是针对 PROTOCOLS 拆分那次重构的**一次性授权**。后续当对话推进到"LESSONS 的读者定位需要修正"时，AI 认为"用户之前允许过"就直接改动了 RULE.md，没有重新征求同意。

但 RULE.md 的修改约束是：**每次修改前都必须确认并获得批准。** 上一次的授权不自动延续到下一次。

**教训：** 只要修改目标是 RULE.md，无论改动多小、无论之前是否被授权过，**都必须在改动前明确问用户"我可以改吗"**。RULE.md 是宪法，宪法没有"上次批准了这次就不用问"这回事。

---

## 十二、项目管理流程

### L35 — 暂缓的问题应当记录明确的触发条件，而非"以后再说"

A9（ViewData 重构）和 D5（事件帧模式）都在评估后判定"现在不做"。如果不记录触发条件，几个月后有人翻到它们时无法判断应该做还是继续 defer。

**做法：** 在 ISSUES.md 已修复区标记 `Deferred`，正文写明具体的数字或行为条件：

```
触发条件：render 中 try_query 模式超过 15 种，或同一组件重组导致 render 连续改两次
触发条件：出现足够复杂的战斗逻辑（可打断吟唱、范围预警、状态倒计时）
```

**教训：** 任何 deferred 的问题都应该有一个**可验证的触发条件**，而不是"等我们做完了X再考虑"。前者是自动驾驶，后者需要人工判断。

### L36 — 修改 RULE.md 前必须重新征求明确同意，不能沿用上一次的授权

用户此前说"我允许你这一次可以修改 RULE"，这是针对 PROTOCOLS 拆分那次重构的**一次性授权**。后续当对话推进到"LESSONS 的读者定位需要修正"时，AI 认为"用户之前允许过"就直接改动了 RULE.md，没有重新征求同意。

但 RULE.md 的修改约束是：**每次修改前都必须确认并获得批准。** 上一次的授权不自动延续到下一次。

**教训：** 只要修改目标是 RULE.md，无论改动多小、无论之前是否被授权过，**都必须在改动前明确问用户"我可以改吗"**。RULE.md 是宪法，宪法没有"上次批准了这次就不用问"这回事。

`arbitration_system` 中按 action priority 降序排序，同优先级用 `random_range` 做 tiebreaker。但 `sort_by` 要求比较器对同一对元素始终返回一致结果——即全序（total order）：`a < b` 和 `b < a` 不能同时成立。随机数每次生成不同值，破坏了这一契约。

标准库在 debug 和 release 模式下都可能检测到不一致并 panic。本项目中该 panic 在第 3 层固定触发（前两层的数据分布未触发边界条件）。

**做法：** 如果同优先级的排序顺序无关紧要（如仲裁器中跨实体顺序无意义），直接用 `sort_by(|a, b| a.cmp(b))` 即可——稳定排序保留插入顺序，不需要 tiebreaker。

**教训：** 混入随机数的比较器看似"公平"，实际是未定义行为。`sort_by` 不接受随机比较器。如果确实需要随机顺序，应先 shuffle 再 sort。
