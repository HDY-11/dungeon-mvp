> **⚠️ 修改前必须阅读或回忆 [RULE.md](../RULE.md)——它定义了本文档的维护规则和更新时机。**
>
> 每条记载一个设计取舍，含两个小节：
> - **决策**：当前选的方向和理由（罗盘）
> - **背景**：舍弃的方案和演化过程（护卫）
>
> 数值和游戏规则见 [GAME.md](../GAME.md)。

# 设计决策记录 —— tui

**归属范围：** TUI 后端的决策：绘制方式、外观目录、终端布局。

**编号：** `DsnT1` 起逐个递增（本 crate 目前有两条）。
判据：决策**落点在哪个 crate 的代码/接口**里就归哪里；跨多个 crate 的分层/契约/路线决策留根目录。

---

## 主题索引（语义概览）

> 条目按编号顺序排列；本表按主题归类。`grep '^<!-- &'` 可列出全部条目与结论。

| 主题 | 条目 |
|---|---|
| 渲染实现 | DsnT1 · DsnT2 |


<!-- &DsnT1  Buffer 直写 + 持久 Canvas + 背景色优先；与外观无关的全部留在后端 -->
### DsnT1 渲染架构：Buffer 直写替代 Paragraph + 持久 Canvas + 背景色优先

**决策**

三项独立决策，按实施顺序排列：

**① 地形背景色全面化（立刻可做）**
所有 Tile 提供背景色，不仅仅是水体。`Tile::bg_color()` 改为对 Wall/Floor/Stalactite 也返回颜色值，而非仅水体有背景：

```
Wall:        bg=None → bg=(50, 50, 60)    铁灰
Floor:       bg=None → bg=(20, 22, 25)    近黑微亮
Stalactite:  bg=None → bg=(60, 55, 20)    暗黄
```

字符 glyph 不变（保留 `#`/`.`/`~`/`≈`），但背景色提供了"画布"层，视觉从"符号浮在黑纸"变为"符号嵌在纹理上"。改 6 行，零架构影响。

**② Buffer 直写替代 Paragraph/Line/Span（近期重构）**
当前渲染流程为全量重绘路径：

```
ECS queries → Vec<Vec<(char,Color,Color)>> → Vec<Line> (800 Span 分配)
  → frame.render_widget(Paragraph::new(lines), area) → ratatui 内部转为 Buffer Cell
```

重构后直写 `frame.buffer_mut()`：

```
ECS queries → 直接写入 Buffer Cell
```

跳过 Paragraph/Line/Span 中间层，消除每帧 ~800 次小分配。ratatui 的 layout（Layout/Constraint/Block）仍可使用，只绕过文本 widget 层。ratatui 的 ANSI diff 仍然在下游工作。

**③ 持久 Canvas + dirty tracking（中期优化）**
引入持久化的帧缓冲 `Canvas` 作为 ECS Resource，而非每帧重建：

```
Canvas: cells: Vec<Vec<Cell>>, dirty: HashSet<(usize, usize)>
```

每帧流程：
- 行动推进（实体移动/攻击/死亡）→ 标记对应 cell dirty
- 渲染时只重建 dirty cell，写入 Buffer
- 非 dirty cell 直接拷贝到 Buffer（memcpy，不经过任何逻辑判断）

**④ 半块字符叠加（配合背景色策略）**
实体覆写地形的渲染路径中引入半块字符技术（参考 Brogue 终端渲染），在一个字符格内同时显示实体和地板：

```rust
// 当前：实体完全覆盖地形
cell.glyph = entity.glyph           // 实体的 'r'
cell.fg    = entity_color            // 怪物色
cell.bg    = terrain_color           // 地形色（背景层）

// 半块字符：一格显示两层
cell.glyph = '▄'                     // 下半块
cell.fg    = entity_color            // 实体色作为前景
cell.bg    = terrain_color           // 地形色作为背景
```

效果：格子上半截显示地板纹理，下半截显示实体。同一格可以区分"什么地面 + 谁站在上面"。视觉密度翻倍，不增加格子数。与①背景色策略配合使用——背景色提供了"地面层"，半块字符提供了"叠加层"。

**注意：** 仅在地形有背景色时有效（即①实施之后），否则 `bg` 为 `Color::Reset` 与 `fg` 无区分。在已探索但不可见区域不应使用半块——灰色滤镜会抹平两层差异。

**30fps 固定帧率（已实现）：** 主循环改为定时器驱动，33ms 帧间隔。每帧批量消费所有待处理输入，渲染频率固定为 ~30FPS。动画效果的前提条件已满足，为后续伤害数字淡出、弹道尾迹、Buff 闪烁铺路。实现细节：`src/main.rs` 主循环。

**背景**

对 Brogue 终端渲染的分析引发此次设计讨论。核心发现：当前渲染慢的原因不是 ratatui（它的 ANSI diff 机制在终端输出层面已经做到了增量），而是"全量重建 + 文本 widget 封装"导致的上游浪费。Brogue 在纯终端时代用 Canvas 缓冲 + 增量 flush + 固定帧率实现了流畅画面，其本质是"只输出变化"而非"全量重建"。

**状态：** 部分落地（2026-11）——① 地形背景色已落地；② 仅地图段直写 `frame.buffer_mut()`，
其余 UI 仍用 `Paragraph`；③ 持久 Canvas + dirty tracking 未实施。迁移后 `tui` 的输入是
`SceneFrame`，本条中"ECS queries → 文本"为迁移前语境；迁移后的渲染分层见 DsnT2。

---

**原编号：** `DsnT1`（迁移前）
<!-- &/DsnT1 -->

<!-- &DsnT2  迁移后渲染分层：只消费 SceneFrame/UiView；四栏、事件/信息栏、投掷叠加层按契约画 -->
### DsnT2 渲染分层与栏位渲染（迁移后）

**决策**

- `tui` 只消费 `SceneFrame` 与 `UiView`，不读世界（R1 边界）；不再有 ECS queries → 文本的旧管线。
- 四栏布局按契约 `render_api::layout` 画（DsnA2）：摄像机 / 信息 / 事件 / 状态；
  `SceneFrame.focus` 决定焦点边框。
- 事件栏按 DsnA3/DsnA6：等级前缀、暗色进行中行、按滚动偏移裁窗口、标题 `事件 ↑N`；
  状态栏画 `HudView` 的 HP/MP/EXP、扩展行（中毒/法术/装备）与键位提示。
- 信息栏按 DsnA4：每项两行（名字 + 类型·血量条·距离），选中行反色，项多时窗口跟随。
- 投掷瞄准页按 DsnE28：在地图之上叠加 `*` 弹道、`X` 落点与底部提示行。
- 调试面板不再渲染（DsnE25）；`DevLogBuffer` 仍由装配层灌入，仅作诊断保留。
- CJK 显示宽度按全角 2 格计算，避免 `Paragraph` 换行算法在 CJK 之间插入空格。

**背景**

迁移前（DsnT1 语境）tui 直接查询 ECS；拆出 `render-api` / `presentation` 后，后端只负责
"把视图模型画到终端"，布局几何、页面状态、事件排序都不在 tui。

**代价**

- `SceneFrame` 是每帧快照，无法做旧管线里的持久 Canvas / dirty tracking（DsnT1 ③ 未实施）。
- 栏位渲染依赖契约字段；新增栏位信息要先改 `render-api`。

**状态：** 已落地（DsnA2/A3/A4/A6、DsnE25、DsnE28，2026-11）。

**关联：** DsnA2 / DsnA3 / DsnA4 / DsnA5 / DsnA6 / DsnE21 / DsnE25 / DsnE28 / DsnT1
<!-- &/DsnT2 -->

