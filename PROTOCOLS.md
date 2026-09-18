# PROTOCOLS.md — 操作流程与模板

> **⚠️ 修改前必须阅读或回忆 [RULE.md](RULE.md)——它定义了本文档的维护规则和更新时机。**
>
> 本文是 [RULE.md](RULE.md) 的配套文档，包含具体的操作步骤、模板和命令。
> RULE.md 定义**原则和约束**，本文提供**怎么做**。

---

## 一、提交规范

### 1.1 前缀

| 前缀 | 用途 |
|------|------|
| `feat:` | 新功能 |
| `fix:` | Bug 修复 |
| `refactor:` | 重构（不改变外部行为） |
| `docs:` | 文档/注释变动 |
| `chore:` | 构建/工具/依赖变动 |

### 1.2 粒度

- 每个 commit 对应**一个逻辑变更**。修复 + 相关文档更新放在同一个 commit
- **跨 crate、逻辑独立的修复**，各自一个 commit。核心逻辑与边缘清理不应混在同一 commit 中
- **同一轮 ISSUES 驱动修复中对 ISSUES.md 的多次操作**，合并为一个 commit

### 1.3 提交流程

```bash
git status
git add -A
git commit -m "<前缀>: <简要说明>"

# 多逻辑合并提交用多行正文：
#   refactor: 修复 5 个存档/文档/清理问题
#
#   - I15: Tile 自定义 serde
#   - I12: F9 读档后刷新视野记忆
#   - A5: 删除 global.rs 空壳模块
#   - I14: 下楼 Skills 从 PlayerClass 推导
#   - D6: GAME.md 升级描述标记已移除
git push origin
```

---

## 二、ISSUES 模板

### 2.1 Deferred 示例

```
### A{N} — 标题（Deferred — 触发条件达成时重新评估）
**触发条件：** 当某计量超过 N 时重新评估
```

### 2.2 新增问题格式

```
### {ID} — 简短标题

**问题：** 描述表现和根因。
**影响：** 对正确性/体验/维护性的影响。
**位置：** `path/to/file.rs:line`
```

### 2.3 子编号格式（修复不彻底 / 修复引入回归）

当一个问题的修复不彻底、或修复引入了新的回归问题时，使用**子编号格式**追加记录，保持与原问题的追溯链路：

```
A4  — 原始问题标题 ✅
  A4L — 修复中遗留的子问题 ✅（第2轮）
    A4La — 同一根因再次出现 🟡（第3轮，以此类推）
```

- 子编号在父编号后追加大写字母：`A4` → `A4A` → `A4B`；或追加 `L`（遗留/Lingering）→ `La` → `Lb`
- 每个子编号独立记录表现、根因、位置，但通过缩进和前缀保持父子的视觉关联
- 父编号的修复记录保留不动，子编号记录新的发现（"修复后又出现"或"修复引入新问题"）
- 同根因出现 ≥3 次时，**必须写入 LESSONS.md**

---

## 三、LESSONS 写法模板

```
### L{N} — 标题

一段或多段描述。包含：
- 问题背景
- **错误做法**（可选）
- **正确做法**
- 为什么更好

可以用代码块展示对比，但不是必须。
```

---

## 四、GAME 数值更新操作

1. **替换表格** — 直接替换数值行，保持表格格式
2. **删除标记** — 已移除的机制在行内标记 `~~已移除~~`
3. **公式格式** — 用 Rust 表达式写入代码块：`hp = 20 + level * 5`

---

## 五、门禁（一条命令）

REFACTOR 期间「新方向」的验收统一走一个脚本：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/gate.ps1
```

依次执行（任一步失败即退出码 1，并在末尾列出失败项）：

| 步骤 | 命令 |
|---|---|
| 1 | `cargo check --workspace` |
| 2 | `cargo test -p render-api -p ecs_core -p presentation -p utils -p tui -p sys` |
| 3 | `cargo test -p dungeon-app --test mvp_loop_test`（端到端：喂按键 → 世界推进 → 真实绘制） |
| 4 | `cargo test -p dungeon-app --lib`（装配层单元） |
| 5 | `cargo build -p dungeon-app`（bin 能编出来） |
| 6 | `cargo clippy -p render-api --all-targets -- -D warnings` |
| 7 | `cargo clippy -p presentation --all-targets -- -D warnings` |
| 8 | `cargo clippy -p ecs_core --all-targets -- -D warnings` |
| 9 | **依赖边界**（`cargo tree`）：`tui` ↛ `ecs_core`/`presentation`、`presentation` ↛ ratatui/crossterm、`render-api` ↛ `ecs_core` |

第 9 步是 DESIGN Dsn28「渲染后端可替换」的唯一机械保证：代码里"看起来没用到"
不代表依赖表里没有，而依赖表是这条边界**唯一**可靠的表达。违反时门禁直接 FAIL
（已用「给 `tui` 注入 `ecs_core` 依赖」验证过确实会失败）。

第 3–5 步回答的是另一个问题：**各层单测都绿，不代表装起来还能跑**。
`mvp_loop_test` 从 `crossterm::KeyCode` 一路断言到真实 ratatui 绘制，
把"主循环接线"从"人工玩一遍"变成可回归的检查。

选项：`-Online`（去掉 `--offline`，需要联网拉依赖时）、`-SkipClippy`（只跑 1–2）。

### 为什么不覆盖全部

- **旧 `dungeon-*` crate 与根 `dungeon-app` 的测试不在门禁内**：它们针对被取代的
  旧架构，纳入只会长期红着（REFACTOR.md §10.4 / §10.6 第 4、5 项）。需要全量时
  单独跑 `cargo test --workspace`，并自行判断失败项属于哪一类。
- 每个 Phase 收尾仍按 [RULE.md](RULE.md) 的文档纪律更新对应文档；门禁只管代码。

### 维护注意

- `scripts/gate.ps1` **必须保存为 UTF-8 with BOM**。Windows PowerShell 5.1 在没有
  BOM 时会按系统 ANSI 代码页（中文 Windows 是 GBK）解析 `.ps1`，中文注释会变成
  语法错误。用只写 UTF-8 无 BOM 的编辑器/工具改过之后，要重新加回 BOM。
---

## 六、实现改动后的规格复核（模板）

RULE.md §八定义了流程；这里是**收尾报告模板**，直接照抄填写。

```markdown
### 规格复核

**影响范围：** 本次改动触及 <子系统>，据此复核 <GAME.md Gm4 / ecs_core/DESIGN.md DsnE8>。

| 章节 | 判定 | 处置 |
|---|---|---|
| Gm4 玩家初始值 | 仍然有效 | — |
| Gm1 速度倍率 | 文档过时（实现已改为倍率） | 已就地更新 |
| DsnE4 行动系统 | 实现偏离（保活检查缺失） | 见下方待决 |
| DsnX12 合成系统 | 文档失效（物品未迁移） | 已标 ~~已废弃~~，指向 DsnX13 |

**待你决定的分叉：**
- <条目>：建议改代码对齐文档 / 建议改文档承认新行为，理由 …
```

**判定四选一**（含义见 RULE.md §八）：仍然有效 / 文档过时 / 实现偏离 / 文档失效。

**上报分级：**

- 较严重（影响玩家可见数值语义、存在真实分叉、跨多 crate）→ **先报告，批准后**写入
  对应 crate 的 ISSUES.md，编号按 RULE.md §七；
- 轻微（措辞/行号/表述）→ 可直接改文档，在同一轮汇总里列出。

**收尾硬性要求：** 最终回复必须含「规格复核」一节，否则视为流程未完成。