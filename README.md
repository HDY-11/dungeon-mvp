# Dungeon MVP

一个用 Rust 写的**终端 Roguelike**：回合制、增量时间片（AV）驱动，`ratatui` + `crossterm` 负责画面，`bevy_ecs`（0.20.1，仅 ECS 子 crate，默认特性关闭）负责世界。

```text
  ##########      @  你            %  尸体（可搜刮）
  #   @    #      g  怪物          ~  治疗泉
  #  g  %  #      *  投掷弹道      >  楼梯
  ##########      e 背包  t 投掷  ? 查看
```

## 当前状态

| 系统 | 状态 | 一句话 |
|---|---|---|
| 行动系统 | ✅ | 六阶段（生成/仲裁/计时/校验/落地/完成）+ AV + 格子预留，玩家与 AI 共用 |
| 战斗 | ✅ | 五域伤害、暴击、护甲曲线、抗性、武器成长、仇恨/威胁表 |
| 地图与楼层 | ✅ | 种子重建 80×60 地图、下楼、泥潭、治疗泉、视野记忆 |
| 物品 | ✅ | 蓝图 `parts` → 组件装配、身体七槽 + 行囊 12 格、重量、投掷、存档 V5 |
| 法术 | ✅ | 数据文件 + 施法行动；玩家侧火花术 / 治愈术 |
| 界面 | ✅ | 四栏布局、事件时间轴、信息栏、背包 3×3 焦点网格、投掷瞄准页 |
| 怪物 AI | ✅ | 8 种模板、游荡/追击/逃跑/等待、威胁表与记忆 |
| 内容 | 🚧 | `content/` 为内容草案（世界观/生物/构筑方向），非代码事实源 |
| 未迁移 | ⬜ | 合成、buff（护盾/狂暴）、技能卷轴与熟练度、怪物投掷 |

> 业务领域已收敛到单一 crate `ecs_core`。旧实现在 `archive/`（仍可编译可测，只作对照）。
> 更细的状态与数字见 [GAME.md](GAME.md) 头部迁移表、[REFACTOR.md](REFACTOR.md) 与 [DESIGN.md](DESIGN.md)。

## 快速开始

```bash
cargo run                        # 直接开玩（终端需 ≥ 80×24）
cargo test --workspace           # 全部测试：30 个测试目标
```

一键门禁（构建 / 测试 / 端到端 / clippy × 6 crate / 依赖边界 / 文档）：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\gate.ps1
```

长跑模拟（6 个种子，检查确定性、无异常、覆盖楼层）：

```bash
cargo run --release -p dungeon-app --example playthrough
```

门禁**覆盖**什么、**不覆盖**什么（旧 crate 测试不在门禁内）见 [PROTOCOLS.md §五](PROTOCOLS.md)。

## 操作

| 按键 | 作用 |
|---|---|
| 方向键 / `Home` `End` `PgUp` `PgDn` | 移动；撞向怪物即攻击（八方向，单键直接行动） |
| `Tab` | 切换焦点：摄像机 → 信息 → 事件 → 状态（顺时针） |
| `↑` `↓` | 事件栏聚焦时翻历史；信息栏聚焦时选择关注目标（地图反色） |
| `.` / 空格 | 原地等待 |
| `1` `2` `3` | 施放法术书对应格（火花术 / 治愈术） |
| `e` | 背包 / 身体与容器页面（装—卸—用—丢—拿） |
| `t` | 投掷瞄准（副手投掷物；自动瞄准 + 方向键微调，`Enter` 确认、`Esc`/`x` 取消） |
| `>` | 下楼（站在楼梯上） |
| `?` | 查看模式（光标移动，`Esc`/`?` 退出） |
| `q` / `Esc` | 退出（弹出确认框） |
| `F5` / `F9` | 快速存档 / 读档（开发用） |

> 键位表的唯一来源是 `presentation/src/input`，外观的唯一来源是 `tui/src/catalog.rs`。
> 未迁移子系统（技能卷轴等）当前按键无效果。

## 架构

分层与数据流（完整决策见 [DESIGN.md DsnX14](DESIGN.md) 与 DsnX19）：

```text
                    ┌──────────────────────────────────────┐
                    │  dungeon-app：装配 / App::run_with   │
                    │  Frontend 接缝（TuiFrontend / 测试） │
                    └───────┬──────────────────────┬───────┘
                            │                      │
   ecs_core ──世界──▶ presentation ──SceneFrame──▶ render-api ◀── tui ──▶ 终端
     │  ▲                                                ▲
     │  └──────────────── sys（日志 / 按键来源）──────────┘
     └──▶ utils / terrain-forge（地图生成）
```

| crate | 职责 | 不许依赖 |
|---|---|---|
| `ecs_core` | 唯一业务/领域层：组件、资源、事件、行动链路、战斗、地图、怪物、结算 | ratatui / wgpu / `render-api` / `presentation` |
| `presentation` | 唯一认识 `ecs_core` 的集成层：世界 → `SceneFrame`、相机、页栈、输入映射 | ratatui / crossterm |
| `render-api` | 后端无关的只读契约：`SceneFrame` / `VisualKey` / `UiView` / `InputEvent` | `ecs_core` |
| `tui` | TUI 后端：`VisualKey` → glyph/颜色，`SceneFrame` → ratatui | `ecs_core` / `presentation` / `dungeon-app` |
| `sys` | 平台层：日志、终端按键来源，不认识游戏 | 游戏 crate |
| `dungeon-app`（根） | 装配：`App` + `Frontend` 接缝主循环 + 平台键位翻译 | 业务实现 |

三条边界由 `scripts/gate.ps1` 用 `cargo tree` 强制（违反即门禁 FAIL）。

## 目录一览

```text
ecs_core/       领域层（组件/系统/行动/战斗/物品/法术/怪物/存档合同）
presentation/   世界 → SceneFrame、页栈与输入映射
render-api/     只读渲染契约与输入事件
tui/            ratatui 后端
sys/            日志与终端按键来源
src/            dungeon-app：App、Frontend、main
tests/          端到端 / 存档 / 前端接缝测试
content/        内容设计草案（世界观/生物/构筑，非代码事实源）
docs/           架构图（architecture-map.html）、bevy 0.20 升级评估
archive/        旧实现（只作对照，不在门禁内）
terrain-forge/  地图生成 git submodule
```

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
| [content/README.md](content/README.md) | 内容设计草案与待拍板清单 | 内容设计 |
| [docs/architecture-map.html](docs/architecture-map.html) | 实体/组件/系统关系图（浏览器直接打开） | 新人 / 架构评审 |

**归属判据一句话：** 形状进 `DESIGN`，值进 `GAME`，问题进 `ISSUES`，教训进 `LESSONS`
（详见 [DESIGN DsnX16](DESIGN.md) 与 [RULE.md §七](RULE.md)）。

## 验证与数字

- 测试：`30 个测试目标 / 662 passed / 1 ignored`（每次发布前以 `cargo test --workspace` 实测为准）。
- 门禁：`scripts/gate.ps1` **10/10 PASS**（含六 crate clippy `-D warnings`、依赖边界、`docs_check`）。
- 长跑：`playthrough` 6 个种子，异常：无；覆盖到达 1–4 层。
- 版本：`bevy_ecs 0.20.1`（`default-features = false, features = ["std"]`）；`ratatui 0.30`；`crossterm 0.29`；edition 2024。

## 建议阅读顺序

1. [GAME.md](GAME.md)：游戏现在是什么样、数字从哪来。
2. [DESIGN.md](DESIGN.md) + [ecs_core/DESIGN.md](ecs_core/DESIGN.md)：为什么这样分层、哪些路走过后被否。
3. [RULE.md](RULE.md)：文档怎么写、写到哪里。
4. [REFACTOR.md](REFACTOR.md) / [ISSUES.md](ISSUES.md) / [LESSONS.md](LESSONS.md)：当前欠账与踩坑。
5. [content/](content/README.md)：内容方向草案（会变，不承诺是代码现状）。

## 设计参考

FFX CTB（增量式时间片推进）、DCSS（aut 系统）、Brogue（有机洞穴 / 水体环境格）、
terrain-forge（`room_accretion`）、Dwarf Fortress（背景色为主的渲染）、
Dota 2 / Overwatch（能力即组件）、Minecraft 1.16+（Registry + ItemStack）。
