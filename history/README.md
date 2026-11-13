# history · 已归档文档

> **不是当前事实源。** 这里的文档已被后续决策取代、是某一时刻的只读快照，或是一次性研究记录。
> 保留它们的唯一理由是**可检索、可追溯「当时为什么这么想」**；核对当前行为请以代码、
> [GAME.md](../GAME.md)、[DESIGN.md](../DESIGN.md) 与各 crate 文档为准。

| 归档文件 | 原路径 | 归档原因 | 日期 |
|---|---|---|---|
| [AUDIT-data-2026-11-04.md](AUDIT-data-2026-11-04.md) | `ecs_core/AUDIT-data.md` | 一次性的数据一致性审计快照，文中自述「不代表当前状态」；结论已被 DsnE22/23/24/25/27/28 取代 | 2026-11-04 |
| [RESEARCH-item-systems.md](RESEARCH-item-systems.md) | `ecs_core/RESEARCH-item-systems.md` | 物品系统调研；用户判定其偏教程、非实战案例，建议已由 DsnE22/DsnE27 重新实现 | 2026-11 前 |
| [RESEARCH-containers-save.md](RESEARCH-containers-save.md) | `ecs_core/RESEARCH-containers-save.md` | 容器/存档选型研究；结论已落进 DsnE18/DsnE19，文档本身只作引用记录 | 2026-11 前 |
| [ecs-core-split.md](ecs-core-split.md) | `docs/ecs-core-split.md` | ecs_core 拆分研究；结论由迁移方案 v2 吸收，Step 3 已降级 | 2026-11-12 |
| [migration-baseline.md](migration-baseline.md) | `docs/migration-baseline.md` | M0 基线快照（30 组 / 669 passed）；P2b 后目标数已变，只作回退参照 | 2026-11-13 |

## 规则

- 归档只移动，不删除；移动用 `git mv` 保留历史。
- 引用归档文件时请写 `history/...`；当前文档（`docs/`、README 文档地图）只链当前文件。
- 想把某条归档结论复活，先检查 `DESIGN.md` 是否已有更新的决策（RULE §五）。
