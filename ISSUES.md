> **⚠️ 修改前必须阅读或回忆 [RULE.md](RULE.md) 的内容，了解本文档的维护规范。**

# 发现的问题记录

本文档记录当前实现中与设计意图不一致或有改进空间的问题，
在后续开发中可作参考。

问题按维度分组：**设计 / 架构 / 实现 / 游戏逻辑**，组内按严重程度降序。
优先级标记：🔴 高（影响正确性或游戏体验） / 🟡 中（维护性或功能缺口） / 🟢 低（整洁或边缘情况）

---

## ✅ 已修复

### A7 — 拆分 ops.rs 为 fov / pathfinding / ops ✅已修复

**修复前：** `ops.rs` 是万能工具袋——FOV、A\*、公式、查询、拾取、碰撞图、渲染收集等 9 个无关功能挤在同一个文件中。

**修复后：** 提取 `dungeon-core/src/fov.rs`（`calculate_visible_tiles`）和 `dungeon-core/src/pathfinding.rs`（`astar` + `AStarNode`）。ops.rs 保留剩余的紧密相关工具函数（公式、属性计算、实体查询、拾取、碰撞图、视野记忆、渲染收集）。

**统计：**
| 文件 | 行数 | 职责 |
|------|------|------|
| `fov.rs` | ~25 | 对称阴影投射视野计算 |
| `pathfinding.rs` | ~80 | A\* 8 方向寻路 |
| `ops.rs`（剩余） | ~120 | 公式/查询/记忆/碰撞/渲染 |

### A9 — 渲染层直接查询 ECS（Deferred — 条件触发时重新评估） ✅已修复

**当前评估：** 不做 ViewData 重构。理由：
- 当前 render 的 ~8 处 `try_query().expect()` 在 I17 后已有足够信息量
- 组件重命名会触发编译错误（编译期隔离足够）
- ViewData 方案会新增 ~50% 代码量并增加每帧填充开销

**触发条件：** 以下任意一条满足时重新评估：
1. render 中 `try_query` 模式超过 **15 种**（从当前 ~8 增长）
2. **同一组件重组导致 render 连续两次以上需要修改**时

### D4 — 升级满血满蓝已文档化（有意设计） ✅已修复

**修复前：** `apply_exp_system` 中升级后 HP/MP 全恢复，但 GAME.md 和 DESIGN.md 均未记录。属于"有意但未说明"的行为，新开发者看到会困惑。

**修复后：** GAME.md 升级效果中增加 `HP/MP 全恢复（设计简化，方便体验不同楼层）` 行，并注明参见 D4。

### A6 — 行动类型从 dungeon-core 移至 dungeon-action ✅已修复

**修复前：** `dungeon-core/src/action_types.rs` 包含 `ActionQueue`、`ActionKindV3`、`CanMove`/`Chase`/`Flee`等行动领域类型。它们被放在 core 中只因依赖方向限制，导致 core 被行动系统的改动拖慢。

**修复后：** 整个 `action_types.rs` 迁移到 `dungeon-action/src/types.rs`。所有引用路径更新：
- `dungeon-action` 各模块：`crate::types::*`
- `dungeon-world`：`dungeon_action::*`
- `dungeon-render`：新增依赖 `dungeon-action`
- `dungeon-core`：删除 `pub mod action_types`，测试迁至 `dungeon-action`

**删除文件：** `dungeon-core/src/action_types.rs`、`dungeon-core/src/tests.rs`

### I19 — 提取 setup_world/descend 共享函数 + 修复 G9/G10/I16 ✅已修复

**修复内容（四项在同一个重构中完成）：**

**I16 — 单房间物品为 0**：`place_ground_items` 当 `rooms.len() == 1` 时退回到 `rooms[0]` 内随机偏移放置。

**G9 — 玩家与楼梯重合**：`pick_stair_pos` 当 `rooms.len() <= 1` 时用醉汉游走从出生点走 60 步，找到 ≥15 格外的 walkable 格作为楼梯位置。

**G10 — 怪物阻挡关键位置**：`generate_monster_population` 新增 `exclude: &[(usize, usize)]` 参数，收集和随机补充阶段跳过排除坐标。`setup_world` 和 `descend` 传入 `[spawn, stairs_pos]`。

**I19 — 重复代码**：提取 `spawn_monsters`、`place_ground_items`、`pick_stair_pos` 三个共享函数，`setup_world` 和 `descend` 分别调用。消除 ~55 行重复代码。

**教训：** 三个不同的问题（重合、阻挡、缺物品）共享同一根因（单房间退化）和同一修复点（init.rs）。将其一次性解决比分开修更高效。共享函数提取应在修复的同时进行，而非先提取再修复——否则两次修改同一区域。

### I18 — `on_stairs()` 修复：过滤 Player 组件 ✅已修复

**修复前：** `try_query::<&Position>()` 查询任意实体位置，迭代顺序不确定性导致可能读到怪物/物品的位置而非玩家，使下楼判定失效。

**修复后：** 改为 `try_query::<(&Player, &Position)>()`，只查询玩家的位置。无玩家时返回 false。

**教训：** 任何"判断玩家状态"的函数都应在查询组件时显式加入 Player filter。`&Position` 可能匹配到任何实体——编译器不会警告，行为在运行时才暴露。

### I20 — 移除 `advance_and_settle_parallel` 末位重复 rebuild ✅已修复

**修复前：** `advance_until_player_acted`（内部每 action 后 rebuild）→ schedule.run → `rebuild_occupancy`（末位）。调度器不改变实体位置，末位 rebuild 冗余。

**修复后：** 删除末位 `rebuild_occupancy` 调用。碰撞图仅由 `advance_action_queue` 在每 action 后维护，职责清晰。

### D7 — EventLog 容量提升至 50 ✅已修复

**修复前：** max=10，战斗密集时关键信息 2-3 回合后被滚出屏幕。

**修复后：** max=50。

### I21 — Position 增加 `#[derive(PartialEq, Eq)]` ✅已修复

**修复前：** `Position` 无 PartialEq，测试中需逐字段比较 x 和 y。

**修复后：** 增加 `#[derive(PartialEq, Eq)]`。测试代码可直接 `assert_eq!(pos1, pos2)`。

**教训：** 值类型（所有字段都是 Copy 的简单结构体）应默认实现 PartialEq + Eq，无需等待测试需要时才加。

### D6 — GAME.md 升级描述与代码一致 ✅已修复

**修复前：** GAME.md 仍写着"获得 3 个属性点（待分配）"，但 PendingLevelUp 已在 I7 中删除。

**修复后：** 该行标记为 `~~已移除~~`，并注明参见 I7。GAME.md 的"升级效果"描述与 `apply_exp_system` 的实际行为一致。

**教训：** 代码与设计文档之间没有自动同步机制。每次删除游戏机制（如 I7）后应在 GAME.md 中搜索相关文字。ISSUES.md 的已修复列表应包含文档更新。

### I14 — 下楼时 PlayerClass 与 Skills 联动保障 ✅已修复

**修复前：** `descend()` 中 Skills 通过 `player_data.6`（`sk.list.clone()`）持有独立副本，与 `PlayerClass` 字段无编译期联动。如果将来添加职业特有技能，两个字段可能 drift。

**修复后：** Skills 改为从 `PlayerClass::skills()` 推导，与 `setup_world` 和 `restore` 一致。同步清理了不再需要的 `Skills` 组件查询和旧 Position 字段。

**教训：** 派生数据不应手动复制，应从权威源推导。`descend()` 中有三个不同路径（setup_world / restore / descend）重建玩家，它们生成 Skills 的方式应统一。

### I13 — Tile 序列化合约由类型管理 ✅已修复

**修复前：** `map_tiles` 用 `tile as u8` 保存、`if v == 0 { Wall } else { Floor }` 恢复。判别值隐式依赖编译器分配，且丢失了 ShallowWater/DeepWater/Stalactite。

**修复后：** 由 I15 的 Tile 自定义 Serde 统一解决——序列化合约归类型自身管理，调用方只需 push/pull Tile。`Vec<Tile>` 与旧版 `Vec<u8>` 二进制格式一致，无须迁移旧存档。

### I12 — 主循环 F9 读档后刷新视野记忆和碰撞图 ✅已修复

**修复前：** `process_key` 中 F9 读档后（`title_screen` 中做了但这里遗漏了）不跑 `fov_system`、`update_map_memory`、`update_visible_memory`、`rebuild_occupancy`，导致读档后第一帧黑屏/灰色空地图、怪物和碰撞图不可用。

**修复后：** F9 读档后立即执行完整的刷新链，与 `title_screen` 的读档逻辑一致。`process_key` 和 `title_screen` 之间不再有隐藏的不一致。

**教训：** 同一功能的跨入口实现（title_screen vs process_key 的 F9）应提取为公共方法，或至少确保双方逻辑一致。"一个地方修了、另一个没修"是重复代码的经典隐患。

### P8 — 测试覆盖不全 ✅已修复

**修复前：** `dungeon-action` 和 `dungeon-world` 零测试。仅 `dungeon-core` 有 6 个单元测试 + 3 个场景集成测试。

**修复后：**
| crate | 前 | 后 | 新增内容 |
|-------|-----|-----|---------|
| dungeon-core | 6 | 6 | 不变 |
| dungeon-action | 0 | 6 | 队列推进/等待/保活检查/tap-tap 方向/tap-tap 等待/攻击流程 |
| dungeon-world | 0 | 2 | 存档读档回环（Tile+Stats+Inventory+Equipment）、下楼数据保持 |
| 场景测试 | 3 | 3 | 不变 |
| **总计** | **9** | **17** | |

**教训：** 测试编写中的两个关键发现：
1. `rooms[0].center()` 可能返回非 walkable 格（矩形 bounding box 的墙点）——这是生成流程中一个隐藏的脆弱点，测试迫使它暴露
2. `world.get_mut()` 不能同时借两个不同组件——须分两步操作（取物品 → 再装备），这和主流程中 `descend` 的做法一致

### A3 — action/world tick 边界清理 ✅已修复

**修复前：** `dungeon-action/src/tick.rs` 的串行 `advance_and_settle()` 与 `dungeon-world/src/tick.rs` 的并行版功能重复。串行版从未被调用（`main.rs` 使用并行版，`scenario_test.rs` 也使用并行版），属于死代码。

**修复后：** 
- 删除 `dungeon-action/src/tick.rs` 中的 `advance_and_settle()`（action 只保留 `advance_until_player_acted`）
- 删除 `dungeon-world/src/tick.rs` 中的 `advance_and_settle_serial()`（world 只暴露并行版）
- 更新两个 crate 的 `lib.rs` 导出

职责边界：action 负责"队列推进和执行"，world 负责"编排和状态同步"。

### A4 — 环境修饰从 Map impl 提取到独立模块 ✅已修复

**修复前：** `generate_water`、`carve_expand`、`generate_stalactites`、`ensure_connectivity`、`ensure_spawn_accessible`、`ensure_connection_between`、`has_path_between`、`collect_walkable_regions`、`is_away_from_rooms`、`detect_cave_regions` 等 ~450 行代码全部在 `Map` 的 `impl` 块中。Map 职责膨胀——既要容纳 tile 数据还要管理完整的生成管线。

**修复后：** 新建 `dungeon-core/src/map_gen.rs` 模块，将上述方法全部移入作为自由函数（如 `map_gen::generate_water(map, ...)`）。Map 只保留 `generate()` 入口 + 基础查询方法（`count_tile`、`count_walkable_neighbors`、`count_neighbor_tile`、`carve_corridor`、`render`）。

**统计：**
| Map impl | 前 | 后 |
|----------|-----|-----|
| 方法数 | ~18 | ~7 |
| 行数 | ~600 | ~160 |

**教训：** 序列化合约和生成管线都应从核心类型中分离——Serialize/Deserialize 归 Tile、生成管线归 map_gen、基本查询留 Map。

### A5 — global.rs 空壳模块 ✅已修复

**修复前：** `dungeon-core/src/global.rs` 仅含两行注释（"全局 World 不再使用 OnceLock"、"线程局部 RNG 已移除"），无任何代码。`lib.rs` 仍 `pub mod global;`，全局无引用。

**修复后：** 删除 `pub mod global;` 行 + 删除 `global.rs` 文件。注释内容已在 DESIGN.md 和 LESSONS.md 中有足够记录。

**教训：** 代码移除后公共模块声明也应同步清理。P6 曾清理了 action.rs，但 global.rs 被遗忘——每次移除整个模块后都应 grep `pub mod` 确认。

### I15 — 存档 Tile 精度丢失（自定义 Serde 长期方案）✅已修复

**修复前：** `GameSave::capture` 用 `tile as u8` 保存 Tile，`restore` 用 `if v == 0 { Wall } else { Floor }` 恢复。Tile 的 5 种变体（Wall/Floor/ShallowWater/DeepWater/Stalactite）中 `1~4` 全部映射为 Floor，读档后全部水体+钟乳石消失。

**修复后：** Tile 实现自定义 `Serialize`/`Deserialize`，以 u8 判别值序列化（保持与旧版 `Vec<u8>` 相同的二进制格式），restore 直接读取 Tile 值，不再丢失精度。新增变体须在末尾追加。

**教训（L27）：** 自定义 Serde 实现使类型的序列化合约由类型本身管理，而非分散在 capture/restore 两处。同时保持与旧存档的二进制兼容——所有用 `as u8` 序列化 enum 的地方都应改用此模式，避免判别值隐式依赖编译器分配。

---

### P1 — 保活检查只检查即将执行的条目 ✅已修复

队列推进时对所有条目做批量保活检查，不满足的立即剔除，防止 Chase/Flee 在等待期间条件已失效却仍留在队列中白耗 AV。

### P3 — 并行 Schedule 每帧重建（Won't Fix） ✅已修复

每帧构建开销 <1μs，且保持测试跨 World 兼容，保留现状。

### P6 — action.rs 是空壳模块 ✅已修复

删除 action.rs，所有引用统一到 action_types。

### P9 — VisibleMemory 在视野边缘闪烁 ✅已修复

加入 VISIBLE_FORGET_DELAY=3 遗忘延迟，避免实体在视野边缘来回移动时闪烁。

### P10 — 存档缺少对 ActionQueue 的序列化 ✅已修复

按位置映射保存/恢复队列条目，Attack 条目因 Entity 引用跳过。

### D1 — 三套 RNG 并存，游戏不可复现 ✅已修复

`GameRng` 成为唯一随机源：新增便捷方法，`LootTable::roll()` 改为接受 `&mut impl Rng`，暴击/游荡/仲裁全部走 `GameRng`，删除线程局部 RNG，种子从硬编码 `0` 改为 `map_seed.wrapping_add(42)`。

### D2 — 存档/读档丢弃 Intent 缓冲区状态 ✅已修复

`GameSave` 新增 `chase_intents` / `flee_intents` / `wander_intents` 字段，capture 按位置保存，restore 通过 position→entity 重映射恢复，`#[serde(default)]` 兼容旧存档。

### D3 — crate 依赖链文档与实际不符 ✅已修复

修正 README.md 中 crate 划分树和依赖链描述，移除冗余的重复树结构。

### A1 — dungeon-core 与 dungeon-world 大量代码重复 ✅已修复

以 core 的 systems 为 canon：`calculate_visible_tiles` 移入 ops.rs，删除 core 的 api.rs（`setup_world` 移入 tests.rs），删除 world 的 systems.rs，world 的 tick 改引用 core 的 systems。

### I1 — 对角穿墙角不对称：玩家可穿，怪物不可穿 ✅已修复

移除 A\* 中的对角穿墙角检查，玩家和怪物行为一致（均可穿墙角）。

### I2 — 逃跑无退出条件（触发后永远逃跑） ✅已修复

引入滞回区间：`CanFlee::condition`（决策进入）保持 HP < 25%，`check_condition`（保活退出）改为 HP < 30%。

### I3 — 火球技能击杀无经验/无掉落，且会伤害玩家自身 ✅已修复

删除整个 Firebolt 技能条目和相关代码，法师职业改为护盾+狂暴。

### I4 — 装备卸载回滚不完整 ✅已修复

`Inventory` 新增 `can_add()` 预检方法，装备卸载前先检查背包容量，有空间再执行，避免部分添加后无法回滚。

### I5 — 怪物游荡使用确定性方向而非随机 ✅已修复

从 `(FloorNumber + monster_count) % 8` 改为 `rand::random::<u8>() % 8`，每个怪物独立随机方向。

### I6 — apply_exp_system 在每个 ready 条目后调用（Won't Fix） ✅已修复

该函数有 early return（`pending.amount == 0`），非击杀条目开销 <1μs。事件帧模式下每个条目后调用反而是正确行为（即时反馈经验变化）。

### I7 — PendingLevelUp 悬空 ✅已修复

删除整个 PendingLevelUp 机制，升级时不再累积属性点数，只提升等级和 HP/MP。

### I8 — 怪物生成数量固定 12 只 ✅已修复

怪物生成尝试次数从固定 `12` 改为 `room_centers.len()`，地面物品数量改为 `room_centers.len().min(8)`，随可用房间数自动变化。

### I9 — 废弃注释和空白行 ✅已修复

删除 core/systems.rs 中的 `// use crate::world; // 已移除` 注释和多余空行。

### I11 — 渲染层在已探索暗处直接渲染实体实时位置（X 射线透视） ✅已修复

渲染层遍历 renderables 时，删除 `else if explored[ey][ex]` 灰色渲染分支。暗处实体不再直接画出实时位置，改由 `visible_mem` 循环在已探索区域显示上次看到的位置。

### G2/G3 — 死后游戏仍推进 ✅已修复

死后跳过 `advance_and_settle`，q 键直接退出（跳过确认弹窗）。

### G7 — 楼梯不可达 ✅已修复

`Map` 新增 `ensure_connection_between()`：BFS 检查从出生点到楼梯是否有 walkable 路径，若无则用加权醉汉游走（70% 概率指向楼梯方向，30% 随机）挖掘通道。在 `setup_world` 和 `descend` 中楼梯放置后调用。

### A2 — 背包 UI 250+ 行在 main.rs ✅已修复

将 `InvPanel`/`DetailSource`/`Page` 枚举、`collect_ground_items_in`、`open_inventory` 整体提取到独立模块 `src/inventory.rs`。`lib.rs` 添加 `pub mod inventory`，main.rs 通过 `dungeon_tui::inventory::open_inventory` 调用。

---

## 一、设计层面（Design）

---

### 🟡 D5 — 事件帧模式（Deferred — 触发条件达成时重新评估）

**问题：** 当前玩家确认行动后批量推进到玩家行动完成，中间所有怪物行动对玩家不可见。

**提议方案：** 增加可切换的"事件帧模式"（按 `s`），每帧只执行一个事件，Enter 步进。

**当前评估：** 暂缓实现。在当前战斗系统（纯数值 chase/flee/wander）下，事件帧模式提供的信息量不足以补偿节奏损失——玩家的最优策略不会因看到每个怪物单步移动而改变。

**触发条件：** 出现**足够复杂的战斗逻辑**，即新增的怪物/boss 有需要玩家在过程中作出反应的能力——例如范围攻击预警、状态效果倒计时、可打断的吟唱、地形变化。当单次 tick 内的行动序列构成决策信息时，事件帧模式从"nice to have"变为"need to have"。

---

## 二、架构层面（Architecture）

---

### 🟡 A6 — `dungeon-core` 职责膨胀（行动类型不应在 core 中）

**问题：** `dungeon-core/src/action_types.rs` 定义了 `ActionQueue`、`ActionKindV3`、`CanMove`/`Chase`/`Flee`/`Wander`/`Wait`、`Reaction`、`InputBuffer`、`PlayerPreview`、`ChaseIntents`/`FleeIntents`/`WanderIntents`。这些是**行动系统的领域类型**，不是"核心数据"。

它们被放在 core 中的唯一原因是依赖链方向：`core ← action`。如果 action 持有自己的类型，core 无法引用它们，但 action 需要这些类型被 core 中的 `ops.rs` 和 `systems.rs` 使用。结果是：**core 的变化速度被行动系统拖快**——添加新行动时 core 要 recompile。

**位置：** `dungeon-core/src/action_types.rs`（整个文件）

**建议修复方向：**
1. 将 `action_types.rs` 迁移到 `dungeon-action` crate
2. 或新增 `dungeon-action-types` 中间 crate 同时被 core 和 action 引用
3. 短期：至少将 `InputBuffer` / `PlayerPreview` 移入 dungeon-action（它们只在 action 和 main.rs 中使用）

---

### 🟡 A7 — `ops.rs` 是万能工具袋（低内聚）

**问题：** `dungeon-core/src/ops.rs` 包含以下互不相关的功能：

| 功能 | 消费方 | 主题 |
|------|--------|------|
| 经验/HP/MP 公式 | core、render | 数值 |
| 有效属性计算 | action、render | 战斗 |
| 实体查询（player_entity、on_stairs） | action、world、render、main | 查询 |
| 拾取逻辑（pickup_ground） | main | 交互 |
| 视野记忆更新 | world、main | 状态 |
| 碰撞图重建 | action、world | 物理 |
| 渲染数据收集 | render | 渲染 |
| FOV | core（system） | 视野 |
| A* 寻路 | action | 寻路 |

这些函数没有主题关联，唯一的共同点是"被多个 crate 使用"——这是**实现共享**而非**概念内聚**。新功能自然地被塞入 ops.rs，进一步膨胀。

**位置：** `dungeon-core/src/ops.rs`

**建议修复方向：** 按主题拆分：`fov.rs`、`pathfinding.rs`、`formulas.rs`、`world_query.rs`。

---

### 🟢 A8 — `monster_def.rs` 混合定义与算法

**问题：** `dungeon-core/src/monster_def.rs` 名义上是"怪物定义"模块，包含外观、属性、掉落表等数据定义。但其中也包含了 `generate_monster_population()`——一个约 80 行的**世界初始化算法**（噪声密度层 → 元胞扩散 → 数量钳制）。这属于 `dungeon-world/src/init.rs` 的职责范畴。

**根因：** `setup_world` 和 `descend` 都需要生成怪物种群，所以这个函数被放在了 core 中。但"被两个地方调用"不应成为把算法塞入数据模块的理由。

**位置：** `dungeon-core/src/monster_def.rs:103-188`

**建议修复方向：** 将 `generate_monster_population` 移至 `dungeon-world/src/init.rs` 或独立 `population.rs`。

---

### 🟢 A10 — world init 直接操作 Map 内部细节

**问题：** `dungeon-world/src/init.rs` 中的 `setup_world` 和 `descend` 直接访问 `map.rooms[0].center()` 和 `map.tiles`。Map 的生成细节泄漏到 world 初始化逻辑中：

- `rooms[0].center()` 的语义是"出生点"，但 Map 没有提供 `spawn_point()` 方法
- `rooms.iter().skip(1)` 依赖调用方知道 rooms[0] 是出生房间
- 楼梯选择逻辑 `max_by_key` 直接操作 rooms 的内部结构

**位置：** `dungeon-world/src/init.rs`

**建议修复方向：**
1. Map 增加 `spawn_point() -> (usize, usize)` 和 `farthest_room_from(point) -> (usize, usize)` 方法，封装 rooms 内部细节
2. `generate_monster_population` 改为接收 `&Map` 而非裸 tiles 数组
---

## 三、实现层面（Implementation）

---

### 🟢 I10 — 斜向键按住不放无法连续移动（无 OS key-repeat）— 终端环境限制，暂不处理

**问题：** 按住 Home/End/PgUp/PgDn 不放，角色不会连续斜向移动。因为多数终端不发 Home/End 的 OS key-repeat 事件，tap-tap 系统需要两个事件完成一次移动（预览+确认），但按住斜向键只产生一个事件。

**结论：** 由终端环境引起，不在本项目控制范围内，等未来统一运行环境后再修复。

**位置：** `src/main.rs:60-74`（输入线程）

---

### 🟡 I17 — 少量 `.unwrap()` 调用待处理（约 10 处）

**当前状态：** 约 25 处 `try_query().unwrap()` 已替换为 `try_query().expect("...")`（共 6 个文件：ops/ui/inventory/persist/execute/tick）。剩余约 10 处非 try_query unwrap：

| 模式 | 数量 | 风险 |
|------|------|------|
| `get::<T>(entity).unwrap()` | ~4 | execute.rs（Inventory/Equipment on attacker） |
| `ItemRegistry::global().get(id).unwrap()` | ~3 | init.rs（物品生成） |
| `query().next().unwrap()` | ~1 | init.rs descend |
| 其他杂项 | ~3 | action_types.rs f32 partial_cmp、inventory.rs slot.take() |

---

## 四、游戏逻辑层面（Game Logic）

---

### 🟢 G8 — 水体生成保护距离可能过大（优先级低）

**问题：** `generate_water` 使用 `is_away_from_rooms(x, y, 6)` 保护房间中心不被水体覆盖。曼哈顿距离 6 对于半径 4-6 的圆形/菱形房间可能过大，导致水体偏少。

**位置：** `dungeon-core/src/lib.rs:241`

---

## 其他

### 🟢 P7 — 玩家确认行动后无法取消（被 D5 锁定）

**问题：** tap-tap 双击确认后行动进入 `ActionQueue` 无法撤回。

**说明：** 事件帧模式（D5，已 defer）可以部分解决此问题——事件帧模式下玩家可以在自己行动执行前切换方向。在 D5 重新评估前此问题无解。

