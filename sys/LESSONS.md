> **⚠️ 修改前必须阅读或回忆 [RULE.md](../RULE.md)——它定义了本文档的维护规则和更新时机。**

# 经验教训 —— sys

**归属范围：** OS 封装的教训：输入线程、终端生命周期。

**编号：** `LSYS1`、`LSYS2`… 每个 crate 独立编号。
判据：教训的读者是 AI。**在任何 crate 都适用**的原则留根目录；只在某个 crate 的代码里有落点的归该 crate。


### LSYS1 — 订阅模式优于轮询（输入线程）

独立输入线程 + mpsc channel 比主线程直接 `event::read()` 阻塞更适合游戏循环：
- 主循环保持非阻塞
- 输入线程独立限流（16ms 轮询）
- 50ms 去重过滤 OS key-repeat
- 模态对话时用 AtomicBool 暂停输入线程，主线程直读 stdin

**原编号：** `LSYS1`（迁移前）

---

### LSYS2 — 键盘事件去重应从事件类型入手，不能仅依赖时间窗口

**问题背景：** 输入线程依赖 50ms 同键去重过滤重复按键。`KeyEventKind::Release` 事件与 `Press` 的 `key.code` 相同，去重逻辑只能靠时间窗口区分。但 Release 的到达时间受终端调度、事件缓冲、线程切换的影响——可能落在 49ms（被过滤）或 51ms（通过），结果完全不可控。

```rust
// ❌ 仅靠时间窗口去重——Release 在 50ms 边界上随机通过
if key.code == last_code && now - last_time < Duration::from_millis(50) {
    continue;
}
```

**错误做法：** 增大去重窗口。100ms 虽然能覆盖 Release，但会延迟 tap-tap 的响应，手感变钝。

**正确做法：** 从事件类型上区分 Press 和 Release，时间窗口只用于过滤 OS key-repeat：

```rust
// ✅ 事件类型过滤 Release，33ms 窗口只过滤 key-repeat
if key.kind != KeyEventKind::Press { continue; }
if key.code == last_code && now - last_time < Duration::from_millis(33) { continue; }
```

现代终端（Windows Terminal、Kitty、WezTerm）会为一次按键同时产生 `Press` 和 `Release` 两个事件，`key.kind` 区分了它们。传统终端（Conhost、xterm、SSH）的所有事件都是 `Press`，此过滤无害。

**为什么更好：** 时间窗口解决的是"同一个 Press 重复到达"的问题（OS key-repeat 硬件抖动的产物）。Release 是另一个事件类型，不该由时间窗口来过滤。两件事各司其职，不需要为了覆盖 Release 而把窗口拉到影响手感的大小。

**参见 ISSUES.md #I56**

**原编号：** `LSYS2`（迁移前）

---
