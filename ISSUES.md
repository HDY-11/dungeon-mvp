> **⚠️ 修改前必须阅读或回忆 [RULE.md](RULE.md)——它定义了本文档的维护规则和更新时机。**

# 发现的问题记录

本文档记录当前实现中与设计意图不一致或有改进空间的问题，
在后续开发中可作参考。

问题按维度分组：**设计 / 架构 / 实现 / 游戏逻辑**，组内按严重程度降序。
优先级标记：🔴 高（影响正确性或游戏体验） / 🟡 中（维护性或功能缺口） / 🟢 低（整洁或边缘情况）

---

## ✅ 已修复

### I35 — 怪物颜色统一使用 Renderable 组件（地图+行动轴） ✅已修复

**修复前：** `timeline.rs` 和 `ui.rs` 使用 `entity_color(entity.to_bits(), 0)` 实时哈希计算怪物颜色。读档后 Entity ID 重建导致颜色不一致。更根本的问题是：独特色是"渲染时实时计算的"，不被持久化。

**修复后：**
1. `entity_color` + `hsv_to_rgb` 从 `dungeon-render` 移至 `dungeon-core/src/color.rs`（纯数学，无 TUI 依赖）
2. `spawn_monsters` 在 spawn 后立即用 `entity_color(entity.to_bits(), 0)` 写入 `Renderable.color`——独特色在生成时固定
3. `timeline.rs` 改为直接读取 `Renderable.color`，不再实时哈希
4. `ui.rs` 移除怪物 entity_color 覆写，直接使用 renderable 的已存颜色

**效果：** 独特色在存档中持久化（SavedMonster 的 r/g/b 字段），读档/下楼后地图和行动轴颜色一致。

**位置：** `dungeon-core/src/color.rs`（新模块）、`dungeon-world/src/init.rs:67`、`dungeon-render/src/timeline.rs:41`、`dungeon-render/src/ui.rs:126`
**教训见：** `LESSONS.md L40`（新增——独特色应在生成时固定存储于组件，而非渲染时实时计算）

### A11 — 删除 Stats::monster() 死代码 ✅已修复

**修复前：** `components.rs` 中 `Stats::monster(glyph, floor)` 无调用方，缺少蝎子匹配，功能完全重复于 `monster_def::monster_stats()`。

**修复后：** 删除整个方法（~30 行死代码）。

**位置：** `dungeon-core/src/components.rs:120-156`

### A4La — Map 残留 generate_water / is_away_from_rooms / count_walkable_neighbors 死方法 ✅已修复

**修复前：** A4/A4L 后 Map impl 仍有三个零调用的方法。同模式第三次发生。

**修复后：** 删除三个方法。Map impl 仅保留 `count_tile` / `count_neighbor_tile` / `carve_corridor` / `render` / `spawn_point`。

**位置：** `dungeon-core/src/lib.rs`
**教训见：** `LESSONS.md L38`

### I33 — 丢弃物品产生地面拾取物 ✅已修复

**修复前：** 背包详情页按 `d` 直接 `inv.drop_stack(idx)` 删除物品栈，物品永久消失。丢弃是唯一不可逆的物品销毁路径。

**修复后：** 丢弃时获取玩家位置，在地面 spawn ItemPickup 实体（含 glyph/color）。事件日志显示"丢弃了xxx在脚下"。

**位置：** `src/inventory.rs:268-280`

### I34 — ActiveBuffs 未加入存档 ✅已修复

**修复前：** `GameSave` 仅保存旧 `Buffs`，玩家在 Buff 持续期间存档后，读档后 Buff 丢失。

**修复后：** `GameSave` 新增 `active_buffs: Vec<SavedActiveBuff>` 字段（`#[serde(default)]` 兼容旧存档），capture 时序列化玩家 ActiveBuffs，restore 时重建 Buff 列表。

**位置：** `dungeon-world/src/persist.rs`

### I32 — SkillKind::duration 单位歧义（回合/秒） ✅已修复

**修复前：** `duration: i32` 可负值；旧 Buffs 系统读作 3 帧 ≈ 50ms，新 ActiveBuffs 读作 3 秒；技能描述写"持续3回合"。

**修复后：** `duration` 改为 `u32`（禁止负值）；技能描述统一为"持续3秒"。

**位置：** `dungeon-core/src/components.rs:160-164`

### 🔴 G14 — 护盾/狂暴技能双倍叠加（执行层移除旧系统写入） ✅已修复

**修复前：** `execute_skill` 同时写入旧 `Buffs` 和新 `ActiveBuffs`，`effective_attack`/`effective_defense` 对两者求和。每次使用 Shield/Berserk 时护盾/狂暴数值在 ~3 帧内翻倍（+10 而非 +5）。

**修复后：** `execute_skill` 移除了旧 `Buffs` 写入路径，`effective_attack`/`effective_defense` 只读新 `ActiveBuffs`（旧 Buffs 参数保留但不再参与计算）。使用技能护盾/狂暴正确只加 +5。

**位置：** `dungeon-action/src/execute.rs:315-340`、`dungeon-core/src/ops.rs:24-50`
**教训见：** `LESSONS.md L39`

### I27 — 怪物颜色可区分性差（修复三次） ✅已修复

**第一次修复：** `unique_color` 取 `entity.to_bits()` 低 6 位偏移 ±32，相邻 ID 色差 <1。

**第二次修复：** 黄金比例 `wrapping_mul` 扩散 + 范围采样 ±64。但极端基准色（老鼠 `255,0,0`）的通道被 clamp 吞噬，仍无差异。

**第三次修复：** 废弃基准色方案。改用 `SipHash(entity_bits ⊕ seed)` 的高位直接映射 RGB，无基准色限制，微小 ID 变化经 hash 后产生大幅颜色跳跃。

**位置：** `dungeon-render/src/color.rs:12-20`

### I29 — 泛型 Buff 系统（ActiveBuffs + AV 推进） ✅已修复

**修复前：** Buff 使用回合计数（`shield_turns: i32`），与 AV 时间轴脱钩。每帧减 1 回合，不同帧消耗速度不同。技能只能通过职业锁定。

**修复后：** 新增 `ActiveBuffs(Vec<Buff>)` 和 `ActiveCooldowns(Vec<Cooldown>)` 泛型组件，`advance_action_queue` 中与队列同步推进（`remaining_av -= dist`）。`effective_attack/defense` 查询 ActiveBuffs。旧 `Buffs` 组件保留过渡期兼容。

**位置：** `dungeon-core/src/components.rs`、`dungeon-action/src/execute.rs`、`dungeon-core/src/ops.rs`

### I30 — UI 整合：Buff/视野/HP 标注移至行动轴 ✅已修复

**修复前：** Buff 显示在 stats 面板（文本行），视野实体显示在 stats 面板底部，行动轴只显示行动名和倒计时。信息分散。

**修复后：** 行动轴整合为三区：①队列条目（符号+行动+耗时）②分割线③实体状态（符号+怪物名+血量）④次级标注（Buff，dim 样式）。stats 面板移除 Buff 和视野段。

**位置：** `dungeon-render/src/timeline.rs`、`dungeon-render/src/ui.rs`

### I31 — x 键光标查看模式 ✅已修复

**修复前：** 无查看模式，玩家无法了解地图上未知位置的详细信息。

**修复后：** 新增 `LookCursor` 资源 + `open_look_mode`（方向键移动、x/Esc 退出）。地图上光标格叠加暗黄色背景高亮。stats 面板底部显示光标位置的地形名和实体名+HP。

**位置：** `dungeon-core/src/resources.rs`、`src/main.rs`、`dungeon-render/src/ui.rs`

### I28 — 事件日志从 stats 面板移至地图下方 ✅已修复

**修复前：** 事件日志位于右侧 stats 面板底部，占用了属性显示空间且不易阅读。

**修复后：** 地图区增加垂直分割，地图占 `VIEWPORT_HEIGHT`，下方独立显示事件日志（`── 事件 ──` 分隔线，最近 5 条）。

**位置：** `dungeon-render/src/ui.rs`

### G13 — 玩家面板显示不应公开的调试信息（房间数/怪物数） ✅已修复

**修复前：** 属性面板中显示 `房间 N` 和 `怪物 N`，这些是地图生成和种群统计的调试数据，玩家不应看到。行动轴宽度 22 偏高，压缩了地图和属性区的可用空间。事件日志仅显示 5 条，战斗密集时关键信息快速滚出屏幕。

**修复后：** 删除房间/怪物数量行。行动轴收窄至 16，释放水平空间。事件日志增至 12 条。

**位置：** `dungeon-render/src/ui.rs`

### G11 — `rooms[0].center()` 不可行走导致出生卡墙 ✅已修复

**修复前：** `spawn_point()` 直接返回 `rooms[0].center()`，不做 walkable 校验。`generate_stalactites` 在房间内每格 7% 概率将 Floor 变 Stalactite，可能覆盖房间中心点；`ensure_spawn_accessible` 只检查邻居不检查中心自身。下楼后玩家可能在不可行走格上出生，无法移动。

**修复后：** `spawn_point()` 先检查中心是否 walkable，若否则以螺旋搜索（半径 1→20）寻找最近的可行走格。确保返回值永远可通行。

**位置：** `dungeon-core/src/lib.rs:421-442`
**触发条件：** `generate_stalactites` 在 room[0] 每格 7% 概率 → 约每 14 次下楼触发一次。

### A7 — 拆分 ops.rs 为 fov / pathfinding / ops ✅已修复

**修复前：** `ops.rs` 是万能工具袋——FOV、A\*、公式、查询、拾取、碰撞图、渲染收集等 9 个无关功能挤在同一个文件中。

**修复后：** 提取 `dungeon-core/src/fov.rs`（`calculate_visible_tiles`）和 `dungeon-core/src/pathfinding.rs`（`astar` + `AStarNode`）。ops.rs 保留剩余的紧密相关工具函数（公式、属性计算、实体查询、拾取、碰撞图、视野记忆、渲染收集）。

**统计：**
| 文件 | 行数 | 职责 |
|------|------|------|
| `fov.rs` | ~25 | 对称阴影投射视野计算 |
| `pathfinding.rs` | ~80 | A\* 8 方向寻路 |
| `ops.rs`（剩余） | ~120 | 公式/查询/记忆/碰撞/渲染 |

### I17 — 全部 `.unwrap()` 替换为 `.expect()` ✅已修复

**状态：** 全部 ~35 处 `.unwrap()` 已替换。生产代码零 unwrap。

### I10 — 斜向键无 OS key-repeat（Won't Fix — 终端环境限制） ✅已修复

**问题：** 按住 Home/End/PgUp/PgDn 不放，角色不会连续斜向移动。多数终端不发斜向键的 OS key-repeat 事件。

**结论：** 终端环境引起，不在项目控制范围内。

### G8 — 水体生成保护距离调整（6→3） ✅已修复

**修复前：** `is_away_from_rooms(x, y, 6)` 保护距离 6，对半径 4-6 的房间偏大，水体几乎不出现。

**修复后：** 保护距离改为 3。视窗内可见 ~9-19 格水体（约 1-2% 地图面积），以水洼和窄溪流形式分布在通道边缘和房间过渡带，不淹没房间内部。

**评估：** 当前密度适合洞穴环境，也为未来的水体减速/加速 Buff 预留了触发空间——每层自然涉水 3-5 次，有存在感但不泛滥。

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

### 🟡 D9 — 下楼不保存 ActiveBuffs

**问题：** `descend()` 中 `player_data.5` 硬编码为 `Buffs::new()`（空），且 `ActiveBuffs` 组件完全未在 `descend` 中捕获和重建。下楼后玩家身上的护盾/狂暴 Buff 全部丢失。

```rust
// init.rs:196 — 永远空的
Buffs::new(), cls.clone(), atk.0.clone())
// init.rs:207 — 下楼后插入的也是空的
cmd.insert(ActiveBuffs::new());
```

**对比：** 存档/读档（`persist.rs`）正确保存和恢复了 `ActiveBuffs`——说明下楼丢失不是有意设计，而是遗漏。

**影响：** 🟡 中 — 玩家在楼梯口开 Shield 下楼→Buff 消失，与存档读档行为不一致。

**位置：** `dungeon-world/src/init.rs:193-207`

### 🟢 D10 — `buff_tick_system` 仍在处理废弃的旧 `Buffs` 组件

**问题：** `buff_tick_system` 每帧修改旧 `Buffs` 组件的 `shield_turns`/`berserk_turns`/`shield_def`/`berserk_atk` 字段。但 `effective_attack`/`effective_defense` 已在 G14 修复中改为只读新 `ActiveBuffs`。旧 Buffs 的修改永远不会被消费。

```rust
// systems.rs:47-50 — 仍在运行，产生无用副作用
pub fn buff_tick_system(mut query: Query<&mut Buffs, With<Player>>) {
    for mut b in query.iter_mut() {
        if b.shield_turns > 0 { b.shield_turns -= 1; if b.shield_turns <= 0 { b.shield_def = 0; } }
        if b.berserk_turns > 0 { b.berserk_turns -= 1; if b.berserk_turns <= 0 { b.berserk_atk = 0; } }
    }
}
```

**违反 LESSONS L39：** 双系统共存应推进到 Phase 3（移除旧系统），当前停留在 Phase 1 且 `buff_tick_system` 仍在 Schedule 中注册并每帧运行。

**位置：** `dungeon-core/src/systems.rs:47-50`、`dungeon-world/src/tick.rs:13`

---

## 二、架构层面（Architecture）

### 🟡 A10 — 事件日志显示条数回归：代码 take(5) 而非声称的 12

**问题：** `ui.rs` 事件日志渲染使用 `.take(5)`，但 ISSUES.md G13（已修复）记录"事件日志增至 12 条"。G13 修复要么未落地，要么被后续提交覆盖。

```rust
// ui.rs:193 — 当前代码
for msg in log.messages.iter().rev().take(5) {
```

**确认：** 搜索 `.take\(1[0-9]\)` 无匹配，仅 `.take(5)` 一处。G13 本已解决的问题重现。

**位置：** `dungeon-render/src/ui.rs:193`

### 🟡 A11 — `ActiveCooldowns` 悬空功能

**问题：** `ActiveCooldowns` 组件在 `advance_action_queue` 中有完整的 AV 推进逻辑，但在 `descend` 中既未保存也未恢复，且没有任何技能向其写入数据。组件有定义、有推进、有存档支持，但无任何写入点。

```rust
// components.rs:189 — 定义
#[derive(Component, Clone, Debug, Default)]
pub struct ActiveCooldowns(pub Vec<Cooldown>);

// execute.rs:33-38 — 推进逻辑
{
    let mut q = world.query::<&mut ActiveCooldowns>();
    for mut cds in q.iter_mut(world) {
        cds.0.retain_mut(|c| { c.remaining_av -= dist; c.remaining_av > 0.0 });
    }
}
```

**违反 LESSONS L20：** 未完成的游戏机制不应留在代码中。

**位置：** `dungeon-core/src/components.rs:189`、`dungeon-action/src/execute.rs:33-38`

### 🟡 A12 — ActionKindV3 枚举跨 8 个文件飘散，新加怪物行为成本高

**问题：** `ActionKindV3` 枚举同时承载玩家行动（Move/Wait/Skill/Attack）和怪物行为（Chase/Flee/Wander），两种扩展节奏不同的东西被捆绑在一个枚举中。每新增一种怪物行为，需要修改 **7-8 个文件**：

| 文件 | match 点 | 修改内容 |
|------|---------|---------|
| `dungeon-action/src/types.rs` | 枚举定义 | +1 变体 |
| `dungeon-action/src/execute.rs` | `execute_entry` + `check_condition` | +2 arm |
| `dungeon-action/src/player.rs` | `handle_timed_action` | +1 确认对 |
| `dungeon-render/src/timeline.rs` | `action_display` | +1 arm |
| `dungeon-world/src/persist.rs` | `SavedActionKind` + capture + restore | +3 处 |
| `dungeon-action/src/monster.rs` | 决策系统 | +1 输出 |

**根因：** 枚举是编译期全匹配的，适合**变体少且稳定**的场景（如 Tile = 5 种地形，EquipmentSlot = 3 个槽位）。怪物行为需要持续扩展（巡逻/召唤/远程/毒雾等），用枚举每加一种就要通改所有 match 点。

**建议方向：** 玩家行动保持枚举（Move/Wait/Skill/Attack 扩展频率极低），怪物行为改用 trait 对象：

```rust
pub trait MonsterBehavior: Send + Sync {
    fn execute(&self, world: &mut World, entity: Entity);
    fn check_condition(&self, world: &World, entity: Entity) -> bool;
    fn display_name(&self) -> &'static str;
    fn priority(&self) -> u32;
    fn av_cost(&self, agility: u32) -> f32;
}
```

`ActionEntry` 加 `behavior: Option<Box<dyn MonsterBehavior>>` 字段，`execute_entry`/`check_condition`/`action_display` 中的怪物分支统一调用 trait 方法——不再需要 match。

**收益：** 加新怪物行为从 7-8 个文件 → 1 个新组件 + 1 个 impl。
**代价：** 虚表调用有微小运行时开销，对 ECS 回合制游戏可忽略。

**位置：** `dungeon-action/src/types.rs:20`（ActionKindV3 定义）、`dungeon-action/src/execute.rs` `execute_entry`+`check_condition`、`dungeon-render/src/timeline.rs` `action_display`、`dungeon-world/src/persist.rs` SavedActionKind、`dungeon-action/src/monster.rs` 决策系统、`dungeon-action/src/player.rs` `handle_timed_action`

## 三、实现层面（Implementation）

### 🟡 I24 — Buff/Skill 系统缺陷（含子问题 I24a〜I24c）

**问题：** 当前 Buff 系统和技能机制有三个互相关联的缺陷。ActiveBuffs（I29）修复了缺陷①，但缺陷②③和 I29 引入的回归（G14）仍未解决。

**I24a — Buff 持续时间不可预测 ✅已修复（见 I29）**
`buff_tick_system` 每帧减 1 回合，与 AV 推进脱钩。已由 ActiveBuffs 组件 + AV 同步推进修复。

**I24d — Buff 双倍叠加 ✅已修复（见 G14）**
I29 引入双写双读回归。已移除旧 Buffs 写入路径，`effective_attack/defense` 只读新 ActiveBuffs。

**I24b — 技能数量少且职业锁定 🟡**
技能通过 `PlayerClass::skills()` 硬编码，战士固定 3 技能，无法扩展，每局玩法相同。技能来源是职业而非道具。

**I24c — 无冷却维度 🟡**
技能只有 MP 消耗，没有冷却。强技能无法通过冷却平衡。`ActiveCooldowns` 组件已存在但未被任何技能使用。

**影响：** 当前系统不支持复杂战斗设计。自由组合、道具学习、冷却平衡均不可实现。

**方案方向（设计中，见 DESIGN.md §15）：**
- Buff/冷却改为 `remaining_av: f32`，在 `advance_action_queue` 中同步推进 ✅（由 I29 完成）
- 技能改为从道具学习，`Skills` 组件动态扩展
- 冷却下限约 1000 AV

### 🟡 I22 — clippy 警告约 29 个未处理

**问题：** `cargo clippy` 报告约 29 个警告（已修复 34 个，原 63 个）。

**已修复类型（34 个）：**
`unnecessary_cast`(6)、`useless_format`(2)、`map_identity`(1)、`unnecessary_map_or`(1)、`manual_div_ceil`(1)、`sort_by_key`(2)、`new_without_default`(4)、`derivable_impls`(3)、`unnecessary_mut_passed`(3)、`needless_borrow`(5)、`unused_variables`(1)、`RoomShape` Default(1)、`ActionQueue`/`PlayerPreview` 默认派生(2)、cast usize(2)

**剩余类型：**
| 类型 | 数量 | 说明 |
|------|------|------|
| `collapsible_if` | ~14 | 安全但逐个修复繁琐 |
| `needless_range_loop` | ~6 | 迭代器可读性更佳 |
| `type_complexity` | ~3 | 需要定义 type alias |
| 其他 | ~6 | 零星警告 |

**建议：** 不影响正确性，可逐步清除。

### 🟡 I23 — 测试覆盖缺口：dungeon-core 和 dungeon-render 零单元测试

**问题：** 核心 crate 的单元测试覆盖不均衡。
| crate | 单元测试数 | 覆盖内容 |
|-------|-----------|---------|
| dungeon-core | 0 | ❌ 核心公式（伤害/升级/属性）、FOV、寻路、序列化均无直接测试 |
| dungeon-render | 0 | ❌ UI 渲染逻辑无测试 |
| dungeon-action | 8 | ✅ |
| dungeon-world | 2 | ✅ |
| 场景集成测试 | 3 | ✅ 间接覆盖部分 core 逻辑 |

**风险：** dungeon-core 包含战斗公式、升级曲线、FOV、A* 寻路、Tile/Stats 序列化——任一公式修改都可能无声破坏平衡，无单元测试意味着只能靠手动打游戏验证。

<!-- I35 已移至 ✅已修复（修复前/修复后记录见上方） -->

### 🟢 I38 — `lib.rs` pathfinding 模块注释与事实矛盾

**问题：** `lib.rs` 中 pathfinding 模块声明侧有一条残留注释声称"已移除"：

```rust
pub mod pathfinding;
// pub mod pathfinding; // 已移除（find_path 未使用）
// pub use pathfinding::*; // 已移除
```

但 `pub mod pathfinding;` 是生效的，`execute.rs` 中 `dungeon_core::pathfinding::astar` 也在使用。

**位置：** `dungeon-core/src/lib.rs:7-9`

### 🟢 I39 — `effective_attack/defense` 签名残留废弃 `_buffs` 参数

**问题：** `effective_attack` 和 `effective_defense` 带有 `_buffs: Option<&Buffs>` 参数，前缀下划线表示"不使用"。G14 修复时移除了求和逻辑但保留了参数占位，所有调用方仍在传入 `world.get::<Buffs>(entity)` 做无用查询。

```rust
pub fn effective_attack(
    stats: &Stats, inv: &Inventory, equip: &Equipment,
    _buffs: Option<&Buffs>,           // ← 废弃参数，从不使用
    active_buffs: Option<&ActiveBuffs>,
) -> u32
```

**违反 LESSONS L39：** 新旧系统共存应推进到 Phase 3（移除旧系统引用），当前停留在 Phase 1 未进展。

**位置：** `dungeon-core/src/ops.rs:24-50`；调用点：`execute.rs:293`、`ui.rs:134`

## 四、游戏逻辑层面（Game Logic）

### 🟡 G15 — Buff 持续时长新旧系统差异 60 倍

**问题：** `SkillKind { duration: 3 }` 传入两个系统得到不同时长：

| 系统 | 解读 | 实际时长 | 玩家行动次数 |
|------|------|---------|-------------|
| 旧 `Buffs` | 3 帧（每帧减 1） | ~50ms（60fps） | <1 次 |
| 新 `ActiveBuffs` | 3 秒（3000 AV） | ~3000ms | ~10-12 次 |

这不是同一 Buff 的平行实现，而是 Buff 时长被 **60 倍放大**。GAME.md 已确认语义为"3s（3000 AV）"，旧系统的 3 帧 ≈ 50ms 在 I29 修复前即存在，是独立于双倍叠加的第二个问题。

**影响：** 🟡 中 — 旧系统实际生效时间极短（3 帧≈50ms），玩家几乎感觉不到；新系统 3s 是合理的。移除旧 `Buffs` 后此问题将自然消失。

### 🟢 G16 — 暴击率不随装备变化（面板预期不一致）

**问题：** `execute_attack()` 中暴击判定只用 `attacker_stats.crit_rate`（基础值 5%），不包含装备和 Buff 的暴击加成：

```rust
// execute.rs:302 — 只读基础 stats
let crit_roll = world.resource_mut::<GameRng>().random_f32();
let is_crit = attacker_stats.crit_rate > crit_roll;
```

但 `equipment_bonus()` 中 `StatBonus` 有 `crit_rate: f32` 字段，攻击戒指等物品可定义 `crit_rate` 加成。背包详情页会显示这些加成数据，而实际战斗不生效，给玩家错误的预期。

**影响：** 🟢 低 — 当前无物品带非零 `crit_rate` 加成，但不修复的话将来添加此类物品时会无声失效。

**位置：** `dungeon-action/src/execute.rs:302`、`dungeon-core/src/items.rs`（StatBonus 定义）

### A4L — A4 重构遗漏：Map impl 残留两套重复方法 ✅已修复

**修复前：** A4 将 `collect_walkable_regions` 和 `detect_cave_regions` 复制到 `map_gen.rs` 作为自由函数，但原 impl 方法**未删除**。两套代码完全一致。A4 的统计表显示 Map impl 方法数从 ~18 降到 ~7，但实际应为 ~5。

**修复后：** 两个 impl 方法已删除。所有调用方已走 `map_gen.rs` 自由函数版本。

**教训：** 重构跨文件移动方法后应检查原位置是否仍有残余。

### I26 — arbitration_system 排序比较器违反全序契约 ✅已修复

**修复前：** `arbitration_system` 中同 priority 的实体用 `random_range()` 做 tiebreaker，每次比较产生新随机值，违反 `sort_by` 的全序契约。标准库排序算法在检测到不一致比较时会 panic。下楼至第 3 层时固定触发。

**修复后：** 移除随机 tiebreaker。仲裁器只关心**同实体**的优先级排序（同实体高优先级先入队，低优先级被 `has_entity` 过滤），跨实体同优先级的顺序无意义。直接用 `pb.cmp(pa)` 降序，稳定排序保留插入顺序即可。

**教训：** `sort_by` 的比较器必须是全序（total order）——`a < b` 和 `b < a` 不能同时成立。混入随机数的比较器看似聪明，实际是未定义行为，标准库可能在任意数据分布下 panic。

**位置：** `dungeon-action/src/monster.rs:67-70`

### G9 — 玩家与楼梯重合 ✅已修复（三次）

**修复前：** `pick_stair_pos` 用 `farthest_room_from(spawn)` 取得离出生点最远房间的中心作为楼梯位置。单房间时返回自身。

**第一次修复（I19）：** 尾部加入醉汉游走，检测 rooms.len() ≤ 1。《实际上醉汉游走在 `farthest_room_from` 之后，而该方法对任意非空 rooms 都返回 `Some`，醉汉游走是死代码。》

**第二次修复（G14）：** 增加 `map.rooms.len() > 1` 守卫使醉汉游走可达。但 60 步失败后的兜底 `(spx, spy)`——即出生点本身，仍未解决。

**第三次修复（G14 续）：** 兜底改为螺旋搜索半径 15~40 的最近可行走格，保证不返回出生点。

**教训：** 两条逻辑路径（正常 + 退化）都要确认退化路径的兜底本身是否有 bug。

### G12 — 渲染层叠顺序未定义：怪物与掉落物在同一格时谁在上层不确定 ✅已修复

**修复前：** `collect_renderables` 查询所有 `(Position, Renderable)` 实体并按 ECS 迭代顺序返回，仅对玩家 `@` 做了特殊排序（放最后）。怪物、物品、楼梯在同一格时，哪一层渲染在上方由迭代顺序决定，不可预测。怪物站在物品上时可能被物品盖住。

**修复后：** 收集时增加 Entity 查询，在排序阶段区分实体类型。图层优先级：物品/楼梯 (0) → 怪物 (1) → 玩家 (2)。同层保持原迭代顺序。

**位置：** `dungeon-core/src/ops.rs:150-163`

## 其他

### 🟢 P7 — 玩家确认行动后无法取消（被 D5 锁定）

**问题：** tap-tap 双击确认后行动进入 `ActionQueue` 无法撤回。

**说明：** 事件帧模式（D5，已 defer）可以部分解决此问题——事件帧模式下玩家可以在自己行动执行前切换方向。在 D5 重新评估前此问题无解。

