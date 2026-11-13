# 容器、尸体与存档投影：调研

> 范围：ecs_core 的容器/尸体/怪物装备现状，以及"容器树 + 效果 + 确定性世界"该怎么进存档。
> 基线：HEAD `ba1b2cf`（DsnE18 第②片尸体容器已落地 `4e8be6b`）。工作区另有并行改动（task-2/task-3，以及未提交的 **DsnE19 存档草稿 + `src/save.rs` 实现**与 `floor.rs` 的 `rebuild_floor` 重构）；本文内部行号按写作时快照，相关文件正在被改，引用以函数名 / DESIGN 锚点为准。
> 不重复 `RESEARCH-item-systems.md`（物品定义/容器选型/掉落生成/bevy_save 已有 §A~§I）；本文只回答三个问题：尸体容器落地后还差什么、怪物装备该走哪条、容器树怎么进存档。外部资料取自 2026-11 源码（行号按当时 master/3.6 分支）。
> 纪律：只列**实际打开过**的来源；凡属本文自己的推理，标 `[推断]`。

## 结论先行

1. **尸体容器不是待做项**（`4e8be6b` 已做），但缺三样：失败分支会泄漏物品实体（`system/death.rs:77-83`）、`CORPSE` 4 格与 `equipment` 列表之间没有不变量、搜刮与拾取各写了一份"并堆→开新格"（`item/mod.rs` 内两处）。先补这三样，再谈扩展。
2. **怪物装备现在是"死亡时按模板静态实例化"**（`MonsterTemplate.equipment`）：活着不穿、不算数值、不可见、无随机。这是最省的一条路，代价是装备只是"尸体里的纪念品"。要让它影响战斗，把生成改成"spawn 时挂 `BODY` 容器"即可——`effect::collect` 已在战斗两侧自动读装备（`system/combat.rs:48-49`），但这属换道（核心数据模型 + 数值重算），应先落 DESIGN。
3. **存档投影 = 手写按值 DTO，容器用递归树，不写任何 `Entity`**。这同时消掉三个坑：`Entity` 不保证跨版本可读（bevy_ecs 文档明说不保证 wire 兼容）、`MapEntities` 重映射、容器环检查。**bincode 1.3 是位置编码、非自描述，"字段只追加 + `#[serde(default)]`"在它上面不成立**——版本必须分发到不同 struct。
4. **存档点必须定成"进层检查点"**（`descend` 成功后自动存档），否则中途存档会复活已杀怪物、重刷已捡物品（杀怪刷经验/复制物品）`[推断]`，要支持就得上 `FloorDelta`，成本远超本调研的 SaveV1。
5. **不该采用**：整世界反射快照（bevy_save `Snapshot`/`DynamicScene`）——它要求全部组件 `Reflect`、要把 `Entity` 重映射，默认口径还会把瞬态子实体（动作/技能实例）一起拍进档（要排除得额外写过滤规则），与 DsnX17 ⑥ 的取向相反；也不该让 ecs_core 直接读文件或引入 bincode（crate 边界 + R2 依赖变更）。

## 一、尸体容器：已落地部分的审计

已核实的事实（不用再读代码就知道的）：

- 布局：`item/container.rs:211` `CORPSE` = 4 个 `SlotDef::any`（"遗留 1..4"），doc 写明"只装身上的装备、战利品按 Gm8 落地、不占格"。
- 组件：`monster/mod.rs:16` `Corpse { kind: MonsterKindId, name: &'static str }`，`display_name()` = "哥布林的尸体"；渲染复用怪物字形（`presentation/src/extract/mod.rs:561-611`，`VisualLayer::Item`）。
- 生成：`system/death.rs:74-88` 先按 `template.equipment` 逐件 `spawn_item` + `insert`，再 spawn 尸体；`:90-92` 战利品 `roll_loot` 照旧落地；`:95` despawn 死者。
- 搜刮：`item/mod.rs:166` 的 `pickup_system` 在拾取地面后调 `loot_corpses`（`:288`），逐件先并堆、再开新格，成功则从尸体移除；放不下留尸体并记日志；尸体本身不消失。
- 清理：`world/floor.rs:148-159` 下楼时把尸体连同内容一起 despawn（注释点明"否则会留下无主物品"）。
- 测试：`item/item_tests.rs:453/501/516`、`item/container_tests.rs:255`、`world/floor_tests.rs:268`、`presentation/src/extract/tests.rs:206`。

外部对照（我们的做法是刻意偏离）：DCSS 的尸体是**会腐烂的物品**（`crawl-ref/source/corpse.cc` 的 `is_rottable`/`_maybe_rot_corpse` 改 `freshness`，到期转骷髅或销毁），怪物物品走另一条路 `monster_drop_things`（`mon-death.cc:3610-3617`）；NetHack 的尸体是带 `ROT_CORPSE` 计时器的食物（`src/mkobj.c:311-317`），背包由 `relobj` 掉地（`src/mon.c:1820-1833`）。两家都不把尸体当容器。Caves of Qud 与 flecs/bevy 官方示例本轮未读（见末节），所以"尸体=容器无先例"只对 DCSS/NetHack 成立。
我们换来的好处：尸体与背包/箱子共用一套容器语义，"从尸体上拿东西"=容器转移，代码只有一份；代价是**清理要级联**（已做）、存档多一层递归树（§3）、UI 要多读一份容器内容（下面第 4 条）。

缺口（按"改哪个文件、改成什么形状"写）：

1. **失败分支泄漏物品实体**。`system/death.rs:77-83` 里 `spawn_item` 已建好实体，`insert` 失败只 `log::warn("…该物品丢失")`——实体仍在世界里，无 `Position`、无 `GroundItemAt`、不在任何容器中，任何查询都看不到，只有整世界 despawn 才会消失。改：失败分支 `commands.entity(item).despawn()`（或落地上作为兜底；既然容量应是模板不变量，先 despawn + 日志更诚实）。同时把这段抽成 `fn spawn_corpse(commands, template, position)`，死亡系统只负责"结算 → 掉东西 → 回收"。
2. **没有"装备件数 ≤ 尸体格数"的不变量**。`CORPSE` 固定 4 格，`equipment` 是静态表，谁都可以加到第 5 件然后线上静默丢件。改：在 `item/container_tests.rs` 旁（或 `monster/template_tests.rs`）加测试：遍历 `MonsterKindId` 全部变体，断言 `monster_template(kind).equipment.len() <= CORPSE.len()`；再加一条"`equipment` 里每件 id 都存在且 `is_equipment()`"。
3. **转移逻辑重复**。`pickup_system`（地面→行囊）与 `loot_corpses`（尸体→行囊）各写一份"先找同类可并堆 → 再找 `free_slot_for` → 放不下留原处"，约 40 行同构代码；等箱子/交易一来必然是两份语义漂移。改：新建 `ecs_core/src/item/transfer.rs`，导出 `pub fn move_item(from: &mut Container, from_slot: usize, to: &mut Container, tags) -> Result<MoveOutcome, MoveError>`（或 `merge_or_insert`），`pickup_system`/`loot_corpses`/未来"翻找/开箱"共用；`container.rs` 模块头提到的"环形检查将来放 `world::transfer::would_nest`"可先在此留桩（本层只搬一件，不需要环形检查）。
4. **尸体内容不可见**。`look` 只看快照里的单个实体：`presentation/src/ui/mod.rs:222-245` `look_view` 用 `frame.entities.iter().find(...)`，`render-api/src/ui.rs:242` `LookView` 只有 cursor/tile/entity/footer——玩家在踩上去之前无法知道尸体里有什么，也就没有"要不要搜刮"的决策 `[推断]`。改（建议，UI 线）：Extractor 在 `presentation/src/extract/mod.rs:561-611` 附近把尸体 `Container` 内容按槽序读成字符串列表，塞进 `EntityView` 的新字段；`LookView` 增 `details: Vec<String>` 由 tui 渲染。不要为此在 render-api 里暴露 `Entity`/`World`。
5. **没有腐烂/消失**。现状空尸体永久留在格子上（同一格可能叠两具：怪物走到尸体格上死亡 `[推断]`）。DCSS/NetHack 有腐烂是因为它们有饥饿与全局时间 `[推断]`；我们做腐烂要新增"剩余 AV"字段并让"中途存档"口径必须入档（§3 已否掉中途存档）。结论：**本期不做腐烂**；同格多尸体允许存在（搜刮遍历全部），但 look 需要聚合（同第 4 条，否则 `find` 只显示第一具）。

文档现状：DsnE18 的状态段（`<!-- &DsnE18 -->` 后第一段）仍写"落地前待拍板…尚未改任何代码"，与条目末尾的"落地记录（第②片）"自相矛盾（RULE §八「文档过时」）；「开放决策」段 6 条已落地 3 条（装备并入身体槽位、尸体三档选 (a)、槽位存法）却未勾销，ItemTag 词汇实际落地为 Head/Chest/Weapon/Shield/Feet/Ring/Pack（无 Hands，副手=`Shield`）。这些归 DsnE18 的收尾，和 task-2 的 D7/D9/D11 是一件事，本文不重复报告数值。

## 二、怪物装备：三条路与选择

现状（已核实）：`monster/template.rs:197` `pub equipment: &'static [ItemId]`；GOBLIN `&[ITEM_DAGGER]`、MUSHROOM_GOLEM `&[ITEM_HAMMER]`、其余 6 种 `&[]`。怪物 spawn 时 `monster_base_bundle`（`world/init.rs:342-366`）**不带 `Container`**；装备只在死亡时（`death.rs:74-88`）实例化。⇒ 装备既不影响怪物攻防、也不可见、也不随机，且"每只该物种必掉固定装备"。

外部对照：DCSS 的怪物装备槽是 `mon_inv_type`（武器/副手/护甲/盾/珠光…），生成时决定并参与攻击；其头文件注释 **"Adding slots breaks saves. YHBW."**——槽位表的顺序本身是存档契约，只能追加。NetHack 的 `minvent` 同理在死亡时 `relobj` 掉地。我们的"尸体即装备容器"是把 DCSS 的"怪物装备槽 + 掉落"压成一层；简化代价是装备与"怪物现在拿什么"脱钩。

三条路（按代价排序）：

- **A. 维持死亡时静态实例化**（现状 + §1 修复）。成本≈0；不变量靠测试钉住。代价：装备只是"尸体纪念品"，且每只哥布林必掉匕首会造成稳定供给 `[推断]`，掉率/经济口径要 GAME 拍板（属内容数值，[试调]）。
- **B. spawn 时挂 `BODY` 容器（装备仍是模板静态表）**。改 `world/init.rs` 的 `monster_base_bundle`：按 `template.equipment` `spawn_item` 后插入 `Container::new(&BODY)`（用 `BODY` 的过滤槽位，匕首进主手、石锤进主手、其余部位空）；死亡时把身体槽位里的实体**转移**进尸体（`system/death.rs` 用 `Container::take_slot` + `corpse.insert`），不再重复 `spawn_item`——天然修掉 §1 的泄漏与双实例化。收益：装备立刻影响战斗，因为 `effect::collect` 的 `equipped_defs` 走"身体容器→槽位→注册表"，而 `system/combat.rs:48-49` 对攻击方与目标**都**调 `collect_actor`，`combat.rs` 一行不用改。代价：怪物攻防变了 ⇒ GAME Gm4/Gm8 数值重算 + 战斗测试可能变；`equipment` 必须挂在能接受它的槽位，否则 insert 失败（同 §1 不变量测试覆盖）。
- **C. spawn 时随机装备（DCSS 式）**。给怪物挂 `BODY` 后从掉落表/装备池掷骰；随机源按 RULE §九.1 #5 属换道：`DerivePurpose` 只能在末尾追加新变体（如 `MonsterGear`，新 offset；改既有值=改同种子世界，`world/init_tests.rs:150-195` 的 golden 会抓），且随机序列要绑定"生成顺序确定"。好处是经济多样；代价是数值 + 存档口径（若允许中途存档，每只怪的随机装备也要入档；检查点口径下不必，因为重建可复现）。

建议：**先 A 收尾（修 §1 三件），B 作为"装备要有战斗意义"时的下一跳，C 等经济系统成熟**。理由：B/C 都动核心数据模型与数值（换道），当前 R2 还在收尾，且 B 不做的话 C 没有意义（随机装备也只是一堆尸体内容）。

数据一致性（形状必须现在就定，交给数据线执行）：
- 同一物种的 `equipment` 与 `loot` 不应重叠（HEAD 的哥布林是 `loot` 12% 匕首 + 尸体必得匕首=双源；工作区 task-2 已删 loot 侧那一条）。建议加测试：`for kind: template(kind).loot 中任何 item ∉ template(kind).equipment`；跨物种不设限。
- 口径一句话（写进 Gm7/Gm8 与 DsnE18）：**尸体=该物种的固定装备（必得）；掉落表=材料/消耗品/变体装备（概率）**。这样"装备来源"清单才只有"尸体 + 地面刷新"两处。
- **最小可测内容（现状即可测）**：`GOBLIN→ITEM_DAGGER`（id 24，`[Weapon]`，200ms、攻+2）、`MUSHROOM_GOLEM→ITEM_HAMMER`（id 23，`[Weapon]`，450ms、攻+4）——两件都能进主手槽，其余 6 种空手。测试只需 `new_game` 后杀一只哥布林/傀儡，断言尸体 `Container` 里有该 id；要扩第二对（如 `CAVE_CRAB→木盾`）得先给盾/甲注册掉落来源（`GROUND_ITEM_POOL` 目前只有 id 3/23/24/40/25/32，无 id 1/2），属内容数值，等 B 路一起拍板。

## 三、存档投影：SaveV1 的形状

### 3.1 先定语义：进层检查点

`world/floor.rs:62-132` 的 `descend` 已经把"层内容由 `(MapSeed,floor)` 重建"写实：清尸体/怪物/地面物品 → 楼层 +1 → `generate_map` → 生成楼梯/格子效果/地面物品/怪物 → 玩家落位到新出生点、记忆重置。这是 DsnE17 的确定性口径，也是 "存档只存 seed + floor + 玩家状态"（`DESIGN.md:1119`）成立的前提。

但这个前提只在**存档=刚进层**时严格成立。若允许层中途存/读：已杀怪物、已捡地面物品、已搜刮尸体都会在重建后复现——杀怪刷经验、捡货复制（与旧档 G32"杜绝 SL 刷掉落"同类问题）。要支持就得存 `FloorDelta`（死怪列表、地面物品差、尸体内容、记忆），成本和风险都远超本调研。

⇒ **SaveV1 语义 = 进层检查点**：`descend` 成功返回 true 后触发一次自动存档；App 层在 `apply_player_command` 前后比较 `FloorNumber`（`events.rs` 现有 6 个事件里没有 FloorChanged，最省的是比较资源），或在 `world/floor.rs` 末尾发一个 `FloorChanged` 事件。退出时若玩家在本层中途，只能"回到本层入口"或放弃本次 run（UI 措辞属 presentation；不要承诺"任意时刻存档"）。

### 3.2 该存什么 / 不存什么

```text
SaveV1 {
  map_seed: u64, floor: u32,
  rng: GameRng { state: u64, steps: u64 },      // resources.rs；读档用 from_state 续接
  player: { position, level, exp, hp_current, mp_current },
  body: SavedContainer,                          // 递归容器树，见 3.3
  effects: Vec<SavedEffect>,                     // 只允许玩家名下，目前仅 Poison
}
```

- 不存：地图 tiles（可重建；`Tile` 的 u8 serde 虽是冻结契约 `DESIGN.md:955`，也没必要用）、monsters、地面物品、楼梯、格子效果、尸体（检查点时刻一定没有）、`VisibleMemory`/`OccupancyMap`/`CellEffects` 索引（派生）、动作/技能子实体（DsnX17 ⑥）、`EventLog`、`last_step_av`。
- 派生重算（不存）：`health.max = max_hp_for(level, defense)`、`magic.max = max_mp_for(level, magic_mastery)`、`exp.exp_to_next = exp_to_next_level(level)`——就是 `balance.rs:6/12/17` 那三只，和 `system/experience.rs` 升级时用的一致。基础 `Attack/Defense/MagicMastery/Speed/Crit*` 目前没有任何系统改（全是 `player_base_bundle` 字面量），所以不存；将来若有成长系统，加字段并升 version。
- `MapMemory.explored`：检查点语义下刚进层=全空（`descend` 里 `MapMemory::new()`），可省略；只有"中途存档"才需要（`[[bool; 80]; 60]` = 4800 格，serde 只给到 32 长数组，要转 `Vec<u8>` 位图）。
- `WorldClock.total_av`：`Poison` 字段是"还剩多少 AV"而不是到期时刻（`effect/status.rs:41-53` 注释明说"让存档不需要冻结时钟口径"）⇒ 不必存；将来若有依赖绝对时间的机制再加。
- `rng.state` 校验：`GameRng::from_state` 不校验 `state==0`（xorshift 0 态永远 0），坏档会让整局随机冻结。`restore` 必须拒绝 `state==0` 并给出明确错误（或 `from_state` 加断言，两处都合理）。

### 3.3 容器树与校验

```text
SavedItem { id: u16, amount: u32, container: Option<Box<SavedContainer>> }
SavedContainer { slots: Vec<Option<SavedItem>> }   // len 必须 == ContainerDef::len()
```

- **布局不入档**：`ItemDef::container_def()`（`item/registry.rs:126`）给出 `&'static ContainerDef`，槽位过滤也来自静态定义——存档只记"第几格放了什么"。这与 DsnE16/DsnE18 的"定义静态、内容动态"一致；将来改 `CORPSE`/`BODY` 槽数必须升 version（旧档 `slots.len()` 校验失败要**报错**，不能静默丢件）。
- 还原：`spawn_item(commands, id, 1)`（`item/mod.rs:126`，Pack 类自动挂 `Container`）建实体；`Amount` 按 `amount` 写回（`Amount::new` 会把 0 变 1 ⇒ 校验 `1..=max_stack`）；再按存档槽位 `container.insert(slot, item, tags)`。嵌套由 `container_def()` 决定——有 `Some(def)` 的 item 必须带 `Some(SavedContainer)`，反之必须不带（双向校验）。
- 坏档必须 `Err` 不 panic（旧档 I71/A24 的教训）：`ItemId::exists()`、amount 范围、`slots.len()==def.len()`、每件 tags 被槽位 `accept` 接受、嵌套深度上限（现在只有 BODY→PACK 一层，写死 `max_depth=2` 或递归计数）、玩家唯一、effect 只能挂玩家。
- 效果：`Vec<SavedEffect>`，目前 `Poison { remaining_av, ticks_left, damage_per_tick }`（三字段都可入档）→ 还原 `world.spawn((EffectOf(player), Poison{...}))`；`OwnedEffects` 由 relationship target 自动维护（`effect/ownership.rs` 注释"不要手动插入"）。这满足 `DESIGN.md:704`「持久效果必须能被存档统一枚举写出」。

### 3.4 编码、版本与 IO 归属

- 文件 = `b"DSV2"` + `SAVE_VERSION: u32` + bincode payload（旧架构是 `DSV1` + bincode，见 `archive/dungeon-world/src/persist.rs` 的 `SAVE_MAGIC`；新架构不承诺读旧档，DsnX17 ⑤）。
- **bincode 1.3 是位置编码、非自描述**：缺字段会直接 EOF，`#[serde(default)]` 救不了（那是 JSON 这类自描述格式的写法）⇒ 不能靠"结构体末尾追加字段"演进；必须 `match version` 分发到 `SaveV1`/`SaveV2` 各自的 struct，迁移显式写。DsnX17 ⑤ 的"只追加"= 每次形状变化升 version，而不是在旧 struct 上塞字段。
- 依赖边界：ecs_core 无 bincode（现在只有 serde + serde_json），DTO 只 `derive(Serialize, Deserialize)`；bincode + 文件头 + 路径放 `dungeon-app`（根 `Cargo.toml` 已有 `bincode = "1.3"`），落盘用 `sys::files::write_bytes_atomic`（先 `.tmp` 再 rename，断电不留半档；`sys/src/files.rs` 模块头写明"文件字节读写，业务序列化不在这里"）。
- 读档路径必须复用下楼：把 `floor.rs:90-100` 的③步抽成 `world::floor::rebuild_floor(world)`（`generate_map` + `PlayerSpawn` + stairs/cell_effects/ground_items/monsters；工作区 DsnE19 草稿已这么抽），`descend` 与 `restore` 共用；restore 顺序 = `insert_core_resources(MapSeed)` → 写 `FloorNumber` → `rebuild_floor` → 建玩家（复用 `player_base_bundle`）→ 还原 body 树 → 还原 effects → 跑 fov/memory/occupancy/cell_effects 重建 + `clear_events`。否则"初始化/下楼/读档"三份生成逻辑必然漂移（旧档 `post_load_refresh` 那类补丁就是漂移的产物 `[推断]`）。
- 存档格式属换道（RULE §九.1 #3：存档/serde/版本）：DsnE19 草稿已在工作区（未提交），落地前按 §3.5 补齐；换道本身仍要与代码分 commit、标 `design:`。

### 3.5 与工作区 DsnE19 草稿的对齐清单

工作区 `ecs_core/DESIGN.md` 已有一份未提交的 DsnE19，配套实现 `src/save.rs`（355 行，含 capture/restore/`refresh_derived`）与 `tests/save_test.rs` 也在工作区：`SaveV1{version,map_seed,floor,player}`、`PlayerSave.body: Vec<Option<ItemSave>>`、`ItemSave{id,count,slots}` 递归、`SaveError` 枚举、F5/F9 在 `App::handle` 拦截——方向与本节一致（对比：本文 §3.4/表格建议"投影在 ecs_core、IO 在集成层"，实现把两层都放根 crate，可接受，代价是 restore 依赖 `ecs_core::rebuild_floor` 等 public API）。落地前建议补/确认这六点：

- **缺 `rng: GameRng{state,steps}`（未提交的 `src/save.rs` 里确实没有）**：层内容走 `derived_rng` 可复现，但全局 `GameRng` 被 `roll_loot` 消耗（`system/death.rs` 战利品结算）；读档走 `new_game(map_seed)` 会把全局 rng 拨回初始值，与"同一局未读档"分叉，也违背旧档 G32「精确恢复、杜绝 SL 刷掉落」。补法：`SaveV1` 加 `rng{state,steps}`，restore 用 `GameRng::from_state` 并校验 `state != 0`（§3.2 末）。
- **F5/F9 任意时刻存读 = 层内容回滚**：草稿把存/读挂在 `App::handle` 的 `Key::F(5)`/`Key::F(9)`，只处理了"丢地上的东西消失"，没处理反方向——已杀怪物/已捡地面物品/已搜尸体在 `rebuild_floor` 后复现，可刷经验、复制物品 `[推断]`。要么把读档语义写成"回到本层入口"（UI 文案明说），要么 V2 上 `FloorDelta`；不要同时承诺"确定性世界"和"任意时刻读档"。
- **restore 的派生重建与下楼共用一份**：草稿只写复用 `rebuild_floor`；未提交的 `src/save.rs` 已有私有 `refresh_derived`（重跑 occupancy/cell_effects 系统）且显式 `MapMemory::new()`。要确认它还重建 fov/`VisibleMemory` 并 `clear_events`，且与 `descend` 第⑤步同源——现在两处各写一份，改一处漏一处的风险在旧档 `post_load_refresh` 上出现过 `[推断]`。
- **`MapMemory.explored` 被主动重置**：`src/save.rs` 还原时 `MapMemory::new()`，读档后小地图清空。进层检查点口径下无害（刚进层本就空）；保留 F5/F9 任意时刻的话这是体验倒退，需明确"不存"并在 UI 说明，或存 4800 格位图（§3.2）。
- **version 在 payload 里、文件没有 magic 头**：`SaveV1.version` 要等 bincode 反序列化完才能读；而 bincode 1.3 非自描述，未来 v2 档喂给 v1 读法可能在版本检查前就 Encode 失败（玩家看到"编码失败"）。建议文件头 = `b"DSV2"` + `u32`，先拒未知版本再 `match version` 分发（§3.4）。
- **`health/magic` 存 current 还是 max**：实现选了 `VitSave{current,max}` 且注释"读档不重算"；§3.2 建议只存 current、用 `balance::max_hp_for/max_mp_for` 重算。两种都可：存 max 则调平衡公式后旧档不跟随（靠 version/迁移说明），重算则公式只有一处——选定一个并写进 DsnE19，别两处都算。

## 我们该怎么改（文件级）

| 文件 | 改什么 | 归属 |
|---|---|---|
| `ecs_core/src/system/death.rs` | 失败分支 despawn 物品；抽出 `spawn_corpse`；B 路方案时改为"转移身体槽位"而不是重新实例化 | 本轮可做（尸体收尾） |
| `ecs_core/src/item/mod.rs` | `loot_corpses`/`pickup_system` 改用 `item::transfer`；搜刮失败语义保持不变 | 本轮可做 |
| `ecs_core/src/item/transfer.rs`（新） | `move_item`/`merge_or_insert`；`would_nest` 留桩 | 本轮可做 |
| `ecs_core/src/item/container_tests.rs` | 加"equipment.len() ≤ CORPSE.len()"与"equipment 全是装备且存在"的遍历测试 | 本轮可做 |
| `ecs_core/src/monster/template.rs` | `equipment` 与 `loot` 不重叠的注释与测试；Gm7/Gm8 口径同步 | 与 task-2 数据线协作 |
| `ecs_core/DESIGN.md` | DsnE18 状态改"已落地"、勾销已决开放项；DsnE19 草稿按 §3.5 补齐（rng/派生重建/版本头）后提交 `design:` | 需要 Lead 拍板 |
| `ecs_core/src/world/floor.rs` | 抽出 `rebuild_floor`（工作区草稿已有）；`descend` 成功后供 App 判断的返回/事件 | 存档落地时 |
| `ecs_core/src/save/{mod,model,capture,restore}.rs`（新） | 纯投影：capture &World→SaveV1；restore 校验 + 重建；`pub mod save;` 加到 `lib.rs` | 存档落地时 |
| `dungeon-app/src/save.rs`（新） | `DSV2` 头/version 分发/bincode/路径；`App::handle` 比较 `FloorNumber` 触发自动存档 | 存档落地时 |
| `presentation/src/extract/mod.rs` + `render-api` `LookView` + tui | 尸体内容摘要进 look（建议） | UI 线 |

## 未能核实的线索（不要当结论用）

- Brogue 的存档实现与"没有上行楼梯"：只在 `src/brogue/IO.c:2629` 找到 `saveGame()` 调用，定义不在已读文件里；`Rogue.h:455-456` 同时有 `DOWN_STAIRS`/`UP_STAIRS`。⇒ 本调研没引用 Brogue 的层持久化结论。
- flecs v4 移除 snapshot 插件的**说明原文**没找到；证据只是 `include/flecs/addons/` 30 项里没有 `snapshot.h`。v3 的 `snapshot.h` 文档明确"快照与世界绑定、不能跨世界恢复、无序列化 API"，这足以否掉"用 flecs snapshot 做存档"的设想，但不代表 v4 有替代品。
- bevy_save 的 snapshot/Migrate API 只在 README 与 `RESEARCH-item-systems.md` §H 里读到，未在 bevy_ecs 0.16.1 上实际跑过。
- DCSS `monster_drop_things` 内部是否按怪物装备槽过滤未读（只看到调用点），所以"槽位顺序绑定存档"只限 `mon_inv_type` 枚举顺序，不扩展到掉落筛选逻辑。
- Caves of Qud 与 flecs/bevy 官方示例里的尸体回收 / 物品容器：本轮时间预算只够读 DCSS/NetHack 源码，**未读**这两处，所以第 1 节的"无先例"不等于"业界都不这么做"。
- 旧档 `GameSave.rng_state/rng_steps` 的恢复口径只读到代码，没跑过旧档验证；本文的新档方案不依赖它。

## 来源

| 来源 | 类型 | 一句话要点 | 链接 |
|---|---|---|---|
| DCSS `crawl/crawl`（`corpse.cc` / `mon-death.cc` / `mon-inv-type.h` / `files.cc`） | 源码 | 尸体=会腐烂的物品（非容器）；战利品与尸体两条路径；逐层持久化；装备槽枚举顺序是存档契约 | [链接](https://github.com/crawl/crawl) |
| NetHack `NetHack/NetHack` 3.6（`mkobj.c` / `mon.c` / `save.c` / `steal.c`） | 源码 | 尸体=带 ROT_CORPSE 计时器的食物；怪物背包由 `relobj` 掉地；每层一个 level 文件 | [链接](https://github.com/NetHack/NetHack) |
| flecs（v3.2.12 `snapshot.h`；v4 addons 目录） | 源码 | snapshot 纯内存、与世界绑定、无序列化 API；v4 addons 中已无 `snapshot.h` | [链接](https://github.com/SanderMertens/flecs) |
| bevy_ecs 0.16.1 `Entity` 文档 | API 文档 | 直接 `Serialize`/`Deserialize` `Entity` 不保证长期 wire 兼容 → 存档不得写 `Entity` | [链接](https://docs.rs/bevy_ecs/0.16.1/bevy_ecs/entity/struct.Entity.html) |
| bevy_save（hankjordan） | 源码 | 反射快照实现（需 `Reflect`+`MapEntities`+Prefab 投影）；本文据此否掉"整世界快照" | [链接](https://github.com/hankjordan/bevy_save) |
