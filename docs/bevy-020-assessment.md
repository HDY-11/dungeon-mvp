# bevy_ecs 0.20.1 升级评估（实测数据）

- 基准：`main`，HEAD `2a69176`（仇恨/威胁表已提交）
- 基准验证：29 个测试目标、660 passed / 1 ignored（DsnE29 后）
- 实验方式：**直接在 main 工作树**把 `bevy_ecs` 0.16 → 0.20.1，跑编译取数据（未改任何业务代码）
- 实验改动（当前未提交）：
  - 根 `Cargo.toml`：`bevy_ecs = "0.16"` → workspace 依赖 `{ version = "0.20.1", default-features = false, features = ["std"] }`，根包与 dev-deps 改为 `{ workspace = true }`
  - `Cargo.lock`：随之更新（新增 0.20.1 链）
  - `archive/*` 四个旧 crate 仍钉 0.16（Cargo 会同时保留 0.16.1 与 0.20.1 两份）

## 1. 编译结果（一键实测）

| 目标 | 错误数 | 备注 |
|---|---|---|
| `cargo check -p ecs_core --lib` | **55** | 全部为 API 迁移类，无逻辑/类型推导型错误 |
| `--all-targets`（lib + 测试目标） | **94**（lib 55 + 测试 39） | 15 个文件 |

错误码分布：E0425 ×51、E0599 ×16、E0433 ×10、E0432 ×9、E0407 ×8。

错误类别（合计 94）：

| 类别 | 数量 | 说明 |
|---|---|---|
| `Events` / `EventReader` / `EventWriter` 已改名 | 70 | 0.17 起缓冲事件改名 `Messages` / `MessageReader` / `MessageWriter` |
| `ScheduleLabel` 手写 impl 里的遗留方法 | 8 | `as_dyn_eq` / `dyn_hash` 已不属于该 trait（`DynEq`/`DynHash` 改为 blanket impl） |
| `SystemParam` 校验返回 `Result` | 13 | 测试里 `system.get_mut(&mut world)` 现返回 `Result<EffectContext, SystemParamValidationError>`，需 unwrap |
| `World::send_event` 已移除 | 3 | 改为 `World::write_message` |

注：`x` 个错误是测试代码里的，生产库错误 55 个；下游 crate（presentation / tui / app / tests）在 ecs_core 编译通过之前无法编译，但静态扫描显示它们**没有使用** `EventReader/EventWriter/Events`（0 处），预计下游接近零改动。

## 2. 迁移触点（全仓实测，不含 archive/）

| 触点 | 数量 | 文件数 | 修法 |
|---|---|---|---|
| `#[derive(Event)]` 事件类型 | 9 | `ecs_core/src/events.rs` | 改 `#[derive(Message)]`（缓冲事件语义） |
| `Events<T>` | 74 | 10 | 改 `Messages<T>` |
| `EventReader` | 18 | 11 | 改 `MessageReader` |
| `EventWriter` | 14 | 8 | 改 `MessageWriter` |
| `world.send_event(...)` | 3 | 3（均测试） | 改 `write_message(...)` |
| 自定义 `ScheduleLabel` impl | 4 个标签 | 1（`schedule.rs`） | 删 `as_dyn_eq`/`dyn_hash`，只保留 `dyn_clone` |
| `system.get_mut(&mut world)` | 6 | 2（均测试） | 结果 unwrap/expect |
| `world.run_system_once(...)` | 111 | 14 | 0.20 返回 `Result<Out, RunSystemError>`；79 处是 `let _ =`（需补 `.unwrap()` 以暴露参数校验失败），其余需按返回值调整 |
| `SystemState` 使用点 | 13 | — | 仅 6 处 get/get_mut（上表），其余为类型声明 |
| `world.run_system` / `iter_many` / `Single` / 手写 `QueryData` / `ExclusiveFunctionSystem` / `Reflect` | 0 | — | 0.17–0.20 的相关破坏性改动**不触及**本仓库 |

也确认：全仓库没有 Observer、SparseSet、`multi_threaded`、`register_system` 使用；`Entity::from_bits` 等旧 API 仅在已修过的测试中出现。

## 3. 依赖与构建成本

- 关闭默认特性（`default-features = false, features = ["std"]`）后：
  - `bevy_reflect` 从依赖树消失（ecs_core 依赖树：121 → **105** 个唯一包行，约 -16 个包）
  - 无新增编译错误（仍为 94），说明我们未用到 reflect 相关路径；后续可长期保持关闭。
- 增量 `cargo check -p ecs_core --lib`：约 **10s**；`--all-targets` 约 **4s**（依赖已编译）。
- 首次解析/编译 0.20.1 依赖链时 Cargo 新增约 38 条 lock 记录（含 bevy_platform / bevy_tasks / bevy_ptr / bevy_utils / glam / slotmap / variadics_please 等；trim 后 bevy_reflect 链被去掉）。
- archive 四个 crate 继续用 0.16：Cargo 保留两个版本，会多编译一份 bevy_ecs（0.16.1 之前已编译过，增量影响小；彻底解决可把 archive 移出 workspace）。

## 4. 逐项 API 映射（修法参考）

| 0.16 用法 | 0.20 用法 |
|---|---|
| `#[derive(Event)]`（缓冲） | `#[derive(Message)]` |
| `Events<T>` | `Messages<T>` |
| `EventReader<'w,'s,T>` | `MessageReader<'w,'s,T>` |
| `EventWriter<'w,T>` | `MessageWriter<'w,T>` |
| `reader.read()` | `reader.read()`（`MessageReader`） |
| `writer.send(x)` | `writer.write(x)` |
| `world.send_event(x)` | `world.write_message(x)` |
| 手写 `ScheduleLabel` 的 `as_dyn_eq/dyn_hash` | 删除；`DynEq`/`DynHash` 已有 blanket impl，仅保留 `dyn_clone` |
| `world.run_system_once(s)` 直接得 Out | `world.run_system_once(s)?` 或 `.unwrap()`（返回 `Result<Out, RunSystemError>`） |
| `system.get_mut(&mut world)` 直接得 Param | 返回 `Result<Param, SystemParamValidationError>`，需 unwrap |
| 0.19 Resources-as-Components | 我们的 2 处 `Query<Entity>` 未与 `Res` 同系统，暂未触发；如触发需 `Without<IsResource>` |
| 0.20 独占系统统一 | `&mut World` 仍可作系统；未引用 `ExclusiveFunctionSystem`，无需改动 |

## 5. 工作量估计（迁移到全绿可提交）

| 阶段 | 内容 | 预计 |
|---|---|---|
| S1 | 事件体系改名（events.rs + 30 余处引用）+ `ScheduleLabel` + `send_event` | 2–3 h |
| S2 | 测试适配：`run_system_once` 111 处、`get_mut` 6 处、测试里 `Messages` | 2–3 h |
| S3 | 下游 4 个 crate + 根 tests/examples 编译 | 1–2 h |
| S4 | 全量验证（660 tests、gate 10/10、soak 6 种子）与语义回归排查 | 2–4 h |
| 合计 | | **7–12 人时（约 1–1.5 个工作日）** |

## 6. 风险清单

1. **事件语义必须全改**：0.16 的 `#[derive(Event)]` 类型在 0.20 仍可编译（会变成 observer 事件），如果只改引用不改 derive，会出现"类型能编译但不在 `Messages` 里"的边界错误。迁移时以 `derive(Message)` 为唯一口径。
2. **`run_system_once` 的静默跳过**：0.20 参数校验失败返回 `Err(Skipped)`；测试里 `let _ =` 会把失败吞掉。统一 `.unwrap()`，避免出现"测试假绿"。
3. **资源即组件（0.19）**：当前无冲突；但后续若在系统里同时写 `Query<Entity>` 广查询与 `Res<T>`，需要 `Without<IsResource>`。全仓此类查询只有 2 处，可控。
4. **archive 双版本**：4 个 archive crate 钉 0.16 会让 Cargo 同时编译两份 bevy_ecs；建议升级后把 archive 移出 workspace（或单独 workspace），否则 CI 时间会增加。
5. **无权承诺性能收益**：0.17–0.20 官方有 ECS 内部优化，但没有本项目的公开基准；升级理由是能力与设计空间，不是帧率。

## 7. 结论与建议

- 成本侧：核心 crate 只有 **94 个编译错误、全部属于机械迁移**，预计 **1–1.5 天**可全绿；默认特性裁剪已验证不引入额外错误，还能去掉约 16 个依赖包。
- 收益侧（前一轮已列）：行动种类插件化（可移除 schedule）、真实可用的 observer/EntityEvent、关系/嵌套查询、资源即组件、独占系统统一。
- 按"有新设计思路和实现手段 → 有必要升级"的判据：**建议升级**，并把升级排到"行动种类插件化"之前，让插件化直接吃 0.20 的 schedule 能力。
- 执行建议：S1→S2→S3→S4 一次做完（机械迁移期间不改架构），全绿且 gate/soak 通过后提交；随后另开 DsnE30「ECS 能力升级与采用边界」再动关系化容器/observer 规则表等设计项。

## 8. 当前工作树状态

- `Cargo.toml` / `Cargo.lock` 已按上述实验改动；未改任何业务代码；未提交。
- 若决定不升级：`git checkout -- Cargo.toml Cargo.lock` 即可回退。
- 若继续升级：从 S1 开始，按上面映射表批量替换并逐步跑 `cargo check -p ecs_core --all-targets`。

## 9. 执行结果（2026-11-11，已完成）

- 实际迁移：32 个业务文件；`events.rs` 9 个事件改 `#[derive(Message)]`；`schedule.rs` 的 4 个 `ScheduleLabel` 删掉 `as_dyn_eq`/`dyn_hash`；`run_system_once` 12 处补 `.expect`（原先未处理的 `Result` 会静默吞掉参数校验失败）；`SystemState::get/get_mut` 6 处 unwrap；`mvp_loop_test` 两处 unused import 清理。下游 crate（presentation/tui/app/tests）**业务代码零改动**。
- 唯一语义坑：bevy 0.20 的 `Entity::to_bits()` 改用 `NonMaxU32` 编码（`bits = u32::MAX − index`），**生成顺序与 bits 顺序相反**。原来按 `to_bits()` 升序做的平局/稳定序（行动仲裁、地面/尸体列表、威胁平局、最近目标平局、UI 列表）全部改为 `Entity::index_u32()`；`EntityId::from_bits(to_bits())` 仍作为不透明 id 保留。详见 `ecs_core/LESSONS.md` LECS25。
- 编译：`cargo check --workspace --all-targets` **0 error / 0 warning**。
- 测试：`cargo test -p ecs_core --lib` **367 passed**；workspace **29 目标 660 passed / 0 failed / 1 ignored**；四 crate clippy `-D warnings` 全绿。
- 依赖：`bevy_ecs = { version = "0.20.1", default-features = false, features = ["std"] }`，`bevy_reflect` 不再进树；`archive/*` 四个旧 crate 仍钉 0.16（后续可移出 workspace 去掉双版本编译）。
