# ecs_core 拆分研究：内容 / 纯规则 / ECS 运行时

> 状态：研究结论，未实施。日期 2026-11-12。
> 关联：DsnE22（属性词表与数据化）、DsnE27（部件化 + 组件真源）、DsnE30（层—区块）。

## 0. 结论（先给答案）

值得把「内容数据 / 纯规则 / ECS 运行时」的边界做成**可执行的约束**，但**现在不建议整包拆成新 crate**。分三步：

| 步骤 | 做什么 | 何时 | 估计 |
|---|---|---|---|
| Step 1 | 在 `ecs_core` 内新增零 bevy 的 `content/` 模块，纯数据定义搬家，`pub use` 保持调用方不变，加守卫测试 | 现在 | 0.5–1 天 |
| Step 2 | 抽 `dungeon-rules` crate（stat / modifier / damage / thrown / balance + vitals 公式），支撑离线模拟 | 出现批量模拟需求时 | 2–3 天 |
| Step 3 | 抽 `dungeon-content` crate（数据定义 + JSON 加载校验 + validator CLI） | 内容 v1.2 schema 稳定、怪物/地块/布局数据化之后 | 4–6 天 |

不做 Step 3 的原因：内容仍在宽泛迭代（用户 2026-11-12 判断「内容并不算完善」），现在整包搬家会把未定 schema 固化；而且当前有 5 个混合文件，纯搬家必须先改数据形状，等于借重构之名做内容定稿。

## 1. 现状度量

`ecs_core/src` 非测试源码约 **12.4k 行**，其中：

- **pure（无 `bevy_ecs`）约 3.8k 行（30%）**：`rules/{stat,modifier,damage,thrown}`、`balance`、`item/{registry,quality,loot}`、`map/{tile,layer,map_gen}`、`spatial/*`、`action/{mod,generation/player,execution/movement}`。
- **依赖 bevy 约 8.6k 行（70%）**：`components/resources/events`、`action/{entity,validation,settle}`、`effect/*`、`item/{mod,container,actions}`、`monster/*`、`spell`、`system/*`、`world/*`。
- `bevy_ecs` 直接引用约 **105 处、36 个文件**；另有 8.7k 行测试（多数贴近运行时，移动成本高）。

### 混合文件（拆分的关键阻塞）

| 文件 | 混了什么 | 影响 |
|---|---|---|
| `item/container.rs` | `ItemTag/SlotAccept/SlotDef/ContainerDef`（纯数据） + `Container`（组件） | registry 依赖了混合文件，content 无法直接抽走 |
| `monster/template.rs` | `MonsterTemplate`（数据表） + `MonsterKindId: Component` + `MonsterStats.stats: Stats` | 同文件里既有内容卡又有运行时类型 |
| `spell/mod.rs` | `SpellDef` 注册表（数据） + 施法系统 | 一个文件两种职责 |
| `rules/vitals.rs` | hp/mp 曲线公式（纯） + `refresh_vitals_system` | 公式无法单独给离线模拟用 |
| `map/mod.rs` | `Map` 类型 + `Resource` | 类型与资源包装相邻，改动面小 |

依赖方向本身是干净的：运行时模块单向依赖 pure 模块；问题是 pure 数据没有自己的模块边界。

## 2. 三个概念必须分开

| 概念 | 例子 | 约束 |
|---|---|---|
| 内容数据 | 物品蓝图、法术表、怪物卡、地块属性、容器布局、平衡常数 | 可序列化、可校验、**不 import bevy** |
| 纯规则 | 伤害/抗性折叠、成长曲线、AV 折算、攻击间隔 | 无 ECS、可离线跑模拟 |
| ECS 运行时 | 组件、系统、关系、调度、世界资源 | 依赖 bevy_ecs |

当前真正的问题不是「内容与结构混在 `ecs_core`」——那本来就是模拟内核的职责；而是**内容数据与运行时类型混在同一文件/同一 struct**，导致「注册表看起来是纯的，其实一挪就断裂」。

## 3. 收益评估（区分能兑现和兑现不了的）

| 收益 | 价值 | 说明 |
|---|---|---|
| 边界可执行 | 高 | 用守卫测试禁止 `content/` 出现 `bevy_ecs`，防止怪物卡里再长出组件 |
| 离线模拟/平衡工具 | 高 | TIME v2 已经靠手工/临时脚本做了一轮，抽 rules 后可直接复用 `melee/thrown/AV` 公式 |
| 内容独立校验 CLI | 中 | 现在只有启动 panic；独立 validator 才能在内容编辑期跑 |
| 编译隔离 | 低 | `ecs_core` 增量编译本来就 10s 内；content 一变仍要重编 runtime |
| 内容 schema 独立迭代 | 低/负 | 内容未定稿，过早冻结 schema 反而添乱 |
| 减少认知负担 | 中 | 「这个文件能不能碰 bevy」有明确答案 |

## 4. 成本与风险

- **Step 1**：搬 5 个文件/类型 + 改 import + 测试跟随；不加 crate，不动 workspace/gate，风险低。原 `Container`/`MonsterKindId` 等运行时类型留在原处，content 只收纯数据。
- **Step 2**：新增 crate 要动 `Cargo.toml`、gate 的 clippy 列表、README/REFACTOR 交叉引用；用 `ecs_core` 重导出保持调用方零改动；风险中。
- **Step 3**：前置是怪物模板/地块/容器布局/平衡常数的数据化（本身就是内容工作），这是最大成本，也是不应现在做的原因；风险中高。
- 共通风险：拆分瞬间出现「两处真源」（类型重复定义）；必须坚持单点定义 + re-export，禁止两边各留一份。

## 5. 判据：什么时候进入下一步

- 进入 Step 2：需要跑批量战斗模拟/蒙特卡洛；或规则公式开始被非 ECS 测试复制。
- 进入 Step 3：内容 schema 连续两轮无破坏性变更；怪物/地块/布局完成数据化；内容侧需要独立校验工具。
- 在此之前维持现状，只做 Step 1 的边界。

## 6. Step 1 的落地清单（若拍板）

1. 新目录 `ecs_core/src/content/`：
   - `item.rs`：`ItemTag/SlotAccept/SlotDef/ContainerDef/BODY/TRAVELER_PACK/CORPSE`、`Quality`、`ItemPart/WeaponSpec/Behavior`
   - `registry.rs`：现有 `ItemDef/ItemId/Registry 加载与校验` 搬入
   - `spell.rs`：`SpellId/SpellDef/SpellTargeting/SpellEffect`
   - `monster.rs`：`MonsterTemplate`、8 种静态表、掉落/属性表（只搬数据，不搬 `MonsterKindId: Component`）
   - `tile.rs`：`Tile/TILE_PROPS`（纯数据，可从 `map/tile.rs` 移入）
   - `balance.rs` 保留原位（纯规则常量，Step 2 再动）
2. `ecs_core/src/lib.rs`：`pub mod content; pub use content::*;`，调用方路径不变。
3. 运行时侧保留：`Container` 组件、`MonsterKindId` 组件、`MonsterStats`、系统、资源。
4. 新增守卫测试：扫描 `src/content/**/*.rs`，断言不出现 `bevy_ecs`；命中即失败并打印文件与行。
5. 验收：不改行为、不改数据 schema；`cargo test --workspace`、四 crate clippy、`docs_check`、gate 全绿；文档加 `REFACTOR.md` 一行与 DESIGN 注记。

## 7. 一句话建议

**先做 Step 1（半天到一天、零语义变化）把边界变成可执行约束；Step 2 等模拟工具需求，Step 3 等内容定稿。** 这样既回应了「内容与结构混在 ecs_core」的担忧，又不在内容未想清楚时冻结它。
