> **⚠️ 修改前必须阅读或回忆 [RULE.md](../RULE.md)——它定义了本文档的维护规则和更新时机。**
>
> 每条记载一个设计取舍，含两个小节：
> - **决策**：当前选的方向和理由（罗盘）
> - **背景**：舍弃的方案和演化过程（护卫）
>
> 数值和游戏规则见 [GAME.md](../GAME.md)。

# 设计决策记录 —— render-api

**归属范围：** 渲染契约的决策：视图模型边界、字段语义与版本策略。

**编号：** `DsnA1` 起逐个递增（DsnA1–DsnA5 五条）。
判据：决策**落点在哪个 crate 的代码/接口**里就归哪里；跨多个 crate 的分层/契约/路线决策留根目录。

---

## 主题索引（语义概览）

> 条目按编号顺序排列；本表按主题归类。`grep '^<!-- &'` 可列出全部条目与结论。

| 主题 | 条目 |
|---|---|
| 渲染契约 | DsnA1 · DsnA2 · DsnA3 · DsnA4 · DsnA5 |
| 界面几何与焦点 | DsnA2 |
| 事件行与时间轴 | DsnA3 |
| 信息栏 | DsnA4 |
| 背包网格 | DsnA5 |


<!-- &DsnA1  后端无关只读契约 SceneFrame/VisualKey/UiView/InputEvent；tui 不依赖 ecs_core -->
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
> （上述 `tui/src/scene.rs` 已在 R1 删除，本条是当时的动因记录。）

**关联：** DESIGN.md DsnX14（渲染分层总览 / crate 布局）| DsnT1（渲染优化属于 TUI 后端内部）| DsnP2（页栈状态放 presentation，渲染放后端）

**状态：** 已定 — `render-api` v1 已落地（34 个测试通过，`cargo clippy -p render-api -D warnings` 干净）；R1（`presentation` 提取层 + `tui` 去 `ecs_core` 依赖）亦已落地，见 DESIGN DsnX14。


---

**原编号：** `DsnA1`（迁移前）
<!-- &/DsnA1 -->

---

<!-- &DsnA2  四栏布局与焦点进契约：game_layout 是唯一布局权威，SceneFrame.focus 由 PageStack 供给；CONTRACT_VERSION 3 -->
### DsnA2 布局契约：四栏几何与栏位焦点（已落地，契约版本 3）

**决策**

界面分成固定四栏：**摄像机（左上）、信息（右上）、事件（左下）、状态（右下）**。
几何定义放进契约层 `render-api/src/layout.rs`，而不是留在 `tui`：

- `Rect { x, y, width, height }`：后端无关的矩形（tui 再转成 ratatui 的 `Rect`）。
- `Pane { Camera, Info, Status, Events }`：栏位身份；`next()` 按**顺时针**
  轮转（摄像机 → 信息 → 状态 → 事件），`ALL` 给出全部四栏。
- `GameLayout { camera, info, status, events }` + `rect(pane)` + `pane_at(x, y)`：
  一次算好四栏矩形，**焦点轮转与鼠标命中读同一份几何**。
- `game_layout(surface)`：右栏占 42% 宽（下限 26 列，且保证地图 ≥24 列），
  顶行占 60% 高（下限 8 行，且保证事件栏 ≥6 行）；宽高低于 60×16 时退化为
  「只有地图，其余矩形为空」。`MIN_SURFACE = 80×24` 是受支持的最小终端。
- `SceneFrame.focus: Pane`：当前焦点栏。焦点是 **UI 状态**，由 `presentation`
  的 `PageStack` 持有、每帧提取进快照；契约仍是只读视图模型。这样 TUI 与未来
  GPU 后端画同一个焦点框，不需要各自维护状态。

`CONTRACT_VERSION` 由 2 升到 3（新增布局模块与 `SceneFrame` 字段）。

**为什么这该进契约**

此前布局的唯一权威是 `tui/src/render/mod.rs::frame_areas`。这带来三个后续成本：
鼠标命中要按终端格反推栏位、`Tab` 轮转要在后端里再写一份顺序、未来 GPU 后端要
重新推导同一套矩形。布局是「这一帧各栏在哪」的**共享事实**，而共享事实属于契约；
后端只负责把契约矩形画成自己的像素/单元格。

**背景（舍弃的方案）**

- **布局留在 tui**：现在能跑，但鼠标点击与焦点顺序都会变成 tui 私有知识，
  第二个后端出现时必然分叉。否决。
- **布局做成可配置（用户调比例）**：没有需求证据，且会把"哪栏能点"变成运行时
  数据；这一轮不做。等有真实的窗口/分屏需求再看。
- **栏位数量可变（列表化 panes）**：V1 固定四栏，`Pane` 用枚举而不是字符串键，
  让"少画一栏/多画一栏"在编译期就要改代码，而不是悄悄少画。

**状态**

已落地阶段 1：契约（6 条测试）+ `PageStack` 焦点与 Tab + tui 四栏渲染
（40 条测试）。事件栏当前渲染历史事件（旧在上、新在下）；「已定未落」的进行中
条目、0 刻度线与关注集属于后续事件模型迭代。信息栏暂按实体稳定顺序列出，
按距离/方位的排序与 `Observable` 标记同属后续阶段。

**关联：** DsnA1（渲染契约）| DESIGN.md DsnP2（页栈状态在 presentation）|
DESIGN.md DsnX14（渲染分层）
<!-- &/DsnA2 -->

---

<!-- &DsnA3  事件栏一条列：EventLine{level,text,running}，Done 旧→新 + Running 剩余降序由 presentation 排；CONTRACT_VERSION 4 -->
### DsnA3 事件行契约：`EventLine` 与"事实 / 进行中"同行（已落地，契约版本 4）

**决策**

`SceneFrame.log: Vec<LogLine>` 换成 `events: Vec<EventLine>`，一行事件的形状是
`{ level, text, running }`：

- `EventLine::done(level, text)`：已经落定的事实（战斗/物品/系统日志）。
- `EventLine::running(level, text)`：已经开始、还没落定的行动；`text` 里由呈现层
  格式化剩余 AV（"哥布林举起了武器（140AV）"）。
- **排序不在后端**：`presentation` 按「Done 旧→新，再 Running 剩余时间降序」拼好
  一列，栏底即 0 刻度（DsnE21）。TUI/GPU 只按给定顺序画，`running` 用不同样式
  （TUI 用暗色）区分。
- `CONTRACT_VERSION` 由 3 升到 4。

**背景（舍弃的方案）**

- **事实与预告别放两个列表**：`log` + `pending` 两个字段会让"现在/过去"的分界由
  每个后端各自拼，排序规则迟早分叉；一列 + `running` 位只表达"这一行是什么"，
  顺序仍是契约给的。
- **在 Render 层把剩余时间换算成进度条**：契约只给剩余 AV 的文本，进度条留作
  后续视觉决策；现在写成文本足够玩家读出"还剩多久"。
- **`LogLine` 保留兼容**：没有第二个消费者，保留两套行类型只会让后端要写两个
  分支。直接替换。

**状态**

已落地：契约（`EventLine` / `contract.rs` 冒烟）、`EventLog` 容量 200（只计事实）、
presentation 投影（事实 + 玩家视野内的 `ActiveAction`）、tui 事件栏渲染与测试。

**关联：** DsnA1（渲染契约）· DESIGN.md DsnE21（事件时间轴与背包格）·
GAME.md Gm14（界面）
<!-- &/DsnA3 -->

---

<!-- &DsnA4  信息栏契约：InfoView{entries,selected}，候选与排序由 presentation 决定；选中行是 UI 状态；CONTRACT_VERSION 5 -->
### DsnA4 信息栏契约：`InfoView` 与"可关注对象"列表（已落地，契约版本 5）

**决策**

`SceneFrame` 在 `hud` 之后加 `info: InfoView`：

- `InfoView { entries: Vec<EntityId>, selected: Option<usize> }`，`selected_id()`
  取当前选中实体的 id。
- **候选与排序由 `presentation` 决定**：`entries` 已经按「近→远，同距按方位
  顺时针（正上=0）」排好（DsnE21）；后端只按给定顺序画，不再自己过滤、排序。
- `selected` 是**界面状态**（`PageStack.info_cursor` 每帧投影，越界夹到最后一行），
  不是"玩家关注了谁"的规则状态；当前用于信息栏选中行标记与地图反色高亮。
- `CONTRACT_VERSION` 由 4 升到 5。

**背景（舍弃的方案）**

- **让后端从 `entities` 里猜"可关注对象"**：过滤规则会在每个后端各写一份并
  迟早分叉；"值得关注"由内容决定（显式 `Observable` 标记），不是视觉属性。
- **把"关注集"塞进契约**：关注集是后续"事件只报关注对象"的规则状态，属于
  ECS/规则层；先只在 UI 上暴露"选中行"，等过滤规则落地再接。

**状态**

已落地：`InfoView` 契约与 `CONTRACT_VERSION 5`、`Observable` 标记（怪物/楼梯/
地面物品/尸体）、presentation 距离+方位排序与选中夹取、tui 信息栏渲染与地图
反色高亮、端到端 Tab+↑↓ 用例。

**关联：** DsnA1（渲染契约）· DsnA2（四栏布局与焦点）·
ecs_core/DESIGN.md DsnE21 · GAME.md Gm14
<!-- &/DsnA4 -->

---

<!-- &DsnA5  背包网格：InventoryView 换成 Nav1/Nav2/Context1/Context2/Detail/Tip 的 3×3 形状，四个焦点位顺时针轮转；CONTRACT_VERSION 6 -->
### DsnA5 背包网格契约：`InventoryView` 的 3×3 形状（已落地，契约版本 6）

**决策**

背包页按玩家设计稿改为前端式网格（DsnE21 阶段 4b）：

- `InventoryView` 去掉两栏 `panels` 与 `InventoryFocus::Panel/Detail`，换成
  `nav1: Vec<InventoryNav>` / `nav2` / `nav1_selected` / `nav2_selected` /
  `context1: ListView` / `context2: ListView` / `detail` / `tips: Vec<UiTextLine>`。
- `InventoryNav { Body, Pack, Corpse, Ground }` 是页签（身体/行囊/尸体/地面）；
  `Body/Pack` 属 `Nav1`，`Corpse/Ground` 属 `Nav2`；`InventoryNavGroup::context()` 给出
  "这个组的内容画在哪一条 Context 栏"。
- 焦点 4 个：`InventoryFocus { Nav1, Nav2, Context2, Context1 }`，`next()` **顺时针**
  （Nav1 → Nav2 → Context2 → Context1 → Nav1）。Nav2 不存在时由 `presentation` 跳过
  Nav2 与 Context2 两个焦点位（合同只定义轮转顺序，不定义跳过规则）。
- `tips` 是契约文本（不写死键位）：后端逐行画出即可，排版（颜色/分组）归 tui。
- `CONTRACT_VERSION` 由 5 升到 6。

**背景（舍弃的方案）**

- **保留两栏 `panels` + 给 `InventoryPanel` 再加变体**：两栏装不下"导航/内容/详情/提示"
  四个区域；再加变体会让后端用 `match` 猜布局。
- **把焦点顺序也写进契约的枚举迭代**：焦点跳过规则依赖"Nav2 有没有页签"（世界状态），
  契约只提供 `next()` 的固定顺序，跳过由 presentation 的 `PageStack` 决定。
- **页签文案写进契约**：文案（身体/行囊/尸体/地面）是内容层的事，契约只给枚举与
  `label()`，后端不拼中文。

**状态**

已落地：`InventoryView` 网格契约与 `CONTRACT_VERSION 6`、`PageStack` 背包焦点状态
（4 焦点/两条 Context 光标/Nav2 长度同步）、presentation 提取层按页签建
Context 与 `InventoryTarget`、tui 网格渲染器（页签/两条 Context/右侧或底部详情/
底部 Tip、窄屏堆叠、全角列宽）与端到端键盘用例。

**关联：** DsnA1（渲染契约）· DsnA2（四栏布局与焦点）·
ecs_core/DESIGN.md DsnE21 · GAME.md Gm7（背包）
<!-- &/DsnA5 -->




