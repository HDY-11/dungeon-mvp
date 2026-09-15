# legacy-tests — 已归档的旧架构集成测试

这里存放的是**针对已作废 `dungeon-*` 架构**的根 crate 集成测试，从 `tests/` 移出：

| 文件 | 原位置 | 失效原因 |
|---|---|---|
| `throw_test.rs` | `tests/throw_test.rs` | `use dungeon_tui::throw::auto_equip_throwable` —— `dungeon-tui` 已不存在 |
| `scenario_test.rs` | `tests/scenario_test.rs` | 依赖 `dungeon_action` / `dungeon_render` / `dungeon_world::setup_world` / `advance_and_settle_parallel` 等旧架构 API |

它们被 `cargo test -p dungeon-app` 编译时直接报错，导致根 crate 的测试门禁长期不可用（ISSUES I88）。

**为什么归档而不是删除：** REFACTOR.md §8 把旧 `dungeon-*` 视为历史参考；这两个文件记录了旧架构的场景测试写法（TestBackend 逐帧截图、装备/投掷原子性场景），对迁移期的行为对照仍有参考价值。它们在 `archive/` 下不参与编译，不会进入任何 `cargo test` 门禁。

**替代品：** `tests/core_loop_test.rs` —— 只用新 `core` 公共 API 的 headless 端到端测试（new_game / apply_player_command / player_alive / request_quit）。

**状态：** 归档于 REFACTOR.md §11 Phase F（I88）。新架构的渲染快照测试（SceneFrame golden / TestBackend）属于 Phase G（presentation + tui），不在本轮范围。
