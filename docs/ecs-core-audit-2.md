# ecs_core 第二次问题审计（2026-11-13）

> 触发：用户 m13959 指出上一版迁移方案不可靠，并点名若干处死代码/结构问题，要求重新查。
> 基线：HEAD `f9934b7`（tag `pre-ecs-cleanup`），`cargo test --workspace` 669 passed / 1 ignored。

## 0. 结论速览

- 用户点名 7 项：**确认属实 5 项、部分属实 2 项**（`action/execution` 并非"只为观测"；`entity.rs` 实际 **1408 行**，不是 1145）。
- 另发现 **11 处死代码/未接线残留**、**6 类结构性风险**。
- 其中无争议死代码已在本次 P0 清除（行为零变化，`ecs_core` 374 passed、workspace 30 组全绿）。
- 原迁移方案的原则按用户 m13959 修订：**取消"行为冻结优先"**，改为"整理阶段默认不改行为，若改先报告"；"不为拆分而拆分"改为**可检验的判据表**（见 §4）。

## 1. 审计方法与判据

- 引用计数：遍历全仓 `*.rs`（排除 `archive/` 与 `.git`），统计每个 `pub` 项在**定义文件之外**的生产代码引用数；再人工核对同名旧实现。
- 行数：`Get-ChildItem ecs_core/src -Recurse -Filter *.rs` 逐文件统计。
- 存活性 grep：`Select-String -Pattern '\bNAME\b'`，区分生产 / 测试 / 注释 / archive。
- 判据（本轮起生效，替代口号式原则）：
  - **死代码**：生产路径 0 引用（注释、archive 不算）。
  - **模块成立性**：目录必须承载 ≥1 个被 ≥2 处调用的非平凡导出，或一个独立流水线阶段；纯 re-export 壳、单函数 mod 不成立。
  - **拆分**：文件 >800 行且含 ≥2 类职责/阶段；测试文件 >800 行且可按主题分组。
  - **行为变更**：内容未上线、版本未稳定，**不强制冻结**；任何行为变更在汇报中单列。

## 2. 用户点名问题核实

| # | 用户判断 | 结论 | 证据 |
|---|---|---|---|
| 1 | `action/generation/ai.rs` 四个函数无引用 | ✅ 成立 | `chase_condition` / `flee_condition` / `wants_to_flee` / `player_visible_to` 生产引用 0；仅 archive 旧 crate 有同名函数 |
| 2 | `action/generation` 已不用但未删/整理 | ◐ 部分成立 | `ai.rs` 死；`player.rs` 的 `PlayerCommand` 仍被 4 处使用（`entity.rs`、`loop_.rs`、`entity_tests`、`effect_tests`）→ 删 ai，保留 player 桥 |
| 3 | `action/execution` 意义不明、只为观测 | ◐ 部分不成立 | `plan_step` 是唯一移动判定（`validation.rs:77/145`）；`moved_position` 被 `entity.rs:708` 与 `examples/playthrough.rs:497` 使用；只有 `can_move_to` 生产引用 0（测试/文档引用） |
| 4 | `action/entity.rs` 1145 行 | ✅ 成立（数字过时） | 实际 **1408 行**：4 类 AI 生成系统 + 玩家生成系统 + 仲裁 + tick + completion + 调度 + 组件定义 |
| 5 | 仲裁可用 EntityHashMap/EntityHashSet/Vec | ✅ 成立 | 现用 `std::collections::{HashMap, HashSet}`；bevy 0.20.1 已导出 `bevy_ecs::entity::{EntityHashMap, EntityHashSet}` |
| 6 | settle 每加行动要手加分支、易忘 | ✅ 成立 | `ReadyDecision` 是 4 个 `Option<&Target>` 元组 + 4 段 `if let`；`CastSpell`/`CastTarget` 配对靠手工检查；无编译期强制 |
| 7 | `combat` 单函数开 mod 不值得 | ✅ 成立 | 目录仅 23 行、1 个 `can_attack_positions`，只有 `validation.rs` 使用 |

## 3. 新发现

### 3.1 死代码（P0 已清）

| 项 | 位置 | 证据 |
|---|---|---|
| `ai.rs` 全文件 | `action/generation/ai.rs` | 4 个函数 0 生产引用 |
| `combat` 模块 | `ecs_core/src/combat/mod.rs` | 1 函数，仅 validation 使用 → 已并入 `validation.rs` 为私有函数 |
| `main_hand_slot_index` | `item/mod.rs:369` | 全仓 0 引用（副手版仍在用） |
| `pack_item_id` | `item/actions.rs:619` | 0 引用 |
| `off_hand_throw_damage` | `item/actions.rs:682` + 导出 | 无调用方（`off_hand_throwable` 仍用） |
| `THROWN_FLOOR_BONUS_STEP` | `balance.rs:142` | 0 引用，只在注释里被提到 |
| `tile_at` | `effect/cell.rs:279` + 导出 | 0 引用（调用方直接读 `map.tiles`） |
| `can_move_to` | `action/execution/movement.rs:120` | 生产 0 引用；只被自身测试与文档引用 |
| 过时文档引用 | `entity.rs:951` 提到不存在的 `execution::positive_timer_delta` | — |
| 过时文档引用 | `rules/mod.rs:4` 指向已删的 `crate::combat` | — |

### 3.2 未接线但保留（登记，不是死代码）

- `map/layer.rs`：`Layer` / `BlockCoord` / `BlockInfo`（S1a 骨架），目前只被自身单测使用。
  - 处置：标记 `scaffold`；若 S1b/S2 在内容裁定后仍未接线，用判据复核后删除（避免"预留一年"）。

### 3.3 结构性风险（未在 P0 处理）

1. **新增行动要改 4 处**：生成器 → 校验器（+`Decision`/`apply_decision`/目标组件）→ `settle` 分支 → `schedule` 校验链；`ActionName::phrase` 还有 `_ => "行动"` 静默兜底。缺少编译期强制。
2. **事件缓冲清单手工同步**：`CoreEventBuffers` 9 个字段 + `update_events_system` 逐个 update；新增事件忘了 update 会跨轮残留，只有注释在守。
3. **`lib.rs` 18 个 `pub use <mod>::*`**：公共 API 无边界、`dead_code` 对 pub 项失效（本轮死代码正是因此长期不可见）、名字冲突风险。
4. **大文件**：`entity_tests.rs` 2659、`entity.rs` 1408、`registry.rs` 919、`validation_tests.rs` 888、`item_tests.rs` 770、`registry_tests.rs` 736、`map_gen.rs` 688、`init.rs` 643、`validation.rs` 633、`template.rs` 592。
5. **archive 四个 crate 仍在 workspace members**（各自钉 bevy 0.16）：每次 workspace 构建都要编译，且与主线版本分叉；建议移出 members（目录保留作参考）。
6. **注释/dossier 漂移**：`movement.rs` 与 `action/mod.rs` 各有一大段"为什么规则纯/唯一判定"的同义叙事；`entity.rs` 顶部阶段图与 `action/mod.rs` 重复。整理时合并到一处。

### 3.4 明确"不是问题"（避免过度清理）

- `action/execution` 的 `plan_step` 不是观测用途：删它会把"唯一移动判定"拆散，必须保留（只拍平目录、删 `can_move_to`）。
- `archive/` 里的同名函数不是本轮死代码（不参与编译的参考副本，除 workspace 成员问题外）。
- `components.rs` 与 `action/entity.rs` 都放组件：前者是 actor 状态/计时，后者是 action 实体专属；边界合理，不合并。

## 4. 修订后的处理顺序与判据

### P0（本次已完成）

删除 §3.1 死代码 + `combat` 模块并入 `validation` + 过时文档引用。行为零变化。
验证：`ecs_core` 374 passed；`cargo test --workspace` 30 组全绿；`cargo clippy -p ecs_core --all-targets -D warnings` 通过。

### P1（约 1–1.5 天，行为零变化或可忽略）

- a) 目录拍平：`action/execution/movement.rs` → `action/movement.rs`（删 `execution/`）；`action/generation/player.rs` → `action/player_command.rs`（删 `generation/`）；删 `can_move_to`；更新引用（`validation.rs`、`entity.rs`、`playthrough.rs`、`test_util` 文档）。
- b) `action/entity.rs` 按职责拆 5 片：`generation/ai.rs`（4 生成系统）、`generation/player.rs`（玩家生成）、`arbitration.rs`（优先级+仲裁+tick）、`completion.rs`（完成+调度）、`entity.rs`（组件+生命周期，≤400 行）。
- c) 仲裁数据结构换 `EntityHashMap` / `EntityHashSet`；确定性排序键保持 `index_u32`。
- d) `Landing` 枚举替代 4 个 target 组件：`Wait | Move(Position) | Attack(Entity) | Cast { spell, target } | Throw { landing }`；`settle_action_system` 一次穷尽 match（新增变体编译失败）；同步 validation/tests。
- e) `ActionName::phrase` 去掉 `_` 兜底（改为穷尽 match 或返回 `String`）；事件缓冲加一个"事件类型清单 vs 缓冲字段"的自检测试。

退出条件：`cargo test --workspace` 组数不减、无新增 ignored；`entity.rs` ≤400 行；`settle.rs` 只依赖 `Landing`。

### P2（约 2–3 天，结构/性能）

- `lib.rs` 显式 re-export 替代 18 个 glob（或至少对 action/item 两大块），恢复 `dead_code` 发现能力。
- archive 四个 crate 移出 workspace members。
- `registry.rs` 拆 DTO/加载器与访问器；`init.rs` 按生成阶段拆。
- 大测试文件按阶段拆（不做逻辑改动）。
- 测试 fixture：`test_util` 改 `test-support` feature 或独立 crate，供根 tests 复用。

### P3（功能/内容切片，按新原则不再等"行为冻结"）

原 M3–M8 重排：效果/on-hit → 层—区块地图 → 时间表 → 存档 V6 → 品质/气力/闪避 → UI 焦点。
每次行为变更在汇报中单列（数值、受影响命令、测试）。

## 5. 对上一版迁移方案（f9934b7）的修订

- 原则："行为冻结优先" → **删除**；替换为"整理阶段默认不改变行为；若改变，先报告"。
- 原则："不为拆分而拆分" → 保留为**判据表**（§1），不再作为无条件口号。
- M1「内容边界」降级：内容仍在变，先不出 crate；模块边界可在 P2 视判据做。
- M2 拆为 P1 a/b/c/d；M3–M8 并入 P3。
- M0 基线保留有效（tag `pre-ecs-cleanup`）。

## 6. 复现命令（摘要）

```powershell
# 引用计数（生产=排除定义文件/archive/_tests.rs）
# 脚本见本次会话 command；核心逻辑：逐 name 全仓 count - 定义数

# 死链检查
Select-String -Path (Get-ChildItem -Recurse -Filter *.rs ecs_core,src,presentation,tui,examples,tests) -Pattern '\bflee_condition\b|\bmain_hand_slot_index\b|\bTHROWN_FLOOR_BONUS_STEP\b'

# 行数 Top20
Get-ChildItem ecs_core/src -Recurse -Filter *.rs | ForEach-Object { "$((Get-Content $_).Count)`t$($_.FullName)" } | Sort-Object { [int]($_ -split "`t")[0] } -Descending | Select-Object -First 20
```
