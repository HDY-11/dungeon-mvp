# archive — 已归档的旧架构

这里存放**记录价值仍在、但已退出新代码依赖**的实现。归档 ≠ 删除：它们在
`archive/` 下**仍可编译、仍可测试**（保留在 workspace members 里），随时能回查
当年的实现细节；但**新方向不依赖它们，也不在其之上做任何新功能**。

| 目录 | 原位置 | 内容 | 入档理由 |
|---|---|---|---|
| `dungeon-core/` | `dungeon-core/` | 旧领域数据/工具（`Stats` 聚合组件、`ActionKindV3`、`ActionQueue`、物品/技能/存档等） | 业务领域已收敛到 `ecs_core`（DESIGN DsnX13） |
| `dungeon-action/` | `dungeon-action/` | 旧行动执行（`handle_player_direction`、中央 match 分派） | 行动已改为 action 子实体（REFACTOR §3.6 / Phase C） |
| `dungeon-world/` | `dungeon-world/` | 旧世界生命周期（`setup_world`、`advance_and_settle_parallel`、存档读档） | 世界循环已改由 `ecs_core::world_loop` + `dungeon-app` 装配层承担 |
| `dungeon-render/` | `dungeon-render/` | 旧渲染管线（后端直接查 `World`） | 渲染契约三层已就位（`render-api` / `presentation` / `tui`，DESIGN DsnX14） |
| `legacy-tests/` | `tests/`（Phase F） | 旧架构集成测试，**不参与编译** | 见 `legacy-tests/README.md`（SYN7） |

## 为什么归档而不是删除

- **行为对照仍需参考**：旧实现是 Phase C/D 六行动 parity 套件的原始口径来源；
  出现"怪行为变了"的疑问时，第一手资料就是这里的代码。
- **测试仍可跑**：归档时实测 `cargo test -p dungeon-core / -p dungeon-action /
  -p dungeon-world / -p dungeon-render` 全绿（19 / 5 / 15 / 6）。
  它们**只覆盖旧架构，不作为新代码的回归保障**，因此**不在 `scripts/gate.ps1` 内**
  （见 PROTOCOLS.md §五「为什么不覆盖全部」）。
- **删除是不可逆的动作，归档是可逆的**：`git mv` 保留了全部历史（`git log --follow` 可追）。

## 归档时的必要修补

移动目录会破坏 `..` 相对路径，因此一并修正（否则归档件立刻变成"编不过的死代码"，
等于事实上删除）：

| 位置 | 修补 |
|---|---|
| `dungeon-core/Cargo.toml` | `terrain-forge` 路径 `../terrain-forge` → `../../terrain-forge` |
| `dungeon-core/src/items.rs` | `include_str!` 的 `../../assets/items.json` → `../../../assets/items.json` |
| `dungeon-*/Cargo.toml` 之间的 `path = "../dungeon-*"` | 无需改（同层相对关系不变） |

## 何时可以真正删除

满足任意一条即可把本目录连同根 `Cargo.toml` 的 4 行 `members` 一起删掉：

1. 新架构完成了物品/装备/技能/存档四类迁移（REFACTOR §8 的"明确不迁入"清单清空），
   不再需要旧实现作对照；
2. 出现与归档件冲突的维护成本（例如旧依赖 `rand 0.10` 阻碍升级），
   且已确认无人再回查。

**在此之前不要再往里加东西**——归档目录只出不进。
