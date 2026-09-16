> **⚠️ 修改前必须阅读或回忆 [RULE.md](RULE.md)——它定义了本文档的维护规则和更新时机。**
>
> 每条记载一个设计取舍，含两个小节：
> - **决策**：当前选的方向和理由（罗盘）
> - **背景**：舍弃的方案和演化过程（护卫）
>
> 过去的设计历史为未来的方向提供合理性论证。
>
> 数值和游戏规则见 [GAME.md](GAME.md)；模块和 API 结构见 [README.md](README.md)。

# 设计决策记录

---

## 一、架构

### Dsn1 四层 Crate 拆分

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

### Dsn2 移除全局 World，改为参数传递

**决策**

所有函数改为显式接收 `&World` / `&mut World` 参数。明确表达读写意图，不需要文档约束。不使用全局 `OnceLock<RwLock<World>>`，不使用 `thread_local!`。

不选 `thread_local!` 的理由：只适合线程生命周期和 World 一致的单线程场景。如果将来要分线程（AI 计算、异步 IO），thread_local 的 World 会无法访问。

**背景**

曾经有一个 `OnceLock<RwLock<World>>` 全局对象 + `world!()` 宏（提交 `41f37f0` 引入，`f43b502` 移除）。RwLock 的不可重入性导致两种死锁模式：
- `advance_action_queue` 持锁调用 `execute_*`
- `render_ui` 持锁调用子函数

第一次遇到时打了"分两阶段取锁"的补丁，但设计上不可靠：任何一个新函数如果忘记遵守约定就会引入死锁。Rust 的借用检查器在编译期保证不会同时持有 `&mut` 引用——比任何运行时锁策略都更强。

---

### Dsn3 并行怪物决策（Schedule）

**决策**

三种怪物行为（追击、逃跑、游荡）的检查条件**没有数据竞争**，通过 bevy Schedule 并行执行：

```
chase_decision_system  ─┐
flee_decision_system   ─┤ 并发 → arbitration_system → ActionQueue
wander_decision_system ─┘
```

适当的并行度是 3（三种行为）+ 1（仲裁）。太少浪费 CPU，太多调度开销超过收益。

**意图缓冲区模式**：三个决策 system 各自写入独立的 `ChaseIntents`/`FleeIntents`/`WanderIntents` 资源，仲裁 system 串行合并。生产者-消费者模式无并发写冲突，数据流显式可追踪。

**背景**

旧设计 `run_monster_decision()` 串行遍历所有怪物，每个怪物检查条件→仲裁→入队，O(n) 且无法并行。保留串行兼容入口供调试——"可调试性比代码整洁更重要"。

---

### Dsn4 碰撞图（Occupancy Map）独立于 Map

**决策**

两个关注点变化频率不同，分离为独立结构：
- `Map`：存 Tile 地形（`Wall`/`Floor`）——楼层生成后基本不变
- `OccupancyMap`：存每个格子被哪个实体占据——每次移动/攻击/死亡后都变化

合并到一个结构意味着每次更新都要复制地形数据。

**背景**

每次行动后全量重建 `rebuild_occupancy()` O(n) ≈ 800 格（40×20），开销 <1μs。增量更新容易漏边界条件（实体死亡、下楼、传送），维护正确性的心智负担远高于全量重建的成本。

---

### Dsn5 视野记忆双结构（MapMemory + VisibleMemory）

**决策**

两个独立的记忆结构，因为它们的数据类型、更新频率、生命周期不同：
- **MapMemory**：记录哪些格子曾经被看到过（boolean 数组）——渲染已探索区域的灰色墙壁/地板。只增不减（探索过的格子不会"遗忘"）
- **VisibleMemory**：记录最后看到的实体（glyph/color/位置）——渲染视野外但已知的实体。需清理已死亡实体（否则显示幽灵）

**背景**

如果合并为一个结构，cleanup 逻辑需要区分"地图记忆（不清除）"和"实体记忆（需清除）"，增加复杂性。视野外的实体在已探索区域以灰色显示。死亡实体自动清理。

---

## 二、核心机制

### Dsn6 行动系统设计哲学

行动系统是项目中最核心的设计，其演化经历了多轮思考（见提交历史中的多次 revert 和 fix）。

#### 决策

**组件式行动授权：** 行动能力由组件赋予。`CanMove`、`CanChase`、`CanFlee`、`CanWander`、`CanWait` 等组件表达了"这个实体具有这种行动的权利"。系统只需遍历并检查哪种行动的条件满足。加新行为=加新组件+加新 system，不修改决策流程。

**AV = 反应时 + 耗时：** 两者相加为单一值入队倒计时。没有独立的冷却系统。

```
反应时 = max(100 - 敏捷 × 3, 20)
耗时修正系数 = max(1.0 - 敏捷 × 0.02, 0.5)
AV = 反应时 + 耗时 × 修正系数
```

**事件驱动推进：** `next_event_distance()` 查询最小剩余 AV，所有条目同步推进该距离，然后批量执行。固定 tick 在事件密集时浪费计算、稀疏时空转。事件驱动只在真正有事件时推进，天然零空转。

**无前摇、无后摇：** 后摇在回合制 Roguelike 中无实际用途——玩家在怪物行动前就已经想好了要干什么。前摇已被反应时覆盖。复杂动作（如施法）应通过组合多个行动实现，而非前后摇。

**保活检查：** 执行前调用 `check_condition()` 验证条件是否仍满足（类比网络 keep-alive）。条件不满足→移出队列，不执行。入队时已检查过一次，但其他实体的行动可能在队列推进中改变世界状态。

**优先级仲裁：** 每个行动组件自带 priority（数值越高越优先）：Flee=200 > Chase=100 > Wander=50 > Wait=0。静态优先级在当前阶段合理——行为种类少，优先级关系固定。未来如需动态优先级，可通过 `PriorityModifier` 组件扩展。

**无预测轴 / 无锁定轴：** 所有显示在行动轴上的条目都是已锁定、必然要执行的（除非保活检查失败）。预测需要模拟未来世界状态，在 ECS 中非常复杂（需克隆 World 或维护回滚日志），收益有限。

**行动队列的本质是查询系统：** 核心不是"谁先执行"的顺序器，而是查询"哪个实体已准备好行动"。重点在于：选择什么行动（决策）、何时做（AV 倒计时）、耗时多久（AV 组成）。

#### 背景

**冷却系统为何被移除：** 旧设计有"冷却计时器"和"队列推进"两个独立的时间维度，需要保持同步（提交记录中有多次"冷却空转"fix）。AV 合一后冷却作为独立概念消失——需要冷却的效果通过调整行动耗时来实现。

**不选行为树（脑链）的理由：** 脑链一次只执行一条链，顺序判断。组件式条件是并行的——系统可以批量检查所有同种条件，完全只读、可并行。

**不选独立决策组件的理由：** 分散在各 Action 组件中的决策权被收回一个中心，降低可扩展性。

**不选状态机的理由：** 状态把"状态"和"行动"耦合，"进入追击模式→执行 CanChase"，决策权从组件抢到了状态机系统手中。

**预测轴/锁定轴为何被移除：** 与组件式决策不兼容。系统只能知道"此刻哪些条件满足"，无法准确知道"下一帧是否仍满足"。

---

### Dsn7 物品系统 — Registry + ItemStack

**决策**

物品定义存入 `assets/items.json`，运行时加载为 `ItemRegistry`（`OnceLock` 全局单例）。背包里只存 `(item_id, count)`。装备直接持有 ItemStack，不占背包空间。

**选择 OnceLock 而非 ECS Resource 的理由：** 物品注册表是纯数据，没有任何 ECS 依赖。放到 Resource 里意味着所有读操作必须先拿 World 引用，增加不必要的耦合。

**为什么装备不占背包格子：** 装备是角色的属性，不是背包的内容。装备直接持有 ItemStack 使装备和背包两个关注点正交。

**背景**

背包格子里存完整的物品定义（名称、描述、属性、图标...）→ 每个格子占用大量内存，且无法实现"同一个物品有多个"而不复制。

---

### Dsn8 ItemPickup 作为独立实体

**决策**

地面上的物品是独立的 ECS 实体，带 `ItemPickup`（存 ItemStack）、`Position`、`Renderable` 三个组件。不是背包 Inventory 的一部分。

实体组件模式使物品可以像其他实体一样被 FOV 系统看到、被 VisibleMemory 记录、被碰撞图排除（`rebuild_occupancy` 中 `pickup.is_none()` 过滤了它们）。

**背景**

如果物品是 Inventory 的一个列表，FOV 渲染需要特殊处理"如何渲染一个不在 ECS 中的位置"。拾取时需 despawn 实体、掉落需 spawn 实体，但频率低（每层几次），开销可忽略。

---

### Dsn9 tap-tap 输入

**决策**

第一次方向键设预览状态（显示"移动(0,-1)"或"攻击"），第二次同键确认入队。不同键取消前一个预览。预览状态存为 ECS Resource（需要跨帧存在——第一次按键到第二次按键之间需要渲染更新）。

**为什么不是鼠标/菜单：** 终端 Roguelike 的核心交互是键盘盲打。菜单选择破坏了"看着地图敲键"的沉浸感。

**背景**

方向键同时承担"移动"和"攻击"两个语义。如果敲一下方向键就直接执行，玩家无法区分"我想走过去"和"我想攻击敌人"——攻击是移动的一个副效应。

---

### Dsn10 装备槽配对背包满的处理

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

### Dsn11 主手/副手系统

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

旧投掷流程（`~~旧方案——已由 Dsn12 替代~~`）：
```
t 键 → 列出 throwable 物品 → 选择目标 → 自动装副手 → 瞄准模式 → Enter入队
```

---

### Dsn12 投掷作为一等 AV 行动

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

### Dsn13 技能系统重设计（设计中）

**决策**

技能不从职业获取，改为从地图道具（技能卷轴）学习。自由组合技能，无职业锁定。

- 职业 → 仅提供初始属性和装备，不锁定技能
- 技能 → 通过卷轴学习/升级，存储在 `Skills` 组件中。每张卷轴教一种技能，重复学习提升熟练度
- 熟练度影响技能强度：治愈 +3HP/级，护盾/狂暴 +2/级
- Buff/冷却 → `remaining_av: f32`，在 `advance_action_queue` 中与队列同步推进
- 冷却持续时间下限约 1000 AV

**状态：** 已实现基础框架（ActiveBuffs 组件 + AV 推进 + 公式集成），旧回合制 Buffs 已移除（D11）。卷轴投放与学习机制是下一个开发目标。数值见 GAME.md。

**背景**

职业锁定技能导致组合自由度低、每局玩法雷同。旧 Buff 以"回合数"计数，与 AV 时间轴脱钩。技能数量和扩展性受限于 `PlayerClass` 硬编码。

---

### Dsn14 物品系统设计方向（从 MC 借鉴，以 Rust 方式）

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

### Dsn15 存档 bincode + 完整快照

**决策**

使用 `bincode` 直接将整个游戏状态序列化为二进制文件。存档是一个完整的快照。

- **不是 JSON/YAML：** 游戏状态中包含 Entity 引用（`Entity` 类型），JSON 无法原生序列化 ECS 关系。bincode 直接作用于内存结构，零映射开销。
- **不是增量/日志：** 增量存档需维护操作日志或变更集，读档需重放。单人 Roguelike 存档频率低、状态量小，完整快照更简单可靠。

**背景**

存档包含地图 tiles 的拷贝而非引用——Map 包含大数组（40×20 Tile），反序列化时需重建完整对象。如果引用注册表中的地图定义，注册表版本变化会导致旧存档不兼容。

---

### Dsn16 线程局部 RNG → GameRng 统一

**决策**

`GameRng` 成为唯一随机源。所有随机操作（仲裁、暴击、游荡、掉落、地图生成）统一走 `GameRng` 或基于 `MapSeed` 的派生 RNG。

**背景**

曾经有一个线程局部的 `RefCell<SmallRng>` 与 `GameRng` 并存，用于仲裁 system 中的随机选择——因为当时仲裁 system 无法访问 `GameRng` 资源。已由 ISSUES D1 解决。

---

## 四、输入与渲染

### Dsn17 33ms 按键去重 + 16ms 轮询

**决策**

输入线程以 16ms 间隔轮询（≈60fps），连续两次相同按键间隔小于 33ms 则丢弃后一次（L46 校准：现代终端过滤 Release/Repeat 事件后，窗口从 50ms 收窄为 33ms）。

**16ms 不是任意选择的：** 它与 60fps 的输入采样对齐。轮询间隔太长（如 100ms）会导致明显可感知的延迟。16ms 是人感知不到的单帧延迟下限。

**背景**

终端键盘的物理按键会触发重复的 key-repeat 事件（长按时）。如果不做去重，方向键长按会导致连续触发预览/确认/预览/确认，玩家瞬间移动多格。33ms 窗口（配合 KeyEventKind 过滤）允许正常双次敲击（tap-tap 确认），但过滤掉键盘重复。

---

## 附录

### Dsn18 提交历史作为设计文档

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

### Dsn19 合成系统：模板 + 碎片 + 核心（已定案）

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

**状态：** 已定案。Phase 1 优先级高于渲染重构。代码层准备：`ItemMeta.tier` 字段已存在（I41），`ItemBehavior` trait 骨架已定义（I40）。

**关联：** ISSUES.md G11 | GAME.md Gm7 | LESSONS.md L20

**背景**

旧方案（Dsn19 讨论阶段）包含"类别（生存/合成/强化/魔法）"和"容量"等过于复杂的维度，被否定。简化后的三段品质 + 渐进实现路径更适合 MVP 节奏。

当前 5 种材料（生物血肉/破布/坚硬木棍/染血兽牙/黑色甲壳）无任何消耗渠道（G11），地面物品每层完全相同（G12）。材料系统处于"拾取了只能堆叠"的状态，Phase 1 碎片可以立即给材料一个出口。

---

### Dsn20 渲染架构：Buffer 直写替代 Paragraph + 持久 Canvas + 背景色优先

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

---

### Dsn21 按键系统：页栈（PageStack）

**决策**

所有 UI 页面通过页栈导航，栈顶决定按键路由和渲染行为。

**页栈模型：**

```
PageStack: Vec<Page>
  [Game]                      ← 栈底，永不 pop
  [Game, Dialog]              ← 按 q/Esc 弹出确认
  [Game, Look]                ← 按 x 进入查看模式
  [Game, Inventory]           ← 按 e 打开背包
  [Game, ThrowSelect]         ← 按 t（无投掷物时）
  [Game, ThrowAim]            ← 按 t（有投掷物时）
```

- 按键分派：`process_key` 读 `stack.last()` → 分派到对应页面处理器
- Back 路由：Esc 在列表页 pop 栈，在详情页切回列表
- 无阻塞旁路：所有页面走主循环 30FPS 渲染，不绕过

**每个页面的责任界定：**

| 页面 | 按键处理 | 渲染 |
|------|---------|------|
| Game | 游戏行动（移动/攻击/技能） | 地图 + 行动轴 + 状态面板 |
| Look | 方向键移动光标，x/Esc 退出 | 地图 + 光标高亮 + 信息面板 |
| ThrowAim | 方向键瞄准，Enter 投掷 | 地图 + 轨迹 + 光标 |
| ThrowSelect | Enter/r/y 装填投掷物 | 叠加层（对话框风格） |
| Inventory | 方向键选择，e 装备，d 丢弃 | 全屏替换地图（`is_fullscreen=true`）|
| Dialog | Y 确认，N/Esc 取消 | 叠加层（对话框风格） |

**为什么不是单一 match / 独立模态：**
- 单一 match（旧方案）：`process_key` 内 ~200 行 match 所有按键，加入新 UI 需要修改多处
- 独立模态（旧方案）：`open_look_mode` 等有自己的 `event::read()` 循环，绕过 30FPS 框架
- 页栈：每个页面一个 handler，共处相同框架，新 UI 只需加一个新枚举变体

**与渲染管道的关系：**
- 页面状态存在 ECS Resource 中（`InventoryUI`、`LookCursor`、`ThrowPreview`）
- 渲染管道的 UI 层读取这些资源 + 页栈，决定绘制什么
- 全屏页（Inventory）跳过游戏 UI 渲染，非全屏页叠加在游戏 UI 之上

**Background**

旧方案将查看模式、投掷瞄准、背包分别实现为独立的阻塞式 UI，各自有 `event::read()` 循环。这在 30FPS 框架下不协调——它们无法参与固定帧率渲染，且 `modal_flag` 需要暂停输入线程。

ISSUES.md A14 记录了查看模式的架构问题。A16 的 `InputBuffer` 资源在页栈框架下变为非必要——按键直接由栈顶处理器消费，不需要中间缓冲。`modal_flag` 在所有页面迁移完成后已为死代码。

---

## 五、数据与方法

### Dsn22 MonsterTemplate 结构体统一 — 设计中，实验方向

**决策（暂缓执行）**

将 `monster_def.rs` 中 7 个分散的 match 函数（`monster_glyph`、`monster_color`、`monster_name`、`monster_attack_name`、`monster_stats`、`monster_loot`、`monster_spawn_weight`）合并为一个 `MonsterTemplate` 结构体 + 一个查表函数。

```rust
pub struct MonsterTemplate {
    glyph: char,
    color: (u8, u8, u8),
    name: &'static str,
    attack_name: &'static str,
    // 系数——不存最终值，存公式参数
    hp_base: i32, hp_per_floor: i32,
    atk_base: u32, atk_per_floor: f64, atk_max: u32,
    def: u32, agi: u32, magic_mastery: u32,
    exp_base: f64, exp_per_floor: f64,
    loot: &'static [LootEntry],
    spawn_weight_base: f32, spawn_weight_per_floor: f32,
    spawn_weight_min: f32, spawn_weight_max: f32,
}

impl MonsterTemplate {
    pub fn stats(&self, floor: u32) -> Stats { /* 统一公式 */ }
    pub fn spawn_weight(&self, floor: u32) -> f32 { /* 统一公式 */ }
    // glyph/color/name 等退化为字段直接访问
}
```

收益：
- 加新怪物从改 7 个 match 变为加 1 行数据
- 对外的 `monster_glyph(kind)` 等函数退化为 `template(kind).glyph` 等字段访问
- 未来切换到 JSON 配置驱动只需改 `fn template()` 的加载源，调用方不变

**当前状态：** 暂缓。当前 `monster_stats` 等函数中的公式是直接赋值（裸数值），尚未抽象为统一的系数+公式体系。GAME.md 的数值标注体系（`[⃞计算]/[⃞直觉]/[⃞试调]`）也未完成。在公式体系设计就绪前，硬套系数反而引入硬编码的假灵活性。先记录方向，等 GAME.md 完成后再实施。

**关联：** GAME.md Gm8（怪物设计）、LESSONS.md（无直接关联）

---

### Dsn23 日志系统分层集成 —— 开发者日志 vs 玩家日志

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

### Dsn24 多类型地图：繁茂洞穴 + 地海（已定案并落地）

**决策**

解决"地图只有一种、楼层无视觉/生态区分"的问题。多分支楼梯暂缓（类型系统稳定后再规划类型分配，见 ISSUES 讨论）。

**地图类型与派生**

- `MapKind`：`Cavern`（标准洞穴）/ `LushCavern`（繁茂洞穴）/ `Undersea`（地海）
- 骨架统一为 room_accretion 洞穴，环境修饰差异化（不引入新算法）
- 类型派生：`map_kind_for(seed, floor)`——F1 固定 Cavern（新手层），之后按 `seed×黄金常数 + floor×31` 哈希取模三分均分 [⃞试调]
- 类型与地图均由 `(MapSeed, floor)` 确定性重建——**存档零改动**，读档后当前层与下楼结果一致

**环境参数表（MapEnvParams）**

| 参数 | Cavern | LushCavern | Undersea |
|------|--------|-----------|----------|
| 深水种子率 ‰ | 2 | 0 | 20 [⃞试调: 8‰ 在真实地图期望种子仅 3，方差大易 0 水域] |
| 深水种子最小房间距离 | 3 | 3 | 1（水域贴近活动区） |
| 深水扩散加成 | 0 | -0.02 | +0.08 |
| 浅水扩散 % | 10 | 2 | 18 |
| 障碍密度 % | 7（钟乳石） | 10（垂藤） | 3（珊瑚礁） |
| 装饰 % | 0 | 25（菌丝）+15（蘑菇丛） | 15（沙岸）+10（海草） |

**新方块（6 种，serde tag 5-10 追加）**

| Tile | 地形 | 可走 | 挡视线 | 生态角色 |
|------|------|------|--------|---------|
| Mycelium 菌丝 | 繁茂 | ✓ | | 真菌地面 |
| FungalPatch 蘑菇丛 | 繁茂 | ✓ | | 真菌点缀（菌丝上 15%） |
| HangingVine 垂藤 | 繁茂 | ✗ | ✓ | 替代钟乳石的障碍 |
| Sand 沙岸 | 地海 | ✓ | | 水域边缘（8 邻域 15%） |
| Seagrass 海草 | 地海 | ✓ | | 浅水点缀（10%） |
| CoralReef 珊瑚礁 | 地海 | ✗ | ✓ | 替代钟乳石的障碍 |

**水域保留关键修复**：`carve_expand` 只挖 Wall（原逻辑挖所有不可走格，会把地海水挖成 Floor）；所有通道挖掘（ensure_connectivity/ensure_connection_between/ensure_spawn_accessible）统一走 `carve_channel`——DeepWater 变为 ShallowWater（涉水通道），保留水域且保证 4 方向连通（G22）。

**生态对应（新怪 5 种，MonsterKindId 变体追加 3-7）**

| 怪物 | 地形 | 定位 | 掉落 | 经验定位 |
|------|------|------|------|---------|
| Sporeling 孢子怪 m | 繁茂 | 弱（HP12 攻4） | 蘑菇 60%、苔藓 40% | ≈老鼠 |
| MushroomGolem 蘑菇傀儡 M | 繁茂 | 中（HP22 攻7） | 苔藓 80%、孢子囊 30%、蘑菇 30% | ≈蝎子 |
| CaveFish 洞穴鱼 f | 地海 | 弱快速（HP10 敏14） | 鱼骨 60%、海藻 30% | ≈老鼠 |
| CaveCrab 洞穴蟹 c | 地海 | 中高防（HP18 防4） | 贝壳 80%、珍珠 10% | ≈蝎子 |
| DeepEel 深鳗 e | 地海 | 中（HP15 攻6） | 鳗皮 60%、海藻 40% | ≈蝎子 |

- 生成权重按 `MapKind` 分派（`monster_spawn_weight(map_kind, kind, floor)`）：繁茂以真菌为主+少量原生物；地海以水生为主+少量蝎子；洞穴保持原状
- 新掉落物 8 种（ID 25-32）：蘑菇/海藻为地形消耗品（r 键使用：+6 HP / +4 MP，上限钳制 [⃞试调]），其余为材料（Dsn19 Phase 2 合成储备）

**关联：** ISSUES G23/I70 | GAME.md Gm7/Gm10 | 生态对应原则（回复对称：繁茂回 HP ↔ 地海回 MP）

**状态：** 已落地。分支楼梯（多楼梯/树状分支）待类型系统稳定后单独规划。

---

### Dsn25 业务领域收敛至 `core`，旧组件体系作废（进行中）

**决策**

未来只有 `core` 承载游戏逻辑/业务领域内容；`core` 内部完全采用 ECS 范式。

- 领域以 ECS 组件/实体/事件/系统表达，不再依赖旧的聚合 `Stats`、`ActionKindV3`、`ActionQueue` 等模型。
- `core` 是后续所有游戏逻辑的权威层；旧 `dungeon-*` 与 `src/` 视为历史/过渡代码，参考价值有限。
- 旧组件体系不迁移，只作为理解历史的参考；新代码以 `core/src` 中的组件和系统为准。
- 当前先按 `core/src/components.rs` 中的方向补全：基础数值组件（`Position`/`Health`/`Magic`/`Level`/`Experience`/`Attack`/...），行动状态（`Idle`/`Active`/`Failure`），能力标记（`Can*`），具体行动组件（`BasicAttack`），领域事件（`AttackEvent`）。

**背景**

refactor 分支已经删除旧模型并引入 ECS 原生行动模型，但旧 crate 仍保留了大量历史实现。继续在旧 crate 上修修补补会延续架构债，因此把业务领域收敛到一个新的 `core` crate，逐步摆脱对旧组件体系和旧目录结构的依赖。

**状态：** 初步补全中。旧文档中的 ActionQueue/ActionKindV3/Stats 描述不再代表未来方向。

---

### Dsn26 渲染契约 `render-api`：后端无关的 SceneFrame（已落地 v1）

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

**关联：** REFACTOR.md §1.1 | DESIGN.md Dsn20（渲染优化属于 TUI 后端内部）| Dsn21（页栈状态放 presentation，渲染放后端）

**状态：** `render-api` v1 已落地（34 个测试通过，`cargo clippy -p render-api -D warnings` 干净）。下一步：`presentation` 提取层 + `tui` 去 `core` 依赖。


---

### Dsn27 行动即实体 + 速度组件：AV 系统的 ECS 化（草案，待 PoC）

**决策**

两条相关决策，合并为一次行动系统重构：

**① 行动即实体（替代 `ActionKind`）**

- 一个行动 = 一个 actor 的子实体（action entity），不再用中央 `ActionKind` enum 分派。
- Action 实体组件：`ChildOf(actor)`、`ActionPriority(u32)`、`ActionTimer`、`ActionSource`、生命周期标记 `Candidate` / `ActiveAction` / `Ready`，以及具体行动 ZST/payload（`Wait` / `Move { dx, dy }` / `BasicAttack { target }` / `Chase` / `Flee` / `Wander`）。
- `Can*` **保持 actor 上的 ZST 组件**，不子实体化：能力回答“能不能做”，action 实体回答“正在考虑/执行什么”。
- 保留生成/仲裁分离，不因当前规模合并为单函数（用户确认；OCP 扩展点）。
- 普通怪 = 新模板 / Bundle（不同 `Can*`）；新增普通怪不需要修改 AI / 生成 / 执行代码。
- 系统流程：生成系统只 spawn 候选 → 仲裁系统唯一写入 actor 行动状态（按 `ActionPriority` + `action_entity.to_bits()` 全序）→ Tick 推进 AV 并加 `Ready` → 执行系统按行动类型专用 query（零中央 match）→ completion 系统消费 `ActionSucceeded/FailedEvent` 并回收 action 实体。
- 玩家行动直接生成 active action，不进入 AI 仲裁。

**② 速度组件（替代 `Agility`）**

- 删除 `Agility`；新增 `MoveSpeed(f64)` / `AttackSpeed(f64)` 两个倍率组件（1.0 基准，越高越快）。
- `AV = base_duration / speed.clamp(MIN_SPEED, MAX_SPEED)`。
- `Move/Chase/Flee/Wander → MoveSpeed`；`BasicAttack → AttackSpeed`；`Wait` 固定 `WAIT_DURATION`（已确认；后续再评估 `WaitSpeed`）。
- 删除 `agility_to_reaction` / `agility_speed_factor` / 旧 `action_av`；先不保留 `BASE_REACTION`（已确认；试玩需要时再加统一常数）。
- 迁移顺序：逐行动（Wait → Move → BasicAttack → Wander → Chase → Flee）；怪物速度先按旧敏捷保行为映射，再在 GAME.md 用 `[⃞试调]` 重调。
- 未来武器速度 → `AttackSpeed`，重甲/地形 → `MoveSpeed`，Buff/装备可动态增删组件。

**背景**

当前 `ActionKind` 中央 enum + `mount_action` match 让新增行动要改多处；`Can*` + ZST + 专用 query 的执行方式其实可以完全去掉 `ActionKind`。同时当前 `ActionTimer` 从未参与执行门禁（I89）、事件也因 Schedule 重建 + 缺少 `Events::update()` 被重复读取（I90）；这两项已在第 1 步修复，否则速度组件不会真正影响行为。

**代价**

- action 实体带来每轮候选 spawn/despawn 的 churn；当前规模可接受，未来可改持久 `ActionSlot` 或行为实体池化。
- 仲裁需要 `ChildOf` 分组；Bevy 0.16 `Children` 是 `linked_spawn`，父实体 despawn 会联动 despawn 子实体。
- 事件必须有真实消费者；`ActionSucceeded/Failed` 正好由 completion 消费，`DeathEvent`/`LevelUpEvent` 要么接线要么删除。

**关联：** REFACTOR.md §2.6 / §3.6 / §8.1 / §10.6 / §10.8 | ISSUES D29、A41、A42、A43、I89、I90、G35

**状态：** ①② 均已落地（Phase B/C 完成）；I89（AV 门禁）与 I90（事件生命周期）已修；§11.6 已按推荐执行（逐行动迁移、倍率速度、先删反应时、`Wait` 固定、怪物速度先保行为）。① 的后续只剩 Phase D 的 ②（速度组件）。

**进展（Phase A/B/C）：**

- Phase A：core 冒烟测试补齐（地图确定性 / 移动 / 攻击只结算一次 / 死亡→经验→升级 / FOV·记忆·占用图 / 快怪多动），`cargo test -p core` 从 4 → 18 个测试（ISSUES P9）。
- Phase B（action 实体 PoC）：`core/src/action/entity.rs` 落地 ① 的完整链路；测试覆盖生成 → 仲裁 → tick(`Ready`) → 执行 → completion，含优先级/平局/忙碌 actor/不写 actor 状态等契约。
- Phase C（全量迁移，C1–C9 完成）：六个行动逐行动迁移并各配 parity 场景；主循环切换到新链路（`world/loop_.rs` 改为「先挂载、再推进」两段式）；`ActionKind` / `mount_action` 中央 match / `finish_action_*` / `decide_monster_actions` / `choose_action` / `run_action_cycle` / 旧独占执行系统与 actor 上的行动 ZST 挂载路径**全部删除**，全库无 `ActionKind` 引用。
- 执行器形态（修正记录）：`execute_move_system` 一度写成 exclusive `&mut World`，理由是「action 实体 → actor 位置的多实体读写无法用普通 `Query` 表达」。**该结论是错的**：那只是因为复用了 `movement::execute_move(&mut World, ...)`——该签名把「读资源 + 读组件 + 写组件」揉进一次 `&mut World` 调用；而 Bevy 的 `Query<&mut T>` 只保证 **per-entity** 唯一可变访问，驱动实体（action）与被写实体（actor）不同，读写两处并无真冲突。把移动落点抽成纯函数 `moved_position(map, occupancy, pos, dx, dy) -> Option<Position>`（`can_move_to` 规则不变）后，执行器自然写成普通参数化系统。副作用是 A41 范围收窄：行动链路里已无 exclusive 系统，可与既有 `CoreSettleSchedule` 系统同调度共存（有测试断言）。
- 迁移期约束（写测试时要注意）：`Ready` 的清理分两条路（`execute_move_system` 显式清，其余靠 completion despawn 实体）；`Idle`/`Failure` 必须互斥（I91）；「挂载玩家行动」与「推进世界」必须分两段调度，否则会把「本轮已执行完」误判成「命令被拒绝」。

---

### Dsn28 渲染后端插件化：`render-api` / `presentation` / TUI / GPU（草案）

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

**关联：** DESIGN Dsn1 / Dsn20 / Dsn21 / Dsn26；REFACTOR §12；ISSUES I87/I88；README 渲染契约章节。

**状态：** 草案；R0 已落地；R1 起待 core Phase A–F 完成后启动（REFACTOR §11 Phase G）。