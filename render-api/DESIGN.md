> **⚠️ 修改前必须阅读或回忆 [RULE.md](../RULE.md)——它定义了本文档的维护规则和更新时机。**
>
> 每条记载一个设计取舍，含两个小节：
> - **决策**：当前选的方向和理由（罗盘）
> - **背景**：舍弃的方案和演化过程（护卫）
>
> 数值和游戏规则见 [GAME.md](../GAME.md)。

# 设计决策记录 —— render-api

**归属范围：** 渲染契约的决策：视图模型边界、字段语义与版本策略。

**编号：** `DsnA1`、`DsnA2`… 每个 crate 独立编号。
判据：决策**落点在哪个 crate 的代码/接口**里就归哪里；跨多个 crate 的分层/契约/路线决策留根目录。

---

### DsnA1 渲染契约 `render-api`：后端无关的 SceneFrame（已落地 v1）

**决策**

渲染器可替换的关键不是“把渲染写成插件”，而是“后端与游戏逻辑之间的只读数据契约”。新增 `render-api` crate：

- 只依赖 `bevy_ecs`（用于 `Resource` derive）与 std，不依赖 `core` / ratatui / wgpu / `bevy_app`。
- 契约核心：`SceneFrame`（每帧从 ECS 提取的场景快照）、`VisualKey`（语义外观键）、`UiView`（页面级视图模型）、`InputEvent` / `InputQueue` / `SurfaceInfo`（后端无关输入与表面尺寸）。
- `VisualKey` 只表达“这是什么”（`Player` / `Monster(id)` / `Tile(id)` / ...），不包含 glyph / 颜色 / 纹理；TUI 与未来 GPU 各自用自己的 catalog 映射外观。
- 契约是只读视图模型：不包含规则、不持久化、后端不得反向修改游戏状态；`CONTRACT_VERSION` 记录结构版本。

依赖方向：

```
core ──> presentation ──> render-api <── tui / gpu
```

`tui` / `gpu` 不得依赖 `core`，这是用 Cargo 依赖强制执行的边界。

**背景**

当前 `tui/src/scene.rs` 直接查询 `core` 组件，`render_game(frame, &mut World)` 把后端与 World 焊死；换 GPU 必须重写提取逻辑。Bevy 的插件系统解决“装配”，但它的渲染可替换性来自“主世界 → Extract → 渲染世界”的分离；本决策只借用这个分离思想，不引入完整 render sub-app。

**关联：** REFACTOR.md §1.1 | DESIGN.md DsnT1（渲染优化属于 TUI 后端内部）| DsnP2（页栈状态放 presentation，渲染放后端）

**状态：** `render-api` v1 已落地（34 个测试通过，`cargo clippy -p render-api -D warnings` 干净）。下一步：`presentation` 提取层 + `tui` 去 `core` 依赖。


---

**原编号：** `DsnA1`（迁移前）
