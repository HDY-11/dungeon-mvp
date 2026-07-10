> **⚠️ 修改前必须阅读或回忆 [RULE.md](RULE.md)——它定义了本文档的维护规则和更新时机。**

# 发现的问题记录

本文档记录当前实现中与设计意图不一致或有改进空间的问题，
在后续开发中可作参考。

问题按维度分组：**设计 / 架构 / 实现 / 游戏逻辑**，组内按严重程度降序。
优先级标记：🔴 高（影响正确性或游戏体验） / 🟡 中（维护性或功能缺口） / 🟢 低（整洁或边缘情况）

## ✅ 已修复

### D16 — Skills 组件下楼和存档丢失 ✅已修复

**修复前：** `descend()` query 遗漏 `&Skills`，下楼后用 `player_class.skills()` 重建空列表。`GameSave::capture()` query 同样遗漏，`restore()` 用 `pc.skills()` 重建空列表。所有已学技能在下楼和存档读档后丢失。

**修复后：**
1. `descend` query 加入 `&Skills`，重建时使用已捕获的 skills 列表
2. `components.rs`: `Skills` 加 `Clone` derive
3. `persist.rs`: `GameSave` 新增 `skills` 字段（`#[serde(default)]` 兼容旧存档），capture/restore 路径加入 Skills 序列化/反序列化（SavedSkill 中转）

**位置：** `dungeon-world/src/init.rs:258-265`、`dungeon-world/src/persist.rs:68-247`、`dungeon-core/src/components.rs:172`

### I43 — 玩家死亡无事件日志推送 ✅已修复

**修复前：** `check_death_system` 中检测到 `stats.hp <= 0` 时仅设置 `game_over = true`，未向 EventLog 推送死亡消息。

**修复后：** `check_death_system` 增加 `mut event_log: ResMut<EventLog>` 参数，死亡时 `event_log.push("你死了")`。

**位置：** `dungeon-core/src/systems.rs:16-22`

### I44 — 投掷暴击日志格式与近战攻击不一致 ✅已修复

**修复前：** 投掷暴击日志为 "石子命中老鼠！造成8伤害暴击"（暴击标记在末尾、缺"点"、缺分隔符）。

**修复后：** 改为 "石子命中了老鼠！暴击，造成8点伤害"，与近战攻击日志格式一致。

**位置：** `dungeon-action/src/execute.rs:389`

### G22 — `enqueue_if_absent` 语义可能导致操作被吞 ✅已修复

**修复前：** `handle_timed_action` 在确认后调用 `enqueue_if_absent`，按**实体**去重（同实体有任何行动在队列即拒绝入队）。玩家 Move 排队时无法再 Attack，按键无声无反应。

**修复后：** `ActionKindV3` 加 `PartialEq` derive。`enqueue_if_absent` 改为 `enqueue_or_replace`（替换语义：移除实体旧行动→添加新行动）。`handle_timed_action` 调用 `enqueue_or_replace`。

**位置：** `dungeon-action/src/types.rs:119-124`、`dungeon-action/src/player.rs:16`

### R1 — RULE.md 编辑流程优化（移除宣誓 + 强化记录优先） ✅已修复

**修复前：** RULE.md 要求编辑前"宣誓"，AI 须在每回合第一次编辑前声明流程步骤。实际效果不佳（"反智能体"），且核心问题（先修后记）未被有效约束。

**修复后：**
1. 移除"宣誓"机制
2. 新增醒目 🚨 区块：**用户报告问题 → 先记录 ISSUES → 再修复**
3. 简化编辑前检查清单，保留三条核心检查
4. 同步保存高优先级 memory（`rulemd-bug-report-flow`），确保每轮启动可见

**位置：** `RULE.md` §六

### P11 — 主循环空闲 sleep 1ms 导致有限机型 CPU 满载 ✅已修复

**修复前：** 主循环无输入时 `sleep(1ms)`，渲染一帧约 5ms，合计 6ms/帧 ≈ 166fps 空转。有限机型上单核 100% 满载，系统可能因过热/调度杀死进程。

**修复后：** 空闲 sleep 改为 32ms，渲染频率降到约 27fps。回合制终端游戏在无操作时不需要高刷新率。

**位置：** `src/main.rs:88`

### I30 — `throw.rs` 残存 `.unwrap()` 违反 I17 ✅已修复

**修复前：** `execute_throw` 中 `player.unwrap()` 是 I17 修复后引入的新 `.unwrap()`。虽然 `player.is_none() ||` 短路保护了它，但重构时脆弱。

**修复后：** 改为 `match player { None => ..., Some(p) => ... }` 彻底消除 unwrap。

**位置：** `src/throw.rs:253`

### I29 — bevy_ecs resource_mut 双重借用导致崩溃 ✅已修复

**修复前：** `open_throw_aim` 中 `resource_mut::<ThrowPreview>()` 返回 `Mut<ThrowPreview>` 后未 drop，就调用 `update_throw_path(world)`——后者内部再次 `resource_mut::<ThrowPreview>()`。bevy_ecs 内部 UnsafeCell 运行时检测到同一资源的二次可变访问并 panic。这是「按 t → 回车」后崩溃的直接根因。

**修复后：** `Muts` 作用域在调用 `update_throw_path` 前结束（`{ let mut tp = ...; }` block 提前 drop）。同时 Enter 分支中的嵌套 `get_mut` 改为三阶段顺序操作。

**教训：** bevy_ecs 的 `Mut<T>` 存活期间不得再通过任何路径调用 `world.resource_mut::<T>()`——编译器不报错（内部 UnsafeCell），但运行时 panic。

**位置：** `src/throw.rs:163-167`

### I28 — 光标穿墙显示怪物真实位置 ✅已修复

**修复前：** `build_stats_panel` 中光标位置的实体查询不检查可见性。光标移动到已探索但当前不可见格（如墙后）时，仍然显示该格的怪物名称和 HP。

**修复后：** 只有光标在**当前可见格**时才显示实体名+HP。已探索不可见格只显示地形名+"(已探索)"，未探索格显示"(未探索)"。

**位置：** `dungeon-render/src/ui.rs:280-320`

### A16 — 主手/副手系统重构 ✅已修复

**修复前：** `Equipment` 只有一个 `weapon` 槽位，无法区分主手和副手。木盾装备在 `Armor` 槽（与皮甲同槽），投掷动作没有来源位置。

**修复后：**
1. `EquipmentSlot`：`Weapon` → `MainHand`，新增 `OffHand`
2. `Equipment`：`weapon` 改为 `main_hand`，新增 `off_hand`
3. `items.json`：锈铁剑 slot → `MainHand`，木盾 slot → `OffHand`
4. `t` 键：打开投掷物选择弹窗 → `r`/`y` 切换 → 自动装副手 → 瞄准模式
5. 投掷伤害不包含主手武器攻击力加成
6. 状态栏显示主手/副手/防具/戒指 4 行装备信息

**位置：** `dungeon-core/src/items.rs`（EquipmentSlot + Equipment）、`src/throw.rs`（投掷选单+瞄准）、`src/inventory.rs`（装备/卸装）、`dungeon-render/src/ui.rs`（状态栏装备显示）

### G16 — 暴击率纳入装备加成 ✅已修复

**修复前：** `execute_attack()` 中暴击判定只用 `attacker_stats.crit_rate`（基础值 5%），`equipment_bonus()` 返回的 `StatBonus.crit_rate` 从未被使用。背包详情页显示的 crit_rate 加成不生效。

**修复后：** 计算有效暴击率时加入 `bonus.crit_rate`，`total_crit_rate = attacker_stats.crit_rate + bonus.crit_rate`，上限钳制为 1.0。

**位置：** `dungeon-action/src/execute.rs:302`

### I25 — `CanMove::condition()` 已删除 ✅已修复

**修复前：** `CanMove::condition()` 是 Action 组件中唯一有定义无调用的静态条件方法。对比 `CanChase::condition`（被 `chase_decision_system` 调用）和 `CanFlee::condition`（被 `flee_decision_system` 调用），`CanMove::condition` 处于悬空状态。

**修复后：** 删除 `CanMove::condition()` 方法。Move 的保活检查由 `check_condition` 中的 `can_move_to()` 内联处理。

**位置：** `dungeon-action/src/types.rs:61-65`

### A14 — 地面拾取逻辑统一使用 ops::pickup_ground ✅已修复

**修复前：** `inventory.rs` 的 'g' 键分支包含一份与 `ops::pickup_ground()` 几乎相同的拾取逻辑实现（查玩家位置→查同格 ItemPickup→添加 Inventory→despawn→推日志）。两处代码重复，未来变更需同步修改。

**修复后：** `inventory.rs` 的 'g' 键直接调用 `ops::pickup_ground(world)`，消除重复。

**位置：** `src/inventory.rs:260-280`

### A13 — `descend` 中 player_data 改用具名变量 ✅已修复

**修复前：** `descend()` 用一个 9 元组 `player_data` 传递玩家数据，成员通过 `.0`/`.1`/…`.8` 索引访问。索引易错、增加组件时需手动同步编号。

**修复后：** 元组解构改为 7 个具名局部变量（`player_stats`/`player_inv_stacks`/`player_equip`/`player_class`/`player_atk_name`/`player_active_buffs_vec`），赋值处按名称引用。

**位置：** `dungeon-world/src/init.rs:250-281`

### D13 — 物品 ID 提取为命名常量 ✅已修复

**修复前：** 物品 ID（0/1/2/3/10/11/12/13/14/15/16/17）在 `init.rs`（`scroll_ids` 数组和 `ground_item_ids`）、`inventory.rs`（`match item_id`）等多处以裸 `usize` 字面量出现。不可 grep、不可追踪，重构时无声错位。

**修复后：** 在 `dungeon-core/src/items.rs` 中定义 `pub const ITEM_RUSTY_SWORD = 0` 等 12 个命名常量。`init.rs` 中的 `scroll_ids` 和 `ground_item_ids` 改用常量引用。`inventory.rs` 的 'r' 键不再直接引用 ID（见 D12）。

**位置：** `dungeon-core/src/items.rs:7-18`（常量定义）

### D12 — 物品使用行为集中分派（use_item 函数） ✅已修复

**修复前：** `UsableItem` trait 和 `SkillScroll` 的 impl 已存在，但 `inventory.rs` 的 'r' 键仍然使用 `match item_id { 15 => ..., 16 => ..., 17 => ... }` 硬编码 match。trait 是悬空的抽象债务。

**修复后：** `items.rs` 新增 `pub fn use_item(item_id, world, user) -> bool` 集中分派函数，内置 match 逻辑。`inventory.rs` 的 'r' 键通过 `dungeon_core::use_item(id, world, player)` 调用。所有物品使用行为集中到 items.rs 一处管理，不再散布在 UI 层。

**位置：** `dungeon-core/src/items.rs`（use_item 函数）、`src/inventory.rs`（'r' 键调用方）

### D11 — `Buffs` 旧组件死代码已删除 ✅已修复

**修复前：** `buff_tick_system` 在 D10 中删除，但 `Buffs` 结构体（含 4 个废弃字段 `shield_turns`/`shield_def`/`berserk_turns`/`berserk_atk`）及其 impl 仍完整保留在 `components.rs` 中。`descend` 中 `Buffs::new()` 作为占位符传入，存档中的 `SavedBuffs` 仍在序列化。违反 LESSONS L39 Phase 3。

**修复后：** 删除 `Buffs` 结构体、所有 impl 块、`SavedBuffs` 序列化结构。清理 `descend` 中的 `Buffs::new()` 占位参数。清理 `persist.rs` 中的 `buffs` 字段和 `SavedBuffs` 转换。清理 `tests.rs` 中的 `Buffs` 导入和 `Buffs::new()` 调用。

**位置：** `dungeon-core/src/components.rs`、`dungeon-world/src/init.rs`、`dungeon-world/src/persist.rs`、`dungeon-action/src/tests.rs`
**教训见：** `LESSONS.md L41`

### A10 — 事件日志显示条数回归（take(5)→take(12)） ✅已修复

**修复前：** `ui.rs` 使用 `.take(5)`，战斗密集时事件日志关键信息快速滚出屏幕。G13 声称修复了但代码未改。

**修复后：** `.take(5)` → `.take(12)`。

**位置：** `dungeon-render/src/ui.rs:162`

### I38 — `lib.rs` pathfinding 注释矛盾（误导注释已清理） ✅已修复

**修复前：** 同一文件同时有生效的 `pub mod pathfinding;` 和声称"已移除"的注释。

**修复后：** 删除两条误导注释。

**位置：** `dungeon-core/src/lib.rs:7-9`

### I42 — 技能键索引与快捷键错位：已学习但按对应键无反应 ✅已修复

**问题：** 卷轴学习将技能追加到 `Skills.list` 末尾。`handle_skill` 用固定索引访问（按键 1→idx=0，按键 2→idx=1...），但技能的实际位置取决于学习顺序。例如先学护盾（快捷键 2）→ 护盾在 `list[0]`，按 2 键却查 `list[1]`→ 返回 None，技能无声失败。**这不是没学的问题，是学了但位置不对。**

**修复后：**
1. `handle_skill`（`dungeon-action/src/player.rs:68`）：改为按键索引 `0→'1'、1→'2'…`，在技能列表中按 `sk.key` 字符查找实际位置，不再假设顺序
2. 传入 action 的索引是 `real_idx`（技能在 list 中的实际位置），`execute_skill` 直接命中

**位置：** `dungeon-action/src/player.rs:68-80`

### D9 — 下楼不保存 ActiveBuffs ✅已修复

**问题：** `descend()` 中 `player_data.5` 硬编码为 `Buffs::new()`（空），且 `ActiveBuffs` 组件完全未在 `descend` 中捕获和重建。下楼后玩家身上的护盾/狂暴 Buff 全部丢失。

```rust
// init.rs:196 — 永远空的
Buffs::new(), cls.clone(), atk.0.clone())
// init.rs:207 — 下楼后插入的也是空的
cmd.insert(ActiveBuffs::new());
```

**对比：** 存档/读档（`persist.rs`）正确保存和恢复了 `ActiveBuffs`——说明下楼丢失不是有意设计，而是遗漏。

**影响：** 🟡 中 — 玩家在楼梯口开 Shield 下楼→Buff 消失，与存档读档行为不一致。

**位置：** `dungeon-world/src/init.rs:193-207`


### D10 — buff_tick_system 已删除 ✅已修复

**问题：** `buff_tick_system` 每帧修改旧 `Buffs` 组件的 `shield_turns`/`berserk_turns`/`shield_def`/`berserk_atk` 字段。但 `effective_attack`/`effective_defense` 已在 G14 修复中改为只读新 `ActiveBuffs`。旧 Buffs 的修改永远不会被消费。

```rust
// systems.rs:47-50 — 仍在运行，产生无用副作用
pub fn buff_tick_system(mut query: Query<&mut Buffs, With<Player>>) {
    for mut b in query.iter_mut() {
        if b.shield_turns > 0 { b.shield_turns -= 1; if b.shield_turns <= 0 { b.shield_def = 0; } }
        if b.berserk_turns > 0 { b.berserk_turns -= 1; if b.berserk_turns <= 0 { b.berserk_atk = 0; } }
    }
}
```

**违反 LESSONS L39：** 双系统共存应推进到 Phase 3（移除旧系统），当前停留在 Phase 1 且 `buff_tick_system` 仍在 Schedule 中注册并每帧运行。

**位置：** `dungeon-core/src/systems.rs:47-50`、`dungeon-world/src/tick.rs:13`

---

### I39 — effective_attack/defense 删除废弃 _buffs 参数 ✅已修复

**问题：** `effective_attack` 和 `effective_defense` 带有 `_buffs: Option<&Buffs>` 参数，前缀下划线表示"不使用"。G14 修复时移除了求和逻辑但保留了参数占位，所有调用方仍在传入 `world.get::<Buffs>(entity)` 做无用查询。

```rust
pub fn effective_attack(
    stats: &Stats, inv: &Inventory, equip: &Equipment,
    _buffs: Option<&Buffs>,           // ← 废弃参数，从不使用
    active_buffs: Option<&ActiveBuffs>,
) -> u32
```

**违反 LESSONS L39：** 新旧系统共存应推进到 Phase 3（移除旧系统引用），当前停留在 Phase 1 未进展。

**位置：** `dungeon-core/src/ops.rs:24-50`；调用点：`execute.rs:293`、`ui.rs:134`

### I40 — 物品系统行为抽象层（UsableItem trait 已定义） ✅已修复

**问题：** MC 的物品系统有三层：`Registry → ItemStack → Item 虚方法`。本项目的物品只有前两层——`ItemDef` 是纯数据结构，没有任何方法。物品"能做什么"的逻辑必须散落在外部 match 中：

```rust
// 没有统一的 use() 抽象，只能 match item_id
match item_id {
    20 => learn_skill(world, SkillKind::Heal),
    21 => learn_skill(world, SkillKind::Shield),
    // 加一种可消耗品 → 加一个 arm
    _ => {},
}
```

**MC 的做法（Item 虚方法）：**
```java
public class Item {
    public InteractionResult use(Level level, Player player, InteractionHand hand) { ... }
    public void inventoryTick(ItemStack stack, Level level, Entity entity, int slot, boolean selected) { ... }
}
```

每件物品通过继承/impl 定义自己的行为，调用方只需 `item.use(...)`——不需要 match。

**影响：** 当前仅装备类有行为（加 stat），材料类完全无用途。即将做的技能卷轴、未来的药水/食物/卷轴都需要行为抽象。没有的话每加一种可交互物品都要改 inventory.rs 和/或 process_key。

**建议方向：** 定义 `UsableItem` trait（与 A12 的 `MonsterBehavior` 同一类问题——用虚表代替枚举 match）：

```rust
pub trait UsableItem {
    fn use_on(&self, world: &mut World, user: Entity) -> bool;
    fn use_verb(&self) -> &'static str;
    fn can_use(&self, world: &World, user: Entity) -> bool;
}
```

**位置：** `dungeon-core/src/items.rs`（ItemDef 定义处，无方法）

### I41 — ItemStack 增加 ItemMeta（NBT 等价物） ✅已修复

**问题：** `ItemStack` 只有 `(item_id, count)` 两个字段，没有存储任意元数据的容器。MC 的 `CompoundTag`（NBT）支持自定义名称、附魔、耐久度、品质/层级等任意键值对。

```rust
// 当前 ItemStack — 无扩展空间
pub struct ItemStack {
    pub item_id: usize,
    pub count: u32,
    // 没有第 3 个字段
}
```

**影响：** 🔴 高 — 以下功能在没有 NBT 等价物的情况下要么不可能，要么需要绕路：
| 功能 | 无 NBT 的代价 |
|------|-------------|
| 装备层级（+1/+2/+3） | 加字段到 ItemStack 或另加 ECS 组件 |
| 附魔/自定义属性 | 需要新组件 + 查询链 |
| 自定义名称 | 不可能——永远是模板名称 |
| 耐久度 | 需要新组件 + 存档迁移 |
| 词缀（前缀/后缀） | 不可能——无法存"锋利的长剑"vs"迅捷的长剑" |

**建议方向：** 在 `ItemStack` 中加一个通用元数据容器，`#[serde(default)]` 兼容旧存档：

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ItemMeta {
    pub name: Option<String>,
    pub tier: u32,
    pub enchantments: Vec<Enchantment>,
    pub durability: Option<u32>,
    pub tags: Vec<String>,
}

pub struct ItemStack {
    pub item_id: usize,
    pub count: u32,
    #[serde(default)]
    pub meta: Option<Box<ItemMeta>>,
}
```


### I35 — 怪物颜色统一使用 Renderable 组件（地图+行动轴） ✅已修复

**修复前：** `timeline.rs` 和 `ui.rs` 使用 `entity_color(entity.to_bits(), 0)` 实时哈希计算怪物颜色。读档后 Entity ID 重建导致颜色不一致。更根本的问题是：独特色是"渲染时实时计算的"，不被持久化。

**修复后：**
1. `entity_color` + `hsv_to_rgb` 从 `dungeon-render` 移至 `dungeon-core/src/color.rs`（纯数学，无 TUI 依赖）
2. `spawn_monsters` 在 spawn 后立即用 `entity_color(entity.to_bits(), 0)` 写入 `Renderable.color`——独特色在生成时固定
3. `timeline.rs` 改为直接读取 `Renderable.color`，不再实时哈希
4. `ui.rs` 移除怪物 entity_color 覆写，直接使用 renderable 的已存颜色

**效果：** 独特色在存档中持久化（SavedMonster 的 r/g/b 字段），读档/下楼后地图和行动轴颜色一致。

**位置：** `dungeon-core/src/color.rs`（新模块）、`dungeon-world/src/init.rs:67`、`dungeon-render/src/timeline.rs:41`、`dungeon-render/src/ui.rs:126`
**教训见：** `LESSONS.md L40`（新增——独特色应在生成时固定存储于组件，而非渲染时实时计算）

### A11 — 删除 Stats::monster() 死代码 ✅已修复

**修复前：** `components.rs` 中 `Stats::monster(glyph, floor)` 无调用方，缺少蝎子匹配，功能完全重复于 `monster_def::monster_stats()`。

**修复后：** 删除整个方法（~30 行死代码）。

**位置：** `dungeon-core/src/components.rs:120-156`

### A4La — Map 残留 generate_water / is_away_from_rooms / count_walkable_neighbors 死方法 ✅已修复

**修复前：** A4/A4L 后 Map impl 仍有三个零调用的方法。同模式第三次发生。

**修复后：** 删除三个方法。Map impl 仅保留 `count_tile` / `count_neighbor_tile` / `carve_corridor` / `render` / `spawn_point`。

**位置：** `dungeon-core/src/lib.rs`
**教训见：** `LESSONS.md L38`

### I33 — 丢弃物品产生地面拾取物 ✅已修复

**修复前：** 背包详情页按 `d` 直接 `inv.drop_stack(idx)` 删除物品栈，物品永久消失。丢弃是唯一不可逆的物品销毁路径。

**修复后：** 丢弃时获取玩家位置，在地面 spawn ItemPickup 实体（含 glyph/color）。事件日志显示"丢弃了xxx在脚下"。

**位置：** `src/inventory.rs:268-280`

### I34 — ActiveBuffs 未加入存档 ✅已修复

**修复前：** `GameSave` 仅保存旧 `Buffs`，玩家在 Buff 持续期间存档后，读档后 Buff 丢失。

**修复后：** `GameSave` 新增 `active_buffs: Vec<SavedActiveBuff>` 字段（`#[serde(default)]` 兼容旧存档），capture 时序列化玩家 ActiveBuffs，restore 时重建 Buff 列表。

**位置：** `dungeon-world/src/persist.rs`

### I32 — SkillKind::duration 单位歧义（回合/秒） ✅已修复

**修复前：** `duration: i32` 可负值；旧 Buffs 系统读作 3 帧 ≈ 50ms，新 ActiveBuffs 读作 3 秒；技能描述写"持续3回合"。

**修复后：** `duration` 改为 `u32`（禁止负值）；技能描述统一为"持续3秒"。

**位置：** `dungeon-core/src/components.rs:160-164`

### G14 — 护盾/狂暴技能双倍叠加（执行层移除旧系统写入） ✅已修复

**修复前：** `execute_skill` 同时写入旧 `Buffs` 和新 `ActiveBuffs`，`effective_attack`/`effective_defense` 对两者求和。每次使用 Shield/Berserk 时护盾/狂暴数值在 ~3 帧内翻倍（+10 而非 +5）。

**修复后：** `execute_skill` 移除了旧 `Buffs` 写入路径，`effective_attack`/`effective_defense` 只读新 `ActiveBuffs`（旧 Buffs 参数保留但不再参与计算）。使用技能护盾/狂暴正确只加 +5。

**位置：** `dungeon-action/src/execute.rs:315-340`、`dungeon-core/src/ops.rs:24-50`
**教训见：** `LESSONS.md L39`

### I27 — 怪物颜色可区分性差（修复三次） ✅已修复

**第一次修复：** `unique_color` 取 `entity.to_bits()` 低 6 位偏移 ±32，相邻 ID 色差 <1。

**第二次修复：** 黄金比例 `wrapping_mul` 扩散 + 范围采样 ±64。但极端基准色（老鼠 `255,0,0`）的通道被 clamp 吞噬，仍无差异。

**第三次修复：** 废弃基准色方案。改用 `SipHash(entity_bits ⊕ seed)` 的高位直接映射 RGB，无基准色限制，微小 ID 变化经 hash 后产生大幅颜色跳跃。

**位置：** `dungeon-render/src/color.rs:12-20`

### I29 — 泛型 Buff 系统（ActiveBuffs + AV 推进） ✅已修复

**修复前：** Buff 使用回合计数（`shield_turns: i32`），与 AV 时间轴脱钩。每帧减 1 回合，不同帧消耗速度不同。技能只能通过职业锁定。

**修复后：** 新增 `ActiveBuffs(Vec<Buff>)` 和 `ActiveCooldowns(Vec<Cooldown>)` 泛型组件，`advance_action_queue` 中与队列同步推进（`remaining_av -= dist`）。`effective_attack/defense` 查询 ActiveBuffs。旧 `Buffs` 组件保留过渡期兼容。

**位置：** `dungeon-core/src/components.rs`、`dungeon-action/src/execute.rs`、`dungeon-core/src/ops.rs`

### I30 — UI 整合：Buff/视野/HP 标注移至行动轴 ✅已修复

**修复前：** Buff 显示在 stats 面板（文本行），视野实体显示在 stats 面板底部，行动轴只显示行动名和倒计时。信息分散。

**修复后：** 行动轴整合为三区：①队列条目（符号+行动+耗时）②分割线③实体状态（符号+怪物名+血量）④次级标注（Buff，dim 样式）。stats 面板移除 Buff 和视野段。

**位置：** `dungeon-render/src/timeline.rs`、`dungeon-render/src/ui.rs`

### I31 — x 键光标查看模式 ✅已修复

**修复前：** 无查看模式，玩家无法了解地图上未知位置的详细信息。

**修复后：** 新增 `LookCursor` 资源 + `open_look_mode`（方向键移动、x/Esc 退出）。地图上光标格叠加暗黄色背景高亮。stats 面板底部显示光标位置的地形名和实体名+HP。

**位置：** `dungeon-core/src/resources.rs`、`src/main.rs`、`dungeon-render/src/ui.rs`

### I28 — 事件日志从 stats 面板移至地图下方 ✅已修复

**修复前：** 事件日志位于右侧 stats 面板底部，占用了属性显示空间且不易阅读。

**修复后：** 地图区增加垂直分割，地图占 `VIEWPORT_HEIGHT`，下方独立显示事件日志（`── 事件 ──` 分隔线，最近 5 条）。

**位置：** `dungeon-render/src/ui.rs`

### G13 — 玩家面板显示不应公开的调试信息（房间数/怪物数） ✅已修复

**修复前：** 属性面板中显示 `房间 N` 和 `怪物 N`，这些是地图生成和种群统计的调试数据，玩家不应看到。行动轴宽度 22 偏高，压缩了地图和属性区的可用空间。事件日志仅显示 5 条，战斗密集时关键信息快速滚出屏幕。

**修复后：** 删除房间/怪物数量行。行动轴收窄至 16，释放水平空间。事件日志增至 12 条。

**位置：** `dungeon-render/src/ui.rs`

### G11 — `rooms[0].center()` 不可行走导致出生卡墙 ✅已修复

**修复前：** `spawn_point()` 直接返回 `rooms[0].center()`，不做 walkable 校验。`generate_stalactites` 在房间内每格 7% 概率将 Floor 变 Stalactite，可能覆盖房间中心点；`ensure_spawn_accessible` 只检查邻居不检查中心自身。下楼后玩家可能在不可行走格上出生，无法移动。

**修复后：** `spawn_point()` 先检查中心是否 walkable，若否则以螺旋搜索（半径 1→20）寻找最近的可行走格。确保返回值永远可通行。

**位置：** `dungeon-core/src/lib.rs:421-442`
**触发条件：** `generate_stalactites` 在 room[0] 每格 7% 概率 → 约每 14 次下楼触发一次。

### A7 — 拆分 ops.rs 为 fov / pathfinding / ops ✅已修复

**修复前：** `ops.rs` 是万能工具袋——FOV、A\*、公式、查询、拾取、碰撞图、渲染收集等 9 个无关功能挤在同一个文件中。

**修复后：** 提取 `dungeon-core/src/fov.rs`（`calculate_visible_tiles`）和 `dungeon-core/src/pathfinding.rs`（`astar` + `AStarNode`）。ops.rs 保留剩余的紧密相关工具函数（公式、属性计算、实体查询、拾取、碰撞图、视野记忆、渲染收集）。

**统计：**
| 文件 | 行数 | 职责 |
|------|------|------|
| `fov.rs` | ~25 | 对称阴影投射视野计算 |
| `pathfinding.rs` | ~80 | A\* 8 方向寻路 |
| `ops.rs`（剩余） | ~120 | 公式/查询/记忆/碰撞/渲染 |

### I17 — 全部 `.unwrap()` 替换为 `.expect()` ✅已修复

**状态：** 全部 ~35 处 `.unwrap()` 已替换。生产代码零 unwrap。

### I10 — 斜向键无 OS key-repeat（Won't Fix — 终端环境限制） ✅已修复

**问题：** 按住 Home/End/PgUp/PgDn 不放，角色不会连续斜向移动。多数终端不发斜向键的 OS key-repeat 事件。

**结论：** 终端环境引起，不在项目控制范围内。

### G8 — 水体生成保护距离调整（6→3） ✅已修复

**修复前：** `is_away_from_rooms(x, y, 6)` 保护距离 6，对半径 4-6 的房间偏大，水体几乎不出现。

**修复后：** 保护距离改为 3。视窗内可见 ~9-19 格水体（约 1-2% 地图面积），以水洼和窄溪流形式分布在通道边缘和房间过渡带，不淹没房间内部。

**评估：** 当前密度适合洞穴环境，也为未来的水体减速/加速 Buff 预留了触发空间——每层自然涉水 3-5 次，有存在感但不泛滥。

### A9 — 渲染层直接查询 ECS（Deferred — 条件触发时重新评估） ✅已修复

**当前评估：** 不做 ViewData 重构。理由：
- 当前 render 的 ~8 处 `try_query().expect()` 在 I17 后已有足够信息量
- 组件重命名会触发编译错误（编译期隔离足够）
- ViewData 方案会新增 ~50% 代码量并增加每帧填充开销

**触发条件：** 以下任意一条满足时重新评估：
1. render 中 `try_query` 模式超过 **15 种**（从当前 ~8 增长）
2. **同一组件重组导致 render 连续两次以上需要修改**时

### D4 — 升级满血满蓝已文档化（有意设计） ✅已修复

**修复前：** `apply_exp_system` 中升级后 HP/MP 全恢复，但 GAME.md 和 DESIGN.md 均未记录。属于"有意但未说明"的行为，新开发者看到会困惑。

**修复后：** GAME.md 升级效果中增加 `HP/MP 全恢复（设计简化，方便体验不同楼层）` 行，并注明参见 D4。

### A6 — 行动类型从 dungeon-core 移至 dungeon-action ✅已修复

**修复前：** `dungeon-core/src/action_types.rs` 包含 `ActionQueue`、`ActionKindV3`、`CanMove`/`Chase`/`Flee`等行动领域类型。它们被放在 core 中只因依赖方向限制，导致 core 被行动系统的改动拖慢。

**修复后：** 整个 `action_types.rs` 迁移到 `dungeon-action/src/types.rs`。所有引用路径更新：
- `dungeon-action` 各模块：`crate::types::*`
- `dungeon-world`：`dungeon_action::*`
- `dungeon-render`：新增依赖 `dungeon-action`
- `dungeon-core`：删除 `pub mod action_types`，测试迁至 `dungeon-action`

**删除文件：** `dungeon-core/src/action_types.rs`、`dungeon-core/src/tests.rs`

### I19 — 提取 setup_world/descend 共享函数 + 修复 G9/G10/I16 ✅已修复

**修复内容（四项在同一个重构中完成）：**

**I16 — 单房间物品为 0**：`place_ground_items` 当 `rooms.len() == 1` 时退回到 `rooms[0]` 内随机偏移放置。

**G10 — 怪物阻挡关键位置**：`generate_monster_population` 新增 `exclude: &[(usize, usize)]` 参数，收集和随机补充阶段跳过排除坐标。`setup_world` 和 `descend` 传入 `[spawn, stairs_pos]`。

**I19 — 重复代码**：提取 `spawn_monsters`、`place_ground_items`、`pick_stair_pos` 三个共享函数，`setup_world` 和 `descend` 分别调用。消除 ~55 行重复代码。

**教训：** 三个不同的问题（重合、阻挡、缺物品）共享同一根因（单房间退化）和同一修复点（init.rs）。将其一次性解决比分开修更高效。共享函数提取应在修复的同时进行，而非先提取再修复——否则两次修改同一区域。

### I18 — `on_stairs()` 修复：过滤 Player 组件 ✅已修复

**修复前：** `try_query::<&Position>()` 查询任意实体位置，迭代顺序不确定性导致可能读到怪物/物品的位置而非玩家，使下楼判定失效。

**修复后：** 改为 `try_query::<(&Player, &Position)>()`，只查询玩家的位置。无玩家时返回 false。

**教训：** 任何"判断玩家状态"的函数都应在查询组件时显式加入 Player filter。`&Position` 可能匹配到任何实体——编译器不会警告，行为在运行时才暴露。

### I20 — 移除 `advance_and_settle_parallel` 末位重复 rebuild ✅已修复

**修复前：** `advance_until_player_acted`（内部每 action 后 rebuild）→ schedule.run → `rebuild_occupancy`（末位）。调度器不改变实体位置，末位 rebuild 冗余。

**修复后：** 删除末位 `rebuild_occupancy` 调用。碰撞图仅由 `advance_action_queue` 在每 action 后维护，职责清晰。

### D7 — EventLog 容量提升至 50 ✅已修复

**修复前：** max=10，战斗密集时关键信息 2-3 回合后被滚出屏幕。

**修复后：** max=50。

### I21 — Position 增加 `#[derive(PartialEq, Eq)]` ✅已修复

**修复前：** `Position` 无 PartialEq，测试中需逐字段比较 x 和 y。

**修复后：** 增加 `#[derive(PartialEq, Eq)]`。测试代码可直接 `assert_eq!(pos1, pos2)`。

**教训：** 值类型（所有字段都是 Copy 的简单结构体）应默认实现 PartialEq + Eq，无需等待测试需要时才加。

### D6 — GAME.md 升级描述与代码一致 ✅已修复

**修复前：** GAME.md 仍写着"获得 3 个属性点（待分配）"，但 PendingLevelUp 已在 I7 中删除。

**修复后：** 该行标记为 `~~已移除~~`，并注明参见 I7。GAME.md 的"升级效果"描述与 `apply_exp_system` 的实际行为一致。

**教训：** 代码与设计文档之间没有自动同步机制。每次删除游戏机制（如 I7）后应在 GAME.md 中搜索相关文字。ISSUES.md 的已修复列表应包含文档更新。

### I14 — 下楼时 PlayerClass 与 Skills 联动保障 ✅已修复

**修复前：** `descend()` 中 Skills 通过 `player_data.6`（`sk.list.clone()`）持有独立副本，与 `PlayerClass` 字段无编译期联动。如果将来添加职业特有技能，两个字段可能 drift。

**修复后：** Skills 改为从 `PlayerClass::skills()` 推导，与 `setup_world` 和 `restore` 一致。同步清理了不再需要的 `Skills` 组件查询和旧 Position 字段。

**教训：** 派生数据不应手动复制，应从权威源推导。`descend()` 中有三个不同路径（setup_world / restore / descend）重建玩家，它们生成 Skills 的方式应统一。

### I13 — Tile 序列化合约由类型管理 ✅已修复

**修复前：** `map_tiles` 用 `tile as u8` 保存、`if v == 0 { Wall } else { Floor }` 恢复。判别值隐式依赖编译器分配，且丢失了 ShallowWater/DeepWater/Stalactite。

**修复后：** 由 I15 的 Tile 自定义 Serde 统一解决——序列化合约归类型自身管理，调用方只需 push/pull Tile。`Vec<Tile>` 与旧版 `Vec<u8>` 二进制格式一致，无须迁移旧存档。

### I12 — 主循环 F9 读档后刷新视野记忆和碰撞图 ✅已修复

**修复前：** `process_key` 中 F9 读档后（`title_screen` 中做了但这里遗漏了）不跑 `fov_system`、`update_map_memory`、`update_visible_memory`、`rebuild_occupancy`，导致读档后第一帧黑屏/灰色空地图、怪物和碰撞图不可用。

**修复后：** F9 读档后立即执行完整的刷新链，与 `title_screen` 的读档逻辑一致。`process_key` 和 `title_screen` 之间不再有隐藏的不一致。

**教训：** 同一功能的跨入口实现（title_screen vs process_key 的 F9）应提取为公共方法，或至少确保双方逻辑一致。"一个地方修了、另一个没修"是重复代码的经典隐患。

### P8 — 测试覆盖不全 ✅已修复

**修复前：** `dungeon-action` 和 `dungeon-world` 零测试。仅 `dungeon-core` 有 6 个单元测试 + 3 个场景集成测试。

**修复后：**
| crate | 前 | 后 | 新增内容 |
|-------|-----|-----|---------|
| dungeon-core | 6 | 6 | 不变 |
| dungeon-action | 0 | 6 | 队列推进/等待/保活检查/tap-tap 方向/tap-tap 等待/攻击流程 |
| dungeon-world | 0 | 2 | 存档读档回环（Tile+Stats+Inventory+Equipment）、下楼数据保持 |
| 场景测试 | 3 | 3 | 不变 |
| **总计** | **9** | **17** | |

**教训：** 测试编写中的两个关键发现：
1. `rooms[0].center()` 可能返回非 walkable 格（矩形 bounding box 的墙点）——这是生成流程中一个隐藏的脆弱点，测试迫使它暴露
2. `world.get_mut()` 不能同时借两个不同组件——须分两步操作（取物品 → 再装备），这和主流程中 `descend` 的做法一致

### A3 — action/world tick 边界清理 ✅已修复

**修复前：** `dungeon-action/src/tick.rs` 的串行 `advance_and_settle()` 与 `dungeon-world/src/tick.rs` 的并行版功能重复。串行版从未被调用（`main.rs` 使用并行版，`scenario_test.rs` 也使用并行版），属于死代码。

**修复后：** 
- 删除 `dungeon-action/src/tick.rs` 中的 `advance_and_settle()`（action 只保留 `advance_until_player_acted`）
- 删除 `dungeon-world/src/tick.rs` 中的 `advance_and_settle_serial()`（world 只暴露并行版）
- 更新两个 crate 的 `lib.rs` 导出

职责边界：action 负责"队列推进和执行"，world 负责"编排和状态同步"。

### A4 — 环境修饰从 Map impl 提取到独立模块 ✅已修复

**修复前：** `generate_water`、`carve_expand`、`generate_stalactites`、`ensure_connectivity`、`ensure_spawn_accessible`、`ensure_connection_between`、`has_path_between`、`collect_walkable_regions`、`is_away_from_rooms`、`detect_cave_regions` 等 ~450 行代码全部在 `Map` 的 `impl` 块中。Map 职责膨胀——既要容纳 tile 数据还要管理完整的生成管线。

**修复后：** 新建 `dungeon-core/src/map_gen.rs` 模块，将上述方法全部移入作为自由函数（如 `map_gen::generate_water(map, ...)`）。Map 只保留 `generate()` 入口 + 基础查询方法（`count_tile`、`count_walkable_neighbors`、`count_neighbor_tile`、`carve_corridor`、`render`）。

**统计：**
| Map impl | 前 | 后 |
|----------|-----|-----|
| 方法数 | ~18 | ~7 |
| 行数 | ~600 | ~160 |

**教训：** 序列化合约和生成管线都应从核心类型中分离——Serialize/Deserialize 归 Tile、生成管线归 map_gen、基本查询留 Map。

### A5 — global.rs 空壳模块 ✅已修复

**修复前：** `dungeon-core/src/global.rs` 仅含两行注释（"全局 World 不再使用 OnceLock"、"线程局部 RNG 已移除"），无任何代码。`lib.rs` 仍 `pub mod global;`，全局无引用。

**修复后：** 删除 `pub mod global;` 行 + 删除 `global.rs` 文件。注释内容已在 DESIGN.md 和 LESSONS.md 中有足够记录。

**教训：** 代码移除后公共模块声明也应同步清理。P6 曾清理了 action.rs，但 global.rs 被遗忘——每次移除整个模块后都应 grep `pub mod` 确认。

### I15 — 存档 Tile 精度丢失（自定义 Serde 长期方案）✅已修复

**修复前：** `GameSave::capture` 用 `tile as u8` 保存 Tile，`restore` 用 `if v == 0 { Wall } else { Floor }` 恢复。Tile 的 5 种变体（Wall/Floor/ShallowWater/DeepWater/Stalactite）中 `1~4` 全部映射为 Floor，读档后全部水体+钟乳石消失。

**修复后：** Tile 实现自定义 `Serialize`/`Deserialize`，以 u8 判别值序列化（保持与旧版 `Vec<u8>` 相同的二进制格式），restore 直接读取 Tile 值，不再丢失精度。新增变体须在末尾追加。

**教训（L27）：** 自定义 Serde 实现使类型的序列化合约由类型本身管理，而非分散在 capture/restore 两处。同时保持与旧存档的二进制兼容——所有用 `as u8` 序列化 enum 的地方都应改用此模式，避免判别值隐式依赖编译器分配。

---

### P1 — 保活检查只检查即将执行的条目 ✅已修复

队列推进时对所有条目做批量保活检查，不满足的立即剔除，防止 Chase/Flee 在等待期间条件已失效却仍留在队列中白耗 AV。

### P3 — 并行 Schedule 每帧重建（Won't Fix） ✅已修复

每帧构建开销 <1μs，且保持测试跨 World 兼容，保留现状。

### P6 — action.rs 是空壳模块 ✅已修复

删除 action.rs，所有引用统一到 action_types。

### P9 — VisibleMemory 在视野边缘闪烁 ✅已修复

加入 VISIBLE_FORGET_DELAY=3 遗忘延迟，避免实体在视野边缘来回移动时闪烁。

### P10 — 存档缺少对 ActionQueue 的序列化 ✅已修复

按位置映射保存/恢复队列条目，Attack 条目因 Entity 引用跳过。

### D1 — 三套 RNG 并存，游戏不可复现 ✅已修复

`GameRng` 成为唯一随机源：新增便捷方法，`LootTable::roll()` 改为接受 `&mut impl Rng`，暴击/游荡/仲裁全部走 `GameRng`，删除线程局部 RNG，种子从硬编码 `0` 改为 `map_seed.wrapping_add(42)`。

### D2 — 存档/读档丢弃 Intent 缓冲区状态 ✅已修复

`GameSave` 新增 `chase_intents` / `flee_intents` / `wander_intents` 字段，capture 按位置保存，restore 通过 position→entity 重映射恢复，`#[serde(default)]` 兼容旧存档。

### D3 — crate 依赖链文档与实际不符 ✅已修复

修正 README.md 中 crate 划分树和依赖链描述，移除冗余的重复树结构。

### A1 — dungeon-core 与 dungeon-world 大量代码重复 ✅已修复

以 core 的 systems 为 canon：`calculate_visible_tiles` 移入 ops.rs，删除 core 的 api.rs（`setup_world` 移入 tests.rs），删除 world 的 systems.rs，world 的 tick 改引用 core 的 systems。

### I1 — 对角穿墙角不对称：玩家可穿，怪物不可穿 ✅已修复

移除 A\* 中的对角穿墙角检查，玩家和怪物行为一致（均可穿墙角）。

### I2 — 逃跑无退出条件（触发后永远逃跑） ✅已修复

引入滞回区间：`CanFlee::condition`（决策进入）保持 HP < 25%，`check_condition`（保活退出）改为 HP < 30%。

### I3 — 火球技能击杀无经验/无掉落，且会伤害玩家自身 ✅已修复

删除整个 Firebolt 技能条目和相关代码，法师职业改为护盾+狂暴。

### I4 — 装备卸载回滚不完整 ✅已修复

`Inventory` 新增 `can_add()` 预检方法，装备卸载前先检查背包容量，有空间再执行，避免部分添加后无法回滚。

### I5 — 怪物游荡使用确定性方向而非随机 ✅已修复

从 `(FloorNumber + monster_count) % 8` 改为 `rand::random::<u8>() % 8`，每个怪物独立随机方向。

### I6 — apply_exp_system 在每个 ready 条目后调用（Won't Fix） ✅已修复

该函数有 early return（`pending.amount == 0`），非击杀条目开销 <1μs。事件帧模式下每个条目后调用反而是正确行为（即时反馈经验变化）。

### I7 — PendingLevelUp 悬空 ✅已修复

删除整个 PendingLevelUp 机制，升级时不再累积属性点数，只提升等级和 HP/MP。

### I8 — 怪物生成数量固定 12 只 ✅已修复

怪物生成尝试次数从固定 `12` 改为 `room_centers.len()`，地面物品数量改为 `room_centers.len().min(8)`，随可用房间数自动变化。

### I9 — 废弃注释和空白行 ✅已修复

删除 core/systems.rs 中的 `// use crate::world; // 已移除` 注释和多余空行。

### I11 — 渲染层在已探索暗处直接渲染实体实时位置（X 射线透视） ✅已修复

渲染层遍历 renderables 时，删除 `else if explored[ey][ex]` 灰色渲染分支。暗处实体不再直接画出实时位置，改由 `visible_mem` 循环在已探索区域显示上次看到的位置。

### G2/G3 — 死后游戏仍推进 ✅已修复

死后跳过 `advance_and_settle`，q 键直接退出（跳过确认弹窗）。

### G7 — 楼梯不可达 ✅已修复

`Map` 新增 `ensure_connection_between()`：BFS 检查从出生点到楼梯是否有 walkable 路径，若无则用加权醉汉游走（70% 概率指向楼梯方向，30% 随机）挖掘通道。在 `setup_world` 和 `descend` 中楼梯放置后调用。

### A2 — 背包 UI 250+ 行在 main.rs ✅已修复

将 `InvPanel`/`DetailSource`/`Page` 枚举、`collect_ground_items_in`、`open_inventory` 整体提取到独立模块 `src/inventory.rs`。`lib.rs` 添加 `pub mod inventory`，main.rs 通过 `dungeon_tui::inventory::open_inventory` 调用。

---

### A4L — A4 重构遗漏：Map impl 残留两套重复方法 ✅已修复

**修复前：** A4 将 `collect_walkable_regions` 和 `detect_cave_regions` 复制到 `map_gen.rs` 作为自由函数，但原 impl 方法**未删除**。两套代码完全一致。A4 的统计表显示 Map impl 方法数从 ~18 降到 ~7，但实际应为 ~5。

**修复后：** 两个 impl 方法已删除。所有调用方已走 `map_gen.rs` 自由函数版本。

**教训：** 重构跨文件移动方法后应检查原位置是否仍有残余。

### I26 — arbitration_system 排序比较器违反全序契约 ✅已修复

**修复前：** `arbitration_system` 中同 priority 的实体用 `random_range()` 做 tiebreaker，每次比较产生新随机值，违反 `sort_by` 的全序契约。标准库排序算法在检测到不一致比较时会 panic。下楼至第 3 层时固定触发。

**修复后：** 移除随机 tiebreaker。仲裁器只关心**同实体**的优先级排序（同实体高优先级先入队，低优先级被 `has_entity` 过滤），跨实体同优先级的顺序无意义。直接用 `pb.cmp(pa)` 降序，稳定排序保留插入顺序即可。

**教训：** `sort_by` 的比较器必须是全序（total order）——`a < b` 和 `b < a` 不能同时成立。混入随机数的比较器看似聪明，实际是未定义行为，标准库可能在任意数据分布下 panic。

**位置：** `dungeon-action/src/monster.rs:67-70`

### G9 — 玩家与楼梯重合 ✅已修复（三次）

**修复前：** `pick_stair_pos` 用 `farthest_room_from(spawn)` 取得离出生点最远房间的中心作为楼梯位置。单房间时返回自身。

**第一次修复（I19）：** 尾部加入醉汉游走，检测 rooms.len() ≤ 1。《实际上醉汉游走在 `farthest_room_from` 之后，而该方法对任意非空 rooms 都返回 `Some`，醉汉游走是死代码。》

**第二次修复（G14）：** 增加 `map.rooms.len() > 1` 守卫使醉汉游走可达。但 60 步失败后的兜底 `(spx, spy)`——即出生点本身，仍未解决。

**第三次修复（G14 续）：** 兜底改为螺旋搜索半径 15~40 的最近可行走格，保证不返回出生点。

**教训：** 两条逻辑路径（正常 + 退化）都要确认退化路径的兜底本身是否有 bug。

### G12 — 渲染层叠顺序未定义：怪物与掉落物在同一格时谁在上层不确定 ✅已修复

**修复前：** `collect_renderables` 查询所有 `(Position, Renderable)` 实体并按 ECS 迭代顺序返回，仅对玩家 `@` 做了特殊排序（放最后）。怪物、物品、楼梯在同一格时，哪一层渲染在上方由迭代顺序决定，不可预测。怪物站在物品上时可能被物品盖住。

**修复后：** 收集时增加 Entity 查询，在排序阶段区分实体类型。图层优先级：物品/楼梯 (0) → 怪物 (1) → 玩家 (2)。同层保持原迭代顺序。

**位置：** `dungeon-core/src/ops.rs:150-163`

### A18 — 存档未保存副手投掷物 (off_hand) ✅已修复

**修复前：** `GameSave` 保存了主手、防具、戒指，但从未保存副手字段。restore 中硬编码 `off_hand: None`，存档后副手石子永久丢失。

**修复后：** `GameSave` 新增 `off_hand_item_id`、`off_hand_count`（`#[serde(default)]` 兼容旧存档），capture 时序列化副手栈，restore 时恢复。投掷物存档后不再丢失。

**位置：** `dungeon-world/src/persist.rs`

### D14 — 投掷不经过 AV 行动系统 ✅已修复

**修复前：** 投掷是唯一绕过 `ActionQueue` AV 行动系统的玩家行动。`process_key` 中 `t` 键返回硬编码 `Ok(false)`，主循环据此跳过 `advance_and_settle`。投掷后世界时间静止。

**修复后：** `ActionKindV3::Throw{tx,ty}` 新增枚举变体，`execute_throw` 下沉至 `dungeon-action/execute.rs` 并复用 `equipment_bonus`。瞄准确认后 `enqueue` AV 行动计算耗时 190ms，返回 `true` 触发世界推进。投掷与移动/攻击/技能走同一生命周期。

**位置：** `dungeon-action/src/types.rs`、`dungeon-action/src/execute.rs`、`src/main.rs`、`src/throw.rs`

### I43 — `line_bresenham` 零长度路径导致无限循环崩溃 ✅已修复

**修复前：** `line_bresenham(x0,y0, x1,y1)` 在 `x0==x1 && y0==y1`（起点等于终点）时，方向推导 `sx = if x0 < x1 { 1 } else { -1 }` 和 `sy = if y0 < y1 { 1 } else { -1 }` 因 `x0<x1` 为假而得到 `sx = -1, sy = -1`，每一步朝远离目标的方向走，永不终止。坐标递减至负值后 `as usize` 回绕到 `usize::MAX`，在后续 `map.tiles[py][px]` 越界 panic。

**触发场景：** 进入投掷瞄准模式时，光标初始化为玩家位置。`update_throw_path` 立即调用 `line_bresenham(px, py, px, py)` 计算弹道，触发退化路径。

**修复后：** 函数入口加 `if x0 == x1 && y0 == y1 { return Vec::new(); }`，零长度路径直接返回空向量。

**教训：** 方向派生自比较的迭代算法（Bresenham、DDA 等）在起终点相同时，所有方向的比较都为假，推导出"反向"步进——必须显式处理退化情形。

**位置：** `dungeon-core/src/ops.rs:196`

### G22 — 投掷无伤害（怪物先于投掷行动） ✅已修复

**修复前：** 投掷耗时 400ms，玩家投掷 AV=70+400×0.80=390ms，怪物追击 AV=85+250×0.90=310ms。怪物先执行，移动后投掷落空，始终显示"石子落在地上"。

```
怪物追击 AV=310 < 投掷 AV=390 → 先执行 → 怪物移动 → 投掷落空
```

**修复后：** 投掷耗时改为 **190ms**，玩家投掷 AV=70+190×0.80=**222ms**，快于怪物追击（310ms）。投掷在怪物移动前命中。

```
怪物追击 AV=310 > 投掷 AV=222 → 后执行 → 投掷命中 → 怪物移动
```

**位置：** `src/throw.rs:176`、`GAME.md` 行动表、`DESIGN.md` Dsn12

### G15 — Buff 持续时长新旧系统差异 60 倍 ✅已修复

**修复前：** `SkillKind { duration: 3 }` 传入两个系统得到不同时长：旧 Buffs 读作 3 帧（~50ms），新 ActiveBuffs 读作 3s（3000 AV），相差 60 倍。

**修复后：** 旧 `Buffs` 系统已由 D11 完整移除，ActiveBuffs 为唯一 Buff 系统。60 倍差异随旧系统消失而自然消除。

**关联：** D11（移除旧 Buffs 结构体）、D10（移除 buff_tick_system）

---

## 一、设计层面（Design）

---

### 🟡 D15 — `place_skill_scrolls` 缺少 exclude 参数

**问题：** `place_skill_scrolls` 是唯一不接受 exclude 参数的放置函数。技能卷轴可能生成在楼梯/出生点上。

```rust
// ops.rs:179 — 无 exclude 参数
fn place_skill_scrolls(world: &mut World, _floor: u32, rng: &mut impl Rng) {
```

对比同模块的其他函数：
- `spawn_monsters(world, floor, rng, exclude)` ✅
- `place_ground_items(world, item_ids, exclude)` ✅
- `scatter_stones(world, rng, exclude)` ✅

`descend` 中对 `place_skill_scrolls` 的调用也不传 exclude。

**影响：** 🟡 中 — 技能卷轴（治愈/护盾/狂暴）可能覆盖玩家关键交互位置。概率低（30 次尝试散布到可行走格）但存在。

**位置：** `dungeon-core/src/ops.rs:179`（定义）、`dungeon-world/src/init.rs:330`（descend 调用点）

---

### 🟡 D5 — 事件帧模式（Deferred — 触发条件达成时重新评估）

**问题：** 当前玩家确认行动后批量推进到玩家行动完成，中间所有怪物行动对玩家不可见。

**提议方案：** 增加可切换的"事件帧模式"（按 `s`），每帧只执行一个事件，Enter 步进。

**当前评估：** 暂缓实现。在当前战斗系统（纯数值 chase/flee/wander）下，事件帧模式提供的信息量不足以补偿节奏损失——玩家的最优策略不会因看到每个怪物单步移动而改变。

**触发条件：** 出现**足够复杂的战斗逻辑**，即新增的怪物/boss 有需要玩家在过程中作出反应的能力——例如范围攻击预警、状态效果倒计时、可打断的吟唱、地形变化。当单次 tick 内的行动序列构成决策信息时，事件帧模式从"nice to have"变为"need to have"。

---



---

## 二、架构层面（Architecture）


### 🟡 A11 — `ActiveCooldowns` 悬空功能

**问题：** `ActiveCooldowns` 组件在 `advance_action_queue` 中有完整的 AV 推进逻辑，但在 `descend` 中既未保存也未恢复，且没有任何技能向其写入数据。组件有定义、有推进、有存档支持，但无任何写入点。

```rust
// components.rs:189 — 定义
#[derive(Component, Clone, Debug, Default)]
pub struct ActiveCooldowns(pub Vec<Cooldown>);

// execute.rs:33-38 — 推进逻辑
{
    let mut q = world.query::<&mut ActiveCooldowns>();
    for mut cds in q.iter_mut(world) {
        cds.0.retain_mut(|c| { c.remaining_av -= dist; c.remaining_av > 0.0 });
    }
}
```

**违反 LESSONS L20：** 未完成的游戏机制不应留在代码中。

**位置：** `dungeon-core/src/components.rs:189`、`dungeon-action/src/execute.rs:33-38`

### 🟡 A12 — ActionKindV3 枚举跨 8 个文件飘散，新加怪物行为成本高

**问题：** `ActionKindV3` 枚举同时承载玩家行动（Move/Wait/Skill/Attack）和怪物行为（Chase/Flee/Wander），两种扩展节奏不同的东西被捆绑在一个枚举中。每新增一种怪物行为，需要修改 **7-8 个文件**：

| 文件 | match 点 | 修改内容 |
|------|---------|---------|
| `dungeon-action/src/types.rs` | 枚举定义 | +1 变体 |
| `dungeon-action/src/execute.rs` | `execute_entry` + `check_condition` | +2 arm |
| `dungeon-action/src/player.rs` | `handle_timed_action` | +1 确认对 |
| `dungeon-render/src/timeline.rs` | `action_display` | +1 arm |
| `dungeon-world/src/persist.rs` | `SavedActionKind` + capture + restore | +3 处 |
| `dungeon-action/src/monster.rs` | 决策系统 | +1 输出 |

**根因：** 枚举是编译期全匹配的，适合**变体少且稳定**的场景（如 Tile = 5 种地形，EquipmentSlot = 3 个槽位）。怪物行为需要持续扩展（巡逻/召唤/远程/毒雾等），用枚举每加一种就要通改所有 match 点。

**建议方向：** 玩家行动保持枚举（Move/Wait/Skill/Attack 扩展频率极低），怪物行为改用 trait 对象：

```rust
pub trait MonsterBehavior: Send + Sync {
    fn execute(&self, world: &mut World, entity: Entity);
    fn check_condition(&self, world: &World, entity: Entity) -> bool;
    fn display_name(&self) -> &'static str;
    fn priority(&self) -> u32;
    fn av_cost(&self, agility: u32) -> f32;
}
```

`ActionEntry` 加 `behavior: Option<Box<dyn MonsterBehavior>>` 字段，`execute_entry`/`check_condition`/`action_display` 中的怪物分支统一调用 trait 方法——不再需要 match。

**收益：** 加新怪物行为从 7-8 个文件 → 1 个新组件 + 1 个 impl。
**代价：** 虚表调用有微小运行时开销，对 ECS 回合制游戏可忽略。

**位置：** `dungeon-action/src/types.rs:20`（ActionKindV3 定义）、`dungeon-action/src/execute.rs` `execute_entry`+`check_condition`、`dungeon-render/src/timeline.rs` `action_display`、`dungeon-world/src/persist.rs` SavedActionKind、`dungeon-action/src/monster.rs` 决策系统、`dungeon-action/src/player.rs` `handle_timed_action`

---

### 🟢 A15 — `open_look_mode` 在 main.rs 中混合渲染和状态管理

**问题：** `open_look_mode` 直接在函数体内 `terminal.draw()` 渲染 + `event::read()` 处理输入，完全绕过了主循环的渲染编排。如果将来要将查看模式移至 dungeon-render crate，需提取至少两个关注点。

**影响：** 🟢 低 — 当前实现（~30 行）简洁有效。仅在 render crate 架构升级时才需处理。

**位置：** `src/main.rs:214-249`

---

### 🟢 A17 — InputBuffer 资源创建但从未使用

**问题：** `init.rs` 中 `world.insert_resource(InputBuffer::default())` 创建了 `InputBuffer`，`persist.rs` 也重建了它，但整个代码库除 types.rs 中的定义和 push/pop 方法外**没有任何调用方**。`main.rs` 的输入处理完全不经过 `InputBuffer`。

```rust
// types.rs:186-206 — 定义和方法已存在
// init.rs:18 — 插入资源
// main.rs — 输入流程完全不使用
```

**影响：** 🟢 低 — 类似 PendingSkill/PendingPickup（已删除）的同模式悬空代码。资源占用可忽略，但属于"代码骨架先于实际使用"的模式，历史上这种模式容易腐败。

**位置：** `dungeon-action/src/types.rs:186-206`（定义）、`dungeon-world/src/init.rs:18`（插入）、`dungeon-world/src/persist.rs:142`（重建）

---

### 🟢 A18 — ratatui 内置 widget 闲置（Gauge/List/Clear/Scrollbar/Table 未使用）

**问题：** 项目中使用的 ratatui widget 仅限于 `Paragraph` + `Span` + `Layout` + `Block`，五个内置 widget 完全未使用，对应功能由手写代码替代：

| widget | 手写替代位置 | 手写行数 | 可简化到 |
|--------|------------|---------|---------|
| `Gauge` | `ui.rs` 中 `bar()` 函数 | ~8 | 2 行构造 |
| `List` | `inventory.rs` 背包列表循环（选中态+▸+滚动） | ~30 | 5 行 |
| `Clear` | 模态弹窗覆盖逻辑 | 依赖 modal | 1 行 |
| `Scrollbar` | 事件日志 `take(12)` 硬截断 | ~5 | 无截断+滚动条 |
| `Table` | `ui.rs` 属性面板手算 `{:>3}` + `"   "` 分隔 | ~15 | 3 行列定义 |

**影响：** 🟢 低 — 正确性不受影响。代码量约多写 50 行，背包列表的可维护性（选中态/滚动边界）不如 `List` 开箱即用。仅在新增类似 UI（合成台、技能树）时值得一次性迁移。

**位置：** `dungeon-render/src/ui.rs`（Gauge/Table/Scrollbar）、`src/inventory.rs`（List）

---

## 三、实现层面（Implementation）


### 🔴 I31 — throw.rs 架构混乱，多处违反软件工程原则

**问题：** `src/throw.rs` 是近期新增文件（~380 行），存在以下架构问题：

**① `execute_throw` 违反单一职责原则（SRP）**
~75 行函数同时负责：目标验证、玩家/副手检查、怪物查找、伤害计算（含暴击）、HP 扣减、死亡判定、掉落生成、事件日志、副手消耗。可提取至少 3 个独立函数。

**② 暴击率计算重复实现 ✅已修复（Phase 1）**
`execute_throw` 已下沉至 `dungeon-action/execute.rs`，暴击率通过 `ops::equipment_bonus()` 统一计算（与 `execute_attack` 一致），不再手动遍历 armor/ring。

**③ 副手消耗逻辑重复 ✅已修复（Phase 1）**
`execute_throw` 尾部单次统一消耗副手，命中/未命中分支不再各自一份。共享函数 `ops::consume_off_hand` 提取至 `dungeon-core/src/ops.rs`。

**④ `open_throw_select` UI 与游戏逻辑耦合 ✅已修复（Phase 1）**
Enter 处理器中的装备管理已提取为 `ops::equip_throwable_to_off_hand()` 共享函数，`throw.rs` 和 `inventory.rs` 的两处内联代码统一调用此函数。

**⑤ `update_throw_path` 借用模式脆弱**
读 cursor→drop→计算→写 path 的 dance 容易因重构引入 I29 类崩溃。

**⑥ 使用 `.expect()` 的可 panic 路径**
`open_throw_aim` 和 `execute_throw` 中多处 `.expect()`，组件缺失时直接 panic 而非降级返回。

**⑦ 架构错放——核心游戏逻辑在应用层（src/） ✅已修复（Phase 1）**
`execute_throw` 已完整迁移至 `dungeon-action/src/execute.rs`，与 `execute_attack` 同级。修改战斗公式只需改 `dungeon-action` 一个 crate。遗留的 `update_throw_path`（纯弹道算法）留在应用层是合理的（UI 逻辑）。

**影响：** 🟡 中（剩余 ①⑤⑥）

**剩余建议方向：**
```
① execute_throw 仍 ~70 行（从应用层下沉后未进一步拆分）
⑤ update_throw_path 的 borrow dance
⑥ .expect() 降级处理
```
            handle_kill (击杀+掉落)
  路径:     update_throw_path (保持)
  目标:     execute_throw 从 ~75 行缩到 ~15 行
```

**位置：** `src/throw.rs`

### 🟡 I24 — Buff/Skill 系统缺陷（含子问题 I24a〜I24c）

**问题：** 当前 Buff 系统和技能机制有三个互相关联的缺陷。ActiveBuffs（I29）修复了缺陷①，但缺陷②③和 I29 引入的回归（G14）仍未解决。

**I24a — Buff 持续时间不可预测 ✅已修复（见 I29）**
`buff_tick_system` 每帧减 1 回合，与 AV 推进脱钩。已由 ActiveBuffs 组件 + AV 同步推进修复。

**I24d — Buff 双倍叠加 ✅已修复（见 G14）**
I29 引入双写双读回归。已移除旧 Buffs 写入路径，`effective_attack/defense` 只读新 ActiveBuffs。

**I24b — 技能数量少且职业锁定 🟡**
技能通过 `PlayerClass::skills()` 硬编码，战士固定 3 技能，无法扩展，每局玩法相同。技能来源是职业而非道具。

**I24c — 无冷却维度 🟡**
技能只有 MP 消耗，没有冷却。强技能无法通过冷却平衡。`ActiveCooldowns` 组件已存在但未被任何技能使用。

**影响：** 当前系统不支持复杂战斗设计。自由组合、道具学习、冷却平衡均不可实现。

**方案方向（设计中，见 DESIGN.md §15）：**
- Buff/冷却改为 `remaining_av: f32`，在 `advance_action_queue` 中同步推进 ✅（由 I29 完成）
- 技能改为从道具学习，`Skills` 组件动态扩展
- 冷却下限约 1000 AV

### 🟡 I22 — clippy 警告约 29 个未处理

**问题：** `cargo clippy` 报告约 29 个警告（已修复 34 个，原 63 个）。

**已修复类型（34 个）：**
`unnecessary_cast`(6)、`useless_format`(2)、`map_identity`(1)、`unnecessary_map_or`(1)、`manual_div_ceil`(1)、`sort_by_key`(2)、`new_without_default`(4)、`derivable_impls`(3)、`unnecessary_mut_passed`(3)、`needless_borrow`(5)、`unused_variables`(1)、`RoomShape` Default(1)、`ActionQueue`/`PlayerPreview` 默认派生(2)、cast usize(2)

**剩余类型：**
| 类型 | 数量 | 说明 |
|------|------|------|
| `collapsible_if` | ~14 | 安全但逐个修复繁琐 |
| `needless_range_loop` | ~6 | 迭代器可读性更佳 |
| `type_complexity` | ~3 | 需要定义 type alias |
| 其他 | ~6 | 零星警告 |

**建议：** 不影响正确性，可逐步清除。

### 🟡 I23 — 测试覆盖缺口：dungeon-core 和 dungeon-render 零单元测试

**问题：** 核心 crate 的单元测试覆盖不均衡。
| crate | 单元测试数 | 覆盖内容 |
|-------|-----------|---------|
| dungeon-core | 0 | ❌ 核心公式（伤害/升级/属性）、FOV、寻路、序列化均无直接测试 |
| dungeon-render | 0 | ❌ UI 渲染逻辑无测试 |
| dungeon-action | 8 | ✅ |
| dungeon-world | 2 | ✅ |
| 场景集成测试 | 3 | ✅ 间接覆盖部分 core 逻辑 |

**风险：** dungeon-core 包含战斗公式、升级曲线、FOV、A* 寻路、Tile/Stats 序列化——任一公式修改都可能无声破坏平衡，无单元测试意味着只能靠手动打游戏验证。

<!-- I35 已移至 ✅已修复（修复前/修复后记录见上方） -->

// pub mod pathfinding; // 已移除（find_path 未使用）
// pub use pathfinding::*; // 已移除
```

但 `pub mod pathfinding;` 是生效的，`execute.rs` 中 `dungeon_core::pathfinding::astar` 也在使用。

**位置：** `dungeon-core/src/lib.rs:7-9`
**位置：** `dungeon-core/src/items.rs:54`（ItemStack 定义）



### 🟢 I26 — `place_skill_scrolls` 的 `_floor` 参数未使用

**问题：** `place_skill_scrolls(world, _floor, rng)` 函数签名中 `_floor: u32` 带下划线前缀（标注"未使用"），函数体内确实从未使用该参数。GAME.md 中所有卷轴在 F1+ 均可出现，无楼层相关性——所以参数的存在是正确的设计扩展点，但当前是死参数。

**影响：** 🟢 低。如果将来实现楼层滚稀有卷轴的逻辑，需要用到此参数。

**位置：** `dungeon-world/src/init.rs:94`

### 🟢 I27 — `descend` 中 `GameRng` 种子与 `setup_world` 不一致

**问题：** `execute_attack` 中的暴击判定使用 `GameRng`。`setup_world` 中 `GameRng` 种子为 `map_seed.wrapping_add(42)`，但 `descend` 中不创建或重置 `GameRng`——下楼后旧 RNG 状态继续使用。每次下楼 RNG 状态不重置，导致两个问题：

1. **不可回放性**：如果记录种子回放，下楼后玩家暴击/不暴击的结果不可复现
2. **隐蔽的种子泄露**：下楼后 `GameRng` 的状态取决于下楼前玩家进行了多少次暴击判定

对比地图生成（使用 `MapSeed + floor_number` 派生的独立 `SmallRng`，下楼后重新 seed），`GameRng` 的推进方式不一致。

**影响：** 🟢 低 — 当前游戏没有"回放"功能需求，暴击判定使用旧 RNG 状态在功能上正确（只是种子不干净）。仅在引入回放功能时需要修复。

**位置：** `dungeon-world/src/init.rs:79`（setup_world 的 GameRng 初始化）

---


### 🟢 I45 — `calc_player_crit` 与 execute_attack 内联暴击计算重复

**问题：** `execute.rs:374-393` 的 `calc_player_crit` 从 Stats + equipment_bonus 计算暴击率/倍率，与 `execute_attack` 中 `:302-305` 的内联计算几乎相同。差异仅在于：`execute_attack` 的 `total_crit_rate` 计算在攻击者（可能非玩家）路径中，而投掷固定在玩家。

**影响：** 🟢 低 — 代码重复，未来暴击率计算逻辑修改需同步两处。

**位置：** `dungeon-action/src/execute.rs:302-305` vs `:374-393`

---

## 四、游戏逻辑层面（Game Logic）




### 🟡 G17 — 材料物品无消耗渠道

**问题：** 生物血肉（id=10）、破布（11）、坚硬木棍（12）、染血兽牙（13）、黑色甲壳（14）五种材料物品只能拾取和堆积，没有任何消耗途径。背包 36 格在 4-5 层后会被材料大量占用，玩家被迫在"拾取所有材料"和"留空间给有用物品"之间做无趣的选择。

```rust
// 当前材料的全部用途：占背包格
// 没有任何合成/升级/交换/消耗机制消费它们
```

**影响：** 🟡 中 — 材料的存在感为零。玩家的理性选择是"忽略所有材料掉落"。

**建议方向：** 至少为材料规划一个基础消耗渠道：交换经验值、升级装备、恢复 HP/MP 等。DESIGN.md §17 也将合成/配方标记为"暂缓"，但材料长期零消耗会腐蚀掉落系统的设计合理性。

**位置：** `assets/items.json` items 10-14

### 🟡 G18 — 地面物品每层完全相同

**问题：** `ground_item_ids = [0, 1, 2, 3, 0, 1, 3, 2]` 硬编码在 `init.rs` 中，每层生成完全相同的 8 件物品（锈铁剑×2、木盾×2、皮甲×2、攻击戒指×2）。无随机变化、无楼层关联、无稀有度梯度。

```rust
// init.rs:235 — 每层都一样的物品组合
let ground_item_ids = [0, 1, 2, 3, 0, 1, 3, 2];
place_ground_items(&mut world, &ground_item_ids, &[(spawn_x, spawn_y), (stairs_pos.0, stairs_pos.1)]);
```

GAME.md 记载的是"每层约 8 件物品"——数量和描述匹配，但缺少"多样性和随机性"的设计意图。

**影响：** 🟡 中 — 玩家打完第一层就见过所有可拾取物品了，下楼探索的动力之一是"找新装备"的期待落空。

**建议：** 至少随机化物品种类和数量，低层出基础装备，深层引入稀有/魔法物品或更高的装备层级。

**位置：** `dungeon-world/src/init.rs:235`

### 🟢 G19 — 战斗公式缺乏层次深度

**表现：** 当前 `max(攻击 - 防御, 1)` 的差值公式完全线性，1 点攻击永远对应 1 点伤害。无穿甲穿透、无元素属性/抗性、无距离衰减、无背后/侧击加成。装备增强集中在 +攻击/+防御 两个维度。

**影响：** 🟢 低 — MVP 阶段可以接受。但扩展到 8+ 种怪物、3+ 种武器类型时，所有战斗都会感觉"差不多"——只有数值差异，没有策略差异。当需要设计"抗高攻怪"和"抗高防怪"两种不同策略时，当前公式无法提供区分度。

**位置：** `dungeon-action/src/execute.rs:285-310`（execute_attack）

### 🟢 G21 — `execute_throw` 中 GameRng 多次 `resource_mut` 调用脆弱

**问题：** `execute_throw` 中 `GameRng` 被多次通过 `world.resource_mut::<GameRng>()` 获取：

```rust
// throw.rs:260 — 伤害附加随机
let extra = world.resource_mut::<GameRng>().random_range(0, 2) as u32;
// throw.rs:275 — 暴击判定（另一次 resource_mut）
let roll = world.resource_mut::<GameRng>().random_f32();
// throw.rs:285 — 掉落判定（再一次 resource_mut）
let mut rng2 = world.resource_mut::<GameRng>();
let stacks = lt.roll(&mut rng2.rng);
```

虽然每次 `Mut<GameRng>` 在下次调用前已 drop（不会触发 I29 类崩溃），但模式脆弱——中间插一句代码就可能产生双重借用 panic。应绑定为 `let mut rng = world.resource_mut::<GameRng>()` 统一使用。

**影响：** 🟢 低 — 当前正确运行。重构风险点，但不会在现有代码中触发崩溃。

**位置：** `src/throw.rs:260` `:275` `:285`

---



## 其他


### 🟢 P7 — 玩家确认行动后无法取消（被 D5 锁定）

**问题：** tap-tap 双击确认后行动进入 `ActionQueue` 无法撤回。

**说明：** 事件帧模式（D5，已 defer）可以部分解决此问题——事件帧模式下玩家可以在自己行动执行前切换方向。在 D5 重新评估前此问题无解。

