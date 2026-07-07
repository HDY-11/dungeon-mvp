> **⚠️ 修改前必须阅读或回忆 [RULE.md](RULE.md) 的内容，了解本文档的维护规范。**

# 发现的问题记录

本文档记录当前实现中与设计意图不一致或有改进空间的问题，
在后续开发中可作参考。

问题按维度分组：**设计 / 架构 / 实现 / 游戏逻辑**，组内按严重程度降序。
优先级标记：🔴 高（影响正确性或游戏体验） / 🟡 中（维护性或功能缺口） / 🟢 低（整洁或边缘情况）

---

## ✅ 已修复

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

### P1 — 保活检查只检查即将执行的条目

队列推进时对所有条目做批量保活检查，不满足的立即剔除，防止 Chase/Flee 在等待期间条件已失效却仍留在队列中白耗 AV。

### P3 — 并行 Schedule 每帧重建（Won't Fix）

每帧构建开销 <1μs，且保持测试跨 World 兼容，保留现状。

### P6 — action.rs 是空壳模块

删除 action.rs，所有引用统一到 action_types。

### P9 — VisibleMemory 在视野边缘闪烁

加入 VISIBLE_FORGET_DELAY=3 遗忘延迟，避免实体在视野边缘来回移动时闪烁。

### P10 — 存档缺少对 ActionQueue 的序列化

按位置映射保存/恢复队列条目，Attack 条目因 Entity 引用跳过。

### D1 — 三套 RNG 并存，游戏不可复现

`GameRng` 成为唯一随机源：新增便捷方法，`LootTable::roll()` 改为接受 `&mut impl Rng`，暴击/游荡/仲裁全部走 `GameRng`，删除线程局部 RNG，种子从硬编码 `0` 改为 `map_seed.wrapping_add(42)`。

### D2 — 存档/读档丢弃 Intent 缓冲区状态

`GameSave` 新增 `chase_intents` / `flee_intents` / `wander_intents` 字段，capture 按位置保存，restore 通过 position→entity 重映射恢复，`#[serde(default)]` 兼容旧存档。

### D3 — crate 依赖链文档与实际不符

修正 README.md 中 crate 划分树和依赖链描述，移除冗余的重复树结构。

### A1 — dungeon-core 与 dungeon-world 大量代码重复

以 core 的 systems 为 canon：`calculate_visible_tiles` 移入 ops.rs，删除 core 的 api.rs（`setup_world` 移入 tests.rs），删除 world 的 systems.rs，world 的 tick 改引用 core 的 systems。

### I1 — 对角穿墙角不对称：玩家可穿，怪物不可穿

移除 A\* 中的对角穿墙角检查，玩家和怪物行为一致（均可穿墙角）。

### I2 — 逃跑无退出条件（触发后永远逃跑）

引入滞回区间：`CanFlee::condition`（决策进入）保持 HP < 25%，`check_condition`（保活退出）改为 HP < 30%。

### I3 — 火球技能击杀无经验/无掉落，且会伤害玩家自身

删除整个 Firebolt 技能条目和相关代码，法师职业改为护盾+狂暴。

### I4 — 装备卸载回滚不完整

`Inventory` 新增 `can_add()` 预检方法，装备卸载前先检查背包容量，有空间再执行，避免部分添加后无法回滚。

### I5 — 怪物游荡使用确定性方向而非随机

从 `(FloorNumber + monster_count) % 8` 改为 `rand::random::<u8>() % 8`，每个怪物独立随机方向。

### I6 — apply_exp_system 在每个 ready 条目后调用（Won't Fix）

该函数有 early return（`pending.amount == 0`），非击杀条目开销 <1μs。事件帧模式下每个条目后调用反而是正确行为（即时反馈经验变化）。

### I7 — PendingLevelUp 悬空

删除整个 PendingLevelUp 机制，升级时不再累积属性点数，只提升等级和 HP/MP。

### I8 — 怪物生成数量固定 12 只

怪物生成尝试次数从固定 `12` 改为 `room_centers.len()`，地面物品数量改为 `room_centers.len().min(8)`，随可用房间数自动变化。

### I9 — 废弃注释和空白行

删除 core/systems.rs 中的 `// use crate::world; // 已移除` 注释和多余空行。

### I11 — 渲染层在已探索暗处直接渲染实体实时位置（X 射线透视）

渲染层遍历 renderables 时，删除 `else if explored[ey][ex]` 灰色渲染分支。暗处实体不再直接画出实时位置，改由 `visible_mem` 循环在已探索区域显示上次看到的位置。

### G2/G3 — 死后游戏仍推进

死后跳过 `advance_and_settle`，q 键直接退出（跳过确认弹窗）。

### G7 — 楼梯不可达

`Map` 新增 `ensure_connection_between()`：BFS 检查从出生点到楼梯是否有 walkable 路径，若无则用加权醉汉游走（70% 概率指向楼梯方向，30% 随机）挖掘通道。在 `setup_world` 和 `descend` 中楼梯放置后调用。

### A2 — 背包 UI 250+ 行在 main.rs

将 `InvPanel`/`DetailSource`/`Page` 枚举、`collect_ground_items_in`、`open_inventory` 整体提取到独立模块 `src/inventory.rs`。`lib.rs` 添加 `pub mod inventory`，main.rs 通过 `dungeon_tui::inventory::open_inventory` 调用。

---

## 一、设计层面（Design）

---

### 🟡 D5 — 行动系统设计：事件帧模式（Event-Frame Mode）— 计划中

**问题：** 当前玩家确认行动后 `advance_until_player_acted` 批量推进到玩家行动完成，中间所有怪物的行动对玩家不可见不可干涉。

**提议方案：** 增加一个可切换的"事件帧模式"，按 `s` 在两种模式间切换：

| 模式 | 行为 | 适用场景 |
|------|------|---------|
| **玩家行动模式**（默认） | 玩家确认→批量推进到玩家行动完成→渲染 | 快速推进 |
| **事件帧模式** | 每帧只执行一个事件→渲染→等待玩家按 Enter 或确认下一步 | 精细观察/战术决策 |

**核心改动：**
- `EventFrameMode` Resource + `s` 键切换 + UI 指示器
- `ActionQueue` 增加 `add_or_replace` 方法（事件模式下替换已有条目）
- `advance_one_event()` 推进到下一事件点并最多执行一个条目
- 事件模式下 Enter 键触发推进
- 怪物决策在队列空 + 意图缓冲区空时触发

**位置：** `dungeon-action/src/execute.rs`、`src/main.rs`（主循环）、`dungeon-action/src/player.rs`

---

### 🟢 D4 — 升级满血满蓝未文档化（优先级低）

**问题：** `apply_exp_system` 中升级后满血满蓝，在 GAME.md 和 DESIGN.md 中均未记录。

**说明：** 设计简化，方便测试不同楼层体验。如未来需要更严格的生存挑战，可改为回复 30%~50%。

---

## 二、架构层面（Architecture）

---

---

## 三、实现层面（Implementation）

---

### 🟢 I10 — 斜向键按住不放无法连续移动（无 OS key-repeat）— 终端环境限制，暂不处理

**问题：** 按住 Home/End/PgUp/PgDn 不放，角色不会连续斜向移动。因为多数终端不发 Home/End 的 OS key-repeat 事件，tap-tap 系统需要两个事件完成一次移动（预览+确认），但按住斜向键只产生一个事件。

**结论：** 由终端环境引起，不在本项目控制范围内，等未来统一运行环境后再修复。

**位置：** `src/main.rs:60-74`（输入线程）

---

## 四、游戏逻辑层面（Game Logic）

---

### 🟡 G9 — 仅1个连通房间时玩家出生在楼梯上（重合）

**问题：** 下楼后 `detect_cave_regions` 可能只检测出 1 个连通区域（尤其深层地图经水体/钟乳石/连通性处理后）。此时 `setup_world` 和 `descend` 中玩家和楼梯均使用 `rooms[0].center()`。`max_by_key` 在只有 1 个房间时返回自身，结果楼梯和玩家挤在同一格。

**根因链：** terrain-forge 生成 → 水体/钟乳石切割 → ensure_connectivity 连接所有区域 → 只剩 1 个超大连通区 → `detect_cave_regions` 返回 1 个 Room → 玩家和楼梯位置相同。

**用户反馈：** 下到第二层后发现出生在楼梯上，界面显示"房间1"。

**位置：** `dungeon-world/src/init.rs`（stairs_pos 选择逻辑）、`dungeon-world/src/init.rs`（descend 中 stairs_pos 同样逻辑）

**建议修复方向：**
1. 玩家出生时，5 格范围内不能有其他怪物（怪物生成排除玩家邻域）
2. 楼梯在玩家 15-25 格外随机生成，而非依赖 `rooms` 列表

### 🟡 G10 — 怪物阻挡楼梯/物品（怪物生成未排除关键位置）

**问题：** `generate_monster_population` 基于噪声密度层 + 元胞扩散在全地图 walkable 格上放置怪物，但**不排除楼梯位置和物品位置**。下楼后：

1. **楼梯被怪物堵住**：怪物生成在楼梯格 → `OccupancyMap` 标记该格被占用（怪物没有 Stairs/ItemPickup 组件，不会被 `rebuild_occupancy` 过滤）→ 玩家走开后无法走回楼梯 → 表现为"楼梯不可行走"。
2. **物品格被怪物堵住**：同���，怪物站在物品上 → `handle_player_direction` 检查 `OccupancyMap` 发现该格有实体且是 Monster → `has_enemy = Some` → 转为攻击而非拾取。

**用户反馈：** 从楼梯上走开后无法走回；有时道具（地面物品）也不能走到上面。

**位置：** `dungeon-core/src/monster_def.rs:generate_monster_population`

**建议修复方向：** `generate_monster_population` 增加排除列表参数（stairs_pos、spawn_pos、ground_item_positions），对应坐标不放置怪物。

---

## 三、实现层面（Implementation）

---

### 🟢 I16 — 仅1个房间时地面物品为0

**问题：** `setup_world` 和 `descend` 中地面物品位置列表通过 `rooms.iter().skip(1)` 获取（跳过出生房间）。若 `rooms.len() == 1`，`skip(1)` 后列表为空，导致 **0 个地面物品**生成。

```rust
let room_centers: Vec<(usize, usize)> = world.resource::<Map>().rooms.iter().skip(1).map(|r| r.center()).collect();
let item_count = room_centers.len().min(ground_item_ids.len());
```

`room_centers.len()` = 0 → `item_count` = 0 → 无物品。

**用户反馈：** 第二层没有生成任何道具。

**位置：** `dungeon-world/src/init.rs:108`（setup_world）、`dungeon-world/src/init.rs:199`（descend）

**建议修复方向：** 当 `rooms.len() == 1` 时，退回到 `rooms[0]` 内部放置物品，但用 `is_away_from_spawn` 或随机偏移避免与出生点/楼梯完全重叠。或直接为单房间场景准备一份备用位置列表。

---

### 🟡 I17 — 大量 `.unwrap()` 调用（未处理错误路径约 35 处）

**问题：** 整个代码库的生产代码中散布着约 **35 处 `.unwrap()` 调用**，分布在 8 个文件中。任何一处 panic 都会导致整个游戏崩溃（无错误恢复机制）。

**分类统计：**

| 模式 | 数量 | 代表位置 | 风险 |
|------|------|---------|------|
| `try_query().unwrap()` | ~25 | ops.rs、ui.rs、inventory.rs、persist.rs | 理论上安全（组件提前注册），但违背"失败路径应被显式处理"的原则 |
| `get::<T>(entity).unwrap()` | ~4 | execute.rs（Inventory/Equipment on attacker） | 若实体缺少组件则 panic；攻击者必有装备/背包的假设目前成立，但扩展后易破 |
| `ItemRegistry::global().get(id).unwrap()` | ~3 | init.rs（物品生成） | items.json 若缺 id 直接 panic |
| `query().next().unwrap()` | ~1 | init.rs descend | rooms 为空或玩家不存在时 panic |
| 其他杂项 | ~3 | action_types.rs f32 partial_cmp、inventory.rs slot.take() | 低风险但无信息量 |

**根因：** 项目早期使用 `.unwrap()` 作为"快速原型"手段。随着代码增长的稳定，这些临时桩一直没有被替换为有信息量的错误处理。

**建议修复方向（逐步）：**

1. `try_query().unwrap()` → `try_query().expect("component X registered at init")` — 零成本改动，崩溃时提供调试信息
2. `get::<T>(entity).unwrap()` → 改用 `if let Some(t) = world.get::<T>(entity)` 或 `expect()`
3. `ItemRegistry::global().get(id).unwrap()` → `expect("item {id} exists in registry")` 或 `unwrap_or_else` 提供应急默认值
4. 关键路径（execute_attack、descend）中的 `.unwrap()` 应优先处理

**优先级说明：** 标记为 🟡 而非 🔴 是因为大部分 unwrap 在当前架构下不会实际触发（组件已注册、物品 ID 已定义）。但它们是**定时炸弹**——任何组件注册顺序变化或 items.json 疏漏都会无声地转为 panic。长远目标是**零 unwrap**（测试代码除外）。

---

## 四、游戏逻辑层面（Game Logic）

---

### 🟢 G8 — 水体生成保护距离可能过大（优先级低）

**问题：** `generate_water` 使用 `is_away_from_rooms(x, y, 6)` 保护房间中心不被水体覆盖。曼哈顿距离 6 对于半径 4-6 的圆形/菱形房间可能过大，导致水体偏少。

**位置：** `dungeon-core/src/lib.rs:241`

---

## 其他

### P7 — 玩家确认行动后无法取消（将被 D5 部分解决）

**问题：** tap-tap 双击确认后行动进入 `ActionQueue` 无法撤回。

**说明：** 事件帧模式（D5）部分解决了此问题——事件帧模式下玩家可以在自己行动执行前切换方向。

