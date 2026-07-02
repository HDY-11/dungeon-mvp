# Dungeon MVP

Rust 终端 Roguelike，基于 `ratatui` + `crossterm` + `bevy_ecs`。

## 架构

```
dungeon-core/         ← 纯逻辑（无渲染依赖）
  components.rs       ← ECS 组件（ActionValue, ActionPrediction, Stats, …）
  resources.rs        ← ECS 资源（GamePacing, PendingInput, …）
  systems.rs          ← AV 引擎（advance_by, next_key_point_distance, …）
  ai.rs               ← 怪物脑链（MonsterBrain, AiBehavior）
  items.rs            ← 物品/装备
  pathfinding.rs      ← A* 寻路
  save.rs             ← 存档/读档
  api.rs              ← setup_world, descend, FOV, 工具函数
  tests.rs            ← 单元测试（33 个）

dungeon-render/       ← 渲染层（依赖 core + ratatui）
  color.rs            ← (u8,u8,u8) → ratatui::Color 转换
  timeline.rs         ← build_timeline（行动轴）
  ui.rs               ← render_ui + build_stats_panel
  title.rs            ← draw_title + draw_level_up

src/main.rs           ← 应用层：主循环、输入处理、模式切换
```

## AV 引擎

行动值（AV）系统：所有实体在跑道上竞速，AV 递减至 0 时执行行动。

- `ActionValue`: current_av（距执行剩余）、base_av（本轮总长）、speed、reaction_time
- `advance_by(amount)`: 统一推进，处理执行（AV≤0）和锁定（AV≤reaction_time）
- `advance_to_next_decision_point`: 循环推进至锁定或执行
- 锁定 = 进入反应时窗口，行动不可再改
- 进入战斗：玩家受伤/造成伤害；退出战斗：视野内无怪物

## 操作

| 按键 | 功能 |
|------|------|
| `↑↓←→` | 移动 / 攻击 |
| `1-4` | 技能 |
| `e` | 背包 |
| `>` | 下楼 |
| `.` | 等待 |
| `m` | 强制手动模式 |
| `a` | 恢复自动模式 |
| `F5` | 存档 |
| `F9` | 读档 |
| `q` / `Esc` | 退出 |

## 构建

```bash
cargo run
cargo test -p dungeon-core
```
