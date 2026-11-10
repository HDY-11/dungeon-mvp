# ITEMS — 物品 / 装备 / 武器 / 施法工具

> 状态：**草案（骨架）**；施法工具（法杖）部分与 RULES.md R1 绑定，等用户回答。
> 前提：items.json 里现有物品是测试占位，不是正典。

## 1. 硬约束（来自 DsnE27 / DsnE23 / DsnE18 / DsnE20 / DsnE24）

- 一律「蓝图 + parts」：`equippable / consumable / weapon / container / modifiers / behaviors`；不给 ItemDef 新增 Option 字段。
- 数值只走 `StatId` + `Modifier{stat, source, op, value}`；新属性=词表加一行（属代码改动，见 RULES 骨架）。
- 类别用 tags；装备槽位 tags：Head/Chest/Weapon/Shield/Feet/Ring/Pack（身体七部位：头/躯干/主手/副手/脚/手指/背包）。
- 品质六档；品质不改实例，只影响数值预算/成长 EXP 倍率/（未来）学派门槛。
- 武器成长是实例状态 `WeaponProgress`，蓝图给 `WeaponGrowth`。
- 行囊限重 20.0；只有行囊限重。
- 法术相关物品目前为空：无施法工具；法术书只是玩家可用的两招、不入档。

## 2. 物品模板

- ID / 名称 / 品质 / tags / 重量 / 堆叠
- parts 清单（含参数）
- 数值修正：stat + op + value（例：attack +3；max_health +3）
- 行为：trigger（OnUse/OnEquip/OnHit/OnTurn）+ effects（白名单内）
- 来源：怪物掉落 / 地面刷新 / 合成（未迁移）/ 商店（无）
- 叙事钩子 / 状态

## 3. 待设计清单（等世界观）

- [ ] 武器线：每档品质的「攻速/力量上限/成长曲线」预算（从 DsnE24 公式反推，不拍板）
- [ ] 防具线：防御 / 抗性 / HP 上限的取舍
- [ ] **施法工具（法杖）**：占槽、是否施法前提、品质如何给「额外 MP 投入上限」、是否影响吟唱时间/移速 → 阻塞于 RULES R1-Q1/Q2
- [ ] 消耗品：治疗/回蓝/状态解除/增益（当前只有蘑菇+6HP、海藻+4MP）
- [ ] 材料与掉落经济：材料用途（合成未迁移前先不承诺）
- [ ] 容器/背包升级
- [ ] 关键物品（钥匙/剧情物，可选）
- [ ] **三空间物品分组**：物质空间=科技装置 / 幻想空间=法器 / 亚空间=圣物与污染相关物品（按 CONTENT.md 的三层世界观）
- [ ] **科技线物品机制**：充能/弹药/过热？——DsnE27 已删耐久，需要新部件/组件（骨架预警，见 RULES.md R1.5）
- [ ] **精神线物品**：祝福/诅咒圣物（OnEquip 修正 + 污染代价；可能需要新 StatId 或新组件）

## 4. 物品条目

（空，等世界观）

## 5. 已拍板

（空）
