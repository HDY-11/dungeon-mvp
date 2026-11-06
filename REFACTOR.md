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
| M2 | 构造期随机源收敛为单入口（`DerivePurpose` + `derived_rng`），"第二入口"审计与共享状态写入者清单（DsnE15） | DsnE5 / DsnE15 / ECS34 | 本次提交 |
| M1 | 移动的判定与落地分阶段：`ActionPhase::Validate`（每行动一个校验器，定案 + **预留**目标格）+ **一个**落地系统 `settle_action_system`（唯一写 `Position`，`action/settle.rs`）；六个执行器 → 一个；占格改用 `Blocks` 能力（ECS45） | DsnE14 / DsnE2 / LECS23 / LECS24 | 本次提交 |
| M3 | 保活覆盖**所有**待执行行动：校验器过滤器放开到 `PendingAction<A>`，失效即刻取消；定案/预留仍只对到点行动（ECS43，用户拍板"立即取消"） | DsnE8 / DsnE14 / ECS43 | 本次提交 |
| M4 | **物品系统规则层**：静态注册表（21 行）+ 堆叠背包 + 四个装备槽 + 怪物掉落表（8 张）+ 消耗品 + 地面刷新；修正器补 `Stat` 维度，装备加成进伤害链路与攻击耗时 | DsnE16 / DsnE10 / DsnE12 | 本次提交 |
| M6 | **容器化**（DsnE18 第①片）：物品=实体（Item+Amount）；**身体=BODY 容器**（七部位）、行囊是塞在「背包」槽里的一件物品；EquipSlot/EquippedBy 退役，装备=身体槽位里的物品 | DsnE18 / DsnE16 | 本次提交 |
| M7 | **容器化第②片**：尸体=容器（怪物模板 `equipment` 进尸体、踩上自动搜刮，战利品仍落地）；背包页两栏渲染/详情面板/装-卸-用-丢四动作 + 键盘端到端 | DsnE18 / Gm7 | 本次提交 |
| M8 | **存档 V1**：bincode 手写投影（文件头 magic+version）、F5 存/F9 读、读档复用 `rebuild_floor`；地面/怪物/随机流不入档 | DsnE19 / DsnX10 | 本次提交 |
| M5 | **楼层推进（下楼）**：`PlayerCommand::Descend` + `world/floor.rs`（白名单清场 + 复用初始化系统重建层内容 + 派生索引重建）；换层不是行动 | DsnE17 / Gm12 | 本次提交 |
| H11 | 效果实体三落点（`EffectOf` 级联 / `EquippedBy` 不级联 / 每格一实体）+ **最小闭环一对**：格子减速 × `IgnoresSource(Terrain)` | DsnE12、ECS28、ECS42 | 与 H9 同一提交 |
| D1 | 确定性修补：仲裁的授予/回收顺序显式排序（`ordered_grants` + `losers.sort`）；四个 Schedule 改经 `schedule::core_schedule()` 构建，`ambiguity_detection` 提到 `Error` | DsnX17 ③ | 本次提交 |
| M9 | **四栏界面与焦点**：布局几何进契约（`render-api::layout`：`Rect`/`Pane`/`GameLayout`/`game_layout`，右栏 42%、顶行 60%、最小 80×24 降级）；`SceneFrame.focus` 由 `PageStack` 每帧供给；`Tab` 顺时针轮转、tui 四栏渲染 | DsnA2 / Gm14 | 本次提交 |
| M10 | **事件时间轴**：`LogLine` → `EventLine{level,text,running}`（契约 4）；`EventLog` 容量 200 只计事实；进行中行动由 `ActiveAction+ActionTimer` 投影（`ActionName::phrase`），Done 旧→新 + Running 剩余降序、栏底 0 刻度 | DsnE21 / DsnA3 / Gm14 | 本次提交 |
| M11 | **信息栏观察候选**：`Observable` 显式标记（怪物/楼梯/地面物品/尸体，容器内容不标）；信息栏契约 `InfoView{entries,selected}`（契约 5），presentation 按「近→远、同距方位顺时针（正上=0）」排序；`PageStack.info_cursor` 选行、地图反色高亮 | DsnE21 / DsnA4 / Gm14 | 本次提交 |
| M12 | **地面全显式拾取**：`pickup_system` 退出结算链（踩上不再自动捡）；新核心动作 `take_from_ground(world, index)`（空部位装备 / 消耗品按需用 1 / 其余进行囊，超重与满格留原地）；背包页「地面」栏 + `InventoryTarget::Ground` + `Enter` 拿取；playthrough 改为显式拾取 | DsnE21 / DsnE20 / Gm7 | 本次提交 |
| M13 | **背包 3×3 网格**：契约 `InventoryView` 换成 Nav1/Nav2/Context1/Context2/Detail/Tip（契约 6，DsnA5）；Nav1=身体/行囊、Nav2=尸体/地面（按脚下情况出现）；4 焦点顺时针（Nav2 不存在时跳过）、`←/→` 切页签、`↑/↓` 选行；每条 Context 保留光标；tui 网格渲染（详情先右侧后底部、窄屏 Context 堆叠）| DsnE21 / DsnA5 / Gm7 | 本次提交 |
| M14 | **物品数据外置（DsnE22 阶段 ①）**：物品表搬到 `ecs_core/assets/items.json`（`schema_version` 1，`include_str!` 内嵌 + `LazyLock`），加载器逐行校验（编号顺序/名字/重量/堆叠/攻速/消耗品数值/Pack 布局/未知字段）；`Stat`/`EffectSource`/`Modifier`/`ItemTag`/`ConsumeEffect` 加 serde；类型形状与行为零变化 | DsnE22 / DsnE20 / Gm7 | 本次提交 |
| M15 | **属性词表 + 修正算子（DsnE22 阶段 ②a）**：`Stat` 枚举退役 → `rules/stat.rs` 的 `StatId(u16)` + `STATS` 词表（数据里按名字序列化，未知名字加载期报错）；`Modifier` 改为「一条修正一个算子」（`Add` / `MultiplyBase` / `MultiplyTotal` + `value`），折叠 `(base+ΣAdd)×ΠMulBase×ΠMulTotal` 与旧口径逐值等价；物品数据 schema 升 2，加载器拒绝未知属性/非有限值/非正乘区因子 | DsnE22 / DsnE10 | 本次提交 |

**当前基线（实测）：** `cargo test -p ecs_core` **258 passed**；`cargo test --workspace`
29 目标 **536 passed / 0 failed / 1 ignored**；`scripts/gate.ps1` 10 步全绿。

**H9/H11 的三个开放项已全部定案**（DsnE12「开放项」表）：装备落点 → 两个关系类型分开
级联语义（`EffectOf` 级联 / `EquippedBy` 不级联）；格子效果粒度 → 每格实体 + 格索引；
来源桶集合 → 维持 `Intrinsic`/`Equipment`/`Status`/`Terrain` 四项不拆。

---

## 2. 未完成

### 2.1 Phase H 剩余（只碰形状，不填值）

| # | 任务 | 位置 | 约束 |
|---|---|---|---|
| H10 | 技能三层骨架（激活/委派/行为）+ **一个 dummy 技能** | 新模块 | **关键验收**：加 dummy 只改"新文件 + `ActionPhase::Validate` 一行 + `ActionPhase::Execute` 一行"（M1 之后行动 = 生成器 + 校验器，落地是通用的）；超过则先修扩展点 |
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
| SYN4 | 玩家确认行动后能否撤回（可选：保留现状 / 加"确认窗口" / 复活事件帧模式） | 三条路都要改玩家可见节奏；"撤销是否消耗 AV 与随机步数"会影响回放一致性（DsnX17）。技术阻塞已随 Phase C 消失，**判定权在用户**，2026-09 明确"暂不决定" |

> **ECS43 已于 2026-09 由用户拍板并落地**（保活覆盖所有待执行行动、失效即刻取消）——
> 见 `ecs_core/ISSUES.md` 已修复表与 DESIGN DsnE14 代价第 6 条。

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
| C2 | 泥潭格（可走、代价×2，洒在 F1 地板） | `CellEffects` / `CellMoveCost`（**0 用户**） | ✅ 已落地 |
| C3 | 蝎子攻击附加中毒（每回合掉血、3 回合消失） | `EffectOf` + **第一个效果 owner 系统** | ✅ 已落地 |
| C4 | 踏水靴：地面物品 + 走过去拾取 + 立即装备 | `EquippedBy`（**0 用户**）+ 装备→效果翻译 | ✅ 已落地 |
| C5 | 浅水有移动代价（`TILE_PROPS.move_cost` 首次非 1.0，×1.5） | H9 接线 + `Terrain` 桶 | ✅ 已落地（数值 [⃞试调]） |

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
