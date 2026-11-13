# ecs_core 整理迁移 · M0 基线

> 建立时间：2026-11-13。对应迁移方案：[`docs/ecs-core-migration-plan.html`](ecs-core-migration-plan.html)。
> 基线提交：`f9934b7`（tag `pre-ecs-cleanup`）。

## 1. 构建与工具链

| 项 | 值 |
|---|---|
| Rust | 1.95.0（2026-04-14），edition 2024 |
| bevy_ecs | 0.20.1（workspace 依赖，`default-features=false, features=["std"]`） |
| 测试目标 | 30 个 test result 组 |

## 2. 测试基线（2026-11-13 实测）

- `cargo test --workspace --no-fail-fast`：**669 passed / 0 failed / 1 ignored**。
- `ecs_core` 单测：**374 passed / 0 failed**。
- 其余关键目标：presentation 80、tui 44、render-api 36、terrain-forge 26、dungeon_core 19、dungeon_world 15、core_loop 13、contract 4 等（完整输出见本次运行记录，已删除临时文件）。
- 迁移的纯整理阶段（M0–M2）验收标准：测试数只增不减、本组数字不降、无新增 ignored。

## 3. 行为基线（手工记录）

- 模拟一致性验证命令：`cargo run --release -p dungeon-app --example playthrough`。
- soak 种子：`2026, 1, 2, 3, 7, 11`；上轮结果全部「异常：无」。
- 迁移期间（M0–M2）要求：同一 seed 的动作序列与事件文案保持可比；仲裁/调度重构前后用同一组 soak 结果对照。

## 4. 上次门禁记录（迁移前）

- `scripts/gate.ps1`：**10/10 PASS**（最近一次在 bevy 0.20.1 升级 + 前端接缝提交后）。
- `python scripts/docs_check.py`：PASS。
- 四 crate clippy `--all-targets -- -D warnings`：全绿。

> M0–M2 每阶段结束需要复跑：`cargo test --workspace`、`docs_check`、四 crate clippy、gate、soak。

## 5. 回退点

- 标签：`pre-ecs-cleanup` → `f9934b7`。
- 任一阶段可 `git revert <phase-commit>` 回到前一阶段末；M2 拆成 M2a（调度分组）与 M2b（仲裁归约）两个提交，允许只回退后者。

## 6. 本阶段未做的事

- 未改任何代码；未跑 soak（沿用上轮记录）。
- 未引入 crate 拆分、未改数据 schema、未动内容。
