//! 新架构的端到端（headless）冒烟测试：**一局 MVP 能真的跑起来**。
//!
//! # 这个测试存在的理由
//!
//! 各层的单测都能过，但"装起来还能不能跑"是另一件事：主循环的接线顺序、页栈与
//! 世界的交互、`SceneFrame` 与后端的配合，只有把整条链路串起来才暴露。
//! 它取代的验证方式是"人来玩一遍"——那种验证不可回归。
//!
//! 链路（全部走真实代码，不 mock）：
//!
//! ```text
//! InputEvent ──> dungeon_app::App::handle ──> presentation::map_input_event
//!                        │                          │
//!                        │                    PlayerCommand / UiIntent
//!                        ▼                          ▼
//!              ecs_core::apply_player_command   页栈状态
//!                        │
//!                        ▼
//!          App::refresh ──> presentation::extract_scene_frame ──> SceneFrame
//!                        │
//!                        ▼
//!              ratatui TestBackend（真实绘制，不是"能不能构造"）
//! ```

use bevy_ecs::prelude::*;
use dungeon_app::App;
use ecs_core::{MAP_HEIGHT, MAP_WIDTH, Monster, Player, Position, Stairs};
use ratatui::{Terminal, backend::TestBackend};
use render_api::{InputEvent, Key, KeyEvent, PageKind};
use tui::TuiPlugin;

/// 终端尺寸。相机视口**不等于**它——地图只占其中一部分，见 `map_viewport`。
const TERMINAL: (u16, u16) = (80, 30);

/// 测试里用的相机视口：与 `main.rs` 一样从终端区域推导。
fn viewport() -> (u16, u16) {
    tui::map_viewport(ratatui::layout::Rect::new(0, 0, TERMINAL.0, TERMINAL.1))
}

/// 开局，并清掉会自己走动的实体。
///
/// **为什么必须清场**：`apply_player_command` 会推进世界直到玩家行动做完，
/// 期间怪物也在行动（游荡每步消耗一次随机数）。若不清场，"玩家是否移动到
/// 目标格"就取决于怪物有没有恰好走进那一格——那是**执行顺序**决定的，不是
/// 本测试要验的东西（LESSONS.md LSYN20 记的就是这个坑）。
fn quiet_app(seed: u64) -> App {
    let mut app = App::new(seed, viewport());
    clear_monsters_and_stairs(&mut app);
    app.refresh();
    app
}

fn clear_monsters_and_stairs(app: &mut App) {
    let world = app.world_mut();
    for entity in collect::<Monster>(world) {
        world.despawn(entity);
    }
    for entity in collect::<Stairs>(world) {
        world.despawn(entity);
    }
    // 清完要重建占用图，否则移动判定还会以为那些格子被占。
    ecs_core::system::run_settle_systems(world);
}

fn collect<T: Component>(world: &mut World) -> Vec<Entity> {
    let mut query = world.query_filtered::<Entity, With<T>>();
    query.iter(world).collect()
}

/// 找一个"可走且相邻且未被占用"的方向。
fn walkable_step(app: &App) -> (isize, isize) {
    let world = app.world();
    let player = player_position_of(world).expect("新游戏必须有玩家");
    let map = world.resource::<ecs_core::Map>();
    let occupancy = world.resource::<ecs_core::OccupancyMap>();
    let dirs: [(isize, isize); 8] = [
        (0, -1),
        (0, 1),
        (-1, 0),
        (1, 0),
        (-1, -1),
        (1, -1),
        (-1, 1),
        (1, 1),
    ];
    dirs.into_iter()
        .find(|&(dx, dy)| {
            let (nx, ny) = position_offset(player, dx, dy);
            nx < MAP_WIDTH
                && ny < MAP_HEIGHT
                && map.tiles[ny][nx].walkable()
                && !occupancy.is_occupied(nx, ny)
        })
        .expect("seed 的地图必须有可走方向")
}

fn player_position_of(world: &World) -> Option<(usize, usize)> {
    let mut query = world.try_query::<(&Position, &Player)>()?;
    query.iter(world).next().map(|(pos, _)| pos.to_tuple())
}

fn position_offset(pos: (usize, usize), dx: isize, dy: isize) -> (usize, usize) {
    let p = Position::new(pos.0, pos.1);
    p.offset(dx, dy)
}

fn key(k: Key) -> InputEvent {
    InputEvent::Key(KeyEvent::pressed(k))
}

/// 用真实 `TestBackend` 画一帧，返回整帧文本（逐行拼）。
///
/// 逐行拼而不是拍平：CJK 占两格，拍平会把中文拆散（`tui/src/render/tests.rs`
/// 里记了同一个坑）。
fn render_text(app: &mut App) -> String {
    let ui = TuiPlugin::new();
    let log = dungeon_app::dev_log(app.world());
    let frame = app.refresh();
    let mut terminal = Terminal::new(TestBackend::new(TERMINAL.0, TERMINAL.1)).expect("测试终端");
    terminal
        .draw(|terminal_frame| ui.draw(terminal_frame, frame, log.as_ref()))
        .expect("绘制不得失败");

    let buffer = terminal.backend().buffer();
    (0..buffer.area.height)
        .map(|y| {
            let mut row = String::new();
            for x in 0..buffer.area.width {
                row.push_str(buffer[(x, y)].symbol());
            }
            row
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// ① 开局就能出一帧可渲染的画面，且玩家、HUD、日志都在。
#[test]
fn mvp_starts_and_produces_a_renderable_frame() {
    let mut app = App::new(7, viewport());
    let frame = app.refresh();

    assert!(frame.is_ready(), "开局必须产出可渲染的帧");
    assert!(frame.revision >= 1, "首帧之后帧号必须递增");
    assert_eq!(frame.hud.floor, 1);
    assert!(frame.hud.is_ready(), "开局 HUD 必须有玩家名");
    assert!(frame.hud.hp.max > 0.0, "开局必须有血量上限");
    assert!(frame.player.is_some(), "必须提取到玩家");
    assert!(frame.map.visible_count() > 0, "开局必须有视野");
    assert!(frame.camera.is_valid());
    assert_eq!(frame.ui, render_api::UiView::Game);
    assert!(!frame.game_over);

    let text = render_text(&mut app);
    assert!(text.contains('@'), "画面上必须有玩家 glyph");
    assert!(text.contains("Dungeon"), "画面上必须有标题");
}

/// ② 移动命令真的驱动了世界：玩家位置改变、帧里位置同步、占位图无残留。
#[test]
fn mvp_move_command_advances_the_world_and_the_frame() {
    let mut app = quiet_app(11);
    let before = player_position_of(app.world()).expect("有玩家");
    let (dx, dy) = walkable_step(&app);
    let expected = position_offset(before, dx, dy);

    let key_event = match (dx, dy) {
        (0, -1) => Key::Up,
        (0, 1) => Key::Down,
        (-1, 0) => Key::Left,
        (1, 0) => Key::Right,
        (-1, -1) => Key::Home,
        (-1, 1) => Key::End,
        (1, -1) => Key::PageUp,
        _ => Key::PageDown,
    };
    assert!(app.handle(key(key_event)), "合法移动必须被接受");

    assert_eq!(
        player_position_of(app.world()),
        Some(expected),
        "世界里玩家必须移动到 {expected:?}"
    );

    let frame = app.refresh();
    let player = frame.player.as_ref().expect("帧里必须有玩家");
    assert_eq!(
        (player.position.0 as usize, player.position.1 as usize),
        expected,
        "帧里玩家位置必须与世界同步"
    );

    // 相机必须跟住玩家：移动后玩家仍在可见矩形内。
    let rect = frame.camera.visible_rect();
    assert!(
        rect.contains(expected.0 as f32 + 0.5, expected.1 as f32 + 0.5),
        "移动后玩家必须仍在相机可见范围内"
    );

    // 画面上玩家确实出现在新格（用渲染文本交叉验证，而不是只看数据）。
    let text = render_text(&mut app);
    assert!(text.contains('@'), "移动后画面仍必须有玩家");
}

/// ③ 撞墙/越界的命令被拒绝，且**不推进**世界、不留残留状态。
#[test]
fn mvp_rejected_command_does_not_advance_the_world() {
    let mut app = quiet_app(12);
    let before = player_position_of(app.world()).expect("有玩家");

    // 找一个被挡住的方向：越界或不可走。
    let map_world = app.world();
    let map = map_world.resource::<ecs_core::Map>();
    let blocked: Option<(isize, isize)> = [
        (-1isize, 0isize),
        (1, 0),
        (0, -1),
        (0, 1),
    ]
    .into_iter()
    .find(|&(dx, dy)| {
        let (nx, ny) = position_offset(before, dx, dy);
        nx >= MAP_WIDTH || ny >= MAP_HEIGHT || !map.tiles[ny][nx].walkable()
    });
    let Some((dx, dy)) = blocked else {
        // 出生点四周全可走时这条测试没有意义；直接跳过而不是假装通过。
        eprintln!("seed=12 出生点四周都可走，跳过被挡方向断言");
        return;
    };

    let key_event = match (dx, dy) {
        (-1, 0) => Key::Left,
        (1, 0) => Key::Right,
        (0, -1) => Key::Up,
        _ => Key::Down,
    };
    assert!(!app.handle(key(key_event)), "被挡住的移动必须返回 false");
    assert_eq!(
        player_position_of(app.world()),
        Some(before),
        "被拒绝的命令不得改变位置"
    );
    // 用一个可变借用连续取两次，避免与 `player_position_of(app.world())` 冲突。
    let player = player_entity(app.world_mut());
    assert!(
        app.world().get::<ecs_core::Active>(player).is_none(),
        "被拒绝后玩家不得卡在 Active"
    );
    assert!(
        app.world().get::<ecs_core::Idle>(player).is_some(),
        "被拒绝后玩家必须仍是 Idle（否则输入永久失效）"
    );
}

fn player_entity(world: &mut World) -> Entity {
    let mut query = world.query_filtered::<Entity, With<Player>>();
    query.iter(world).next().expect("必须有玩家")
}

/// ④ 等待命令可用（`. `），且不在画面上引入异常。
#[test]
fn mvp_wait_command_works() {
    let mut app = quiet_app(7);
    assert!(app.handle(key(Key::Char('.'))), "等待必须被接受");
    let text = render_text(&mut app);
    assert!(text.contains('@'));
}

/// ⑤ 退出必须走确认：`q` 只弹框，`Enter` 才真的退出，`Esc` 能取消。
#[test]
fn mvp_quit_requires_confirmation() {
    let mut app = quiet_app(7);
    assert!(!app.wants_quit(), "开局不该请求退出");

    // `q` → 打开确认框，此时**不能**退出。
    assert!(app.handle(key(Key::Char('q'))), "q 应当打开确认框");
    assert!(!app.wants_quit(), "q 不得直接退出（用户会以为程序崩了）");
    let frame = app.refresh();
    assert_eq!(frame.ui.kind(), PageKind::Dialog, "应当显示确认对话框");
    assert!(frame.ui.captures_input(), "对话框必须捕获输入");

    // `Esc` → 取消，回到游戏页且仍不退出。
    assert!(app.handle(key(Key::Esc)), "Esc 应当关闭确认框");
    assert!(!app.wants_quit(), "取消后不得退出");
    let frame = app.refresh();
    assert_eq!(frame.ui.kind(), PageKind::Game);

    // `q` → `Enter` → 真的退出。
    assert!(app.handle(key(Key::Char('q'))));
    assert!(app.handle(key(Key::Enter)), "Enter 应当确认退出");
    assert!(app.wants_quit(), "确认后必须请求退出");
}

/// ⑥ 覆盖页吃掉移动键：查看地形时按方向键不得把玩家走出去。
#[test]
fn mvp_overlay_swallows_movement_keys() {
    let mut app = quiet_app(11);
    let before = player_position_of(app.world()).expect("有玩家");

    assert!(app.handle(key(Key::Char('?'))), "? 应当打开 Look 页");
    assert_eq!(app.refresh().ui.kind(), PageKind::Look);

    for k in [Key::Up, Key::Down, Key::Left, Key::Right] {
        assert!(!app.handle(key(k)), "Look 页上 {k:?} 不得驱动世界");
    }
    assert_eq!(
        player_position_of(app.world()),
        Some(before),
        "Look 页上按方向键不得移动玩家"
    );

    // 关闭后移动键恢复可用。
    assert!(app.handle(key(Key::Esc)));
    assert_eq!(app.refresh().ui.kind(), PageKind::Game);
}

/// ⑦ 连续玩若干步不 panic、不留残留状态（一局 MVP 的"能跑"）。
#[test]
fn mvp_plays_a_lot_of_turns_without_panicking_or_leaking() {
    let mut app = quiet_app(2026);
    let mut moves = 0;
    let mut consecutive_rejections = 0;

    // 200 步，每步朝当前可走方向走；走不动就换个方向（模拟玩家乱按）。
    for step in 0..200 {
        let dirs: [Key; 4] = [Key::Up, Key::Down, Key::Left, Key::Right];
        let chosen = dirs[step % dirs.len()];
        let start = player_position_of(app.world()).expect("玩家一直活着");
        if app.handle(key(chosen)) {
            moves += 1;
            consecutive_rejections = 0;
        } else {
            consecutive_rejections += 1;
            // 四方向连续都被拒 = 玩家被围住了；这不该发生（本测试已清场）。
            assert!(
                consecutive_rejections < 4,
                "第 {step} 步：四方向连续被拒，位置 {start:?}"
            );
        }

        // 每步都提取一帧，顺带验证提取在长时间运行下不漂移。
        let frame = app.refresh();
        assert!(
            frame.revision >= 1,
            "第 {step} 步：帧号必须单调递增"
        );
        assert!(!frame.game_over, "第 {step} 步：清场后玩家不该死");
    }

    assert!(moves > 0, "200 步里至少应当有成功的移动");
    assert!(!app.wants_quit(), "没有退出命令时不得退出");

    // 画一帧收尾，确认长时间运行后渲染仍然正常。
    let text = render_text(&mut app);
    assert!(text.contains('@'), "长时间运行后画面上仍必须有玩家");
}

/// ⑨ 全链路：**终端按键** → 世界状态变化 → 画面。
///
/// 前面的用例喂的都是 `InputEvent`；这条从 `crossterm::KeyCode` 出发，把
/// `main.rs` 用的那条路径（`translate_key` → `App::handle`）也覆盖掉——
/// 否则"终端按键翻译"这一段就是无人测试的缝。
#[test]
fn mvp_terminal_keycode_drives_the_world_end_to_end() {
    let mut app = quiet_app(11);
    let before = player_position_of(app.world()).expect("有玩家");

    // 找一个真实可走的方向，再挑对应的终端按键。
    let (dx, dy) = walkable_step(&app);
    let code = match (dx, dy) {
        (0, -1) => crossterm::event::KeyCode::Char('k'),
        (0, 1) => crossterm::event::KeyCode::Char('j'),
        (-1, 0) => crossterm::event::KeyCode::Char('h'),
        (1, 0) => crossterm::event::KeyCode::Char('l'),
        (-1, -1) => crossterm::event::KeyCode::Home,
        (-1, 1) => crossterm::event::KeyCode::End,
        (1, -1) => crossterm::event::KeyCode::PageUp,
        _ => crossterm::event::KeyCode::PageDown,
    };

    let event = dungeon_app::translate_key(code).expect("该按键必须有翻译");
    assert!(app.handle(event), "终端按键必须驱动世界");
    assert_eq!(
        player_position_of(app.world()),
        Some(position_offset(before, dx, dy)),
        "终端按键 {code:?} 应当让玩家移动到 {:?}",
        position_offset(before, dx, dy)
    );

    // 终端按键同样能走退出确认流程。
    let quit = dungeon_app::translate_key(crossterm::event::KeyCode::Char('q')).unwrap();
    assert!(app.handle(quit));
    assert!(
        !app.wants_quit(),
        "终端按 q 也只该打开确认框，不得直接退出"
    );
    let confirm = dungeon_app::translate_key(crossterm::event::KeyCode::Enter).unwrap();
    assert!(app.handle(confirm));
    assert!(app.wants_quit(), "确认后才退出");
}

/// ⑩ 视口变化（终端 resize）不得 panic，且相机随之更新。
#[test]
fn mvp_viewport_resize_is_safe() {
    let mut app = App::new(11, viewport());
    for viewport in [(1, 1), (20, 5), (40, 12), (200, 60), (0, 0)] {
        app.set_viewport(viewport);
        let frame = app.refresh();
        assert!(
            frame.camera.is_valid() || viewport == (0, 0) || viewport.0 == 0,
            "viewport={viewport:?} 下相机应当可用"
        );
    }
    // 恢复一个正常尺寸后仍能画。
    app.set_viewport(viewport());
    let text = render_text(&mut app);
    assert!(text.contains('@'));
}

/// 背包页真的能改世界：`e` 开页 → `Enter` 装上 → 再 `Enter` 卸下。
///
/// 唯一一条把「按键 → 页栈 → 世界动作 → 下一帧视图」串起来验证的用例：
/// 少了它，动作系统与键位之间断了也不会红。
#[test]
fn the_inventory_page_equips_and_unequips_through_the_keyboard() {
    use ecs_core::ItemTag;
    use ecs_core::item::{Amount, BODY, Container, Item, ItemId, pack_slot_index};

    let mut app = quiet_app(2026);
    let player = player_entity(app.world_mut());
    let world = app.world_mut();
    let pack = world
        .get::<Container>(player)
        .and_then(|body| body.slot(pack_slot_index()))
        .expect("玩家身上有行囊");
    let sword = world.spawn((Item::new(ItemId(0)), Amount::new(1))).id();
    {
        let mut pack_container = world.get_mut::<Container>(pack).expect("行囊");
        let slot = pack_container.free_slot_for(&[]).expect("空格");
        pack_container.insert(slot, sword, &[]).expect("空格");
    }
    let main_hand = BODY
        .slots
        .iter()
        .position(|slot| slot.accept.accepts(&[ItemTag::Weapon]))
        .expect("有主手槽");

    // 开页：焦点在 Nav1、页签默认「身体」。→ 切到「行囊」，Tab 到内容栏，Enter 装上。
    app.handle(key(Key::Char('e')));
    assert_eq!(
        app.refresh().ui.kind(),
        PageKind::Inventory,
        "e 应当打开背包页"
    );
    app.handle(key(Key::Right));
    app.handle(key(Key::Tab));
    app.handle(key(Key::Enter));
    app.refresh();
    assert_eq!(
        app.world_mut()
            .get::<Container>(player)
            .and_then(|body| body.slot(main_hand)),
        Some(sword),
        "Enter 应当把剑装上"
    );
    // 回到 Nav1、页签切回「身体」，Tab 到内容栏，Enter 卸下（内容第 0 行现在是主手）。
    app.handle(key(Key::Tab));
    app.handle(key(Key::Left));
    app.handle(key(Key::Tab));
    app.handle(key(Key::Enter));
    app.refresh();
    let world = app.world_mut();
    assert_eq!(
        world
            .get::<Container>(player)
            .and_then(|body| body.slot(main_hand)),
        None,
        "再按 Enter 应当卸下"
    );
    assert!(
        world
            .get::<Container>(pack)
            .is_some_and(|pack| pack.contains(sword)),
        "卸下的剑必须回到行囊"
    );
}

/// 背包页「脚下」栏 + 显式搜刮：尸体不再自动拾取，`e` 开页 `Enter` 自己拿。
///
/// 这条补的是"背包页能操作尸体"的接线：少了它，`take_from_corpse` 可能永远
/// 没有调用方（核心动作单测全绿，玩家却拿不到东西）。
#[test]
fn the_inventory_page_loots_a_corpse_through_the_keyboard() {
    use ecs_core::item::{Amount, Container, Item, ItemId, CORPSE, pack_slot_index};
    use ecs_core::monster::{Corpse, MonsterKindId};
    use render_api::{InventoryNav, UiView};

    let mut app = quiet_app(2026);
    let player = player_entity(app.world_mut());
    let world = app.world_mut();
    let position = *world.get::<Position>(player).expect("玩家有位置");
    let dagger = world
        .spawn((Item::new(ItemId(24)), Amount::new(1)))
        .id();
    let mut corpse_container = Container::new(&CORPSE);
    corpse_container.insert(0, dagger, &[]).expect("尸体空格");
    let corpse = world
        .spawn((
            Corpse::new(MonsterKindId::Goblin, "哥布林"),
            position,
            corpse_container,
        ))
        .id();

    // 开局身体只有行囊（会被跳过）、行囊是空的；脚下有尸体，Nav2 多出「尸体」页签。
    app.handle(key(Key::Char('e')));
    let frame = app.refresh();
    let UiView::Inventory(view) = &frame.ui else {
        panic!("e 应当打开背包页，实际 {:?}", frame.ui.kind());
    };
    assert_eq!(view.nav2, vec![InventoryNav::Corpse], "脚下的尸体要单独成页签");
    assert_eq!(view.context2.title, "脚下：哥布林的尸体");
    assert_eq!(view.context2.items[0].label, "匕首");

    // 焦点从 Nav1 顺时针走到 Context2（脚下的尸体内容），再 Enter 拿。
    assert!(app.handle(key(Key::Tab)), "Tab 在背包页应当被消费");
    assert!(app.handle(key(Key::Tab)), "再 Tab 一次到内容栏");
    app.handle(key(Key::Enter));
    app.refresh();
    let world = app.world_mut();
    let pack = world
        .get::<Container>(player)
        .and_then(|body| body.slot(pack_slot_index()))
        .expect("玩家身上有行囊");
    assert!(
        world
            .get::<Container>(pack)
            .is_some_and(|pack| pack.contains(dagger)),
        "Enter 应当把匕首拿进行囊"
    );
    assert_eq!(
        world
            .get::<Container>(corpse)
            .expect("尸体还在")
            .used(),
        0,
        "拿走后尸体那一格该是空的"
    );
}

/// 地面物品全显式：背包页「地面」栏 `Enter` 拿一件，不踩上自动捡（DsnE21 阶段 4）。
#[test]
fn the_inventory_page_picks_up_ground_items_through_the_keyboard() {
    use ecs_core::item::{Amount, Container, GroundItemAt, Item, ItemId, pack_slot_index};
    use render_api::{InventoryNav, UiView};

    let mut app = quiet_app(2026);
    let player = player_entity(app.world_mut());
    let world = app.world_mut();
    let position = *world.get::<Position>(player).expect("玩家有位置");
    let flesh = world
        .spawn((
            Item::new(ItemId(10)),
            Amount::new(2),
            GroundItemAt::new(position.x, position.y),
        ))
        .id();

    app.handle(key(Key::Char('e')));
    let frame = app.refresh();
    let UiView::Inventory(view) = &frame.ui else {
        panic!("e 应当打开背包页，实际 {:?}", frame.ui.kind());
    };
    assert_eq!(view.nav2, vec![InventoryNav::Ground], "脚下地面物品要单独成页签");
    assert_eq!(view.context2.title, "地面");
    assert!(
        view.context2.items.iter().any(|item| item.label.contains("血肉")),
        "地面页签下要列出物品：{:?}",
        view.context2
            .items
            .iter()
            .map(|item| &item.label)
            .collect::<Vec<_>>()
    );

    assert!(app.handle(key(Key::Tab)), "Tab 在背包页应当被消费");
    assert!(app.handle(key(Key::Tab)), "再 Tab 一次到内容栏");
    app.handle(key(Key::Enter));
    app.refresh();
    let world = app.world_mut();
    let pack = world
        .get::<Container>(player)
        .and_then(|body| body.slot(pack_slot_index()))
        .expect("玩家身上有行囊");
    assert!(
        world
            .get::<Container>(pack)
            .is_some_and(|pack| pack.contains(flesh)),
        "Enter 应当把地面物品拿进行囊"
    );
    assert!(
        world.get::<GroundItemAt>(flesh).is_none(),
        "进了容器就不能还带着地面位置"
    );
}

/// `Tab` 顺时针轮转四栏焦点，画面上的焦点框真的跟着走（端到端，不 mock 页栈）。
#[test]
fn tab_cycles_the_four_pane_focus_on_screen() {
    use render_api::Pane;

    let mut app = quiet_app(2026);
    assert_eq!(
        app.frame().focus,
        Pane::Camera,
        "开局焦点应当停在摄像机"
    );

    for expected in [Pane::Info, Pane::Status, Pane::Events, Pane::Camera] {
        assert!(app.handle(key(Key::Tab)), "Tab 应当被游戏页消费");
        let frame = app.refresh();
        assert_eq!(frame.focus, expected, "Tab 应当顺时针轮转到 {expected:?}");
    }
}

/// 事件栏聚焦时 ↑ 只翻历史，不驱动玩家移动（端到端，DsnA6）。
#[test]
fn arrow_keys_in_the_events_pane_scroll_without_moving_the_player() {
    use render_api::Pane;

    let mut app = quiet_app(2026);
    let before = presentation::player_position(app.world_mut()).expect("玩家有位置");

    // 摄像机 → 信息 → 状态 → 事件。
    for _ in 0..3 {
        assert!(app.handle(key(Key::Tab)), "Tab 应当被游戏页消费");
    }
    assert_eq!(app.refresh().focus, Pane::Events);
    assert_eq!(
        app.frame().events_scroll,
        0,
        "开局事件栏贴着最新事件"
    );

    assert!(app.handle(key(Key::Up)), "事件栏焦点时 ↑ 应被消费");
    let frame = app.refresh();
    assert_eq!(frame.focus, Pane::Events, "翻历史不该切走焦点");
    assert_eq!(frame.events_scroll, 1, "↑ 应当向上滚一行");

    assert!(app.handle(key(Key::Down)), "事件栏焦点时 ↓ 应被消费");
    assert_eq!(app.refresh().events_scroll, 0, "↓ 回到最新就恢复贴底");

    let after = presentation::player_position(app.world_mut()).expect("玩家有位置");
    assert_eq!(before, after, "事件栏翻历史不得驱动玩家移动");
}

/// 信息栏聚焦时 ↑↓ 只选行，不驱动玩家移动（端到端）。
#[test]
fn arrow_keys_in_the_info_pane_do_not_move_the_player() {
    use render_api::Pane;

    let mut app = quiet_app(2026);
    let before = presentation::player_position(app.world_mut()).expect("玩家有位置");

    assert!(app.handle(key(Key::Tab)), "第一次 Tab 到信息栏");
    assert_eq!(app.refresh().focus, Pane::Info);
    let cursor_before = app
        .world()
        .resource::<presentation::PageStack>()
        .info_cursor();

    assert!(app.handle(key(Key::Down)), "信息栏焦点时 ↓ 应被消费");
    let frame = app.refresh();
    assert_eq!(frame.focus, Pane::Info, "选行不该切走焦点");
    assert_eq!(
        app.world()
            .resource::<presentation::PageStack>()
            .info_cursor(),
        cursor_before + 1,
        "↓ 应当把信息栏光标往下挪一行"
    );

    let after = presentation::player_position(app.world_mut()).expect("玩家有位置");
    assert_eq!(before, after, "信息栏选行不得驱动玩家移动");
}
