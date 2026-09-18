> **⚠️ 修改前必须阅读或回忆 [RULE.md](../RULE.md)——它定义了本文档的维护规则和更新时机。**

# 发现的问题记录 —— sys

**归属范围：** OS 封装：键盘线程、终端生命周期、文件、日志。

**编号：** `SYS1`、`SYS2`… 每个 crate 独立编号，见 [RULE.md](../RULE.md) 与 [REFACTOR.md](../REFACTOR.md) §13。
根目录只记**跨 crate 协同 / 工具链与流程 / 迁移动因**；单 crate 的问题记在对应 crate 的 ISSUES.md 里。

**优先级：** 🔴 高（影响正确性或游戏体验） / 🟡 中（维护性或功能缺口） / 🟢 低（整洁或边缘情况）

## 待处理

（当前没有未处理条目）

## ✅ 已修复

### SYS1 — `sys` 独立构建缺少 `log/std` ✅已修复



**修复前：** `sys/src/logger.rs:46` 调用 `log::set_boxed_logger`，但 `sys` 的 `log` 依赖未显式启用 `std` feature；`cargo test -p sys` 独立编译失败（`cannot find function set_boxed_logger`），workspace 构建只靠其他 crate 的 feature 合并偶然通过。



**修复后：** `sys/Cargo.toml` 改为 `log = { workspace = true, features = ["std"] }` 并加注释说明原因；`cargo test -p sys` 独立通过（0 测试，不再依赖 feature 合并）。



**位置：** `sys/Cargo.toml:8`、`sys/src/logger.rs:46`



**关联：** REFACTOR.md §10.4 / §10.6 第 3 项 / §11.3 Phase F（F1）。



---




**原编号：** `I87`（迁移前）

### SYS2 — 按键去重阈值：文档三处 50ms vs 实现 33ms ✅已修复

**修复前：** Gm11/DsnS1/README 均声明 50ms，实现（LSYS2 修复后）为 33ms + KeyEventKind 过滤。

**修复后：** 三处文档统一为 33ms；DsnS1 保留「50→33 收窄」历史上下文并关联 LSYS2。

**位置：** `GAME.md:524/530`、`DESIGN.md:378-389`（DsnS1）、`README.md:87`

---


**原编号：** `D24`（迁移前）

### SYS3 — 输入线程未过滤 `KeyEventKind::Release`，导致同键触发 2-3 次 ✅已修复

**修复前：** 输入线程仅靠 50ms 同键去重过滤重复按键。`KeyEventKind::Release` 事件与 `Press` 的 `key.code` 相同，依赖去重窗口过滤。但 Release 的到达时间受终端调度影响不可控（可跨 1-3 个 poll 周期），当 50ms 窗口刚好闭合时 Release 通过，产生"按一次触发 2-3 次"的效果。

**修复后：** 双重过滤：
1. 环境自适应：丢弃 `key.kind != KeyEventKind::Press`（现代终端区分事件类型，传统终端所有事件为 Press，不受影响）
2. 去重窗口 50ms→33ms，与帧率（30FPS）对齐，tap-tap 不受影响

**位置：** `src/main.rs:62-72`（输入线程事件循环）
**教训见：** LESSONS.md LSYS2

**修复前：** `render_inventory_overlay` 在 `inv_state.detail == true` 时仅显示操作提示行，无物品名称/属性/描述。

**修复后：** 根据 `detail_source`（装备/背包/地面）获取物品，渲染完整的详情视图：标签、名称（黄色加粗）、数量、类别、属性加成、描述、上下文操作提示。参考旧版 `inventory.rs:81-133` 的详情渲染逻辑。

**位置：** `dungeon-render/src/ui.rs`（render_inventory_overlay detail 分支）


**原编号：** `I56`（迁移前）
