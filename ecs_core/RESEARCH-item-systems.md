# 物品系统外部调研：注册表 / 实例 / 容器 / 装备 / 属性聚合 / 存档

> 调研对象：`ecs_core/src/item/`（`registry.rs` / `inventory.rs` / `loot.rs` / `mod.rs`）、`ecs_core/DESIGN.md` 的 DsnE16 / DsnE10 / DsnE12。
> 纪律：只列**实际打开过**的来源；凡属本文自己的推理，标 `[推断]`。

## 结论摘要

① **Stat 扩维：采纳**——把 `CritRate` / `CritDamage` / `MagicMastery` 加进 `Stat`，而不是给暴击新开组件；暴击参数早已是 `rules/damage.rs` 的一等公民（`CritProfile`），只是 `system/combat.rs:54` 绕过 `apply_modifiers` 直接读组件。② **派生属性：调整**——`Attack`/`Defense`/速度继续读时折叠（成本 ≤4 装备 ×1 修正）；但 `Health.max` 是**存储态**（`heal`/`damage` 都拿它 clamp），必须由一个 owner 系统写，推荐"每轮无条件重算 + 写前比较"，而不是 dirty flag。③ **投掷：采纳**——在 `ReadyDecision` 里加第三个决策组件 `ThrowTarget` + `ItemDef` 上的 `LandingKind`，settle 仍是一个系统；这条正合 `settle.rs` 模块头写的扩展点。④ **存档：采纳**——手写投影，装备在存档点摊平成 `(slot: u8, id: u16)`，**一个 `Entity` 都不写盘**；这是"装备不抄数值"决策的第二个红利（第一个是"卸下即失效"）。

## 来源表

| 来源 | 类型 | 一句话要点 | URL |
|---|---|---|---|
| flecs `game_mechanics/inventory_system` | 示例 | 定义=预制体、实例=`is_a` 实体、容器=实体 + `ContainedBy`(Exclusive)、堆叠=`Amount`、穿戴=`Active` tag | [链接](https://www.flecs.dev/flecs/examples-game_mechanics-inventory_system.html) |
| flecs *Designing with Flecs* | 文档 | 给出"何时该用关系"的清单（含 inventory），并明说组件里放 Vec 是允许的 | [链接](https://www.flecs.dev/flecs/DesignWithFlecs.html) |
| flecs *Relationships* | 文档 | 关系=pair；关系查询常数时间；可表达层级/inventory/交易 | [链接](https://www.flecs.dev/flecs/Relationships.html) |
| flecs `relationships/exclusive_relations` | 示例 | `EcsExclusive`：加第二对 pair 时**同一次操作**移除旧的（"一个槽只有一件"） | [链接](https://www.flecs.dev/flecs/examples-relationships-exclusive_relations.html) |
| flecs `observers/monitor` | 示例 | `EcsMonitor`：实体**进入/离开**查询时触发（`OnAdd`/`OnRemove`） | [链接](https://www.flecs.dev/flecs/examples-observers-monitor.html) |
| flecs `prefabs/slots` | 示例 | 槽可做成预制体子节点，实例化后反向解析（"slot-like lookup without adding a relationship"） | [链接](https://www.flecs.dev/flecs/examples-prefabs-slots.html) |
| Bevy Book *Relations* | 文档 | 官方把 `ContainedBy`/`Contents` 当"容器/角色背包"示例；`linked_spawn`=父死子随 | [链接](https://bevy.org/learn/book/storing-data/relations/) |
| Bevy Book *Change Detection* | 文档 | `Added`/`Changed`/`Ref::is_changed`/`RemovedComponents`；DerefMut 就标脏；有一帧延迟风险 | [链接](https://bevy.org/learn/book/control-flow/change-detection/) |
| `bevy_save`（README） | 源码 | 反射快照 + `MapEntities` + `Prefab` 手写投影；明言 `Entity` 不可作持久 id | [链接](https://github.com/hankjordan/bevy_save) |
| `bevy_inventory::Inventory` | 源码/文档 | 组件里 `Vec<ItemStack>` + `max_slots`；`add` 返回溢出量；直接 derive Serialize | [链接](https://docs.rs/bevy_inventory/0.1.0/bevy_inventory/struct.Inventory.html) |
| Roguelike Tutorial ch.11 *Saving and Loading* | 教程（Specs） | 实体 id 合成、`SimpleMarker`/`ConvertSaveload` 做稳定 id；每种组件要手加进存档表 | [链接](https://bfnightly.bracketproductions.com/rustbook/chapter_11.html) |
| Roguelike Tutorial ch.58 *Item Stats* | 教程（Specs） | `EquipmentChanged` 脏 tag + 一个累加系统；`Attributes{base,modifiers}` 读时求和；`Mob.loot_table: Option<String>` 走全局 `RAWS` | [链接](https://bfnightly.bracketproductions.com/rustbook/chapter_58.html) |

## A. 物品定义：静态表 / 运行时注册表 / 一个定义一个实体

**各实现的做法**

- flecs 走"一个定义一个实体"：`ecs.prefab<IronSword>().set<Attack>({2}).set_auto_override<Health>({10})`，实例是 `is_a` 它的实体；"这件东西是什么种类"由 `item_kind()` 在实例的**全部 id** 里扫 `IsA` 链**运行期算出来**（[示例源码](https://www.flecs.dev/flecs/examples-game_mechanics-inventory_system.html)）。
- flecs 设计指南把预制体定位成"初始化 + 分类 + 更快"，并明说实体句柄稳定、可以安全存进组件（[DesignWithFlecs](https://www.flecs.dev/flecs/DesignWithFlecs.html)）。
- `bevy_inventory` 走另一头：种类是** Rust 枚举 + `ItemKind` trait**（`display_name`/`max_stack`），定义在代码里、没有表（[docs.rs](https://docs.rs/bevy_inventory/0.1.0/bevy_inventory/struct.Inventory.html)）。
- 我们：`ITEMS: &[ItemDef]` + `ItemId(u16)`，`def()` 线性 `find`（`registry.rs:188`）。

10 个 vs 1 万个的真实差别不在"装不装得下"：flecs 那一侧的代价是**种类判定退化成运行期扫描**，且共享字段必须显式 `set_auto_override` 才会在实例上可写——忘了就是"改一件铁剑，全世界的铁剑都变了"。我们这一侧的代价是 `def()` 的 O(n)：调用点在 `ItemStack::def/room/label`、拾取、以及每次求值收装备时。[推断] 当前 n≈22、装备 ≤4，一次求值几十次比较，可忽略；n 到 1e3 以上或背包页每帧查 24 格 `def()` 时才值得动。

**对我们的含义**

**采纳静态表**（否决运行时注册表与实体化定义）。flecs 那套换来的是"运行期可加物品"（mod/脚本），代价是种类判定与 override 纪律都变成运行期；本项目没有 mod 需求，而"表里没有这个号就 panic"能在测试期抓住错误。要改的**不是存储形状，而是查找形状**：`ITEMS` 保持，`def()` 内部换成 `const` 建的稀疏索引（`[Option<&'static ItemDef>; 64]`，号段留空正好让上界可控），超出范围仍走 `find` 兜底；加物品仍是"加一行 + 可能加一条掉落"。这条改动只在 profiler 指出 `def()` 变热时做，现在做属于投机。

## B. 物品实例：纯数据 `(id,count)` vs 每个实例一个实体

**各实现的做法**

- flecs：**每个实例一个实体**。堆叠靠实例上的 `Amount`（transfer 时若目标已有同种类则相加并 `destruct` 源实体）；耐久靠实例上的 `Health`，且必须 `set_auto_override` 才是"每个实例一份"（[示例](https://www.flecs.dev/flecs/examples-game_mechanics-inventory_system.html)）。
- `bevy_inventory`：**纯数据** `ItemStack<K>` 装在 `Vec` 里，整个 `Inventory` 直接 derive `Serialize`（[docs.rs](https://docs.rs/bevy_inventory/0.1.0/bevy_inventory/struct.Inventory.html)）。
- 教程（Specs）：物品也是实体，归属靠 `InBackpack{owner}` / `Equipped{owner}` 这类组件（ch11 的存档类型表里 `InBackpack`、`Equipped` 与 `Item` 并列，[ch11](https://bfnightly.bracketproductions.com/rustbook/chapter_11.html)）。
- 我们：背包=数据、地面=实体、装备=实体，**本来就是混合**。

**对我们的含义**

**采纳混合，并把判据写进文档**：判据不是"哪个更 ECS"，而是"**这件东西有没有实例态、有没有别的系统要挂东西到它身上**"。flecs 敢给每件物品建实体，是因为它有 `Amount`（堆叠）与 `Health`（耐久）这类实例态要放；`ItemStack` 只有 `(id,count)`，建实体只换来遍历、排序与泄漏。边界条件是"**品质 / 耐久 / 附魔**"：一旦出现，背包格里必须能引用一个实例，形状是 `ItemStack{id,count}` 不动 + 一张稀疏的 `ItemInstance` 表（或那一格直接换成实体引用），**而不是把 `ItemDef` 的字段抄进 `ItemStack`**——抄了就是 ISSUES ECS25 那类"两个真相"。这个边界现在没到，不要提前做，但要在 `inventory.rs` 模块头写一句"何时必须升级"。

## C. 容器/背包：组件里的 vec / 每格一个实体 / 关系

**各实现的做法**

- flecs：容器是**实体**（`Container` tag），物品挂 `ContainedBy` 并注册成 `Exclusive`（一个物品只能在一个容器里）；玩家的背包本身也是一个实体，玩家用 `Inventory` 关系指向它（`player.add<Inventory>(player_inventory)`，[示例](https://www.flecs.dev/flecs/examples-game_mechanics-inventory_system.html)）。
- Bevy Book 的 *Relations* 章直接把 `ContainedBy`/`Contents` 当"容器或角色背包"的官方示例，并提醒"不想让容器内容跟着画进场景就别用 `ChildOf`"（[链接](https://bevy.org/learn/book/storing-data/relations/)）。
- `bevy_inventory`：组件里 `Vec<ItemStack>` + `max_slots`，`add` 先填已有堆再开新格、**返回溢出量**（允许部分放入），另有 `has_room_for` 预检与 `split_stack`/`merge_slots`/`compact` 处理碎片（[docs.rs](https://docs.rs/bevy_inventory/0.1.0/bevy_inventory/struct.Inventory.html)）。
- 我们：`Inventory { slots: Vec<Option<ItemStack>> }`，24 格定长，`try_add` **要么全放要么不放**（`inventory.rs:122`）。

**对我们的含义**

**采纳现状**，但把两条与主流不同的口径钉死。① **定长 + `Option`** 换来稳定格号（TUI 翻页/拖拽需要），`bevy_inventory` 用"长度 ≤ max_slots 的紧凑 Vec"换遍历效率——对 UI 形态而言我们的选择更合适；② **整堆拒绝**是有意的（`inventory.rs` 模块头已写理由），`bevy_inventory` 的"返回溢出量、调用方自己决定剩下的去哪"也能自洽，但**两条不能混**：将来若接"捡一半"，必须把 `try_add` 的返回值语义从"0 或 count"改成"溢出量"，并同时改 `mod.rs:205` 那句 `== 0` 的判断——那是一处会被静默踩到的缝。另外 `has_room_for` 那种"先算再动"的两遍扫描在成熟实现里也是常规做法，`try_add` 的预检不是特例。

## D. 装备/挂载：关系 vs 槽位字段；owner 死亡时的级联

**各实现的做法**

- flecs 示例：装备 = 物品实体上的 **`Active` tag**（"穿戴中/已激活"），物品仍留在背包里；"一个物品只能在一个容器里"由 `Exclusive` 保证（[示例](https://www.flecs.dev/flecs/examples-game_mechanics-inventory_system.html)）。"一个槽只有一件"可以用 `EcsExclusive` 关系表达：加第二对 pair 时框架在**同一次操作**里移除旧 pair（[exclusive_relations](https://www.flecs.dev/flecs/examples-relationships-exclusive_relations.html)）。另一种槽位口径在 `prefabs/slots`：把槽做成预制体子节点，实例化后用"预制体子节点"反查"实例子节点"（[slots](https://www.flecs.dev/flecs/examples-prefabs-slots.html)）。
- Bevy：带 `linked_spawn` 的关系（`ChildOf`、以及 Book 里那个 `Contents`）在父实体 despawn 时**级联 despawn 子实体**（[Relations](https://bevy.org/learn/book/storing-data/relations/)）。
- 我们：`EquippedBy` + `EquippedItem{slot,id}`，**不带** `linked_spawn`，owner 死后装备变"无主"（DsnE12 已实测）。

**对我们的含义**

**采纳现状并收紧不变量**。`linked_spawn` 对装备语义相反（战利品会随尸体消失），这条我们已经写进类型，外部来源只是复述了同一件事，无需改。真正值得借的是 flecs 的 **`Exclusive`**：它把"一个槽只有一件"变成**框架保证**，而我们的 `slot_is_taken()`（`mod.rs:218`）是每次装备前手扫全表——"同一轮里两条装备命令"就能塞进两件同槽装备，是个会在 UI 上线后被踩到的窗口。[推断] Bevy 0.16 的关系没有"互斥"属性（打开的文档里未见对应机制，见"未能核实"），所以正确做法不是模仿 `Exclusive`，而是把这条不变量收进**唯一的装备入口**（一个函数或一个 owner 系统），并补一条"装第二件同槽装备必须替换而非并存"的测试。`EquippedItem.slot` 保持字段——它是数据，且让"卸下/交换"只改一个组件。

## E. 属性聚合：读时折叠 vs 缓存 + 失效

**各实现的做法**

- 教程 ch58 写得最直白：每回合遍历所有实体累加重量/先攻惩罚是 "potentially rather slow"，因为 "most entities" 都有装备，而**只在变化时重算**；做法是给 actor 加空 tag `EquipmentChanged`，`EncumbranceSystem` 在 `equip_dirty.is_empty()` 时**直接 return**，否则只重算被标脏的实体，结果写进 `Pools.total_weight` / `Pools.total_initiative_penalty`（[ch58](https://bfnightly.bracketproductions.com/rustbook/chapter_58.html)）。**代价**：拾取、使用、丢弃三个系统都要显式 `dirty.insert(entity, EquipmentChanged{})`——漏一条就是"数字不动"。
- 同一份代码里另一类数值是**读时求和**：`Attributes { might: { base, modifiers } }`，容量用 `(might.base + might.modifiers) * 15` 在读取点算，`base` 从不被覆盖（同上）。
- flecs 提供的是**事件原语**、不是缓存框架：monitor observer 在实体"进入/离开查询"时触发（`EcsMonitor` → `OnAdd`/`OnRemove`），正好是"装备集合变了"的信号，但聚合仍要自己算（[monitor](https://www.flecs.dev/flecs/examples-observers-monitor.html)）。
- Bevy 的等价物是变更检测，官方文档点明两个坑：**DerefMut 就标脏**（值没变也标，要用 `set_if_neq`），以及"改的人跑在查的人之后"会有**一帧延迟**（[Change Detection](https://bevy.org/learn/book/control-flow/change-detection/)）。

**对我们的含义**

**调整：分两类，不搞一刀切。** 第一类，`Attack` / `Defense` / `MoveSpeed` / `AttackSpeed` 继续**读时折叠**——成本是 `装备数 × 修正数`（≤4×1），比任何失效机制都便宜；教程要缓存是因为它要对**每个带装备的实体**做聚合（"most entities"），我们只有玩家和少量怪物，量级差两个数量级 [推断]。第二类，`Health.max` / `Magic.max` **已经是缓存**：`components.rs:48` 把 `max` 当存储态，`heal`/`damage` 都用它 clamp（`components.rs:73-81`），`experience.rs:50` 在升级时重写。所以问题不是"要不要缓存"，而是"**谁是唯一的写者**"：新增一个 `recompute_vitals_system`，每轮对 actor 用 `max_hp_for(level, folded_defense)` 与 `max_mp_for(level, folded_magic)` 重算并写回，**写前比较**（不等才写，避开"mut 必标脏"）；`experience.rs` 里那段重算删掉，只留"升级"的语义。dirty tag 等 actor 数上百再加，届时照抄 ch58 的形状，但要承认"每条改变途径都得标脏"的对称性成本（DsnE10 第 3 条反对 push 的理由，这里变成**已付过**的既有成本）。无论哪类，**base 组件永远是权威**，折叠值从不写回 base，缓存字段在命名与注释里标 `max`/`total`。

## F. 物品行为：数据驱动 / 一件一段代码 / 实体组合

**各实现的做法**

- flecs：行为是**读组件的代码**——`weapon.try_get<Attack>()`，拿不到就打印 "the weapon is a dud"（[示例](https://www.flecs.dev/flecs/examples-game_mechanics-inventory_system.html)）。"是什么"在预制体上，"能做什么"在系统里，没有 `match item_id`。
- 教程：行为是**挂在物品实体上的小 payload 组件**——`Consumable`、`Ranged`、`InflictsDamage`、`AreaOfEffect`、`ProvidesHealing`、`Confusion` 各自独立（ch11 的存档类型表把它们逐个列出，[ch11](https://bfnightly.bracketproductions.com/rustbook/chapter_11.html)），使用时由一个系统对每个 `if let Some(..)` 依次判。
- 我们：闭合词表 `ItemKind::Consumable(ConsumeEffect)`，`pickup_system` 里 `match effect`（`registry.rs:77`、`mod.rs:158`）。

**对我们的含义**

**采纳闭合词表，但把"何时换成实体组合"写成判据。** 只要效果是"立即、无目标、作用于使用者"（回血/回蓝），闭合枚举就是一行数据 + 一个 match，最省；一旦出现"需要目标/范围/持续/叠加"（卷轴、投掷药水、附魔），就该走 DsnE12 的效果实体，而不是往 `ConsumeEffect` 加变体——`registry.rs:71-75` 已经写了这条红线。外部证据支持它：教程那套 payload 组件能长，是因为每个组件背后**恰好有一个系统**消费它，而不是一个越来越长的 match。具体形状：`ItemKind::Consumable(&'static [UseEffect])`，`UseEffect::{Heal, RestoreMagic, GrantStatus{..}}`，`pickup/use` 只保留一个 `apply_use_effects(&[UseEffect], ...)`——加一种效果改两处，仍然没有 `match item_id`。

## G. 掉落生成：表放物种行还是独立注册表；确定性

**各实现的做法**

- flecs / Bevy 都**不提供**掉落系统；flecs 容器示例里的战利品是手写实体列表（[示例](https://www.flecs.dev/flecs/examples-game_mechanics-inventory_system.html)）。
- 教程：**物种行持有"表的名字"**，表本身在 raw 注册表里——`Mob { loot_table: Option<String>, .. }`，取值走全局 `RAWS`（`RawMaster`；ch58 代码里可见 `RAWS.lock()`、`raws.raws.items`），数量用骰子字符串 `"1d6"` 走 `parse_dice_string` + `rng.roll_dice`（[ch58](https://bfnightly.bracketproductions.com/rustbook/chapter_58.html)）。
- 我们：`MonsterTemplate.loot: &'static [LootEntry]`，表**内联在物种行**；`roll_loot` 每条独立掷、顺序固定"概率→数量"、即使 `chance==1.0` 也消耗一次随机数（`loot.rs:59-79`）。

**对我们的含义**

**采纳现状**，并写下两点差异。① 教程用"名字 → 全局表"是因为它的表要能被 JSON 作者改；我们的表是编译期常量，"物种行内联"少一次查表、也少一类"名字打错"的运行期错误——正是 `loot.rs` 模块头已经写的理由。② **确定性口径我们比来源更严**：没有哪份外部文档规定"每条必须消耗一次随机数"，但那是"改一条掉率不会平移后面所有条目"的唯一办法（`loot.rs:62-64`），属项目自定、外部无佐证，**保留**。③ 唯一值得借的是"表可复用"：两个物种掉同一张表时，提一个 `const GOBLIN_LOOT: &[LootEntry]` 共享，而不是复制——仍是编译期，不引入注册表。

## H. ECS 原生物品/装备的存档

**各实现的做法**

- `bevy_save`：反射快照（`Reflect` + `Snapshot`），实体引用靠 `MapEntities` 重映射；它明确警告 **`Entity` 是不透明标识**、"直接序列化 `Entity` 不保证跨版本可读"；并提供 `Prefab` trait 做**手写投影**（示例只存 `position`/`velocity`，其余组件在 `spawn` 时重建）。3.0 起关系类型必须注册，且 `RelationshipTarget` 一侧**不写进快照**，加载时由关系侧重建（[README](https://github.com/hankjordan/bevy_save)）。
- 教程（Specs）：实体 id 是 "purely synthetic"，不保证下次相同，所以用 `SimpleMarker<SerializeMe>` + `SimpleMarkerAllocator` 给可存实体分配**稳定 u32**，`ConvertSaveload` 派生负责组件内 `Entity` ↔ u32；每种组件要逐个列进 `serialize_individually!`，作者在结尾把代价写明："加组件多了几步（注册、derive、加进类型表）"（[ch11](https://bfnightly.bracketproductions.com/rustbook/chapter_11.html)）。
- 我们的独特性（复核后补充的旁证）：valence（Rust 写的 MC 服务器，也用 `bevy_ecs`）的 `Inventory` 同样是**组件 + 槽位**（`slot`/`set_slot`/`replace_slot`/`swap_slot`/`first_empty_slot`/`first_slot_with_item(item, stack_max)`），而它的 `ItemStack::new(ItemKind, count, Option<..>)` **第三个参数就是"每件实例的可选组件数据"**——独立印证了 §B 的判据：实例态出现时必须挂在 `ItemStack` 上（或改成实体引用），不能塞进 `ItemDef`（[docs.rs](https://docs.rs/valence/0.2.0-alpha.1+mc.1.20.1/valence/prelude/struct.Inventory.html)）。
- 我们的独特性：`DsnE17` 规定"换层 = 保留玩家 + 由 `(seed, floor)` 重建其余"，所以**要存的只有玩家**；而 `EquippedItem` 只存 `(slot, id)`、数值全在注册表——装备在存档点可以**无损摊平成数据**。

**对我们的含义**

**采纳手写投影，否决反射与"序列化实体"**（依赖里已有 `serde`/`serde_json`，`ecs_core/Cargo.toml:10-11`，不需要新依赖）。形状：

```rust
struct SaveV1 { version: u32, seed: u64, floor: u32, player: PlayerSave }
struct PlayerSave { level: u64, xp: f64, pos: (usize, usize), hp: f64, mp: f64,
                    bag: Vec<(u16, u32)>, equipped: Vec<(u8, u16)> } // slot 用 u8，不用字符串
```

存：遍历 `Inventory` 的非空格 + 遍历 `EquippedBy`/`EquippedItem` 的四个实体 → 两组纯数据。读：先按 `(seed, floor)` 重建层，再 spawn 玩家、`try_add` 回背包、为每个 `(slot, id)` spawn `EquippedBy + EquippedItem`。**一个 `Entity` 都不写盘**。风险两条：① `slot` 存 `u8` 必须过 `EquipSlot::ALL` 校验，否则坏档能造出非法槽位；② 一旦物品有实例态（耐久），`Vec<(u16,u32)>` 就不够，届时要加"稳定实例 id"（教程的 `SimpleMarker` 正是这个东西），所以**现在就要写 `version`**，让那次升级是迁移而不是破坏。

## I. 各项目明确写出来的坑

| 坑 | 出处 | 表现 |
|---|---|---|
| 级联删除连战利品一起删 | Bevy Book *Relations*（`linked_spawn`） | 用 `ChildOf`/`Contents` 装装备，owner 一死装备消失 |
| `Entity` 不能当持久 id | `bevy_save` README（引 Bevy `Entity` 的 stability warning） | 换版本/重载后实体引用全失效 |
| 关系类型不注册就静默错 | `bevy_save` 3.0 迁移说明 | 未注册 `Relationship`/`RelationshipTarget` 只 warning，目标列表不回填 |
| "值变了才标脏"是错觉 | Bevy Book *Change Detection* | DerefMut 即标脏，值没变也标（需 `set_if_neq`） |
| 改的人跑在查的人之后 → 一帧延迟 | 同上 | 装备生效慢一拍 |
| 每种组件要手加进存档列表 | 教程 ch11 | 漏一种 = 存档静默丢字段 |
| 一个物品能同时在两个容器 | flecs `inventory_system`（`ContainedBy` 必须注册 `Exclusive`） | 忘了注册就能"复制装备" |
| 遍历中增删关系需 defer | flecs 示例（`get_stage` + `merge`） | 迭代中改容器会读到半更新状态 |

**对我们的含义**

这些坑的共同形状是"**约定没进类型**"。本项目已经用类型解决过三次同类问题（`Position` vs `GroundItemAt`、`ChildOf` vs `ActionOf`、`linked_spawn` 语义相反 → 两个关系类型），外部来源只是把同一形状又列了一遍。建议新增一条纪律：凡是"必须由每条改变途径对称维护"的东西（存档类型表、dirty tag、槽位互斥），一律在注释里点名"**它靠纪律，不靠类型**"，并配一条枚举全量的测试——`every_item_id_resolves` 已经是这个形状，照抄即可。

## 落到本项目的四条建议

### ① 暴击/法术强度进 `Stat`（最小闭环，建议先做）

- **建议形状**：`Stat` 加 `CritRate` / `CritDamage` / `MagicMastery`（前两个是 ratio，第三个是绝对值）。`system/combat.rs:54-57` 改成
  `let crit_rate = apply_modifiers(Stat::CritRate, base_rate, &attacker_mods, &[]).clamp(0.0, 1.0);`，`crit_damage` 同理；`CritRate`/`CritDamage` 组件降级成"基础值"，与 `Attack`/`Defense` 完全同构。`MagicMastery` 接法相同，供 `max_mp_for` 使用。
- **改动面**：`rules/modifier.rs`（枚举 3 行 + `is_ratio()`）；`system/combat.rs`（1 处）；`registry.rs`（加一行"暴击之戒"，`Modifier::delta(Stat::CritRate, Equipment, 0.05)`）；`balance.rs`（基础暴击率/倍率常量）。**`apply_modifiers` 一行不动。**
- **风险**：`Stat` 是 `#[non_exhaustive]`，加变体不破坏下游；但折叠后的 `rate` 必须 clamp，否则 `rate > roll` 恒真；暴击已在消费随机数（`combat.rs:58`），加维不新增随机流、顺序不变。
- **只做一件**：只加 `CritRate` + 一行戒指 + 一条"戴上后 `CritProfile.rate` 变大"的测试。一条端到端切片就证明了"加一项可修正数值 = 3 处"，剩下的 `CritDamage`/`MagicMastery` 是复制粘贴。

### ② `Health.max` 收归一个 owner 系统（正确性，建议第二做）

- **建议形状**：新增 `recompute_vitals_system`，每轮对 actor 折叠 `Defense` / `MagicMastery`，写 `Health.max` / `Magic.max`，**写前比较**；`current` 超上限时 clamp 并记一条日志（卸甲掉上限是可感知事件，不能静默）。`Health.max` 的注释改成"cache，唯一写者 = recompute_vitals_system"。
- **改动面**：`system/` 下新增一个文件 + `system/mod.rs` 注册；`system/experience.rs:50` 的重算删掉；`components.rs:48` 注释。`balance.rs` 的公式一行不动。
- **风险**：与伤害结算的顺序（谁先跑）。用"每轮无条件重算"而不是 dirty flag，就把"漏标脏"这一类 bug 整体消掉，代价是每个 actor 每轮一次比较——写前比较让"未变"的 actor 不进变更检测，正好绕开 Bevy 文档那条"mut 必标脏"。若将来 actor 上百，再改成 ch58 的 `EquipmentChanged` tag 形状。
- **只做一件**：只做"穿脱防具后 `Health.max` 变化 + clamp"这一条，并把上限公式的输入从"基础防御组件"改成"折叠后的防御"。这是"两处真值"里最容易验证的一处。

### ③ 投掷：决策组件 + `LandingKind` 挂在物品表上

- **建议形状**：新增行动 `Throw { target }`；**校验阶段**照 `Move` 的样预留落点，留下 `ThrowTarget { landing: (usize, usize) }`；`settle.rs` 的 `ReadyDecision` 加第三个 `Option<&ThrowTarget>`，写一条 `ThrowLandEvent { from, to, item, landing: LandingKind }` 并在同一处扣减背包（`Inventory::remove` + `push_slot` 语义已有）。`LandingKind { Ground, Impact(Entity), Area { radius: u32 } }` 存在 `ItemDef` 上（`ItemDef.throw_landing: Option<ThrowSpec>`），**不**存在于行动 payload——同一种石子落在哪由物品定义决定，由行动决定就会变成"每种投掷物一个行动"。
- **改动面**：`action/validation.rs`（校验器 + 查询）；`action/settle.rs`（`ReadyDecision` + 一个分支）；`item/registry.rs`（`ItemDef.throw_landing`）；`item/mod.rs`（`LandingKind` 定义或放 registry）；`inventory.rs` 复用不加接口。
- **风险**：`settle.rs` 模块头已经警告"若新行动连落地动作都不同，是本文件该长出新形状的信号"——但它同时反对"按行动拆六个执行器"。所以新分支必须**读决策组件**、不能 `match action.kind`，否则把旧失败模式（每行动一个执行器）请回来。另一处：投掷物落地后是地面实体，必须用 `GroundItemAt` 而不是 `Position`（否则挡住玩家，第四次踩同一个坑）。
- **只做一件**：只做"石子 → 落点生成 `GroundItem`"这一条（不命中、不范围），把 landing 的三种语义留空枚举。命中/范围等有内容需求再加。

### ④ 存档投影（建议在"物品有实例态"之前做）

- **建议形状**：见 §H 的 `SaveV1` / `PlayerSave`。`equipped: Vec<(u8, u16)>` 由 `EquippedItem` 直接摊平，`bag` 由 `Inventory::iter()` 摊平；加载时重建实体。`version: u32` 从第一天就有。
- **改动面**：新增 `ecs_core/src/save.rs`（或 `persist.rs`）+ `lib.rs` 导出；`world/init.rs` 拆出一个"按 SaveV1 生成玩家"的入口，避免加载路径复制一遍初始化。依赖零新增。
- **风险**：坏档（`ItemId` 不在表里、`slot` 越界、`count==0`）必须在反序列化后**一次性校验并拒绝**，而不是让 `ItemId::def()` 在某个系统里 panic——`exists()` 已经为这个场景准备好了（`registry.rs:196`）。另一风险是格式随内容阶段频繁变；`version` + 只存稳定字段（id/数量/槽位）能把每次变更限制成一次迁移。
- **只做一件**：只存"玩家 + 装备 + 背包"，**不存地面物品**（`DsnE17` 的 `(seed, floor)` 已能重建层内容）。这一条能把"装备是实体"从"存档难题"降级成"一次 map + 一次 respawn"。

**总体顺序**：④（越晚做越贵，因为实例态一进来投影就要改）先落地形状，①（半天，解锁内容），②（等装备 UI 能显示 HP 上限时做），③（新增面最大：校验 + settle + 背包扣减 + 落地效果）。若只能做一件，做 ④ 的 `SaveV1` 骨架：它是唯一一个"现在几乎免费、以后要付迁移成本"的。

## 审校注（父 agent 复核记录）

调研稿提交后逐条抽检，三处修正/补强，都已在正文标注或此处说明：

1. **flecs `Exclusive` 的准确语义（修正 §D 的一半理由）。** 官方定义是「一个实体对某关系只能有**一个 target**，加第二个时替换第一个」（[docs.rs `Exclusive`](https://docs.rs/flecs_ecs/0.2.2/flecs_ecs/core/flecs/component_traits/struct.Exclusive.html)：*"enforces that an entity can have only a single instance of a relationship. When a second instance is added, it replaces the first instance."*）。它是**主体侧**约束：只有把"槽位"建成**关系类型**（`actor -[MainHand]-> item` / `- [Armor] ->` … 四条关系）时，"一个槽一件"才是框架不变量。我们"`item -[EquippedBy]-> owner` + `EquippedItem.slot` 字段"的形状下它**不成立**（同一 owner 的两件东西都在 `EquippedItems` 列表里，槽位只是字段）。所以 §D 的**结论方向不变**（收紧到唯一入口 + 补替换测试），但不能说"flecs 的机制在我们形状下可用"——要真借它，得先把装备改成"每个槽位一条关系"。
2. **行号抽检**：`system/combat.rs:54-57`（裸读 `CritRate`/`CritDamage`）、`item/inventory.rs:122`（`try_add`）、`item/mod.rs:205`（`try_add == 0` 判断）、`mod.rs:218`（`slot_is_taken`）、`registry.rs:188`/`:196`（`def`/`exists`）**均正确**；`components.rs:47` 应为 **48**（`pub struct Health`），`heal`/`damage` 的 `:73-81` 区间正确。
3. **补一条独立旁证**（§H 内已加）：valence 的 `Inventory` 组件与 `ItemStack` 第三参数（实例级可选数据），从另一个 `bevy_ecs` 项目印证了"背包=数据、实例态挂 `ItemStack`、定义表不存状态"这条判据。

## 未能核实的线索

- **flecs 的清理策略（`OnDeleteTarget` / cleanup policy）**：`Relationships.html` 抓取时被截断在 *Builtin relationships* 之后，未读到该节；因此本文没有断言"flecs 的 owner 死亡时装备如何处理"。想确认应读 flecs 手册的 cleanup policy 一节，或 `relationships` 示例里 `(OnDeleteTarget, Delete)` 的用法。
- **Bevy 0.16 是否有"关系互斥"属性**：只读了 Book 的 *Relations* 页与 `linked_spawn` 的说明，未在 API 文档里逐项确认 `Relationship` trait 是否有 exclusive 类属性。§D 里对"Bevy 无对应机制"的判断标了 `[推断]`。
- **`bevy_equipment` / `bevy_loot` 两个 crate**：搜索出现过同名条目，但未打开任何一页，故全文未引用；`bevy_inventory` 只读了 `Inventory` 一个类型的文档页，未读其 crate 级说明。
- **Unity DOTS / Unreal GAS 的属性聚合**：未打开任何一手页面（Unity 的 samples 仓库与 GAS 文档都没读），所以本文的属性聚合结论只来自 flecs / Bevy / Specs 三条线。
- **Minecraft 的注册表与掉落表**：按任务要求只作对比点，但本次没有打开 wiki 页面，故正文未引用；`GAME.md` 里已有的引用未复核。

## 参考来源

- flecs — game_mechanics/inventory_system: https://www.flecs.dev/flecs/examples-game_mechanics-inventory_system.html
- flecs — Designing with Flecs: https://www.flecs.dev/flecs/DesignWithFlecs.html
- flecs — Relationships: https://www.flecs.dev/flecs/Relationships.html
- flecs — relationships/exclusive_relations: https://www.flecs.dev/flecs/examples-relationships-exclusive_relations.html
- flecs — observers/monitor: https://www.flecs.dev/flecs/examples-observers-monitor.html
- flecs — prefabs/slots: https://www.flecs.dev/flecs/examples-prefabs-slots.html
- Bevy Book — Relations: https://bevy.org/learn/book/storing-data/relations/
- Bevy Book — Change Detection: https://bevy.org/learn/book/control-flow/change-detection/
- bevy_save (README): https://github.com/hankjordan/bevy_save
- bevy_inventory — `Inventory`: https://docs.rs/bevy_inventory/0.1.0/bevy_inventory/struct.Inventory.html
- Roguelike Tutorial in Rust — ch.11 Saving and Loading: https://bfnightly.bracketproductions.com/rustbook/chapter_11.html
- Roguelike Tutorial in Rust — ch.58 Item Stats and Vendors: https://bfnightly.bracketproductions.com/rustbook/chapter_58.html
