> **⚠️ 修改前必须阅读或回忆 [RULE.md](../RULE.md)——它定义了本文档的维护规则和更新时机。**

# 经验教训 —— tui

**归属范围：** TUI 后端的教训：外观定义、布局、终端约束。

**编号：** `LTUI1`、`LTUI2`… 每个 crate 独立编号。
判据：教训的读者是 AI。**在任何 crate 都适用**的原则留根目录；只在某个 crate 的代码里有落点的归该 crate。


### LTUI1 — 渲染层不应暴露游戏逻辑信息

**已修复（I11）：** `renderables` 遍历中 `else if explored[ey][ex]` 分支在已探索暗处画出了实体的实时位置，给玩家提供 X 射线透视。

**教训：** 渲染层只应画出玩家"应该看到"的信息。暗处实体位置依赖 `VisibleMemory`（记忆的上次位置），而非 `renderables`（实时位置）。任何在已探索暗处绘制实体当前位置的行为都是给玩家作弊。

**原编号：** `LTUI1`（迁移前）

---

### LTUI2 — 渲染时实时计算的独特色应在生成时固定存储于组件

**问题背景：** I35 中 `entity_color(entity.to_bits(), 0)` 在渲染层（timeline.rs + ui.rs）实时哈希计算怪物颜色。读档后 Entity ID 重建 → `to_bits()` 改变 → 读档前后颜色不一致。

**错误做法：**
```rust
// 渲染时从 entity bits 实时计算
let color = renderable_color(entity_color(entity.to_bits(), 0));
```

**正确做法：** 独特色在实体生成时计算一次，写入 `Renderable.color` 组件，存档/读档时随之持久化：

```rust
// spawn 时写入 Renderable.color（唯一一次计算）
world.get_mut::<Renderable>(entity).map(|mut r| {
    r.color = entity_color(entity.to_bits(), 0);
});
// 渲染时直接读取组件的颜色值
let color = renderable_color(r.color);
```

**为什么更好：** `Renderable.color` 被 `SavedMonster` 序列化（r/g/b 字段），读档后自动恢复。所有渲染路径读取同一来源，不受 Entity ID 变化影响。渲染层不再承担颜色计算职责。

**参见 ISSUES.md #I35**

**原编号：** `LTUI2`（迁移前）

---

### LTUI3 — UI 操作提示与按键处理器必须同源，不能一边显示一边不处理

**问题背景：** 背包详情页 UI 显示「r:使用/学习」（`ui.rs`），但 `process_inventory_key` 没有 `'r'` 分支——玩家被引导按一个无效键，技能卷轴系统整体断链（I61）。同轮 I57：UI 对无 slot 物品不显示「e:装备」，但处理器没有对应 guard，按 e 直接 panic。

**错误做法：** 在渲染层手写操作提示文案，在处理器层手写 match 分支，两处独立维护。

**正确做法：** 操作提示与按键处理共享同一份「可用操作」判定：

```rust
// 单一判定函数，渲染和处理器都调用它
fn available_actions(item: &ItemStack) -> Vec<&'static str> {
    let mut v = vec!["d:丢弃"];
    if item.def().is_some_and(|d| d.slot.is_some()) { v.push("e:装备"); }
    v
}
// ui.rs: 由 available_actions 生成提示行
// main.rs: match 分支的 guard 与 available_actions 的判定逻辑一致
```

**为什么更好：** 提示与处理是同一决策的两种消费。提示了但不处理 = 玩家按无效键；不提示但可触发 = 隐藏的崩溃路径。任何「显示 X 键」与「处理 X 键」的判定都必须来自同一来源，或至少在改动一侧时 grep 另一侧。

**参见 ISSUES.md #I61 #I57**

---

**原编号：** `LTUI3`（迁移前）

---

### LTUI4 — 布局函数不是"幂等"的：拿子区域再算一次布局，会静默算错

**问题背景：** 给 `presentation` 传相机视口时，用的是"地图区该多大"。后端把布局抽成了 `frame_areas(area)`（竖切调试面板、横切侧栏）。渲染函数收的是**整帧区域**，内部先算一次 `frame_areas` 拿到地图区；而相机视口的计算又调了 `map_viewport(area)`——它内部**再算一次** `frame_areas`。

**后果（一层套一层，但每一步都"看起来对"）：**

1. 渲染与相机各自算出来的地图区**不一样**：前者 48×22，后者 22×14（调试面板高度被减了两次）；
2. 相机按 46×20 的视口夹取，以为"视口比世界的一半还宽"，于是不再把玩家夹在中间；
3. 渲染只画 20×12 格 —— 玩家在世界坐标 (42,20) 时落在第 23 列，**被裁到画面外**；
4. 症状是"游戏能跑但玩家不见了"，而所有单测都是绿的：数据层（`SceneFrame`）完全正确，错的只是"谁该拿哪块区域"。

**错误做法：** 看到"玩家没画出来"就去查渲染的单元格逻辑（`cell_span` / 可见性 / `VisualMap`）。数据全对，查下去只会越查越糊涂——本人在这一步上花了十几轮。

**正确做法：**

- **布局只算一次**，把结果（`FrameAreas`）往下传；需要子区域的函数收**整帧区域**再自己取，绝不接受"已经切好的子区域"当输入；
- 给"相机以为的格数"和"实际画的格数"写一条**自洽断言**（`map_viewport(full) == frame_areas(full).map` 去掉边框）。这一条会直接抓住上面第 1 步；
- 再补一条**不可嵌套**断言（`frame_areas(map) != map`），把"拿子区域当输入"钉成明确的错误用法；
- 端到端测试要断"键位驱动出来的画面里有玩家"，而不是只断"数据里有玩家"。

**为什么更好：** 布局是**有状态含义的纯函数**——它的正确性依赖"输入是哪一个矩形"，而不是"输入是个矩形"。凡是这类函数（坐标变换、裁剪、分页），都该在签名或类型上区分"整帧"与"子区域"，否则误用不会报错，只会静默算错。判别信号：**数据层断言全绿、画面却不对**时，先怀疑"两块代码各自算了同一个派生量"。

**参见 REFACTOR.md §11.3 Phase G（R1 落地记录）；`tui/src/render/tests.rs::layout_is_computed_once_and_viewport_matches_the_drawn_area`**

**原编号：** `LTUI4`（迁移前）

---
