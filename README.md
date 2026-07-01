# Dungeon MVP

一个用 Rust 编写的终端地牢探险游戏（Roguelike），基于 `ratatui` + `crossterm` + `bevy_ecs`。

## 玩法

### 目标
在随机生成的地牢中层层深入，击杀怪物，获取装备，提升等级。

### 操作

| 按键 | 功能 |
|------|------|
| `WASD` | 移动 / 攻击（撞向怪物） |
| `1`-`4` | 释放技能 |
| `e` | 打开/关闭背包 |
| `>` | 下楼（下一层） |
| `.` / `5` | 休息（仅全层无怪物时） |
| `F5` | 手动存档 |
| `F9` | 手动读档 |
| `q` | 退出游戏 |

### 背包（按 `e`）

| 按键 | 功能 |
|------|------|
| `0-9` `a-z` | 选择物品（按序号） |
| `e` | 装备/卸下选中物品 |
| `d` | 丢弃选中物品 |
| `Esc` | 关闭背包 |

### 技能

| 键 | 技能 | MP | 效果 |
|----|------|----|------|
| `1` | 治愈 | 5 | HP+15 |
| `2` | 火球 | 8 | 对邻接敌人造成 15 伤害 |
| `3` | 护盾 | 6 | DEF+5 持续 3 回合 |
| `4` | 狂暴 | 6 | ATK+5 持续 3 回合 |

### 升级加点

升级后进入加点界面：

| 按键 | 属性 |
|------|------|
| `S` | 力量 STR |
| `D` | 敏捷 DEX |
| `I` | 智力 INT |
| `V` | 体质 VIT |
| `Enter` | 确认分配 |

## 系统架构

```
dungeon-tui/          ← 二进制 crate
├── Cargo.toml
└── src/main.rs       ← 游戏循环 + 渲染 + 背包/加点/存档界面

dungeon-core/         ← 库 crate
├── Cargo.toml
└── src/
    ├── lib.rs        ← 模块声明 + Tile/Room/Map 核心类型
    ├── components.rs ← ECS 组件定义
    ├── resources.rs  ← ECS 资源定义
    ├── items.rs      ← 物品/装备
    ├── ai.rs         ← 怪物 AI 行为
    ├── pathfinding.rs ← A* 寻路
    ├── systems.rs    ← 所有 ECS 系统
    ├── api.rs        ← 对外 API + 升级曲线
    ├── save.rs       ← 存档/读档
    └── tests.rs      ← 单元测试
```

### 核心系统

| 系统 | 说明 |
|------|------|
| **行动轴** | 速度决定行动频率（`speed = 50 + DEX×3`），满 100 行动 |
| **FOV** | Bresenham 画线法，墙阻挡视线 |
| **怪物 AI** | 优先行为链：残血逃跑 → A*追猎 → 随机游荡 |
| **装备** | 武器/防具/戒指，加成即时生效 |
| **存档** | bincode 二进制格式，F5/F9 手动 + 下楼自动 |

### 升级曲线

| 属性 | 曲线 | 公式 |
|------|------|------|
| EXP 需求 | 二次 | `20×level² + 30×level` |
| HP 成长 | 一次 | `20 + level×5 + VIT×2` |
| MP 成长 | 一次 | `5 + level×3 + INT` |
| 防御加成 | 对数 | `floor(log₂(level))` |

## 构建与运行

```bash
# 运行
cargo run

# 测试
cargo test -p dungeon-core

# 构建
cargo build --release
```

## 依赖

- `bevy_ecs` — ECS 框架
- `ratatui` — 终端 UI
- `crossterm` — 终端控制
- `rand` — 随机数生成
- `serde` + `bincode` — 存档序列化
