# REFACTOR.md — refactor 阶段的进度板（只放"还没做完的"）

> **定位：** 本文件记录 **refactor 阶段**（纯架构改进）的进度板，不是设计文档、不是契约文档。
> 设计决策在 [DESIGN.md](DESIGN.md)、游戏数值在 [GAME.md](GAME.md)、未闭合问题在
> [ISSUES.md](ISSUES.md) / 各 crate 的 `ISSUES.md`。
>
> **纪律：** 一个阶段做完，就把它的**结论**折进对应的 DESIGN 条目，把**遗留**折进 ISSUES，
> 然后从本文件删掉过程记录——只留一行"已完成（commit X）"。
> 按 RULE §一，本阶段结束后本文件应瘦到只剩"未完成项"，随后整体删除。
>
> **阶段边界（2026-09）：** `refactor` 已于 tag `arch-phase-h` 处 fast-forward 并入 `main`，
> 之后 `main` 上同时有架构与内容两类提交，用**提交前缀**区分：
> `refactor:` 架构改动 / `content:` 内容新增 / `design:` 换道 / `docs:` 文档 / `fix:` 修 bug。

---

## 1. 已完成（一行一条，细节在 DESIGN / commit）

| 阶段 | 内容 | 结论落在哪 | 提交 |
|---|---|---|---|
| A | `ecs_core` 冒烟测试基线（地图确定性 / 移动 / 攻击只结算一次 / 死亡→经验→升级 / FOV·记忆·占用图 / AV 门禁） | 测试本身 | `95449a9` |
| B | action 实体 PoC（生成 → 仲裁 → tick → 执行 → completion） | DsnE8 | `ac6623f` |
| C | 六行动全量迁移，删除 `ActionKind` 中央分派 | DsnE8 | `83700ff`…`a8e2175` |
| D | `Agility` → `MoveSpeed`/`AttackSpeed` 倍率口径（删反应时） | DsnE8 ②、Gm1 | `cff5940`… |
| E | 死抽象清理（身份 ZST / `EntityClass` / `CreatureKind` / `BeAttacked` / `PendingExp` / 5 个死函数） | ISSUES ECS25 | `7f9e206` |
| F | 门禁：`sys` 独立构建、旧集成测试归档、clippy 清零、`scripts/gate.ps1`、crate 改名 `ecs_core` | PROTOCOLS §五、DsnX15 | `7d5b8e1`…`ca5e81c`、`1bb6bd8` |
| G-R1 | 渲染三层落地：`presentation` 抽取、`tui` 去 `ecs_core` 依赖、边界由 `cargo tree` 强制 | DsnX14 | `a1d47be`…`a958ed3` |
| G-R5 | 旧 `dungeon-*` 归档到 `archive/`（仍可编译可测），孤儿文件删除 | `archive/README.md` | `5dcd6c4` |
| H1 | 行动终态单一出口（`ActionEvents::succeed/fail`，字段私有化） | DsnE8、ECS35/36 | `f1f6898` |
| H2 | 伤害结构化输入 + 因子分解（`rules/damage.rs`） | DsnE9 | `389e460` |
| H3 | 调度按角色分组（`ActionPhase`） | DsnE8 | `5a9ea90` |
| H4 | `TileProps` 静态属性表（`map/tile.rs`，加地形从改 6 处到 2 处） | DsnE13、ECS31 | `badfe1d` |
| H6 | 物种能力归位：`abilities: &'static [Ability]` 进模板，spawn 代码不再列举物种名；删掉恒 0 的 `Magic` 占位 | DsnE6、ECS32 | 与 H7 同一提交 |
| H7 | 生成权重归位：`spawn: &'static [SpawnWeightFor]` 进模板，`spawn.rs` 三层 `match` → 一次查表 | DsnE6、ECS32 | 与 H6 同一提交 |
| H8 | 规则修正器求值侧（`rules/modifier.rs`：来源桶 / 两类位 / 过滤 / 折叠） | DsnE10 | `389e460` |
| H9 | 格子属性统一读取入口（`effect::cell_attributes`）+ 格子效果的格索引（`CellEffects`，照 `OccupancyMap` 每轮重建）+ `move_cost` 接线 | DsnE13、ECS27 | 与 H11 同一提交 |
| H11 | 效果实体三落点（`EffectOf` 级联 / `EquippedBy` 不级联 / 每格一实体）+ **最小闭环一对**：格子减速 × `IgnoresSource(Terrain)` | DsnE12、ECS28、ECS42 | 与 H9 同一提交 |
| D1 | 确定性修补：仲裁的授予/回收顺序显式排序（`ordered_grants` + `losers.sort`）；四个 Schedule 改经 `schedule::core_schedule()` 构建，`ambiguity_detection` 提到 `Error` | DsnX17 ③ | 本次提交 |

**当前基线（实测）：** `cargo test -p ecs_core` **127 passed**；`cargo test --workspace`
28 目标 **351 passed / 0 failed / 1 ignored**；`scripts/gate.ps1` 10 步全绿。

**H9/H11 的三个开放项已全部定案**（DsnE12「开放项」表）：装备落点 → 两个关系类型分开
级联语义（`EffectOf` 级联 / `EquippedBy` 不级联）；格子效果粒度 → 每格实体 + 格索引；
来源桶集合 → 维持 `Intrinsic`/`Equipment`/`Status`/`Terrain` 四项不拆。

---

## 2. 未完成

### 2.1 Phase H 剩余（只碰形状，不填值）

| # | 任务 | 位置 | 约束 |
|---|---|---|---|
| H10 | 技能三层骨架（激活/委派/行为）+ **一个 dummy 技能** | 新模块 | **关键验收**：加 dummy 只改"新文件 + `ActionPhase::Execute` 一行"；超过则先修扩展点 |
| H12 / H13 / H14 | 判据成文（`LECS22`）/ 两类测试纪律 / 格子身份判据（`DsnE13`） | 文档 | 随上面几项收尾时一起落地 |

### 2.2 渲染后端（DsnX14 的 R2–R4）

| 阶段 | 内容 | 前置 |
|---|---|---|
| R2 | 页栈 UI：`Inventory` / `Throw` 需要物品与投掷规则迁移（当前给占位页） | 物品系统迁移（S4） |
| R3 | `bevy_app` 宿主 + `ScheduleRunner` 替换主循环 | 无（网络已可用；原"环境无外网"的阻塞已消失） |
| R4 | GPU 后端（消费同一 `SceneFrame`，`ecs_core` 零改动） | R3 |

### 2.3 行为类候选（不属于"只碰形状"，需用户确认）

| # | 内容 | 为什么要确认 |
|---|---|---|
| ECS43 | 保活检查从"执行前各写一份"改成"每回合对所有待执行行动扫一遍"（新增 `ActionPhase::Validate`，与执行器对称） | **行为改动**：提前取消会改变 `GameRng` 消耗时序与行动顺序，六行动 parity 套件要跟着改；按 RULE §八 ③ 判定权在用户。前置（`ActionPhase`、终态单一出口）已备好 |

### 2.4 旧架构归位的取舍（已定）

`archive/` 里的 4 个 `dungeon-*` **仍保留为 workspace members**（可编译可测），
因为它们还是"物品/技能/投掷/存档"这些未迁移子系统的**唯一可运行参考**。
判定见 `archive/README.md`：迁移完成或旧依赖拖累升级时再整体删除。

---

## 3. 内容 spike（C1–C5）：用内容去撞扩展点

> **为什么记在这里：** refactor 阶段的验收是"不变式没破"，内容阶段的验收是
> "**扩展点够不够用**"。后者可量化：每条内容实际改了**几个文件、几行、有没有撞上
> 要改中央分派的地方**（`LECS22` 的实测版）。本文件退场时，这张表折进 GAME.md。

**约束：F1 只看得见 3 种怪（鼠/蝎/哥布林）+ 5 种地形**（其余属繁茂/地海，需下楼）。
内容必须落在这个可见域内，否则加了等于没加。

| # | 内容 | 撞哪个扩展点（此前用户数） | 状态 |
|---|---|---|---|
| C1 | 三只怪能力分化：鼠=猎物、蝎子=不知恐惧、哥布林=完整 | `Ability` 列表（8/8 完全相同） | ✅ 已落地 |
| C2 | 泥潭格（可走、代价×2，洒在 F1 地板） | `CellEffects` / `CellMoveCost`（**0 用户**） | ⬜ |
| C3 | 蝎子攻击附加中毒（每回合掉血、N 回合消失） | `EffectOf` + **第一个效果 owner 系统** | ⬜ |
| C4 | 一件装备 + 地面物品 + 走过去拾取 | `EquippedBy`（**0 用户**）+ `Equipment` 桶 | ⬜ |
| C5 | 浅水有移动代价（`TILE_PROPS.move_cost` 首次非 1.0） | H9 接线 + `Terrain` 桶 | ⬜ **需先定数值（GAME.md）** |

**stop rule：** 任何一条内容若需要改 **≥3 处**、或需要动**中央 match/分派**，
立刻停下——那是扩展点的问题，先修扩展点再回来。

---

## 4. 折进 DESIGN 的内容（本文件不再保留）

2026-09 文档治理时，原 `REFACTOR.md` 里**属于契约**的部分已折进
[DESIGN DsnX17](DESIGN.md)（冻结清单 / serde 只追加 / 确定性 / 显示数据归属 /
无存档承诺 / 瞬态子实体不存档）；**属于设计**的部分已在 DsnE8 / DsnE9–DsnE13 / DsnX14；
**属于问题**的部分已在各处 ISSUES。过程记录见 `git log`。

被删内容的完整原文保存在 `history` 分支与 `docs-pre-governance` tag
（见 `archive/docs-trash-manifest.md`）。
