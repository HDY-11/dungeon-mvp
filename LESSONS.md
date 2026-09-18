> **⚠️ 修改前必须阅读或回忆 [RULE.md](RULE.md)——它定义了本文档的维护规则和更新时机。**

# 经验教训 —— 根目录（跨 crate）

**归属范围：** 跨 crate 可复用的原则：Rust 语言、工具链与流程、测试方法论、通用架构取舍。

**编号：** `LSYN1`、`LSYN2`… 每个 crate 独立编号。
判据：教训的读者是 AI。**在任何 crate 都适用**的原则留根目录；只在某个 crate 的代码里有落点的归该 crate。


### LSYN1 — match arm 是互斥的，没有 fallthrough

当 match arm 的 pattern 匹配了输入，后面的 arm 永远不会执行。

```rust
// ❌ 错误：KeyCode::Char('e') 永远收不到
KeyCode::Char(ch) if ch.is_ascii_lowercase() => { /* 热键 */ }
KeyCode::Char('e') => { /* 装备 — 不可达！ */ }
```

**教训：** 不要依赖 match arm 顺序 + if 内部跳过来实现"有条件的匹配"。

**解决方案：** 用 `Page` 枚举表示页面状态，`match (&page, key.code)` 二元组直接分派。

**原编号：** `LSYN1`（迁移前）

---

### LSYN2 — `wrapping_add_signed` 在 usize 为 0 时回绕

`0usize.wrapping_add_signed(-1) = usize::MAX`。用于边界判断时需额外检查范围。

**原编号：** `LSYN2`（迁移前）

---

### LSYN3 — `sort_by` 比较器必须满足全序契约，不能混入随机数

`arbitration_system` 中按 action priority 降序排序，同优先级用 `random_range` 做 tiebreaker。但 `sort_by` 要求比较器对同一对元素始终返回一致结果——即全序（total order）：`a < b` 和 `b < a` 不能同时成立。随机数每次生成不同值，破坏了这一契约。

标准库在 debug 和 release 模式下都可能检测到不一致并 panic。本项目中该 panic 在第 3 层固定触发（前两层的数据分布未触发边界条件）。

**做法：** 如果同优先级的排序顺序无关紧要（如仲裁器中跨实体顺序无意义），直接用 `sort_by(|a, b| a.cmp(b))` 即可——稳定排序保留插入顺序，不需要 tiebreaker。

**教训：** 混入随机数的比较器看似"公平"，实际是未定义行为。`sort_by` 不接受随机比较器。如果确实需要随机顺序，应先 shuffle 再 sort。

**原编号：** `LSYN3`（迁移前）

---

### LSYN4 — 迁移方法后必须 grep 原 impl 块全部公共方法，交叉核验是否遗漏

**问题背景：** A4 将 Map 的环境修饰方法移到 `map_gen.rs` 后，A4L 发现 `collect_walkable_regions` 和 `detect_cave_regions` 未删除。A4La 又发现 `generate_water`、`is_away_from_rooms`、`count_walkable_neighbors` 仍未删除。同模式第三次发生。

**参见 ISSUES.md #A4La**

**错误做法：** 只删除记忆中"我移动了哪些方法"——人脑记忆不可靠。

**正确做法：** 移动方法后，grep `impl Map` 块中**所有** `pub fn` 的签名，与目标位置交叉核对。一个方法在旧位置有 pub fn 签名、在新位置也有、且旧位置零调用 = 遗漏。

```rust
// 第一步：列出 Map impl 中所有 pub fn
grep -n "pub fn" dungeon-core/src/lib.rs
// 第二步：检查每个方法在新位置是否有对应
grep -n "pub fn" dungeon-core/src/map_gen.rs
// 第三步：检查旧位置中列出的方法是否有调用方
// 零调用的 → 死代码
```

**为什么更好：** grep 不会遗忘。过程化清单比「我记得移了 X、Y、Z」可靠。

---

## 二、架构设计

**原编号：** `LSYN4`（迁移前）

---

### LSYN5 — 移除全局状态，改为参数传递

全局 `OnceLock<RwLock<World>>` 引发两种死锁模式：
- `advance_action_queue` 持锁调用 `execute_*`
- `render_ui` 持锁调用子函数

**教训：** Rust 的借用检查器在编译期保证不会同时持有 `&mut` 引用——这比任何运行时锁策略都更强。函数签名 `fn foo(world: &World)` 和 `fn bar(world: &mut World)` 明确表达了读写意图，不需要文档约束。

**原编号：** `LSYN5`（迁移前）

---

### LSYN6 — 四层 crate 拆分按关注点变化速度分层

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

**原编号：** `LSYN6`（迁移前）

---

### LSYN7 — 装备操作需要原子语义

装备卸载应先预检背包容量（`Inventory::can_add()`），有空间再执行。避免部分添加后无法完全回滚。

**原编号：** `LSYN7`（迁移前）

---

### LSYN8 — 未完成的游戏机制不应留在代码中

`PendingLevelUp` 累积属性点但没有任何消费路径。此类"悬空机制"应删除而非留下代码陷阱。

---

## 六、调试与测试

**原编号：** `LSYN8`（迁移前）

---

### LSYN9 — 测试需要独立的 World

不再依赖全局 World。每个测试函数创建自己的 World，可并行测试（摆脱 `--test-threads=1`）。

**原编号：** `LSYN9`（迁移前）

---

### LSYN10 — 场景测试（scenario_test）比单元测试更适合验证游戏循环

模拟按键→截取渲染帧→验证游戏状态。比孤立测试 action queue 推进更能发现集成问题。

---

## 七、事件帧模式（计划中，D5）

**原编号：** `LSYN10`（迁移前）

---

### LSYN11 — 新字段用 `#[serde(default)]` 兼容旧存档

每次新增字段时，如果存档结构发生变化，用 `#[serde(default)]` 保证旧存档可以反序列化。

**原编号：** `LSYN11`（迁移前）

---

### LSYN12 — enum 的序列化合约由类型自身管理，而非调用方

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

**原编号：** `LSYN12`（迁移前）

---

### LSYN13 — 廉价操作的重复调用仍应消除，不是因为它贵，而是因为它混乱

`advance_and_settle_parallel` 中 `rebuild_occupancy` 在 `advance_until_player_acted`（内部每 action 后调用）和调度器运行后被重复调用。每趟 4800 格重建仅 <1μs，看似无害。

但重复调用制造了一种假象：似乎两处都"负责"维护碰撞图。当未来有人修改一处而忘记另一处时，这个重复就会从"无害冗余"变成"隐蔽 bug"。

**教训：** 消除重复操作的理由不是性能，而是职责清晰。每个副作用应该只有一个责任点。即使开销可忽略，重复也是债务。

**原编号：** `LSYN13`（迁移前）

---

### LSYN14 — 多个 bug 共享根因时，应一次性修复并提取共享函数

G9（玩家与楼梯重合）、G10（怪物阻挡关键位置）、I16（单房间无物品）三个问题表面上不相关，但追溯到根因都是 **房间数为 1 时的退化行为**。各自的"修复方向"都指向 `init.rs` 中 `setup_world` 和 `descend` 的副本。

如果将三者分开修复，就得在两地各改三遍——6 次修改，遗漏风险高。一次性提取 `spawn_monsters`、`place_ground_items`、`pick_stair_pos` 三个共享函数，将修复逻辑写进函数内部，两地各调一次即可。

**教训：** 当多个 bug 的修复点落在同一区域的重复代码上时，**先提取共享函数，再修复**——而不是在副本上各修各的。这既消除了重复，又确保了修复对两个入口同时生效。

**原编号：** `LSYN14`（迁移前）

---

### LSYN15 — `.expect()` 比 `.unwrap()` 更有信息量，且零成本

将 25 处 `try_query().unwrap()` 替换为 `try_query().expect("Player+Position registered at init")` 是一个纯粹的机械变换——不改变一行行为逻辑，不增加一条指令。但崩溃时前者输出 `called 'Option::unwrap()' on a None value`，后者输出 `"Player+Position registered at init"`。

**教训：** 任何时候你确信某个 unwrap 不会失败，都应该用 expect 记录你的理由。这个理由字符串在未来代码演化中比任何注释都可靠——它出现在崩溃堆栈中，而注释不会。

**原编号：** `LSYN15`（迁移前）

---

### LSYN16 — 暂缓的问题应当记录明确的触发条件，而非"以后再说"

A9（ViewData 重构）和 D5（事件帧模式）都在评估后判定"现在不做"。如果不记录触发条件，几个月后有人翻到它们时无法判断应该做还是继续 defer。

**做法：** 在 ISSUES.md 已修复区标记 `Deferred`，正文写明具体的数字或行为条件：

```
触发条件：render 中 try_query 模式超过 15 种，或同一组件重组导致 render 连续改两次
触发条件：出现足够复杂的战斗逻辑（可打断吟唱、范围预警、状态倒计时）
```

**教训：** 任何 deferred 的问题都应该有一个**可验证的触发条件**，而不是"等我们做完了X再考虑"。前者是自动驾驶，后者需要人工判断。

**原编号：** `LSYN16`（迁移前）

---

### LSYN17 — 修改 RULE.md 前必须重新征求明确同意，不能沿用上一次的授权

用户此前说"我允许你这一次可以修改 RULE"，这是针对 PROTOCOLS 拆分那次重构的**一次性授权**。后续当对话推进到"LESSONS 的读者定位需要修正"时，AI 认为"用户之前允许过"就直接改动了 RULE.md，没有重新征求同意。

但 RULE.md 的修改约束是：**每次修改前都必须确认并获得批准。** 上一次的授权不自动延续到下一次。

**教训：** 只要修改目标是 RULE.md，无论改动多小、无论之前是否被授权过，**都必须在改动前明确问用户"我可以改吗"**。RULE.md 是宪法，宪法没有"上次批准了这次就不用问"这回事。

---

## 十二、项目管理流程

**原编号：** `LSYN17`（迁移前）

---

### LSYN18 — 移除废弃结构体时，必须 grep 所有 crate 中对该结构体的导入和引用

**问题背景：** D10 中移除 `Buffs` 结构体时，`components.rs` 中的 struct 定义删除后，编译通过了，但 `tests.rs` 中仍有 `Buffs` 的导入和 `Buffs::new()` 调用——因为测试代码不常被检查到。

**错误做法：** 只删除核心定义（struct + impl），依赖"编译会告诉我哪里还有引用"。

**正确做法：** 在删除前先 grep 结构体名称（大小写敏感）在所有 `.rs` 文件中的出现，列出所有导入和引用点，逐一确认每个引用应该保留还是删除：

```bash
grep -rn "Buff\b" --include="*.rs" src/ dungeon-core/ dungeon-action/ dungeon-world/ dungeon-render/
```

**为什么更好：** `Buffs` 和 `ActiveBuffs` 名称相似，grep 结果会同时包含两者。通过检查每个命中的上下文来区分"旧系统引用（删）"和"新系统引用（留）"——不做这个检查的话，测试文件等不常运行的代码会被遗漏。

**参见 ISSUES.md #D10**

**原编号：** `LSYN18`（迁移前）

---

### LSYN19 — 为 Player 添加新组件后，必须 grep descend 和 persist 两个路径确保一致

**问题背景：** D15（Skills 下楼/存档丢失）是同一个模式在本项目中的第三次发生——前两次是 D8（ActiveBuffs 下楼丢失）和 I34（ActiveBuffs 存档丢失）。每次都是"在 Player 上加了新组件 → 更新了 `setup_world`（首次创建）→ 但忘记更新 `descend`（下楼重建）和/或 `persist`（存档读档）的 query 和 restore"。I79（restore 缺 AttackName）是第四次——三路径中 setup/descend 已加、restore 又漏，且读档测试只断言数据字段、不查组件存在性，导致缺组件被 `unwrap_or` 兜底静默降级。

**参见 ISSUES.md #D15 #I79**

**错误做法：** 只更新 `setup_world` 中的组件插入，假设下楼和存档路径"会自动继承"。

```rust
// setup_world ✅ 加了新组件
cmd.insert(NewComponent::new());
// descend ❌ 忘了加
// persist.rs capture ❌ 忘了加
```

**正确做法：** 每当为 Player 添加新组件（`cmd.insert(...)` 或 `world.spawn(...)` 中的新元素），立即 grep 以下三个位置：

```bash
grep -n "descend" dungeon-world/src/init.rs      # descend 中的 query + spawn
grep -n "capture" dungeon-world/src/persist.rs   # GameSave::capture 中的 query
grep -n "restore" dungeon-world/src/persist.rs   # GameSave::restore 中的 spawn
```

确保三个路径都包含该组件。这是一个机械检查清单，不需要记忆。

**补充（I79 教训）：** 仅 grep 还不够——restore 回环测试（capture→restore）必须显式断言关键组件存在（`world.get::<T>(player).is_some()`），而不是只断言数据字段。没有存在性断言时，缺组件会被运行时 `unwrap_or` 兜底静默降级，测试照样通过。

**为什么更好：** `setup_world` 只执行一次（游戏启动），`descend` 每次下楼都执行，`restore` 每次读档都执行。三个路径不同的代码走的是"同一组件的三个不同拷贝"——这不是继承关系，是并行维护关系。任何遗漏都导致数据静默丢失。grep 在编译前就能发现缺口，断言让缺口在测试中失败，都无需等到运行后手动发现。

**原编号：** `LSYN19`（迁移前）

---

### LSYN20 — 测试若依赖「没有别的实体碰巧动过」，它依赖的其实是执行顺序

**问题背景：** 给行动系统换速度口径（把「敏捷」拆成两个速度倍率）后，一个完全没碰过视野/记忆/占用图的用例 `fov_memory_and_occupancy_update` 失败了：断言「玩家移动到相邻格 (47,26)」，实测停在 (46,26)。

**根因（一条链，逐层剥）：**

1. 用例的逻辑是「选一个相邻可走格 → `apply_player_command(Move)` → 断言位置」，而 `apply_player_command` 会**推进世界直到玩家行动做完**——期间别的实体会各自行动。
2. 玩家选中的目标格在这次运行里被一只**游荡过来的怪物**占了；玩家命令因此在挂载阶段被改判成「走向怪物＝攻击」，位置自然不变。
3. 为什么以前没踩到？因为怪物游荡每步消耗一次随机数，而**哪个怪物在哪一轮行动**取决于各自的速度。改了速度 → 轮次变了 → 随机数消耗时机变了 → 「碰巧没人走过来」不再成立。

于是：**被测代码与断言对象毫无关系**（速度 vs 视野/占用图），改动却在测试里以"随机失败"的形式爆出来。

**错误做法：**

- 看到断言失败就放宽断言（`assert!(pos == dest || pos == start)`）——把真实的行为耦合藏起来；
- 或者去改被测的游荡/随机数逻辑，让它"别走到那里"——为了测试的方便扭曲生产行为。

**正确做法：** 先判断这条用例是否**真的需要**那些会自己动的实体：

- 不需要 → 断言前先清场（按类型 despawn 掉怪物/楼梯等），让目标格在整段推进期间保持空闲；
- 确实需要 → 把交互本身作为断言对象（"怪物占位时命令改判为攻击"），而不是让它当噪声。

```rust
// ✅ 断言与"世界演化"无关的用例：先清场再断言
despawn_all::<Monster>(&mut world);
despawn_all::<Stairs>(&mut world);
run_settle_systems(&mut world);
assert_eq!(player_pos(&world), dest);
```

**为什么更好：** 这类测试**不是 flaky，是过度约束**——它顺带断言了一件没人打算断言的事（"推进期间没有实体进入该格"），而这件事只由执行顺序保证。执行顺序恰恰是重构最常改的东西：改计时、改系统顺序、改调度、增加一个会移动实体的系统，都会踩到它。**判断信号**：一条用例的断言对象（视野、占用图、伤害数值）与它失败的方式（位置不对、轮次不对）对不上时，先去找"还有谁在动"，而不是怀疑被测对象。

**推广：** 凡是「命令驱动推进直到某条件满足」的接口（本项目是 `apply_player_command`），它的副作用范围就是"整个世界演化一轮"，写测试时默认世界里的一切都在动，需要静态前提就自己把它固定住。

**参见 REFACTOR.md §11.3 Phase D（D3 迁移记录）；ISSUES.md #G35**

**原编号：** `LSYN20`（迁移前）

---
