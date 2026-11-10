# AUDIT-data · 数据一致性审计（task-2 / data-verifier）

> **基线**：HEAD `806c379`（「背包页能装/卸/用/丢……R2 完成」）+ 工作区未提交改动（Lead 并行落地 DsnE18 第②片尸体容器：`item/container.rs`、`item/mod.rs`、`system/death.rs`、`monster/template.rs`、`monster/mod.rs`、`world/floor.rs`、`presentation/src/extract/mod.rs` + `GAME.md`/`DESIGN.md`；快照 2026-11-04 23:05）。行号会随并行编辑漂移，请按关键词复搜。
> **范围**：GAME.md Gm4/Gm7/Gm8/Gm10 ↔ `item/registry.rs`、`item/container.rs`、`monster/template.rs`、`item/loot.rs`、`system/death.rs`、`world/init.rs`、`action/entity.rs`、`balance.rs`。
> **实测**：`cargo test -p ecs_core`（HEAD=218 pass；工作区=223 pass/0 fail）；`cargo run --release -p dungeon-app --example playthrough` 6 种子 `[2026,1,2,3,7,11]`。按要求未跑 `scripts/gate.ps1`。
>
> **历史快照（2026-11-04）**：本文是当时审计的事实记录，不代表当前状态。此后 DsnE22/23/24/25/27/28
> 等轮次已改动物品 schema、存档、投掷、HP/MP 曲线与首层节奏；例如 D3 的「投掷未迁移 /
> `ItemKind::Material`」与 D4/D5/D6 的部分结论已过时（投掷已落地 DsnE28、`ItemKind` 已退役
> DsnE27、消耗品与背包页已迁移）。核对当前行为请以 `GAME.md` 与代码为准。

## 一、最高优先级

### P1（代码 bug，建议改代码）死亡同轮升级会把尸体「回满血」
- **事实**：`system/mod.rs:87-88` 结算链是 `death::check_death_system → experience::apply_exp_system`；`system/death.rs:58-62` 玩家死亡只置 `game_over=true`、不 despawn、不清 HP；`system/experience.rs:44-55` 的 `Query<ExpPlayer, With<Player>>` **无存活过滤**，升级时 `health.current = max_hp`。
- **实测**：`seed 2026` 打印「结束原因 死亡（第 104 回合）… HP 38/38」（38 = Lv2 `max_hp_for`，`balance.rs:12`），受伤害 28 与满血自相矛盾；其余 5 个种子为 HP0/33 或 HP0/38。
- **影响**：玩家可见「死了却满血」；死亡后仍发升级事件/HUD 状态矛盾；将来存档会把满血死人写进盘。
- **建议**：`apply_exp_system` 跳过 `!health.is_alive()` 的玩家（或 game_over 时跳过），补「同轮先死后升级不回血」回归测试。

### P2（数据分叉，建议改代码对齐文档）每层物品数与踏水靴数量
- **事实**：`GAME.md:657` 每层物品 **4-8 随机**；`GAME.md:348` 踏水靴 **每层 1 件**。代码 `world/init.rs:514-521`：`pool_count=1..=3` 从 9 件池随机 + `石子 1..=3` → **生成 2-6 件**（结构上不可能 7-8，可低至 2）；靴子只在 `GROUND_ITEM_POOL`（`init.rs:476-486`，9 件）里随机抽，无保底、可为 0、可重复。`ITEM_SPAWN_RADIUS=10`（`init.rs:416`）与文档「出生点 10 格内」一致。
- **实测**：6 种子开局地面实值（打印值-1：`count::<Item>` 含玩家背包里的行囊，`playthrough.rs:96/:108`）= `{4,6,4,2,4,4}`；seed3=2 低于文档下限，6 局无一局到 8。
- **影响**：玩家可见的装备/消耗品供给低于文档；6 局 `equipped_peak` 全 0/7，装备成长路径几乎不可达。
- **建议**：按 RULE §八④「默认不倾向改文档」，优先改 `init.rs` 生成器（池 3-5 + 石子 1-3，或直接 4-8），靴子单独保底 1 件；若接受新行为再改 GAME.md:657/:348。

### P3（文档自相矛盾 + 实现分叉，需 Lead 拍板）移动耗时不受主手武器 speed 影响
- **事实**：`GAME.md:37`「移动=武器 speed（无武器 300）」、`GAME.md:399`「玩家移动/攻击的耗时由主手武器 speed 决定」；但 `action/entity.rs:678-679` 真实移动 duration 恒为 `UNARMED_ATTACK_DURATION`(300)，`:688-693` 只有 `BasicAttack` 才用 `main_hand_weapon_speed`，注释自认「移动仍用 UNARMED_ATTACK_DURATION……把移动交给移速是有意改动」；`GAME.md:55-57` 又写「移动类读 MoveSpeed」→ 文档内部已互相矛盾。
- **实测**：移动 AV 恒 300/1.25=240；攻击 AV = 匕首 160 / 锈铁剑 240 / 石锤 360。
- **影响**：武器只改攻击频率、不改变移动性；「拿匕首跑得快」不存在。
- **建议**：按代码注释口径改 GAME.md:37/:399（移动只受 MoveSpeed），保留 :55-57 作为唯一口径；不要改代码（移动走 MoveSpeed 桶是 DsnE11 的设计）。

## 二、文档 ↔ 数据表不一致（除注明外均建议改文档）

| # | 事实 | 建议 |
|---|---|---|
| D1 | GAME.md:370-395 物品表 24 行；`item/registry.rs` ITEMS **23 行**：文档缺 id4 皮帽 / 40 踏水靴 / 41 旅者行囊，代码缺 19-22 模板碎片 | 补文档 3 行；19-22 标⬜未迁移（或删表行） |
| D2 | GAME.md:246-255 装备加成表缺 皮帽（+1 修正，`registry.rs:189-205/:263`）与 旅者行囊 | 补 2 行 |
| D3 | GAME.md:381 石子槽位写 Consumable；`registry.rs:320-327` 是 `ItemKind::Material`（投掷未迁移） | 改文档 |
| D4 | GAME.md:464-466 消耗品节标「⬜未迁移」；实际已实现（`registry.rs:352-360` 蘑菇 Heal(6)、`:416-423` 海藻 RestoreMagic(4)，`item/mod.rs:203` 自动使用、`:377/:392` 满则不用；`item_tests.rs:169/:187` 钉住），:338-340 也声称已迁移 | 改文档为已迁移 |
| D5 | GAME.md:478 称模板碎片/材料消耗已落地；但 :15/:341 声明合成未迁移、19-22 不在注册表 | 改回未迁移 |
| D6 | GAME.md:19-20/:341 称「背包界面仍属 R2、d/u/r 未迁移」；HEAD 806c379 已实现（`presentation/src/extract/mod.rs:146,161` inventory_view、`tui/src/render/mod.rs:97,477,517`；GAME.md:480-486 自身已写 e 键装卸用丢） | 删「仍属 R2」 |
| D7 | GAME.md:18/:191/:339 写「四个装备槽位」；:219-231 实为七部位（身体=容器） | 统一措辞 |
| D8 | GAME.md:438 哥布林掉模板碎片 4 种各 5%；`monster/template.rs:296-304` 的 GOBLIN_LOOT 无该条目（且 19-22 未注册） | 随 D5 删/标未迁移 |
| D9 | 匕首双源：`template.rs:437` 哥布林尸体必带匕首 + `template.rs:303` GOBLIN_LOOT 仍 12% 匕首；GAME.md:437（12%）与 :492（尸体必带）各写一半 → 实际≈100%+12% | 二选一并同步文档（建议装备只走尸体则删 12% 匕首掉落） |
| D10 | GAME.md Gm8 只有鼠/蝎/哥布林（:505-537）；实际 8 物种（`template.rs:337-577`），且 :737 称「新怪数值见 Gm8」 | 补孢子怪/蘑菇傀儡/洞穴鱼/洞穴蟹/深鳗 5 表 |
| D11 | GAME.md:353/:788 仍写 `EquippedBy` 关系/子实体；代码已退役（`item/mod.rs:14` 明说「没有 EquippedBy」，grep 仅剩注释） | 改为 BODY 容器口径 |
| D12 | Gm10 自相矛盾：:687 浅水 ×1.5 vs :706-707「地形表那一列仍全是 1.0」；`effect/cell_tests::terrain_move_costs_are_pinned_per_tile` 钉住 1.5 | 删 :706-707 后半句 |
| D13 | 蝎子名：GAME.md:516 标题「蝎子」vs `template.rs:372-375` name「变异蝎子」（能力集 :564 用「变异蝎子」） | 统一 |
| D14 | `item/registry_tests.rs:14-47` 名为 `every_item_row_matches_game_doc`，实际硬编码 23 条手写清单（:37 注释承认 40/41 不在 GAME.md）→ 无法发现 D1 | 改测试名/注释，或从 GAME.md 解析表格做机械校对 |

## 三、交叉不变量审计

- ✅ 装备↔槽位：9 件带槽位标签的物品各恰好 1 个身体部位（`item/container.rs:172-183` Only 标签；`registry_tests.rs:226-252`）。
- ✅ 7 个 ItemTag 与 7 个身体槽位一一覆盖（`container.rs:51-66/:172-183`；`registry_tests.rs:117-127`）。
- ✅ `max_stack==1 ⇒ 装备或容器`（`registry_tests.rs:75-91`）；堆叠上限 64（10-14/26/28/30/31）、20（18/25/32）、9（27/29）与 GAME.md 一致。
- ✅ 编号唯一/可反查；武器 speed 与 Weapon 标签同真同假（`registry_tests.rs:51-64/:96-115`）。
- ✅ 掉落常量 8 物种与文档一致，唯一例外见 D8/D9。
- ⚠️ 「容器内物品不得带 GroundItemAt」只靠 `spawn_*`/pickup 路径保证（`item/mod.rs:9,23`），没有枚举全世界的测试；建议补一条不变量测试。
- ⚠️ `count::<Item>` 会把背包里的行囊与尸体容器里的物品计入（`playthrough.rs:96`），用 Item 计数当「地面物品」会多算；建议统一用 `count_ground_items`。
- ✅ 下楼保留玩家/装备/状态、清除尸体及内容（`world/floor.rs:62-132`；`floor_tests.rs:268-298`）；与 Gm12 一致（除 D11 措辞）。

## 四、实测结果（2026-11-04 23:02–23:05）

- `cargo test -p ecs_core`：HEAD 806c379 → 218 passed；工作区（含尸体容器改动）→ **223 passed; 0 failed**。日志 `target/ecs-test-full.log`、`target/ecs-test-2.log`。
- `cargo run --release -p dungeon-app --example playthrough`：EXIT 0；6/6 种子死亡（回合 104/154/70/20/97/118）；每局 0 命令拒绝；`equipped_peak` 全 0/7；`bag_peak` 1/3/2/0/1/2；中毒 tick 0/6/7/6/6/5；存活怪 15/11/8/8/8/12。
- 异常仅两类，均为 example 驱动器误报（非游戏 bug）：(a) 占用图「角色=其他」= 新 `Corpse` 实体（`system/occupancy.rs:16-17` 只登记 `Blocks`；`playthrough.rs:592-601` role 不识别 Corpse）；(b) 「玩家站在不可走格 (Wall)」= 下楼后用旧层 pos 检查（`playthrough.rs:150/:275`）。建议修 example 判据（排除 Corpse、换层后重取 pos），否则回归信号被淹没。
- 物品数结论见 P2。

## 五、平衡性可证伪样例

- **8 攻 vs 4 防**：基础伤害 `max(8-4,1)=4`；暴击 5%×1.5 → 4 或 6，期望 4.1。哥布林 F1 18HP → ETK=5（与 GAME.md:200 的 ETK≈5 一致）；老鼠 10HP → 2 刀。
- **怪物打玩家（防 4）**：鼠/蝎/孢子怪/鱼/蟹=1，哥布林/鳗=2，傀儡=3 → 1v1 玩家显著占优（哥布林需 17 刀换玩家 5 刀）；实测死亡来自群怪+毒（6 局受伤害 28-50，毒 tick 0-7）。
- **武器 AV/DPS**：匕首 160/(10-def)、锈铁剑 240/(11-def)、石锤 360/(12-def)；对全部 8 种怪防 DPS 排序恒为 匕首>锈铁剑>石锤（对哥布林 0.0375/0.0292/0.0222）→ 三档武器不构成权衡、石锤严格劣势；GAME.md:407 自认「未精调」。建议调石锤 speed/攻击，或给锤加破防/眩晕（需数值决策）。
- **装备可达性**：每层 2-6 件且池中含 2 件消耗品，6 局 playthrough 零装备拾取（`equipped_peak 0/7`）→ P2 不只是文档问题，也压低玩家可见的装备成长。

## 六、门禁缺口（给 Lead）

- `scripts/docs_check.py` 只机械检查交叉引用/地形表行数/标记配对/写 Position 白名单/派生 RNG（:4-11、:126-141），**不查** 物品表↔ITEMS、怪物表↔MonsterKindId、掉落表↔`*_LOOT` → D1/D8/D10 这类偏差不会被 gate 抓到。建议加两条枚举计数（GAME 物品表 vs `ITEMS.len()`、Gm8 怪物小节数 vs MonsterKindId 变体数）。
