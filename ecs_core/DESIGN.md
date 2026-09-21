> **⚠️ 修改前必须阅读或回忆 [RULE.md](../RULE.md)——它定义了本文档的维护规则和更新时机。**
>
> 每条记载一个设计取舍，含两个小节：
> - **决策**：当前选的方向和理由（罗盘）
> - **背景**：舍弃的方案和演化过程（护卫）
>
> 数值和游戏规则见 [GAME.md](../GAME.md)。

# 设计决策记录 —— ecs_core

**归属范围：** 领域规则引擎的决策：ECS 模型、行动/速度、地图、怪物、结算链路。

**编号：** `DsnE1`、`DsnE2`… 每个 crate 独立编号。
判据：决策**落点在哪个 crate 的代码/接口**里就归哪里；跨多个 crate 的分层/契约/路线决策留根目录。

---

### DsnE1 并行怪物决策（Schedule）

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

**原编号：** `DsnE1`（迁移前）

### DsnE2 碰撞图（Occupancy Map）独立于 Map

**决策**

两个关注点变化频率不同，分离为独立结构：
- `Map`：存 Tile 地形（`Wall`/`Floor`）——楼层生成后基本不变
- `OccupancyMap`：存每个格子被哪个实体占据——每次移动/攻击/死亡后都变化

合并到一个结构意味着每次更新都要复制地形数据。

**背景**

每次行动后全量重建 `rebuild_occupancy()` O(n) ≈ 800 格（40×20），开销 <1μs。增量更新容易漏边界条件（实体死亡、下楼、传送），维护正确性的心智负担远高于全量重建的成本。

---

**原编号：** `DsnE2`（迁移前）

### DsnE3 视野记忆双结构（MapMemory + VisibleMemory）

**决策**

两个独立的记忆结构，因为它们的数据类型、更新频率、生命周期不同：
- **MapMemory**：记录哪些格子曾经被看到过（boolean 数组）——渲染已探索区域的灰色墙壁/地板。只增不减（探索过的格子不会"遗忘"）
- **VisibleMemory**：记录最后看到的实体（glyph/color/位置）——渲染视野外但已知的实体。需清理已死亡实体（否则显示幽灵）

**背景**

如果合并为一个结构，cleanup 逻辑需要区分"地图记忆（不清除）"和"实体记忆（需清除）"，增加复杂性。视野外的实体在已探索区域以灰色显示。死亡实体自动清理。

---

## 二、核心机制

**原编号：** `DsnE3`（迁移前）

### DsnE4 行动系统设计哲学

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

**原编号：** `DsnE4`（迁移前）

### DsnE5 线程局部 RNG → GameRng 统一

**决策**

`GameRng` 成为唯一随机源。所有随机操作（仲裁、暴击、游荡、掉落、地图生成）统一走 `GameRng` 或基于 `MapSeed` 的派生 RNG。

**背景**

曾经有一个线程局部的 `RefCell<SmallRng>` 与 `GameRng` 并存，用于仲裁 system 中的随机选择——因为当时仲裁 system 无法访问 `GameRng` 资源。已由 ISSUES D1 解决。

---

## 四、输入与渲染

**原编号：** `DsnE5`（迁移前）

### DsnE6 MonsterTemplate 结构体统一 — 设计中，实验方向

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

**原编号：** `DsnE6`（迁移前）

### DsnE7 多类型地图：繁茂洞穴 + 地海（已定案并落地）

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
- 新掉落物 8 种（ID 25-32）：蘑菇/海藻为地形消耗品（r 键使用：+6 HP / +4 MP，上限钳制 [⃞试调]），其余为材料（DsnX12 Phase 2 合成储备）

**关联：** ISSUES G23/I70 | GAME.md Gm7/Gm10 | 生态对应原则（回复对称：繁茂回 HP ↔ 地海回 MP）

**状态：** 已落地。分支楼梯（多楼梯/树状分支）待类型系统稳定后单独规划。

---

**原编号：** `DsnE7`（迁移前）

### DsnE8 行动即实体 + 速度组件：AV 系统的 ECS 化（草案，待 PoC）

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

**状态：** ①② 均已落地（Phase B/C/D 完成）；I89（AV 门禁）与 I90（事件生命周期）已修；§11.6 已按推荐执行（逐行动迁移、倍率速度、先删反应时、`Wait` 固定、怪物速度先保行为）。① 见下方 Phase B/C 进展，② 见下方 Phase D 进展。

**进展（Phase A/B/C）：**

- Phase A：core 冒烟测试补齐（地图确定性 / 移动 / 攻击只结算一次 / 死亡→经验→升级 / FOV·记忆·占用图 / 快怪多动），`cargo test -p core` 从 4 → 18 个测试（ISSUES P9）。
- Phase B（action 实体 PoC）：`core/src/action/entity.rs` 落地 ① 的完整链路；测试覆盖生成 → 仲裁 → tick(`Ready`) → 执行 → completion，含优先级/平局/忙碌 actor/不写 actor 状态等契约。
- Phase C（全量迁移，C1–C9 完成）：六个行动逐行动迁移并各配 parity 场景；主循环切换到新链路（`world/loop_.rs` 改为「先挂载、再推进」两段式）；`ActionKind` / `mount_action` 中央 match / `finish_action_*` / `decide_monster_actions` / `choose_action` / `run_action_cycle` / 旧独占执行系统与 actor 上的行动 ZST 挂载路径**全部删除**，全库无 `ActionKind` 引用。
- 执行器形态（修正记录）：`execute_move_system` 一度写成 exclusive `&mut World`，理由是「action 实体 → actor 位置的多实体读写无法用普通 `Query` 表达」。**该结论是错的**：那只是因为复用了 `movement::execute_move(&mut World, ...)`——该签名把「读资源 + 读组件 + 写组件」揉进一次 `&mut World` 调用；而 Bevy 的 `Query<&mut T>` 只保证 **per-entity** 唯一可变访问，驱动实体（action）与被写实体（actor）不同，读写两处并无真冲突。把移动落点抽成纯函数 `moved_position(map, occupancy, pos, dx, dy) -> Option<Position>`（`can_move_to` 规则不变）后，执行器自然写成普通参数化系统。副作用是 A41 范围收窄：行动链路里已无 exclusive 系统，可与既有 `CoreSettleSchedule` 系统同调度共存（有测试断言）。
- 迁移期约束（写测试时要注意）：`Ready` 的清理分两条路（`execute_move_system` 显式清，其余靠 completion despawn 实体）；`Idle`/`Failure` 必须互斥（I91）；「挂载玩家行动」与「推进世界」必须分两段调度，否则会把「本轮已执行完」误判成「命令被拒绝」。
  > **该约束已在 Phase H1 取消**——`Ready` 的清理收进终态出口（见下方「Phase H1 进展」），
  > 上面这条保留为迁移期的历史记录。

**进展（Phase D：速度组件，D1–D5 完成）：**

- `Agility` / `agility_to_reaction` / `agility_speed_factor` / 旧 `action_av` 全部删除；AV 只剩 `AV = base_duration / clamp(速度, 0.25, 4.0)` 一个口径（`core/src/balance.rs`）。
- 行动类别 → 速度的映射收在 `SpeedRule`（`Move` / `Attack` / `Fixed`），由生成系统在挂载点决定；`ActorSpeeds` 是 AV 计算的纯输入，使公式可在无 ECS 的单测里逐条钉住。执行器完全不接触速度——AV 在挂载时就固化进 `ActionTimer`。
- 怪物模板不再保留任何「敏捷」字段：`MonsterSpeeds { move_speed, attack_speed }` 直接写字面速度值。迁移来源（旧敏捷 → 旧耗时系数的倒数）记在 `MonsterSpeeds::MIGRATION_NOTE`，数值表在 GAME.md Gm1。
- **迁移映射的选择理由**：新 AV 精确等于旧的「`duration` × 耗时系数」一项，丢掉的只有等量叠加的反应时常数项（已确认删除）。因此**角色之间的相对快慢与旧版完全一致**，这是「先按旧敏捷保行为」的可验证含义，由 `template_speeds_preserve_legacy_agility_ordering` 钉住。玩家初始速度 `1.25` 因此是迁移产物而非设计值，已在代码与 GAME.md 双处标注「重新校准时应改回 1.0 并重配平」。
- 顺带确立的新不变式（旧口径做不到）：`AV × 速度 == base_duration` 恒成立、AV 与 `base_duration` 严格成正比。旧式因为有常数反应时项，「300ms 的攻击」实际要付 310 AV、而「500ms 的游荡」付 470 AV——短行动被惩罚得更狠。删除该项是 Phase D 唯一的**有意行为改动**，由 `dropping_reaction_time_*` 的量化结论记录。
- **意外发现（值得记入教训）**：速度组件改变了各行动的执行轮次，于是怪物消耗随机数的时机随之改变，`system::tests::fov_memory_and_occupancy_update` 里「玩家选定的目标格在推进期间保持空闲」这条原本"碰巧成立"的假设立刻失效（怪物游荡到该格，命令被改判成攻击）。教训：**测试若依赖"没有别的实体碰巧走过来"，它就是在依赖执行顺序，而执行顺序正是本次要改的东西**——该类用例必须先清场再断言。
- 元组 `Bundle` 的元数上限：bevy_ecs 0.16 的元组 `Bundle` 只实现到 **15 元**（`all_tuples!(tuple_impl, 0, 15, B)`），玩家基础束原本已 16 个元素，直接加两个速度组件会编译失败。解法是用 `#[derive(Bundle)] struct Speed` 打包这两个组件——**打包不是新增组件**，实体上仍是两个独立组件，查询不变。这是「实体组件数」与「元组元数」解耦的通用手法。

---

**进展（Phase H1：行动终态收成单一出口）：**

H1 只碰**形状**，不改任何行为与数值。它解掉的是 Phase H 计划里记为 ECS35 / ECS36 的一对问题——
「行动终态只有约定、没有机制」：

| | H1 之前 | H1 之后 |
|---|---|---|
| 谁能结束行动 | 6 个执行器各自 `events.succeeded/failed.write(...)`（15 处裸写） | 只有 `ActionEvents::succeed` / `::fail`：字段私有，**执行器拿不到 `EventWriter`** |
| `Ready` 谁清 | `execute_move_system` 显式清，其余 5 个靠 completion `despawn` 顺带 | 终态出口**必然**清（与发事件是同一个原子动作） |
| 漏写终态事件 | **编译通过**，运行期 actor 永久卡 `Active` + 子实体泄漏 | **编译不过**（没有可用的写入句柄） |
| 测试能断言什么 | 6 个 parity 用例里只有 `Move` 能断言 `Ready` 被消费 | 6 个全部能断言（共用 `run_one_action_roundtrip`） |

两个实现要点：

1. **私有字段是这里的机制本体**，不是封装洁癖：它把「恰好一个终态事件」从注释级约定
   变成类型级保证，因此不需要"兜底检测系统"这类补丁。
2. **`SystemParam` 带两个生命周期**：`Commands<'w, 's>` 在 bevy 0.16 里用 `'w` 借
   `Entities`、`'s` 借延迟命令队列，所以 `ActionEvents` 必须写成 `ActionEvents<'w, 's>`
   （第一版写 `Commands<'w, 'w>` 直接编译失败：`SystemParam` 的 `'s` 无法满足）。

**变异验证（断言真的在守不变式）：** 把终态出口里的 `remove::<Ready>()` 去掉后，
**7 个用例立刻失败**（6 个 `parity_*` + `parameterized_move_matches_world_based_move`）。
这条验证是必要的——一条永远为真的断言比没有断言更糟，它会把缺口伪装成已覆盖。

**关联：** REFACTOR.md §11.3 Phase H（H1）；ISSUES ECS35 / ECS36（均已修复）。

**进展（Phase H3：调度按角色分组）：**

H3 把 `build_action_poc_schedule` 从「13 个系统一条 `.chain()`」改成
**五个有名字的角色 + 一条角色链**，解掉的是 `LECS22` 里"顺序协调的中央点"那一类扩展成本：

```rust
#[derive(SystemSet)] pub enum ActionPhase {
    Generate,   // 只 spawn 候选 / 挂载玩家行动
    Arbitrate,  // 唯一写 actor 行动状态的系统
    Tick,       // 推进 ActionTimer，归零加 Ready
    Execute,    // 按行动类型执行的专用系统集合
    Complete,   // 回收 action 实体 + actor 回转 Idle/Failure
}
```

| | H3 之前 | H3 之后 |
|---|---|---|
| 加一个执行器 | 在 13 项长链里找位置，还要确认它与前后系统的顺序 | 往 `ActionPhase::Execute` 加一行 |
| 加一个生成器 | 同上 | 往 `ActionPhase::Generate` 加一行 |
| 顺序定义在哪 | 隐含在一条长元组的**书写顺序**里 | `ActionPhase` 的全序 + 一处 `.chain()` |

**为什么不是"合并成一组、让调度器自由排序"**：五个角色之间是**数据依赖**
（候选 → 仲裁 → 计时 → 执行 → 终态事件），自由排序会破坏语义。
`LECS22` 的结论在这里具体化为：ECS 消除了**类型分发的**中央点（`With<A>` 取代 `match`），
但消除不掉**顺序协调的**中央点——所以正确的目标是"加第 N 个只改一处"，而不是"零处"。

**`ApplyDeferred` 的位置不能省**：生成器用 `Commands` spawn 候选、仲裁用 `Query` 读；
执行器写终态事件、completion 用 `EventReader` 读。分组后这两处落盘必须逐处保留。

**测试怎么证明"分组没改行为"**：新增 `action_phases_keep_their_total_order_and_execute_group_is_open`——
每个角色**自己记录**有没有跑（读调度图只能证明"登记了"，记录才能证明"按这个顺序真的跑了"），
并向 `Execute` 组追加一个扩展系统，验证它能直接接进链路、且整轮照常执行完并回收实体。
这条用例同时是 H10（技能三层骨架）接法的样板。

**关联：** REFACTOR.md §11.3 Phase H（H3）；LESSONS LECS22（扩展点判据）。

**进展（Phase H4：`TileProps` 静态属性表）：**

地形的种类与属性收进 `map/tile.rs` 的一张表（`TILE_PROPS: &[TileProps]`），
`glyph` / `walkable` / `blocks_vision` / `Serialize` / `Deserialize` **全部读表**，
判别值改由 `#[repr(u8)]` 决定——**没有一处 `match` 在描述地形**。

| | H4 之前 | H4 之后 |
|---|---|---|
| 加一个地形要改 | `glyph` / `walkable` / `blocking` / `From<u8>` / `Into<u8>` /（以及 `presentation::tile_id`）**六处** | `Tile` 末尾 + `TILE_PROPS` 末尾，**两处**（枚举无法自动派生） |
| 新属性（移动代价）的落点 | 没有，只能继续往 5 个 match 里加 | `TileProps` 加一列 |
| 漏加一项的报错时机 | 编译期（穷尽 match） | **运行期** → 由穷举测试补回 |

**换表的代价必须用测试补回来**（`LECS22` 的配套纪律）：`table_covers_every_variant_and_round_trips`
逐行枚举全表并断言"表行 = 枚举顺序 = 判别值 = 序列化往返"，另加
`ids_match_the_pre_h4_serde_mapping` / `properties_match_the_pre_h4_values`
逐值钉住搬迁前的口径（防止抄错一格）。

**`move_cost` 是 H8 的预留位**：本轮只加列、恒为 1.0、**不接线**——
移动 AV 仍只由 `MoveSpeed` 决定。`move_cost_is_still_a_reserved_slot` 守住这一点，
防止"顺手填值"绕过 GAME.md（数值口径待定，见 DESIGN DsnX16）。

**变异验证：** 交换 `Tile::Wall` 与 `Tile::Floor` 的声明顺序 → 20+ 个用例失败
（地图/移动/视野都读地形身份）。这既是"判别值受保护"的证据，也说明
**枚举声明顺序是领域事实**，不是可以随手整理的排版。

**关联：** REFACTOR.md §11.3 Phase H（H4）；ISSUES ECS31（已修复）、ECS27（`move_cost` 的来处）；
DESIGN DsnE13。

---

**原编号：** `DsnE8`（迁移前）

### DsnE9 伤害计算的输入接口：从标量参数组到结构化输入

**决策**

`combat::compute_melee_damage` 的**签名结构**改为结构化输入，**但公式与数值一律不变**（仍是 `max(攻击 − 防御, 1)` 与现有暴击逻辑）。本条目只定**接口形状**，不产生任何游戏数值。

**背景**

当前签名是：

```rust
pub fn compute_melee_damage(
    attack: f64, defense: f64, crit_rate: f64, crit_damage: f64, crit_roll: f64,
) -> MeleeResult   // { damage, is_crit }
```

这个形状**恰好表达"减一次防、乘一次暴击"**：每新增一个伤害修正都要改签名 + 改所有调用点，而且返回类型是黑箱（只有最终值与是否暴击，没有各因子的分解）。

**决策内容（只定形状）**

1. **输入分组**：参与伤害计算的量按"来源"分组传入，而不是一串裸标量。至少要能区分：**攻击方数值**、**受击方数值**、**随机输入**（`crit_roll` 一类）。
2. **返回分解**：返回结果携带**各因子的值与最终值**，而不只是最终值。理由有三个真实消费者：战斗日志要能解释"为什么是这个数"、调试面板、以及未来"按因子触发"的效果。
3. **随机数仍在系统层取**：`crit_roll` 由调用方从 `GameRng` 取（沿用现有做法），公式保持纯函数、可单测。

**明确不做**

- **不新增、不修改任何公式与系数**。分区（有哪些因子、各因子叫什么）与系数（防御系数等）**全部待定，见 GAME.md**——本条目只保证这些因子**有位置可放**。
- 不引入"伤害类型"等新概念；那属于内容设计。

**为什么现在做**

伤害公式是**承重层**：装备、Buff、技能、生物范畴的固有性质最终都要落到这个函数的输入或因子位置上。先让形状能容纳"多因子"，后续内容设计就不必再改接口。

**关联：** ISSUES ECS29（现签名无扩展位） | REFACTOR.md §11.3 Phase H（H2） | GAME.md（分区与系数，待定）

**状态：** 待落地（Phase H）。

---

**原编号：** `DsnE9`（迁移前）

### DsnE10 规则修正器：装备/状态/地形如何影响纯函数规则

**决策**

引入"**规则修正器**"这一层：**修正器是纯函数的额外输入，不是实体、也不是组件**。

**背景**

`movement.rs` 的分层卖点是**纯规则不碰 `World`**：`can_move_to` / `moved_position` 只吃「位置 + 方向 + 地图/占用图引用」。但装备（如"无视某类地形减速"）要求规则知道**移动者是谁、带了什么**——若直接给纯函数加 `&World` 参数，就退回 `ECS2` 所批评的形态（把读资源与读组件揉进一次调用），并丢掉可单测性。

**决策内容**

1. **修正器在系统层求值，纯函数只吃结果。** 既有范例是 `ActorSpeeds`：`actor_speeds()` 在系统层查 `MoveSpeed`/`AttackSpeed`，`action_av` 只吃一个纯数据 `ActorSpeeds`。**新修正器一律照此形态。**
2. **修正器预留两类位**（这是形状，不是内容）：
   - **基础值修正**：改变某个数值本身（加/减）；
   - **乘区修正**：作为独立因子相乘。
3. **读取方式是 pull，不是 push。** 装备/状态的效果在**规则求值时查询**，而不是在装备时往 actor 上插组件。理由：push 要求"穿上/脱下/丢弃/被偷"每条改变途径都对称地插入与移除组件，**漏一条就是静默残留**；pull 只付一次查询。
4. **内在能力仍用 push（`Can*`）。** `Can*` 是实体固有属性，生命周期与实体一致，不存在不对称问题——**两条路并存，判据是"能力是固有的还是可授予的"**。

**地形减速的归属**

`DsnE8` 已记「重甲/地形 → `MoveSpeed`」，本条目确认该方向：**地形减速作为速度修正进入 `MoveSpeed` 那条链**，而不是给 `ActionTimer` 加独立代价项。这样与 Phase D 的"倍率速度"模型一致，且"无视某类地形减速"退化为"忽略某个修正来源"。

> **待定（见 GAME.md）：** 具体地形减速的**数值口径**（修正速度还是修正时长、系数取值）属于游戏数值，本条目不定。

**关联：** ISSUES ECS28（移动规则无地形代价维度） | REFACTOR.md §11.3 Phase H（H8） | DsnE8（地形 → MoveSpeed）

**状态：** 待落地（Phase H）。修正器接口先定型，具体修正来源随内容补齐。

---

**原编号：** `DsnE10`（迁移前）

### DsnE11 技能的三层结构：激活 / 委派 / 行为

**决策**

技能**不是**"一个技能一种系统"，也**不是**"一套数据框架描述全部效果"，而是三层，每层用不同机制：

| 层 | 承担什么 | 机制 |
|---|---|---|
| **① 激活** | 触发条件（范围计数、阈值、状态旗标） | 数据；闭合的小词汇表 |
| **② 委派** | 这一次施法立刻发生什么 | **每个技能一处代码**（生成 + 执行同处） |
| **③ 行为** | 需要持续发生的事 | **委派给独立实体 + 它自己的系统** |

**背景与理由**

- **为什么不是"一个技能一种系统"**：每加一个技能要写组件 + 生成器 + 执行器 + 在中央 schedule 注册，成本随技能数线性增长，且中央注册列表会变成新的 `ActionKind`。
- **为什么不是"一套数据框架"**：曾考虑用闭合操作词汇表（`Damage`/`Heal`/`Buff`/`Spawn`…）描述全部技能效果。该方案**在"持续行为"上失效**——例如"自身死亡并把格子变成随时间逐个生成生物的区块"，无法用数据描述，因为它的效果一部分是**之后持续发生的事**。
- **为什么③必须委派**：持续行为有自己的时间轴、生命周期与终止条件（可被特定效果解除）。把它塞进技能数据表就会把数据表变成行为解释器。

**关键原则**

> **技能负责"点火"，不负责"烧多久"。** 复杂效果的载体是**实体**，不是技能数据里的字段。

**技能作为子实体**

技能实例（这一次施法）挂在**瞬态子实体**上，复用行动实体的链路与生命周期：归属（玩家/怪物/装备授予）由"挂在谁身上"决定，用完即 `despawn`，中间状态自动消失。

**与 `Can*` 的关系（待定）**

`Can*` 目前是 actor 上的 ZST 能力标记，且**能力散在 spawn 代码、数值在模板**（见 ISSUES ECS34）。技能属"可授予"（卷轴学习、装备授予）还是"固有"，决定技能走 `Can*` 组件集合还是技能表 + 统一读取入口——**待查 `CanBasicAttack` 与 `Attack` 是否一一对应后再定**（见 REFACTOR.md Phase H 待定项）。

**关联：** ISSUES ECS31/ECS34 | REFACTOR.md §11.3 Phase H（H10/H12） | DsnE12（长期效果的载体）

**状态：** 待落地（Phase H）。本轮只搭骨架 + 一个 dummy 技能走通，**不含任何真技能**。

---

**原编号：** `DsnE11`（迁移前）

### DsnE12 世界级效果实体：长期效果的载体

**决策**

需要"活一段时间"的效果（状态效果、逐次生成的区块、可被特定手段解除的地块效果）**spawn 为独立的世界级实体**，由**类型化 `Query` 发现**、由**恰好一个 owner 系统**管理终结。

**背景**

技能三层结构（DsnE11）的第③层需要载体。实测证据：`ChildOf` 在 bevy 0.16 是 `linked_spawn`（`#[relationship_target(relationship = ChildOf, linked_spawn)]`，`bevy_ecs-0.16.1/src/hierarchy.rs:156`），**父实体 despawn 会级联 despawn 子实体**。因此"自杀并把地块变成生成器"这类技能，其效果实体**不能是施法者的子实体**——否则会在生成的同一瞬间被级联删除。

**决策内容**

1. **不用关系型归属**（`ChildOf` 或自定义关系），用**组件 + 类型化 `Query` 发现**。理由：行动链的候选查询 `Query<(Entity, &ActionPriority, &ChildOf), (With<Candidate>, Without<ActiveAction>)>` **接受 actor 的任意子实体**——用关系挂载持久实体会污染行动仲裁（见 ISSUES ECS30）。
2. **位置索引照 `OccupancyMap` 的形态**：`[[Option<Entity>; MAP_WIDTH]; MAP_HEIGHT]` + 一个维护系统（既有范例 `rebuild_occupancy_system`）。理由：整张地图实体化会让每格读取从数组索引变成哈希/查询，而"格子属性"只需一个**统一读取入口**，不需要统一存储。
3. **每类效果恰好一个 owner 系统**负责它的全部终结路径（过期、被解除、被替换）。其他系统只能改它的数据，不能 `despawn` 它。既有范例：`action_completion_system` 是**唯一**允许回收 action 实体的地方。
4. **效果必须可寻靶**：效果实体携带 tag，解除类效果（如火焰）以"对范围内的 `<tag>` 生效"表达，**不得在结算链路里写"若是火焰且是某效果则删除"的特例分支**（同类教训见 `EntityClass::Item` 的恒假判断）。
5. **存档可发现**：持久效果必须能被存档统一枚举写出——这是"放世界实体空间"而非"各自存在子系统私有结构里"的主要理由。**技能实例（瞬态子实体）不存档**，照 §10.7「行动实体不存档」的既有惯例。
6. **消费随机数的效果系统必须显式排序**：`GameRng` 的确定性与 `steps` 语义是回放/存档基础，Bevy 系统顺序默认未指定。

**明确不做**

- 不定义任何具体效果类型（状态效果/生成器/地块效果都只是**位**）。
- 不决定时长单位口径（AV 或秒）——见 GAME.md 待定。

**关联：** ISSUES ECS30 | REFACTOR.md §11.3 Phase H（H9/H11） | DsnE11（三层结构第③层） | `OccupancyMap`（既有范例）

**状态：** 待落地（Phase H）。本轮只做一个最小效果走通，验证机制。

---

**原编号：** `DsnE12`（迁移前）

### DsnE13 格子属性的身份判据：索引还是实体

**决策**

判断某个"格子相关的东西"该用**索引**还是**实体**，判据是**它是否需要独立身份**：

> **需要独立身份**（能被单独引用、单独寻靶、单独销毁、能被玩家指认为"这一个"）→ **实体**
> **不需要**（同种类的格子可以互换）→ **索引**（数组/网格）

**背景**

讨论中出现过"把地块实体化、用组件表达概念"的方案。该方案**原理可行**（不少引擎如此），但对本项目不划算，因为实测事实是：

- 地图网格是**承重的**：`can_move_to`、`rebuild_occupancy_system`、`map_gen.rs`、`spatial/`（FOV/LOS/A*）、`presentation` 的 `extract_scene_frame` 都按数组索引读它；
- `Tile` 的 u8 序列化（0..10）**是已冻结的存档契约**（只能末尾追加）；
- 全地图 80×60 = **4800 格**，其中绝大多数是同种类的可互换格子。将"种类数据"复制 4800 份属于重复表示（同类问题见已删除的身份 ZST `Rat/Scorpion/...`）。

**决策内容**

1. **地形种类 = 数据**。`Tile` 枚举**保留为种类键**，其属性（可走/挡视线/glyph/移动代价）收敛到**一张静态属性表**，经单一查表函数访问——照 `MonsterTemplate` 的既有形态。**理由**：H4 之前加一个 `Tile` 变体要改 5 处 match（`glyph`/`walkable`/`blocking`/`From<u8>`/`Into<u8>`），属中央分派债（ISSUES ECS31）。
2. **格子级运行期效果 = 实体**（DsnE12）。"浅水"是 11 种之一（需要一份定义），"这一格被冻结了"才是格子独立状态（需要一个实体）。
3. **格子属性有统一读取入口**。地形属性与效果实体是**两个数据来源，一个答案**——由单一函数给出"这个格子的属性是什么"，规则层不必知道数据在哪。
4. **属性表让"网格是否改成组件"这一决定保持便宜**：一旦属性与枚举解耦，将来若要支持多层/多地图，只动存储与访问层，不必再动属性定义。

**明确不做**

- 不改 `Map` 的网格表示；不改 `Map` 是资源还是组件（**仅当要做多层/多地图时才有收益**，见 REFACTOR.md Phase H 待定项）。
- 不新增任何具体地形属性（移动代价的数值属于 GAME.md）。

**关联：** ISSUES ECS27/ECS28/ECS32 | REFACTOR.md §11.3 Phase H（H4/H9/H14） | DsnE12（效果实体）

**状态：** 第 1 项（静态属性表）**已落地**（Phase H4，见 DsnE8「Phase H4 进展」）；第 2–4 项待落地（H9 / H11）。

- ✅ **第 1 项已实现**：`ecs_core/src/map/tile.rs` 的 `TILE_PROPS` 就是本决策说的那张表，
  `Tile::props()` 是唯一查表入口；`glyph` / `walkable` / `blocks_vision` / serde 读写全部读表，
  加一个地形从"改六处"变成"改两处"（ISSUES ECS31 已修复）。
- ⏳ **第 2–3 项待做（H9）**："格子属性统一读取入口"要从"地形属性"扩展到"地形 + 效果实体"，
  届时规则层调用的仍是同一个入口，只是背后多一个数据来源。
- ✅ **第 4 项的前提已就位**：属性表已经与枚举解耦，将来改网格存储不必再动属性定义。
