> **⚠️ 修改前必须阅读或回忆 [RULE.md](RULE.md)——它定义了本文档的维护规则和更新时机。**
>
> 每条记载一个设计取舍，含两个小节：
> - **决策**：当前选的方向和理由（罗盘）
> - **背景**：舍弃的方案和演化过程（护卫）
>
> 数值和游戏规则见 [GAME.md](../GAME.md)。

# 设计决策记录 —— 根目录（跨 crate）

**归属范围：** 跨 crate 的决策：分层、契约、迁移路线、玩法系统边界、流程。

**编号：** `DsnX1`、`DsnX2`… 每个 crate 独立编号。
判据：决策**落点在哪个 crate 的代码/接口**里就归哪里；跨多个 crate 的分层/契约/路线决策留根目录。

---

## 主题索引（语义概览，便于按主题跳转）

> 条目按编号顺序排列（编号是稳定 ID、不复用）；本表按**主题**归类。
> 与 `&ID` 标记配合：`grep '^<!-- &' DESIGN.md` 一次列出全部条目与一句话结论。

| 主题 | 条目 |
|---|---|
| 分层与边界 | DsnX1 · DsnX2 · DsnX13 · DsnX15 |
| 渲染契约 | DsnX14 |
| 数据与存档 | DsnX10 · DsnX17 |
| 物品 / 装备 / 技能 / 合成（多为未迁移） | DsnX3 · DsnX4 · DsnX5 · DsnX6 · DsnX7 · DsnX8 · DsnX9 · DsnX12 |
| 流程与文档 | DsnX11 · DsnX16 |
| 路线 | DsnX18 |


<!-- &DsnX1  四层 crate 拆分：ECS 核心 / 渲染契约 / 集成层 / 后端 -->
### DsnX1 四层 Crate 拆分

**决策**

按**关注点变化速度**分层。游戏数据（core）变化最慢，渲染（render）变化最快。行动规则（action）独立于世界生命周期（world），世界生命周期独立于渲染。

```
core ← action ← world
  ↕        ↗
render ───╯（依赖 core + action，因 timeline 使用行动类型）
```

| 层 | 变化原因 | 关键依赖 |
|---|---|---|
| core | 很少改动（数据类型、公式） | 无内部依赖（自含） |
| action | 添加新行动/技能时需要改动 | core（基础类型） |
| world | 添加新地图特征/怪物行为时 | core + action（行动队列） |
| render | TUI库升级/换框架时需要改动 | core + action（仅行动类型） |

**背景**

render 自 `timeline.rs` 使用 `ActionQueue`/`ActionKindV3`/`PlayerPreview` 后，增加了对 action 的依赖。这是 render 依赖链的"例外"——仅限于类型引用，不依赖行动执行逻辑。如果未来将行动类型提取为独立 crate，render 可以切到那个 crate，取消对 action 的依赖。

---

**原编号：** `DsnX1`（迁移前）
<!-- &/DsnX1 -->

<!-- &DsnX2  移除全局 World，改显式 &World/&mut World 参数传递 -->
### DsnX2 移除全局 World，改为参数传递

**决策**

所有函数改为显式接收 `&World` / `&mut World` 参数。明确表达读写意图，不需要文档约束。不使用全局 `OnceLock<RwLock<World>>`，不使用 `thread_local!`。

不选 `thread_local!` 的理由：只适合线程生命周期和 World 一致的单线程场景。如果将来要分线程（AI 计算、异步 IO），thread_local 的 World 会无法访问。

**背景**

曾经有一个 `OnceLock<RwLock<World>>` 全局对象 + `world!()` 宏（提交 `41f37f0` 引入，`f43b502` 移除）。RwLock 的不可重入性导致两种死锁模式：
- `advance_action_queue` 持锁调用 `execute_*`
- `render_ui` 持锁调用子函数

第一次遇到时打了"分两阶段取锁"的补丁，但设计上不可靠：任何一个新函数如果忘记遵守约定就会引入死锁。Rust 的借用检查器在编译期保证不会同时持有 `&mut` 引用——比任何运行时锁策略都更强。

---

**原编号：** `DsnX2`（迁移前）
<!-- &/DsnX2 -->

<!-- &DsnX3  物品系统用 Registry + ItemStack + LootTable（未迁移） -->
### DsnX3 物品系统 — Registry + ItemStack

**决策**

物品定义存入 `assets/items.json`，运行时加载为 `ItemRegistry`（`OnceLock` 全局单例）。背包里只存 `(item_id, count)`。装备直接持有 ItemStack，不占背包空间。

**选择 OnceLock 而非 ECS Resource 的理由：** 物品注册表是纯数据，没有任何 ECS 依赖。放到 Resource 里意味着所有读操作必须先拿 World 引用，增加不必要的耦合。

**为什么装备不占背包格子：** 装备是角色的属性，不是背包的内容。装备直接持有 ItemStack 使装备和背包两个关注点正交。

**背景**

背包格子里存完整的物品定义（名称、描述、属性、图标...）→ 每个格子占用大量内存，且无法实现"同一个物品有多个"而不复制。

---

**原编号：** `DsnX3`（迁移前）
<!-- &/DsnX3 -->

<!-- &DsnX4  拾取物 Instance 独立成实体，便于渲染与交互（未迁移） -->
### DsnX4 ItemPickup 作为独立实体

**决策**

地面上的物品是独立的 ECS 实体，带 `ItemPickup`（存 ItemStack）、`Position`、`Renderable` 三个组件。不是背包 Inventory 的一部分。

实体组件模式使物品可以像其他实体一样被 FOV 系统看到、被 VisibleMemory 记录、被碰撞图排除（`rebuild_occupancy` 中 `pickup.is_none()` 过滤了它们）。

**背景**

如果物品是 Inventory 的一个列表，FOV 渲染需要特殊处理"如何渲染一个不在 ECS 中的位置"。拾取时需 despawn 实体、掉落需 spawn 实体，但频率低（每层几次），开销可忽略。

---

**原编号：** `DsnX4`（迁移前）
<!-- &/DsnX4 -->

<!-- &DsnX5  装备槽配对失败时背包满的处理顺序（未迁移） -->
### DsnX5 装备槽配对背包满的处理

**决策**

原子操作语义——要么全部成功，要么全部回滚：

1. 从背包移除 ItemStack
2. 装备到槽位（可能换下旧装备）
3. 尝试把换下的装备加入背包
4. 如果加入失败（背包满），**放弃本次操作，恢复原状**（新装备放回背包，旧装备放回槽位）

如果允许"装备成功但旧装备消失"，就会永久丢失物品——这是玩家最痛恨的游戏体验。

**背景**

为什么不先卸载再装备：如果先卸载旧装备（背包空出一个位置）再装备新装备，玩家可以用"装备格"作为额外背包空间——这是一个合法的游戏机制，但增加了实现复杂度。

---

**原编号：** `DsnX5`（迁移前）
<!-- &/DsnX5 -->

<!-- &DsnX6  主手/副手双槽，投掷物走副手（未迁移） -->
### DsnX6 主手/副手系统

**决策**

```
Equipment:
  main_hand  ← 武器（锈铁剑）       — 攻击力的主要来源
  off_hand   ← 盾/石子/副武器       — 防御加成或投掷来源
  armor      ← 盔甲（皮甲）         — 防御加成
  ring       ← 戒指（攻击戒指）      — 属性加成
```

投掷动作期间，主手武器不参与伤害计算——投掷伤害只取决于投掷物本身。限制副手为盾牌/投掷物（不双持），双持需要两套攻击判定逻辑，对 MVP 来说过重。

**背景**

投掷需求的引入暴露了旧 Equipment 结构的问题：只有一个 `weapon` 槽位，无法区分"正在攻击的武器"和"正在投掷的石子"；木盾要塞进 `Armor` 槽，逻辑上不合理。

旧投掷流程（`~~旧方案——已由 DsnX7 替代~~`）：
```
t 键 → 列出 throwable 物品 → 选择目标 → 自动装副手 → 瞄准模式 → Enter入队
```

---

**原编号：** `DsnX6`（迁移前）
<!-- &/DsnX6 -->

<!-- &DsnX7  投掷作为一等 AV 行动，不再旁路（未迁移） -->
### DsnX7 投掷作为一等 AV 行动

**决策**

投掷改为与其他玩家行动相同的[生命周期](RULE.md#行动系统-v3)：

```
瞄准确认 → enqueue(Throw{tx,ty}, AV) → return true
                                         → advance_and_settle
                                           → advance_action_queue
                                             → execute_entry
                                               → execute_throw
```

- 投掷作为玩家行动，在 `PlayerAction` 枚举中与 Move/Wait/Skill 同级
- 引入 `CanThrow` 组件（与 `CanMove`/`CanWait` 同级），支持怪物也拥有投掷能力
- 玩家投掷由按键触发 + 页栈 ThowSelect/ThrowAim 页面管理瞄准流程
- 目标用坐标 `(tx, ty)` 而非 Entity——执行时目标格可能没有怪物，坐标天然可序列化
- 耗时 190ms（快于近战移动，确保先于怪物追击 AV 执行，使远程攻击在怪物移动前命中）

**为什么不保留旁路：** 快速路径不是"更快执行"，而是"不排队、不推进世界"。其他所有玩家行动都会触发世界推进——这是回合制游戏的基本契约。

| 关注点 | 旧设计（Batch 1） | 新设计 |
|--------|-----------------|--------|
| 执行时机 | Enter 即执行 | 入队等待 AV=0 |
| 世界推进 | `Ok(false)`，跳过 `advance_and_settle` | `Ok(true)`，正常推进 |
| 游戏逻辑位置 | `src/throw.rs`（应用层） | `dungeon-action/src/execute.rs` |
| 暴击率路径 | 手动遍历 armor/ring | `ops::equipment_bonus()`（与近战一致） |
| 副手消耗 | 内联 2 处重复 | `consume_off_hand()` 共享函数 |
| 存档副手 | `off_hand: None` 硬编码 | 序列化/反序列化 |

**背景**

投掷在 Batch 1 中以"旁路"实现——不进 ActionQueue、即时执行、返回 `Ok(false)` 跳过世界推进。ISSUES.md D13/D14A 记录了此设计的问题：世界时间不推进、暴击率绕过 `equipment_bonus`、游戏逻辑在应用层（I31 ⑦）、存档丢失副手（A17）。

**关联 Issue：** ISSUES.md #A11 #D13 #D14A #A17 #I31 ⑦ #G14

---

**原编号：** `DsnX7`（迁移前）
<!-- &/DsnX7 -->

<!-- &DsnX8  技能系统重设计：数据驱动的四层结构（设计中·未迁移） -->
### DsnX8 技能系统重设计（设计中）

**决策**

技能不从职业获取，改为从地图道具（技能卷轴）学习。自由组合技能，无职业锁定。

- 职业 → 仅提供初始属性和装备，不锁定技能
- 技能 → 通过卷轴学习/升级，存储在 `Skills` 组件中。每张卷轴教一种技能，重复学习提升熟练度
- 熟练度影响技能强度：治愈 +3HP/级，护盾/狂暴 +2/级
- Buff/冷却 → `remaining_av: f32`，在 `advance_action_queue` 中与队列同步推进
- 冷却持续时间下限约 1000 AV

**状态：** 已定 — 已实现基础框架（ActiveBuffs 组件 + AV 推进 + 公式集成），旧回合制 Buffs 已移除（D11）。卷轴投放与学习机制是下一个开发目标。数值见 GAME.md。

**背景**

职业锁定技能导致组合自由度低、每局玩法雷同。旧 Buff 以"回合数"计数，与 AV 时间轴脱钩。技能数量和扩展性受限于 `PlayerClass` 硬编码。

---

**原编号：** `DsnX8`（迁移前）
<!-- &/DsnX8 -->

<!-- &DsnX9  物品系统的设计方向：从 MC 借鉴但以 Rust 方式表达（未迁移） -->
### DsnX9 物品系统设计方向（从 MC 借鉴，以 Rust 方式）

**决策**

借鉴 Minecraft 1.16+ 的 Registry + ItemStack + LootTable 设计，保留核心借鉴，舍弃与 Rust 不适配的部分。

| MC 特性 | 本项目决策 | 状态 |
|---------|-----------|------|
| `Registry<Item>`（ID → 定义） | 保留 —— `ItemRegistry` + `OnceLock` | ✅ 已实现 |
| `ItemStack + CompoundTag`（物品+元数据） | 保留 —— `ItemStack` + **`ItemMeta`**（强类型 struct，非动态 NBT） | ❌ I41 |
| `Item` 虚方法 | 保留 —— 以 **`ItemBehavior` trait** 代替 MC 的每个物品一个子类 | ❌ I40 |
| 命名空间（`minecraft:iron_sword`） | **舍弃** —— 单人 roguelike 无模组冲突，`usize` ID 足够 | 无需实现 |
| 热更新物品（运行时 reload JSON） | **舍弃** —— 物品定义编译时嵌入（`include_str!`），启动后不变 | 无需实现 |
| LootTable（条件+函数+池嵌套） | **暂缓** —— 当前简化版满足原型，待怪物 8+ 后再做 | 待定 |
| 配方/合成 | **暂缓** —— 材料有消耗渠道之前不做 | 待定 |
| 附魔 | **暂缓** —— 标记为装备池 15+ 后的目标 | 待定 |

**什么是 `ItemBehavior` trait**（设计中的骨架）：
```rust
pub trait ItemBehavior: Send + Sync {
    fn use_on(&self, world: &mut World, user: Entity) -> bool { false }
    fn inventory_tick(&self, world: &World, user: Entity) {}
    fn use_verb(&self) -> &'static str { "使用" }
}
```

**什么是 `ItemMeta`**（设计中的骨架）：
```rust
pub struct ItemMeta {
    pub display_name: Option<String>,
    pub tier: u32,
    pub enchantments: Vec<Enchantment>,
    pub durability: Option<u32>,
    pub tags: Vec<String>,
}
```

**背景**

为什么不用 HashMap 代替 CompoundTag：强类型 struct 在 Rust 中比动态 NBT 更优——字段名错误在编译期捕获而非运行时暴露。
为什么不用每物品一子类：MC 有 800+ Item 子类，我们是数据驱动的，每个行为一个 impl 覆盖所有同类物品。

---

## 三、数据与兼容

**原编号：** `DsnX9`（迁移前）
<!-- &/DsnX9 -->

<!-- &DsnX10  存档用 bincode 完整快照，不做增量 -->
### DsnX10 存档 bincode + 完整快照

**决策**

使用 `bincode` 直接将整个游戏状态序列化为二进制文件。存档是一个完整的快照。

- **不是 JSON/YAML：** 游戏状态中包含 Entity 引用（`Entity` 类型），JSON 无法原生序列化 ECS 关系。bincode 直接作用于内存结构，零映射开销。
- **不是增量/日志：** 增量存档需维护操作日志或变更集，读档需重放。单人 Roguelike 存档频率低、状态量小，完整快照更简单可靠。

**背景**

存档包含地图 tiles 的拷贝而非引用——Map 包含大数组（40×20 Tile），反序列化时需重建完整对象。如果引用注册表中的地图定义，注册表版本变化会导致旧存档不兼容。

---

**原编号：** `DsnX10`（迁移前）
<!-- &/DsnX10 -->

<!-- &DsnX11  提交历史当设计文档用：决策要写进 commit 正文 -->
### DsnX11 提交历史作为设计文档

**决策**

保留提交历史的完整性。不要 squash 那些记录设计转折的提交——它们是未来的开发者理解"为什么代码长这样"的唯一途径。

优秀示例：
- `8263c27` "AV合一 + 保活检查 + 技能执行 + 删除冷却" — 设计收敛点
- `f43b502` "移除全局 OnceLock" — 从全局状态到参数传递的转折点
- `5f8c5e9` "背包改为 Page 枚举 + 二元组匹配" — 从条件逻辑到状态机的设计模式改进
- 连续三个 revert（`09bccd6`、`da7f7e3`、`ba3aed3`） — "简化→失败→回退→重新理解"的健康迭代

**背景**

提交历史比任何文档都更诚实地显示了"当时为什么那样做"和"后来为什么改"。

---

**原编号：** `DsnX11`（迁移前）
<!-- &/DsnX11 -->

<!-- &DsnX12  合成系统：模板 + 碎片 + 核心（未迁移） -->
### DsnX12 合成系统：模板 + 碎片 + 核心（已定案）

**决策**

解决 G11（材料无消耗渠道）的核心方案。采用 **模板碎片 → 模板核心 → 完整模板** 渐进合成系统。

**品质等级（三段，支持渐进）：**

```
基础 → 标准 → 实验
```

- 模板碎片有品质等级，核心也有品质等级
- 核心只能吸纳 ≤ 自身品质的碎片（基础核心只能吸基础碎片）
- **实验级碎片有可能无法被任何核心吸纳**——它要么独立使用（作为一次性窄谱配方），要么永远无法合成完整模板（世界深度设计）

**三类物品的角色：**

| 物品 | 功能 | 背包空间 | 获取方式 |
|------|------|---------|---------|
| 模板碎片 | 窄谱配方。可直接使用合成特定物品 | 占 1 格 | 掉落 |
| 模板核心 | 吸纳匹配碎片→生成完整模板 | 占 1 格 | 掉落（较稀） |
| 完整模板 | 可重复使用的合成许可证 | 占 1 格 | 碎片+核心合成 |

**实现路径（渐进式）：**

```
Phase 1（立刻可做）：模板碎片作为独立消耗品掉落和使用
  → 碎片 = 一次性配方，材料开始有消耗渠道
  → 不需要核心、不需要合成 UI

Phase 2（扩展）：模板核心加入掉落
  → 碎片+核心→完整模板（可重复使用）
  → 引入合成界面

Phase 3（深化）：实验级碎片+条件合成
  → 某些碎片需要特定条件才能被核心吸纳
  → 碎片独立使用和合成的分歧
```

**关键设计点：**
- 碎片不是"残缺品"而是"窄谱模板"——Phase 1 就独立可用，不依赖收集
- 碎片和完整模板都占背包空间（制造背包抉择）
- 分类仅在基础层存在，高级阶段归一化
- 某些碎片永远无法被整合（世界深度设计，Phase 3）

**状态：** 已定 — Phase 1 优先级高于渲染重构。代码层准备：`ItemMeta.tier` 字段已存在（I41），`ItemBehavior` trait 骨架已定义（I40）。

**关联：** ISSUES.md G11 | GAME.md Gm7 | LESSONS.md LSYN8

**背景**

旧方案（DsnX12 讨论阶段）包含"类别（生存/合成/强化/魔法）"和"容量"等过于复杂的维度，被否定。简化后的三段品质 + 渐进实现路径更适合 MVP 节奏。

当前 5 种材料（生物血肉/破布/坚硬木棍/染血兽牙/黑色甲壳）无任何消耗渠道（G11），地面物品每层完全相同（G12）。材料系统处于"拾取了只能堆叠"的状态，Phase 1 碎片可以立即给材料一个出口。

---

**原编号：** `DsnX12`（迁移前）
<!-- &/DsnX12 -->

<!-- &DsnX13  业务领域收敛到单一 ecs_core；旧组件体系作废 -->
### DsnX13 业务领域收敛至 `core`，旧组件体系作废（进行中）

**决策**

未来只有 `core` 承载游戏逻辑/业务领域内容；`core` 内部完全采用 ECS 范式。

- 领域以 ECS 组件/实体/事件/系统表达，不再依赖旧的聚合 `Stats`、`ActionKindV3`、`ActionQueue` 等模型。
- `core` 是后续所有游戏逻辑的权威层；旧 `dungeon-*` 与 `src/` 视为历史/过渡代码，参考价值有限。
- 旧组件体系不迁移，只作为理解历史的参考；新代码以 `core/src` 中的组件和系统为准。
- 当前先按 `core/src/components.rs` 中的方向补全：基础数值组件（`Position`/`Health`/`Magic`/`Level`/`Experience`/`Attack`/...），行动状态（`Idle`/`Active`/`Failure`），能力标记（`Can*`），具体行动组件（`BasicAttack`），领域事件（`AttackEvent`）。

**背景**

refactor 分支已经删除旧模型并引入 ECS 原生行动模型，但旧 crate 仍保留了大量历史实现。继续在旧 crate 上修修补补会延续架构债，因此把业务领域收敛到一个新的 `core` crate，逐步摆脱对旧组件体系和旧目录结构的依赖。

**状态：** 已定 — 初步补全中。旧文档中的 ActionQueue/ActionKindV3/Stats 描述不再代表未来方向。

---

**原编号：** `DsnX13`（迁移前）
<!-- &/DsnX13 -->

<!-- &DsnX14  渲染后端插件化：ecs_core→presentation→render-api←tui/gpu -->
### DsnX14 渲染后端插件化：`render-api` / `presentation` / TUI / GPU（草案）

**决策**

渲染器可替换的关键是“后端与游戏逻辑之间的只读数据契约”，而不是把渲染简单包成插件。采用三层 + 装配层：

```text
core ──> presentation ──> render-api <── tui / gpu
                              ▲
                       dungeon-app（装配 + runner 选择）
```

| crate | 职责 | 禁止依赖 |
|---|---|---|
| `render-api` | 纯数据契约：`SceneFrame` / `VisualKey` / `UiView` / `InputEvent` / `SurfaceInfo` / `CONTRACT_VERSION` | `core`、ratatui、wgpu、`bevy_app` |
| `presentation` | 唯一知道 `core` 的集成层：提取 `SceneFrame`、`VisualKey` 映射、camera、`PageStack`/UI 状态、输入映射 | ratatui、crossterm、wgpu |
| `tui` | TUI 后端插件：终端生命周期、`TuiCatalog`、`SceneFrame` → ratatui 绘制 | `core`、`presentation` |
| `gpu`（未来） | GPU 后端插件：winit + wgpu/Bevy；`GpuCatalog`；消费同一个 `SceneFrame` | `core`、`presentation`、ratatui |
| `dungeon-app` | 装配：选择 backend/runner；`CorePlugin` + `PresentationPlugin` + 输入 + 后端 | 业务实现 |

**契约原则**

- `SceneFrame` 是只读视图模型，不是第二套游戏状态；不持久化；后端不得反向修改。
- `VisualKey` 只表达“这是什么”（`Player` / `Monster(id)` / `Tile(id)` / ...），不包含 glyph / 颜色 / 纹理；TUI 与 GPU 各自用自己的 catalog 映射外观。
- `UiView` 是页面级视图模型；页栈状态在 `presentation`，后端负责布局/绘制。
- `InputEvent` / `InputQueue` 后端无关；平台事件 → `InputEvent` 的翻译在各自后端，页栈路由与 tap-tap 只在 `presentation` 实现一次。
- `SurfaceInfo` 统一终端格子与 GPU 像素；camera 计算在 `presentation`。
- 不建持久渲染 ECS / render world；TUI 每帧快照即可；GPU 后端内部再做 buffer/atlas 与 diff。

**插件与调度**

- `CorePlugin`（`presentation`/app 层）：Startup 初始化；Update 消费 `PlayerCommand`、推进、结算。
- `PresentationPlugin`：PostUpdate 提取 `SceneFrame` + camera；UI/日志状态。
- `InputMapPlugin`：PreUpdate 输入路由；`PageStack` → `UiAction` / `PlayerCommand`；tap-tap。
- `SysInputPlugin`：`sys` 键盘线程 → `InputQueue`。
- `TuiPlugin`：终端生命周期 + `TuiCatalog` + Last 绘制。
- `GpuPlugin`（未来）：winit + wgpu/Bevy；Last/PostUpdate 同步。
- runner：TUI 用 `ScheduleRunnerPlugin::run_loop(33ms)`；GPU 用 winit / 自定义 runner；两者互斥，由装配层选择。

调度顺序：

```text
Startup     : core init, terminal setup
PreUpdate   : poll input -> map
Update      : drain PlayerCommand -> core sim -> settle
PostUpdate  : extract SceneFrame + camera
Last        : TUI draw / GPU sync
```

**后端切换**

- Cargo feature：`tui`（默认）/ `gpu`；装配层添加 `TuiPlugins` 或 `GpuPlugins`。
- 切换后端不改 `core`；只改根 crate 装配与 feature。
- TUI 保留用于 CI/调试；GPU 不要求 TUI 依赖。

**迁移阶段**

| 阶段 | 内容 | 验收 |
|---|---|---|
| R0（已完成） | `render-api` v1（34 tests） | 契约可用 |
| R1 | 新建 `presentation`；`tui` 去掉 `core` 依赖；`TuiPlugin` 消费 `SceneFrame` | `cargo tree -p tui` 无 `core`；`TestBackend` 测试 |
| R2 | 页栈 UI（Game/Dialog/Look 优先；Inventory/Throw 等 core 迁移） | `UiView` 渲染；页栈输入 headless 测试 |
| R3 | 引入 `bevy_app` 插件宿主 + `ScheduleRunner`；替换 `main` 循环 | headless `App::update()` 测试 |
| R4 | GPU 后端：`GpuPlugin` + winit/wgpu/Bevy；消费同一 `SceneFrame` | feature 切换；`core` 零改动 |
| R5 | 清理旧 `dungeon-*` / `src/pages`；文档同步 | workspace 干净 |

**开放决策**

- `bevy_app` 版本/获取：真实 0.16（联网）vs 本地 shim；不升级 `bevy_ecs` 0.17+（事件改名）。
- GPU 路线：完整 Bevy renderer（`bevy_render`/`bevy_winit`/`bevy_sprite`）vs 独立 `wgpu`；前者生态全，后者可控。
- UI 模型粒度：页面级 `UiView` 先行；3+ 页面共性后再抽 `ListView`/`DetailView`。
- `tui` / `sys` 边界：`tui` 是否依赖 `sys` 的 `TerminalSession`；输入轮询保持在 `sys`/`SysInputPlugin`，`tui` 不读输入。
- feature flag vs 运行时 `--renderer` 选择；初期用编译期 feature。
- 命名：`render-api` / `presentation` / `tui` / `gpu`。

**风险**

- 契约过度抽象：只加后端真正需要的字段；`SceneFrame` 保持视图模型。
- 提取成本：80×60 全量快照可接受；后续 dirty/chunk。
- 插件顺序：`SystemSet` + `.chain()` 显式；`App::update()` headless 测试。
- 终端生命周期：`NonSend` + panic-safe guard；Ctrl+C；resize。
- 输入去重/tap-tap：保持在 `presentation`；GPU repeat 归一化。
- GPU 非 drop-in：需要 asset catalog、camera、runner、UI 文本；先做平铺 tilemap + sprite。

**关联：** DESIGN DsnX1 / DsnT1 / DsnP2 / DsnA1；DESIGN DsnX14；ISSUES I87/I88；README 渲染契约章节。

**状态：** 已定 — R0/R1 已落地（见 REFACTOR §1）。

- **R0**（`render-api` v1，34 测试）已完成；
- **R1** 已完成：新建 `presentation`（`extract` / `catalog` / `camera` / `ui` / `input`，
  57 测试）；`tui` 去掉 `ecs_core` 依赖、改消费 `SceneFrame`（`TuiCatalog` +
  `TuiPlugin`，24 测试，含 `TestBackend` 全帧断言）。
  三条边界（`tui` ↛ `ecs_core`/`presentation`、`presentation` ↛ ratatui/crossterm、
  `render-api` ↛ `ecs_core`）已由 `scripts/gate.ps1` 的 `cargo tree` 步骤强制；
- **R2** 部分：页栈的 `Look` / `Dialog` 已落地（`ui::PageStack` → `UiView`，
  输入路由与 tap-tap 口径在 `input`）；`Inventory` / `ThrowSelect` / `ThrowAim`
  需要物品与投掷规则迁移（DsnX13 S4），后端当前给占位页；
- **R3**：工作区里没有 `bevy_app` 且环境无外网，本轮未引入。`TuiPlugin` 已按
  插件形状收敛（`tui/src/plugin.rs`），补 `impl Plugin` 时调用点不变；
- **R4/R5**：GPU 后端与旧 crate 清理待续。

**R1 的实现结论（与草案的差异，均已在代码注释就位）：**

1. **相机夹取放在 `presentation`，只做一次**。世界 80×60、视口随终端变化；
   若让各后端自己处理"视口比世界大 / 贴边越界"，两个后端必然裁得不一样
   （草案只说了"camera 计算在 `presentation`"，这里补上"为什么"）。
2. **地形用并列位图而不是三种 tile 变体**：`MapView.tiles` 全量填地形，
   `visible` / `explored` 是两个并列 `Vec<bool>`；"可见 > 已探索 > 未知"的
   画法留给后端（TUI 用 `Rgb::dim` 压暗），GPU 可以用自己的调色方式。
3. **同格实体的绘制优先级是后端职责，但不能靠遍历顺序**：`entities` 的顺序是
   `presentation` 给的**稳定顺序**（便于 golden 测试），后端必须显式按
   `VisualLayer` 取最大层、再让玩家压过同层。
4. **`SceneFrame` 增加 `PartialEq`**：golden 测试要整帧比较。字段全是普通数据，
   不构成语义风险，故未递增 `CONTRACT_VERSION`（契约字段未增删改）。
5. **`TuiPlugin` 不是 `bevy_app::Plugin`**：R3 之前它是"结构约定"——
   把后端需要的东西收成一个类型，让装配层"选后端"只有一个接缝。

---

**原编号：** `DsnX14`（迁移前）
<!-- &/DsnX14 -->

<!-- &DsnX15  crate 改名 core→ecs_core，消除与标准库 core 的遮蔽 -->
### DsnX15 `core` crate 改名 `ecs_core`（措辞与路径对齐）

**决策**

- 新领域层的 crate 名由 `core` 改为 **`ecs_core`**，目录名同步由 `core/` 改为
  `ecs_core/`，使**包名与目录名一致**（`cargo test -p ecs_core`、`use ecs_core::…`）。
- 依赖方同步：根 `Cargo.toml`、`tui/Cargo.toml`、`src/main.rs`、
  `tests/core_loop_test.rs`、`tui/src/{render,scene}.rs`、`scripts/gate.ps1`。
- 旧 `dungeon-core` 不改名、不动其内部的 `dungeon_core::` 引用——它属于待清理的
  历史层（DsnX13 / DESIGN DsnX14 R5）。

**背景**

`core` 与 Rust 标准库的 `core` crate 同名，代价是具体的、已经被踩到的：

1. `#[derive]` 展开与手写代码里的 `core::fmt` / `core::hash` / `core::convert`
   会被本地 crate 遮蔽——本项目修 I86 时就不得不用手写 `ScheduleLabel::impl`
   绕开 derive 展开，`resources.rs` 里的 `core::convert::Infallible` 也要特意
   写成 `std::convert::Infallible` 才不会被解析到本地 crate。
2. doctest 一律不可用（crate 名 `core` 下，doctest 的 `core::` 指向本地 crate），
   等于永久放弃一种测试形态。
3. 大小写与语义混淆：文档里说「`core` 不依赖 ratatui」时，读者无法区分是
   「本地领域层」还是「Rust 的 core」。`ecs_core` 让 crate 名自带领域含义
   （「用 ECS 写的领域内核」），与 `render-api` / `dungeon-app` 并排也更整齐。

**为什么现在改**

改名的成本与**引用面**成正比，而引用面只在收敛期变小：Phase D 前 `core` 的公共面
是 `pub use *` 全暴露、旧链路还在调用；现在行动/速度两条链路都已收敛，全库对它的
引用只剩 7 个文件、不到 30 处。再往后 `presentation`（Phase G）会成为第二个依赖方，
那时改名要同时动两个集成层。

**代价**

- 与历史文档/commit 里的 `core` 字样不再字面一致；已在 DESIGN DsnX17 的冻结清单
  与本节记录映射关系，检索时注意 `core`（旧名）与 `ecs_core`（现名）。
- `cargo -p core` 这类命令要改成 `-p ecs_core`；已同步 `scripts/gate.ps1`。
- 目录名 `ecs_core/` 与包名一致，避免了「目录 `core`／包名 `ecs_core`」这种
  一眼看不出的不一致。

**被否的方案**

- ~~保留 `core` 只在文档里说明~~：不解决遮蔽问题，I86 那类绕行还会被后来者重新踩。
- ~~改名 `game-core` / `domain`~~：`game-core` 与旧 `dungeon-core` 命名风格重复，
  看不出技术形态；`domain` 丢掉了「这是 ECS」这一关键信息，且与 DDD 的
  domain 语义（业务规则层，不含 ECS 机制）不完全吻合。
- ~~目录留 `core/`、只改包名~~：排除，包名与目录名不一致会让 `cargo -p` 与
  路径引用长期对不上（本次一并改掉）。

**关联：** DESIGN DsnX17 冻结清单 / REFACTOR §1（Phase F）；
ISSUES I86；DESIGN DsnX13 / DsnX14。

**状态：** 已定 — 已落地（Phase F5）。`cargo check --workspace`、`cargo test --workspace`
（25 目标 / 198 passed）、`scripts/gate.ps1`（4 步）在改名后全绿。

**原编号：** `DsnX15`（迁移前）
<!-- &/DsnX15 -->

<!-- &DsnX16  形状进 DESIGN/ISSUES，值进 GAME；讨论只落形状 -->
### DsnX16 结构与内容的分离：形状进 DESIGN/ISSUES，值进 GAME.md

**决策**

新设计推进时，**先定形状、后填值**，且三者的归属是刚性的：

| 类别 | 含义 | 归属 |
|---|---|---|
| **结构（形状）** | 有哪些概念、接口怎么分组、扩展点在哪里、不变式是什么 | `DESIGN.md`（落点在哪个 crate 就进哪个 crate）+ `REFACTOR.md`（本分支阶段计划） |
| **实现问题** | 代码与设计意图不一致、能力缺失、不变式缺失、中央分派债 | `ISSUES.md`（对应 crate） |
| **游戏数值与内容** | 具体数值、系数、公式的分区与取值、技能/怪物/装备的具体定义 | `GAME.md` |

**并据此确认一条工作方式：** 讨论设计时，**只把"形状"落进文档，"值"一律留待内容阶段**——数值用 `[⃞直觉]`/`[⃞试调]` 在 GAME.md 记录，**不因为"顺便想到了"就写进 DESIGN 或 ISSUES**。

**背景**

设计讨论中混入过大量具体内容（装备效果、技能机制、危险程度分档、生物范畴的固有性质）。它们的作用是**暴露结构需求**——例如"无视地形减速的靴子"暴露了"移动规则缺一个输入维度"，"自身死亡并生成持续区块的技能"暴露了"长期效果需要独立实体而非技能数据的字段"。

但**这些内容本身不是设计决策**：它们的取值要在重构完成后按新的结构重新设计。若此时把值写进 DESIGN，会让文档同时承担"罗盘"与"数值表"两种角色，而 `RULE.md` §五 明确禁止 DESIGN 包含具体游戏数值。

**被否的方案**

- ~~把内容需求也写进 DESIGN，以便"不丢失想法"~~：会让 DESIGN 变成设计草稿堆，且与 GAME.md 的数值标注体系（`[⃞计算]`/`[⃞直觉]`/`[⃞试调]`）重复。内容需求的结构含义**已被抽象成 DsnE9–DsnE13**，具体想法在重构后的内容阶段重新设计。
- ~~把"公式分区未定"记成 ISSUES~~：分层错误——**"设计值尚未确定"不是实现问题**。ISSUES 记的是"代码与设计意图不一致"或"能力缺失"，而这里连意图（值）都还没定，属 GAME.md。
- ~~把扩展性判据写进 RULE.md~~：`RULE.md` 装的是**文档维护规则**，判断原则的读者是 AI（防止重复犯错），按 `RULE.md` §三 的定位归 `LESSONS.md`（见 LECS22）。

**适用范围**

本条适用于后续所有设计推进。**判断一个新想法该写哪里的三步**：

1. 它需要**新的形状**吗（新接口/新组件/新扩展点/新不变式）？→ 是 → `DESIGN.md`（落点 crate）
2. 它暴露了**实现缺口**吗（能力缺失、不变式缺失、中央分派债）？→ 是 → `ISSUES.md`（落点 crate）
3. 它只提供**值**（系数、定义、分档数值）？→ 是 → **只记在讨论里，待内容阶段进 `GAME.md`**

**关联：** RULE.md §五（DESIGN 不含数值）/ §三（LESSONS 定位）/ §七（归属判据）；
DESIGN DsnE9–DsnE13（本轮由内容需求抽象出的结构决策）；
ISSUES ECS27–ECS36（本轮记录的实现问题）；
LESSONS LECS22（扩展点判据）。

**状态：** 已定 — 本轮据此完成 `DsnE9`–`DsnE13`、`ECS27`–`ECS36`、`LECS22` 与 REFACTOR Phase H 的写入。

---

**原编号：** `DsnX16`（迁移前）
<!-- &/DsnX16 -->

<!-- &DsnX17  冻结契约：组件/资源/事件/边界 + serde 只追加 + 确定性 + 不承诺旧档 -->
### DsnX17 兼容性与冻结契约（原 DESIGN DsnX17 / §10.7 折入）

**决策**

下面这些是**跨 crate 的硬约束**，任何改动都必须先过它们。它们原先记在 `REFACTOR.md`
（分支内计划文档，合并前应折进 DESIGN），已随 2026-09 的文档治理折入本条——
`REFACTOR.md` 不再承载契约，只承载"还没做完的计划"。

**① 冻结：只允许追加 / 修 bug，不允许重命名、重排、改语义**

| 类别 | 冻结内容 |
|---|---|
| `ecs_core` 组件 | `Position` / `Health` / `Magic` / `Level` / `Experience` / `Attack` / `Defense` / `MagicMastery` / `MoveSpeed` / `AttackSpeed` / `CritRate` / `CritDamage`；标记 `Player` / `Monster` / `Stairs`；状态 `Idle` / `Active` / `Failure`；能力 `Can*` |
| `ecs_core` 资源 | `Map` / `MapSeed` / `FloorNumber` / `GameRng` / `MapMemory` / `VisibleMemory` / `OccupancyMap` / `EventLog` / `TurnManager` |
| `ecs_core` 事件 | `AttackIntentEvent` / `AttackEvent` / `DeathEvent` / `LevelUpEvent` |
| 行动链路 | 行动 ZST 名称（`Wait` / `Move` / `BasicAttack` / `Chase` / `Flee` / `Wander`）、`ActionPriority` / `ActionTimer` / `Candidate` / `ActiveAction` / `Ready`、归属关系 `ActionOf` / `ActionChildren` |
| 应用入口 | `world_loop::{new_game, apply_player_command, request_quit}` 的调用语义（名字可留兼容包装） |
| 渲染契约 | `render-api` v1 的 `SceneFrame` / `VisualKey` / `UiView` / `InputEvent` 字段**可追加**；破坏性改动必须递增 `CONTRACT_VERSION` |
| 架构边界 | `ecs_core` 不依赖 `bevy_app` / ratatui / wgpu / `render-api` / `presentation`；`tui` / 未来的 `gpu` 不依赖 `ecs_core` |

**② serde 只追加**

已 derive serde 的类型（`Tile` / `MapKind` / `MonsterKindId` / `Map` / `Room` 等）**不得重排变体、
不得复用旧判别值**；新增字段一律 `#[serde(default)]`。`Tile` 的判别值现由 `#[repr(u8)]` +
声明顺序决定（H4），因此**枚举的书写顺序本身就是契约**。

**③ 确定性**

- `GameRng` 的算法与 `steps` 语义是回放与存档的基础；**改算法等同改存档格式**。
- 地图生成对 `(MapSeed, FloorNumber)` 必须确定；改生成算法 = 改同 seed 地图，
  需要 DESIGN 记录 + golden seed 测试。
- 消费随机数的系统必须显式排序（Bevy 系统顺序默认未指定）。
- **凡是"一次处理一批实体"的地方，顺序必须由显式排序键给出，不得依赖容器迭代顺序。**
  `std::collections::HashMap` 的 `RandomState` **每个进程种子不同**，所以同一个世界在两次
  运行里迭代顺序可能不同 → 命令发出顺序不同 → archetype 创建顺序不同 → 之后各系统的
  **查询迭代顺序**不同 → 若其中任何人消费 `GameRng`，随机流分配就变了。
  当前已知落点：`action_arbitration_system` 的授予与回收两个循环（`ordered_grants` +
  `losers.sort_unstable_by_key`）。
- **Schedule 构建强制无歧义**：core 的四个 Schedule 一律经 `schedule::core_schedule()` 构建，
  它把 bevy 的 `ambiguity_detection` 从默认的 `Ignore` 提到 **`Error`**——"访问冲突但顺序
  未声明"的系统对会在构建期（即每个测试的第一句）直接失败。这条护栏上线时立刻抓到一处
  真实隐性依赖（`action_completion_system` 读终态事件 vs `update_events_system` 交换缓冲）。

**④ 显示数据不属于领域**

`Tile` 表里的 `glyph`、`MonsterTemplate` 的 `glyph`/颜色是**渲染数据**，
外观的唯一来源是后端的 catalog（`tui::TuiCatalog`，见 DsnT1）。领域层不得把它们当稳定契约。

**⑤ 存档：不承诺兼容旧档（当前阶段明确不做）**

> **决定（2026-09，用户确认）：** 新 `ecs_core` **不承诺、也不追求**读取旧存档。
> 旧档由 `dungeon-world` 的旧数据模型产生（代码在 `archive/`），与新组件体系不是
> "字段差异"而是**两套模型**；为它做兼容层会把已归档的数据结构拖回领域层，
> 得不偿失。**在当前阶段承诺兼容旧档是不合适的。**

由此拆成两件事，别混：

| | 做什么 | 为什么 |
|---|---|---|
| **旧档 → 新架构** | **不做兼容**。存档功能随"存档迁移"这一项在路线表里记为**未做**（`DsnX18`） | 两套数据模型，兼容层会把旧结构拖回领域层 |
| **新档自己的演进** | **做**：serde 只追加（②）、`GameRng` 算法与 `steps` 语义冻结（③） | 这是让"以后的新档"能被以后的代码读回；与旧档无关 |

**⑥ 瞬态子实体不存档**

action 实体与技能实例都是瞬态子实体；存档只存 actor 状态/速度/身份，读档后重新生成行动。
**子实体的组件名不得作为存档格式的一部分。**

**关联：** DsnX13（业务领域收敛）/ DsnX15（crate 改名）/ DsnE8（行动系统唯一权威条目，
含行动链路的完整语义）/ DsnE13（`Tile` 属性表与判别值）/ DsnT1（外观归后端）
| DsnX18（技术路线表）/ RULE.md §八（实现改动后的规格复核）

**状态：** 已定 — 长期有效。改动上述任何一项之前：先改本条，再改代码。

---

**原编号：** `DsnX17`（文档治理时折入）
<!-- &/DsnX17 -->

<!-- &DsnX18  技术路线表：换道必须留痕（旧路线保留并标注被否原因） -->
### DsnX18 技术路线表（换道记录）

**这份表回答一个问题：我们能换道，但换过的道必须留痕。**

**判据（采用 ADR 对 "architecturally significant decision" 的定义，见 RULE §九.1）：**

> 换道 = 影响 **结构 / 非功能特性 / 依赖 / 接口 / 构造技术** 之一的路线替换。

等价的自我检验：能不能写成「**我们不再用 A，改用 B**」，且 A、B 是两种**互斥的做法**。
能 → 换道；只能写成"在 A 里选了更合适的做法" → 实现层，不记本表。
**不测影响大小、不测范围宽窄——只测是不是另一条路。**

**换道时必须做的三件事**

1. 在回复里**明说**这是设计改动（不许夹在"文档整理 / 重构"里）；
2. **改本表**：新行写「~~旧路线~~ → 新路线 + 决策条目」；旧路线为什么失败写进替代条目的
   「背景 / 被否方案」；旧条目按 RULE §五 标 `已被取代`，**保留原文不删**；
3. commit 用 **`design:`** 前缀。

**当前路线**（`~~删除线~~` = 已被否掉的旧路线，只增不删；想改回去之前先读它为什么被否）

| 领域 | 旧路线 | 现路线 | 决策 |
|---|---|---|---|
| 行动模型 | ~~中央 `ActionKind` + `match` 分派~~ | **行动 = actor 的瞬态子实体**（生成→仲裁→tick→执行→completion） | DsnE8 |
| 速度 | ~~`Agility` + 反应时（`AV = 反应时 + 耗时 × 系数`）~~ | **倍率**：`AV = 基础耗时 / clamp(速度)` | DsnE8 ②、Gm1 |
| 领域归属 | ~~分散在 `dungeon-core/action/world/render` 四个 crate~~ | **业务只在单一 `ecs_core`** | DsnX13 |
| 渲染 | ~~后端直接查 `World` 渲染~~ | **后端无关只读契约**：`ecs_core → presentation → render-api ← tui/gpu` | DsnX14 |
| 行动归属表达 | ~~通用层级关系 `ChildOf`~~ | **专用关系** `ActionOf` / （将来的）`EffectOf` | DsnE12、ECS41 |
| 效果/修正器 | ~~一串数值修正器（只覆盖"改一个数"）~~ | **效果即实体 + 求值侧五步**（分桶→过滤→折叠→纯函数） | DsnE10、DsnE12 |
| 地形数据 | ~~属性散在 5 处 `match`~~ | **静态属性表** `TILE_PROPS`（种类键 + 一张表） | DsnE13、ECS31 |
| 存档 | ~~承诺可读旧档 / 兼容旧格式~~ | **不承诺兼容旧档**；新档自身只做"只追加"演进 | DsnX17 ⑤ |
| 测试策略 | ~~实现先行、事后补测~~ | **先有回归基线再重构**（Phase A 之后每步都带 parity/变异验证） | REFACTOR §1 |
| 文档归属 | ~~DESIGN 里塞数值、ISSUES 里塞设计草案~~ | **形状→DESIGN，值→GAME，问题→ISSUES，教训→LESSONS** | DsnX16 |

**明确不算换道**（避免把实现层也拖进来）：模块怎么拆、类型怎么命名、要不要留兼容壳、
表放在哪个文件、某个分支的取舍、折叠顺序、`Wait` 读不读修正器——这些**不记本表、不用报**，
但**非显然的选择必须在代码处留一行注释**说明"为什么这么选 / 替代方案为什么不行"
（否则下一个人会当成冗余删掉；`ChildOf` 的巧合式安全、`linked_spawn` 对装备的反例都是这么丢的）。

**关联：** DsnX16（形状/值分离）/ DsnX17（冻结契约）/ DsnE8（行动系统，"被否方案"表的所在地）
| RULE.md §九.1（换道必须声明）

**代价（Consequences）**

- **多一处必须同步维护的东西**：换道要同时改本表 + 替代条目的「被否方案」，漏一处就出现
  "表说换了、条目没记为什么"。缓解：`design:` 前缀让漏改在 review 时可查。
- **本表会随项目变长**，但它只增行、每行一句话，且行数 = 历史换道次数（本仓库目前 10 行），
  规模天然有界。
- **不覆盖"局部路线"**：像 `Tile` 从 5 处 `match` 收敛到属性表这种"单个类型内部的做法更换"
  按判据只算实现层，因此本表**不记录它**——代价是：将来若有人想把属性表改回 `match`，
  本表不会拦住他，只能靠 `DsnE13` 的「被否方案」。
- **对 AI 的依赖**：判据要人/AI 每次主动执行，没有机械检查能判断"这是不是另一条路"。
  缓解：判不准时按换道处理（多写一行，成本远低于架构悄悄漂移）。

**状态：** 已定（长期维护）。**换道先改这张表，再改代码。**
<!-- &/DsnX18 -->

