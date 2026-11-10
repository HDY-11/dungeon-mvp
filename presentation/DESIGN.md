> **⚠️ 修改前必须阅读或回忆 [RULE.md](../RULE.md)——它定义了本文档的维护规则和更新时机。**
>
> 每条记载一个设计取舍，含两个小节：
> - **决策**：当前选的方向和理由（罗盘）
> - **背景**：舍弃的方案和演化过程（护卫）
>
> 数值和游戏规则见 [GAME.md](../GAME.md)。

# 设计决策记录 —— presentation

**归属范围：** 集成层的决策：提取边界、相机、页栈与输入映射。

**编号：** `DsnP1`、`DsnP2`… 每个 crate 独立编号。
判据：决策**落点在哪个 crate 的代码/接口**里就归哪里；跨多个 crate 的分层/契约/路线决策留根目录。

---

## 主题索引（语义概览）

> 条目按编号顺序排列；本表按主题归类。`grep '^<!-- &'` 可列出全部条目与结论。

| 主题 | 条目 |
|---|---|
| 输入与页栈 | DsnP1 · DsnP2 · DsnP3 |


<!-- &DsnP1  tap-tap：同键第二次才确认；第一次只设预览 -->
### DsnP1 tap-tap 输入

**决策**

第一次方向键设预览状态（显示"移动(0,-1)"或"攻击"），第二次同键确认入队。不同键取消前一个预览。预览状态存为 ECS Resource（需要跨帧存在——第一次按键到第二次按键之间需要渲染更新）。

**为什么不是鼠标/菜单：** 终端 Roguelike 的核心交互是键盘盲打。菜单选择破坏了"看着地图敲键"的沉浸感。

**背景**

方向键同时承担"移动"和"攻击"两个语义。如果敲一下方向键就直接执行，玩家无法区分"我想走过去"和"我想攻击敌人"——攻击是移动的一个副效应。

**状态：** 已被取代（2026-11）。迁移后的输入管线没有实现 tap-tap 预览：`map_input_event`
单键直接产出 `PlayerCommand`（见 GAME Gm11）；本条保留迁移前的预览动机。

---

**原编号：** `DsnP1`（迁移前）
<!-- &/DsnP1 -->

<!-- &DsnP2  按键路由收进页栈：覆盖页优先吃键，页栈状态在 presentation -->
### DsnP2 按键系统：页栈（PageStack）

**决策**

所有 UI 页面通过页栈导航，栈顶决定按键路由和渲染行为。

**页栈模型（当前实现）：**

```
PageStack（presentation crate；装配层作为 ECS Resource 持有并每帧应用意图）
  Game                     ← 栈底，永不 pop
  Dialog                   ← q/Esc 退出确认
  Look                     ← ? 查看模式
  Inventory                ← e 背包（3×3 网格，DsnA5）
  ThrowAim                 ← t 投掷瞄准（DsnE28；无投掷物时先尝试自动装填）
```

- `PageKind::ThrowSelect` 只是 `render-api` 契约里的占位变体：当前流程不进入它，
  `t` 直接进 `ThrowAim`。
- 按键分派：`presentation::map_input_event(PresentationInput)` → `UiIntent`（页栈/焦点）
  或 `PlayerCommand`（世界行动）；`PageStack::apply` 落地 UI 意图，需要世界尺寸的
  （Look 光标移动）由装配层调 `input::apply_ui_intent`。
- 需要世界数据的页面由 `extract` 特判构造视图：`Inventory`（DsnA5）与 `ThrowAim`
  （DsnE28；`ThrowAimView` 的弹道与可达性由核心规则给出，presentation 只消费）。
- 页栈状态**不在 tui**：`tui` 只消费 `SceneFrame` / `UiView`。

**每个页面的责任界定：**

| 页面 | 按键处理 | 渲染 |
|------|---------|------|
| Game | 移动/攻击/施法/等待/投掷/背包/查看 | 地图 + 四栏布局（DsnA2）|
| Look | 方向键移动光标，? / x / Esc 退出 | 地图 + 光标高亮 + 信息面板 |
| ThrowAim | 方向键瞄准，Enter 投掷，x / Esc 取消 | 地图 + 轨迹 + 落点提示 |
| Inventory | 4 焦点轮转（DsnA5），Enter/r/d 动作 | 全屏替换地图（`is_fullscreen=true`）|
| Dialog | Y 确认，N/Esc 取消 | 叠加层（对话框风格） |

**为什么不是单一 match / 独立模态：**
- 单一 match（旧方案）：`process_key` 内 ~200 行 match 所有按键，加入新 UI 需要修改多处
- 独立模态（旧方案）：`open_look_mode` 等有自己的 `event::read()` 循环，绕过 30FPS 框架
- 页栈：每个页面一个 handler，共处相同框架，新 UI 只需加一个新枚举变体

**与渲染管道的关系：**
- 页面状态在 `presentation` 的 `PageStack`；装配层把它作为 ECS Resource 插入，
  每帧 clone → apply 意图 → 写回（`App` 的借用规避方式）
- `extract` 读页栈 + 世界，产出 `SceneFrame`（含 `focus`、`events_scroll`）
  与 `UiView`（页面视图模型）
- 全屏页（Inventory）跳过游戏 UI 渲染，非全屏页叠加在游戏 UI 之上

**Background**

旧方案将查看模式、投掷瞄准、背包分别实现为独立的阻塞式 UI，各自有 `event::read()` 循环。这在 30FPS 框架下不协调——它们无法参与固定帧率渲染，且 `modal_flag` 需要暂停输入线程。

ISSUES.md A14 记录了查看模式的架构问题。A16 的 `InputBuffer` 资源在页栈框架下变为非必要——按键直接由栈顶处理器消费，不需要中间缓冲。`modal_flag` 在所有页面迁移完成后已为死代码。

---

## 五、数据与方法

**原编号：** `DsnP2`（迁移前）
<!-- &/DsnP2 -->

<!-- &DsnP3  投掷瞄准页：t → OpenThrow；装配层自动装填与自动瞄准；extract 特判构造 ThrowAimView -->
### DsnP3 投掷瞄准页（ThrowAim）

**决策**

- `t` 产出 `UiIntent::OpenThrow { x, y }`；由装配层处理：先确认副手有 `Throw` 组件，
  否则调用 `ecs_core::load_throwable_from_pack` 自动装填；失败只写日志、不开页。
- 开页自动瞄准：优先信息栏当前选中实体所在格（若核心 `throw_reachable` 可达），
  否则玩家脚下；方向键手动移动光标，Enter 确认（转 `PlayerCommand::Throw { dx, dy }`），
  x / Esc 取消。
- `ThrowAimView` 由 `extract` 特判构造（有世界与 `Map`）：`path` 用核心 `throw_path`、
  `valid` 用核心 `throw_reachable`、`target` 取落点实体快照；presentation 不复制任何
  射程/视线规则（单一真相源）。
- `PageKind::ThrowSelect` 保留为契约占位，当前流程不使用。

**背景**

迁移前的投掷页是"先选投掷物、再瞄准"的两段流程（DsnX6/DsnX7 的 `ThrowSelect`）。
实现时按用户拍板的"副手持、主手投"直觉模型，简化为一次 `t` 进入瞄准。

**代价**

- 自动装填与开页耦合在装配层，`t` 的失败反馈只有一行日志。
- 瞄准页的可投信息依赖核心的 `throw_reachable`；若规则改口径需同时改 HUD 文案。

**状态：** 已落地（DsnE28，2026-11）。

**关联：** DsnE28 / DsnP2 / DsnA5 / Gm9 / Gm14
<!-- &/DsnP3 -->

