# 伤害模型：统一结算与开放边界（讨论记录，暂不实现）

> 状态：**契约草案，暂不落地**。记录 2026-11-13 关于"每域一个系统 / 标记组件 / 开域"的讨论结论，供后续实现参考。
> 触发：用户 m14244–m14263。当前代码仍保持三条独立管线（近战 `resolve_attack_system`、法术 `resolve_spell_system`、投掷 `resolve_throw_system`），本文不改代码。

## 0. 结论

1. **所有伤害走同一条策略路径**：物理、法术、投掷都产出 `DamageProfile`，交给同一个解析器，策略（无视护甲/无视抗性/增伤/暴击）由数据决定，不为每种伤害类型写 `if`。
2. **结算骨架封闭、策略数据开放**：阶段顺序由核心定义（攻击/基础值 → 增伤 → 暴击 → 护甲 → 抗性 → 收尾），内容通过策略字段与 hook 插槽参与；不开放任意域系统注册器。
3. **不做"每域一个系统 + 已处理标记"**：标记是第二套幂等机制，与 bevy 每轮事件读取的既有语义重复；并行收益微秒级、链路变长、测试变难。
4. **若未来确实需要 ECS 化的域系统，只做薄壳**：域系统 = "取 world、调纯函数、写 `DamageWork`"；公式留在纯函数里，测试主要打纯函数。
5. **失败/插入点**：新增伤害类型 = 数据行；新增策略维度 = 策略字段；新增"新计算阶段"才动核心代码（罕见，需设计评审）。

## 1. 当前实现（对照）

| 管线 | 入口 | 结算 | 策略位置 |
|---|---|---|---|
| 近战 | `system/combat.rs::resolve_attack_system` | `rules/damage.rs::melee` | 攻击/防御/力量上限/暴击/抗性 |
| 法术 | `spell/mod.rs::resolve_spell_system` | `spell::spell_damage` | 精通、魔法抗性；不吃护甲 |
| 投掷 | `throw/mod.rs::resolve_throw_system` | `rules/thrown.rs::thrown` | 固定基值/增伤/暴击/物理护甲/物理抗性 |

三者共享 `AttackEvent → apply_damage_system → DamageTakenEvent` 出口，但数值公式各自独立。

## 2. 目标形状

```text
攻击/施法/投掷/环境
      │  计算 base 与 DamageProfile
      ▼
DamageProfile { armor_policy, resist_policy, damage_mult, crit, ignored_sources, ... }
      │
      ▼
resolve_damage(profile, DamageWork) → 纯函数，按固定阶段折叠
      │
      ├─ 阶段 1  基础值（按伤害来源提供：attack+strength / amount+mastery / item damage）
      ├─ 阶段 2  增伤与条件修正（profile.damage_mult + 内容 hook）
      ├─ 阶段 3  暴击（rate/damage/roll）
      ├─ 阶段 4  护甲域（policy: ignore / scale / penetrate）
      ├─ 阶段 5  抗性域（policy: ignore / scale / kind-specific）
      └─ 阶段 6  收尾（下限 1、NaN 归一、写 DamageWork.stages 供日志/调试）
      ▼
DamageTakenEvent { source, target, amount, profile, stages }
```

### 2.1 `DamageProfile`（草案字段）

- `armor_policy`: `Ignore | Scale | ScaleBy(f64) | Penetrate(f64)`（穿透按 `armor_after = armor × (1 − p)` 或等价形式，实现时定一种）
- `resist_policy`: `Ignore | Scale | Kind(ResistKind) | Penetrate(f64)`
- `damage_mult`: 额外乘区（内容侧增伤，可为 1.0）
- `crit`: `Option<CritProfile>`（不暴击的伤害类型传 `None`）
- `ignored_sources`: `&'static [EffectSource]`（复用效果来源词表）
- `damage_type`: 未来引入 `DamageTypeId`（数据注册表）；在只做物理的第一层可缺省为 `Physical`
- `tags`: 未来用于元素/学派/条件判断的轻量标记（先不要，等第二类内容）

### 2.2 阶段职责

- 纯函数：输入 `DamageWork { base, profile, defense, resist, crit_roll, ... }`，输出伤害与逐阶段快照。
- 事件：现有 `AttackEvent` 保留（兼容攻击后效果/武器经验），新增/替换为携带 profile 的事件在实现时决定；不在此文档冻结。
- `DamageTakenEvent`：保持统一出口（仇恨、on-hit、死亡、日志都依赖它）。

## 3. 开放/封闭边界（重要）

| 内容 | 开/闭 | 说明 |
|---|---|---|
| 伤害类型（物理/火/冰/毒/精神…） | 开（数据） | 未来 `DamageTypeDef { name, armor_policy, resist_kind, damage_mult, ... }` |
| 策略字段（无视护甲/穿透/增伤/暴击） | 开（数据） | 物品/法术/武器蓝图引用 |
| on-hit 效果 | 开（数据） | 已有 `Behavior` + `OnHitEffect` 方向 |
| 条件乘区（背刺/眩晕增伤/潮湿） | 开（hook） | 管线阶段之间的修正系统，读世界状态 |
| 阶段顺序与合并规则 | 闭（核心代码） | 保证确定性、可模拟、可测试 |
| 动态"域注册器/插件" | 暂不做 | YAGNI；等第二类真实内容验证插槽是否够用 |

## 4. 实施切片（未来）

- D0：纯函数 `resolve_damage` + 数值/策略单测（不接线）。
- D1：近战接入 `DamageProfile`（`melee()` 退化为其物理特化）。
- D2：法术、投掷接入；三条管线删除重复公式。
- D3：hook 插槽（`SystemSet`），至少一个真实内容消费者（如潮湿增伤）验证。
- D4：伤害类型数据注册表（元素/魔法第二层前）。

## 5. 未决问题

1. `Penetrate` 的数学形式：先减护甲值还是乘 `(1−p)`？（影响曲线与平衡）
2. `AttackEvent` 是否直接携带 profile，还是新增 `DamageIntentEvent` 后废弃 `AttackEvent`？
3. 暴击在法术/投掷中的可用性：由 profile 决定（`crit: None` 即不可暴击）。
4. 条件 hook 的执行顺序与幂等：hook 只允许修改 `DamageWork`，不允许再发伤害事件。
5. 日志：逐阶段快照是否进 `EventLog`（只进调试日志，避免刷屏）。
