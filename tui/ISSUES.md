> **⚠️ 修改前必须阅读或回忆 [RULE.md](../RULE.md)——它定义了本文档的维护规则和更新时机。**

# 发现的问题记录 —— tui

**归属范围：** TUI 后端：catalog 外观、ratatui 绘制、终端布局与调试面板。

**编号：** `TUI1`、`TUI2`… 每个 crate 独立编号，见 [RULE.md](../RULE.md) 与 [REFACTOR.md](../REFACTOR.md) §13。
根目录只记**跨 crate 协同 / 工具链与流程 / 迁移动因**；单 crate 的问题记在对应 crate 的 ISSUES.md 里。

**优先级：** 🔴 高（影响正确性或游戏体验） / 🟡 中（维护性或功能缺口） / 🟢 低（整洁或边缘情况）

## 待处理

### TUI1 — ratatui 内置 widget 闲置（Gauge/List/Clear/Scrollbar/Table 未使用）（Deferred）

**问题：** 项目中使用的 ratatui widget 仅限于 `Paragraph` + `Span` + `Layout` + `Block`，五个内置 widget 完全未使用，对应功能由手写代码替代：

| widget | 手写替代位置 | 手写行数 | 可简化到 |
|--------|------------|---------|---------|
| `Gauge` | `ui.rs` 中 `bar()` 函数 | ~8 | 2 行构造 |
| `List` | `inventory.rs` 背包列表循环（选中态+▸+滚动） | ~30 | 5 行 |
| `Clear` | 模态弹窗覆盖逻辑 | 依赖 modal | 1 行 |
| `Scrollbar` | 事件日志 `take(12)` 硬截断 | ~5 | 无截断+滚动条 |
| `Table` | `ui.rs` 属性面板手算 `{:>3}` + `"   "` 分隔 | ~15 | 3 行列定义 |

**影响：** 🟢 低 — 正确性不受影响。代码量约多写 50 行，背包列表的可维护性（选中态/滚动边界）不如 `List` 开箱即用。仅在新增类似 UI（合成台、技能树）时值得一次性迁移。

**状态：** Deferred — 触发条件：新增合成台/技能树等列表型 UI 时一次性迁移（Gauge/List/Scrollbar）。

**位置：** `dungeon-render/src/ui.rs`（Gauge/Table/Scrollbar）、`src/inventory.rs`（List）

---


## ✅ 已修复

### TUI2 — Gm11 模态对话框描述过时：modal_flag 已是死代码 ✅已修复

**修复前：** Gm11 写「AtomicBool 暂停输入线程」，页栈（DsnP2）已接管，`modal_flag` 为死代码。

**修复后：** Gm11 改为「页栈按键路由」，标注旧方案已废弃；`modal_flag` 死代码清理并入 A36。

**位置：** `GAME.md:534`

---


**原编号：** `D25`（迁移前）
