> **⚠️ 修改前必须阅读或回忆 [RULE.md](../RULE.md)——它定义了本文档的维护规则和更新时机。**
>
> 每条记载一个设计取舍，含两个小节：
> - **决策**：当前选的方向和理由（罗盘）
> - **背景**：舍弃的方案和演化过程（护卫）
>
> 数值和游戏规则见 [GAME.md](../GAME.md)。

# 设计决策记录 —— sys

**归属范围：** OS 封装的决策：输入线程模型、终端生命周期、日志分层。

**编号：** `DsnS1`、`DsnS2`… 每个 crate 独立编号。
判据：决策**落点在哪个 crate 的代码/接口**里就归哪里；跨多个 crate 的分层/契约/路线决策留根目录。

---

### DsnS1 33ms 按键去重 + 16ms 轮询

**决策**

输入线程以 16ms 间隔轮询（≈60fps），连续两次相同按键间隔小于 33ms 则丢弃后一次（LSYS2 校准：现代终端过滤 Release/Repeat 事件后，窗口从 50ms 收窄为 33ms）。

**16ms 不是任意选择的：** 它与 60fps 的输入采样对齐。轮询间隔太长（如 100ms）会导致明显可感知的延迟。16ms 是人感知不到的单帧延迟下限。

**背景**

终端键盘的物理按键会触发重复的 key-repeat 事件（长按时）。如果不做去重，方向键长按会导致连续触发预览/确认/预览/确认，玩家瞬间移动多格。33ms 窗口（配合 KeyEventKind 过滤）允许正常双次敲击（tap-tap 确认），但过滤掉键盘重复。

---

## 附录

**原编号：** `DsnS1`（迁移前）

### DsnS2 日志系统分层集成 —— 开发者日志 vs 玩家日志

**决策**

两层日志设计，面向不同用户：

```
玩家可见层：EventLog (ECS Resource)
  → 游戏内终端渲染，按 EventLevel 着色（红/黄/青/灰/亮红）
  → 上限 50 条，自动丢弃最旧

开发者层：log crate (全局静态)
  → 文件持久化，5MB 自动轮转
  → 所有 EventLog::push 自动转发到此层
  → 独立调用点可直接写 log::info! / log::error!
```

**为什么两层不合并：**
- 目标用户不同——玩家需要终端实时反馈，开发者需要持久化逐帧追踪
- 频率不同——`log::debug!` 可以写详细的战斗公式分解，但玩家终端不需要看
- 生命周期不同——EventLog 只存活于游戏会话，日志文件需跨会话保留

**分层机制：**
- `EventLog::push(msg: EventMessage)` 内部调用 `log::info!` 或 `log::warn!` + 存入 `Vec<EventMessage>`
- 渲染层通过 `msg.level` 按类别着色，不再显示裸字符串
- `ResultLogExt::expect_log` 和 `OptionLogExt::expect_log` 提供 panic 前日志记录

**背景**

旧的 EventLog 只有 `Vec<String>`，开发者无法在崩溃后追溯战斗过程；`panic.log` 只记录 panic 信息，不知道崩溃前发生了什么。引入 `log` crate 后，任何 panic 之前的 `log::info!` 调用都已写入文件，崩溃现场可复现。

**关联：** `dungeon-core/src/logger.rs`、`dungeon-core/src/ext.rs`、`dungeon-core/src/resources.rs`（EventLog）

---

**（本文档末尾 — 此后追加新条目）**
---

**原编号：** `DsnS2`（迁移前）
