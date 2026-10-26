> **⚠️ 修改前必须阅读或回忆 [RULE.md](RULE.md)——它定义了本文档的维护规则和更新时机。**

# 发现的问题记录 —— 根目录（跨 crate）

**归属范围：** 跨 crate 协同、工具链与流程、迁移动因；以及不属于任何单一 crate 的问题。

**编号：** `SYN1`、`SYN2`… 每个 crate 独立编号，见 [RULE.md](RULE.md) §七。
根目录只记**跨 crate 协同 / 工具链与流程 / 迁移动因**；单 crate 的问题记在对应 crate 的 ISSUES.md 里。

**优先级：** 🔴 高（影响正确性或游戏体验） / 🟡 中（维护性或功能缺口） / 🟢 低（整洁或边缘情况）

## 待处理

<!-- &SYN2  事件帧模式 Deferred；触发条件是出现需要玩家中途反应的能力 -->
### SYN2 — 事件帧模式（Deferred — 触发条件达成时重新评估）

**问题：** 当前玩家确认行动后批量推进到玩家行动完成，中间所有怪物行动对玩家不可见。

**提议方案：** 增加可切换的"事件帧模式"（按 `s`），每帧只执行一个事件，Enter 步进。

**当前评估：** 暂缓实现。在当前战斗系统（纯数值 chase/flee/wander）下，事件帧模式提供的信息量不足以补偿节奏损失——玩家的最优策略不会因看到每个怪物单步移动而改变。

**触发条件：** 出现**足够复杂的战斗逻辑**，即新增的怪物/boss 有需要玩家在过程中作出反应的能力——例如范围攻击预警、状态效果倒计时、可打断的吟唱、地形变化。当单次 tick 内的行动序列构成决策信息时，事件帧模式从"nice to have"变为"need to have"。
<!-- &/SYN2 -->

---

<!-- &SYN4  行动确认后不可撤回：技术阻塞已随 Phase C 解除，方案待定 -->
### SYN4 — 玩家确认行动后无法取消（阻塞已随 Phase C 消除，方案待重定） 🟡

**问题：** tap-tap 双击确认后，玩家的行动**不可撤回**——按错方向后只能等它执行完。

**原记录：** "行动进入 `ActionQueue` 无法撤回"，并称"事件帧模式（D5，已 defer）可以部分解决，
在 D5 重新评估前此问题无解"。

**本轮核实（该描述已过时，须更正）：**

- `ActionQueue` 与 `ActionKind` **已在 Phase C 删除**（REFACTOR §1（Phase C）），
  行动改为 **action 子实体**（`PlayerActionRequest` → 直接产出一个 active action）；
- 因此**旧的技术阻塞已不存在**：当时不可取消的原因是"行动一旦入队就脱离输入路径"，
  而现在的行动生命周期是**一回合内生成 → 仲裁 → tick → 执行 → completion**
  （`ecs_core/src/action/entity.rs`）。

**当前真实状态：** 玩家路径的语义**保持不变**（确认即在本回合执行完，无撤回窗口），
所以**对玩家而言症状依旧**；但"无解"的判断已失效——可撤回性现在是一个**设计选择**，
而不是被数据结构锁死的必然。

**待你决定的分叉（本次不改代码）：**

1. **保留现状**（确认即执行）：回合制语义最简单，缓存/回放无歧义；
2. **引入"行动确认窗口"**：确认后不立即 tick，允许在校验通过前用反方向键撤销——
   代价是玩家可见的节奏变化，且需要定义"撤销是否消耗 AV / RNG 步数"（涉及
   `GameRng.steps` 与回放一致性，见 DESIGN DsnX17）；
3. **复活事件帧模式**（原 D5 方案）：触发条件见 SYN2，目前仍未达成，暂不建议。

**位置：** `ecs_core/src/action/entity.rs`（玩家生成与挂载路径）、`presentation/src/input`（tap-tap 口径）

**关联：** SYN2（事件帧模式，仍 Deferred）；DESIGN DsnE8（挂载与推进两段式）。

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
| SYN1 | 测试覆盖缺口（部分）（旧架构侧缺口随 R5 归档不再跟踪） | `ecs_core/`、`presentation/`、`render-api/` | SYN5 / LECS22 |
| SYN3 | terrain-forge submodule 配置缺失：无 .gitmodules + 163 个未提交变更 | `.gitmodules`、`terrain-forge/` |  |
| SYN5 | 新 `core` 冒烟测试缺口：地图/移动/战斗/成长/视野/AI 无回归保障 | `core/src/`（`world/init.rs`、`world/loop_.rs` | SYN1 / SYN6 |
| SYN6 | `core` doctest 因 crate 名 `core` 与标准库冲突失败（根因已消除：F5 改名 `ecs_core`） | `core/src/resources.rs:65` | DsnX15 |
| SYN7 | 根集成测试失效，`cargo test -p dungeon-app` 无法编译 | `tests/`、`archive/legacy-tests/`、`Cargo.toml` |  |
| SYN8 | Gm9 投掷伤害表与自身公式/代码不符 | `GAME.md:471-475` |  |
| SYN9 | SL 刷掉落漏洞：RNG 状态不持久化，读档重放随机序列 | `dungeon-core/src/resources.rs`（GameRng）、`dungeon-action/src/execute.rs`（handle_kill）、`dungeon-world/src/persist.rs` |  |
| SYN10 | RULE.md 编辑流程优化（移除宣誓 + 强化记录优先） | `RULE.md` §六 |  |
| SYN11 | 主循环空闲 sleep 1ms 导致有限机型 CPU 满载 | `src/main.rs:88` |  |
| SYN12 | 全部 `.unwrap()` 替换为 `.expect()` | — |  |
| SYN13 | 斜向键无 OS key-repeat（Won't Fix — 终端环境限制） | — |  |
<!-- &/SYN4 -->

