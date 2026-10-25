# Dungeon MVP

Rust 终端 Roguelike：`ratatui` + `crossterm` + `bevy_ecs`（0.16）。

> **本仓库当前状态：** `refactor` 分支正在把业务领域收敛到单一 crate `ecs_core`。
> 旧实现在 `archive/`（仍可编译可测，只作对照）。**新架构只实现了"行动 + 战斗 + 经验 +
> 地图"这一条闭环**——物品、技能、投掷、存档都还没迁（见
> [GAME.md](GAME.md) 头部的迁移状态表）。

## 快速开始

```bash
cargo run                      # 跑游戏
cargo check --workspace        # 全工作区构建检查

# 验收走一条命令（9 步：构建 / 新方向测试 / 端到端 / bin / clippy / 依赖边界）
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/gate.ps1

cargo test -p ecs_core         # 96 passed
cargo test -p presentation     # 57 passed
```

门禁**覆盖**什么、**不覆盖**什么（旧 crate 测试不在门禁内）见 [PROTOCOLS.md §五](PROTOCOLS.md)。

## 现在能玩什么

| 按键 | 作用 |
|---|---|
| 方向键 / `Home` `End` `PgUp` `PgDn` | 移动 / 朝该方向攻击（8 方向，**tap-tap**：第一次预览，同键第二次确认） |
| `.` | 等待（同样 tap-tap 确认） |
| `?` | 查看模式（光标移动，`Esc`/`?` 退出） |
| `q` / `Esc` | 退出（弹出确认框，`Enter` 确认） |

> 上面之外的按键（背包 `e`、投掷 `t`、拾取 `g`、下楼 `>`、存档 `F5/F9`、技能 `1-4`）
> 属于**未迁移子系统**，当前版本按了没有效果。键位表的唯一来源是
> `presentation/src/input`，外观的唯一来源是 `tui/src/catalog.rs`。

## 架构

分层与依赖方向（细节见 [DESIGN.md DsnX14](DESIGN.md)）：

```text
ecs_core ──> presentation ──> render-api <── tui（未来还有 gpu）
                                  ▲
                          dungeon-app（装配 + 选择后端）
```

| crate | 职责 | 不许依赖 |
|---|---|---|
| `ecs_core` | 唯一业务/领域层：组件、资源、事件、行动链路、战斗、地图、怪物、结算 | ratatui / wgpu / `render-api` / `presentation` |
| `presentation` | 唯一认识 `ecs_core` 的集成层：世界 → `SceneFrame`、相机、页栈、输入映射 | ratatui / crossterm |
| `render-api` | 后端无关的只读契约：`SceneFrame` / `VisualKey` / `UiView` / `InputEvent` | `ecs_core` |
| `tui` | TUI 后端：`VisualKey` → glyph/颜色，`SceneFrame` → ratatui | `ecs_core` / `presentation` |
| `dungeon-app`（根） | 装配：`App` + 主循环 + 平台键位翻译 | 业务实现 |

三条边界由 `scripts/gate.ps1` 用 `cargo tree` 强制（违反即门禁 FAIL）。

**已归档（`archive/`，新代码零依赖，仍可 `cargo test -p dungeon-*`）：**
`dungeon-core` / `dungeon-action` / `dungeon-world` / `dungeon-render` + 旧集成测试。
归档理由、何时可真正删除、以及文档治理的垃圾桶清单见
[archive/README.md](archive/README.md) 与 [archive/docs-trash-manifest.md](archive/docs-trash-manifest.md)。
`terrain-forge/` 是仍在使用的 git submodule（地图生成）。

## 文档地图

| 文档 | 装什么 | 读者 |
|---|---|---|
| [GAME.md](GAME.md) | **数值**与内容（每个数字带 `[⃞计算]/[⃞直觉]/[⃞试调]` 标注） | 游戏设计 |
| [DESIGN.md](DESIGN.md) | **决策**：跨 crate 的分层/契约/迁移路线 + DsnX17 冻结契约 | 架构评审 |
| `ecs_core/DESIGN.md` 等 | 各 crate 自己的决策（`DsnE` / `DsnP` / `DsnA` / `DsnT` / `DsnS` / `DsnU`） | 改该 crate 的人 |
| [ISSUES.md](ISSUES.md) + 各 crate | **未闭合问题**（按 crate 分片，编号 `SYN` / `ECS` / …） | 所有人 |
| [LESSONS.md](LESSONS.md) + 各 crate | **教训**（读者是 AI，每条对应一次真实踩坑） | AI |
| [REFACTOR.md](REFACTOR.md) | 本分支**还没做完的计划**（做完的已折进 DESIGN/ISSUES） | 本分支开发者 |
| [RULE.md](RULE.md) / [PROTOCOLS.md](PROTOCOLS.md) | 文档维护规则 / 操作流程与模板 | 所有人 |

**归属判据一句话：** 形状进 `DESIGN`，值进 `GAME`，问题进 `ISSUES`，教训进 `LESSONS`
（详见 [DESIGN DsnX16](DESIGN.md) 与 [RULE.md §七](RULE.md)）。

## 设计参考

FFX CTB（增量式时间片推进）、DCSS（aut 系统）、Brogue（有机洞穴 / 水体环境格）、
terrain-forge（`room_accretion`）、Dwarf Fortress（背景色为主的渲染）、
Dota 2 / Overwatch（能力即组件）、Minecraft 1.16+（Registry + ItemStack，未迁移子系统）。
