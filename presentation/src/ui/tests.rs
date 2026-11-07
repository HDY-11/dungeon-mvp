//! `ui` 的测试：页栈状态机。
//!
//! 页栈是纯状态机（唯一依赖 `SceneFrame` 的是 Look 页的**显示**），所以绝大多数
//! 用例不需要 `World`——这也正是把页栈从后端搬到集成层的好处之一。

use render_api::{PageKind, SceneFrame, UiView};

use super::*;
use crate::catalog::map_tile;

fn config() -> ExtractConfig {
    ExtractConfig::new(80, 60)
}

/// 新页栈在游戏页，且没有任何覆盖。
#[test]
fn new_page_stack_is_on_the_game_page() {
    let pages = PageStack::new();
    assert!(pages.is_game());
    assert_eq!(pages.kind(), PageKind::Game);
    assert!(!pages.is_quit_pending());
    assert_eq!(pages.view(&config(), &SceneFrame::empty()), UiView::Game);
}

/// 退出确认：打开后捕获输入，取消/确认都回到游戏页。
#[test]
fn quit_dialog_captures_input_and_closes() {
    let mut pages = PageStack::new();
    assert!(pages.apply(UiIntent::OpenQuitDialog));
    assert!(pages.is_quit_pending());
    assert_eq!(pages.kind(), PageKind::Dialog);

    // 对话框必须捕获输入（否则移动键会穿透到世界）。
    let view = pages.view(&config(), &SceneFrame::empty());
    assert!(view.captures_input());
    assert!(view.is_overlay());
    assert!(!view.is_fullscreen());

    // 取消与确认都只是关闭页栈；真正的退出由装配层按 intent 决定。
    assert!(pages.apply(UiIntent::CancelQuit));
    assert!(pages.is_game());
    assert!(!pages.is_quit_pending());
}

/// 游戏页上"关闭页面"没有意义：返回 false，让输入层去打开退出确认。
#[test]
fn closing_on_the_game_page_is_not_consumed() {
    let mut pages = PageStack::new();
    assert!(!pages.apply(UiIntent::ClosePage));
    assert!(pages.is_game());
}

/// 非退出对话框时，确认/取消不该被消费（防止误退出）。
#[test]
fn confirm_without_pending_quit_is_not_consumed() {
    let mut pages = PageStack::new();
    pages.open_look(5, 5);
    assert!(!pages.apply(UiIntent::ConfirmQuit));
    assert_eq!(pages.kind(), PageKind::Look, "Look 页不该被确认键关掉");
}

/// Look 页：光标夹在世界内，且四边都夹得住。
#[test]
fn look_cursor_is_clamped_to_the_world() {
    let mut pages = PageStack::new();
    pages.open_look(0, 0);
    assert_eq!(pages.kind(), PageKind::Look);

    pages.move_look_cursor(-1, -1, (80, 60));
    assert_eq!(pages.look_cursor(), (0, 0), "左上角不得越界");

    pages.move_look_cursor(1000, 1000, (80, 60));
    assert_eq!(pages.look_cursor(), (79, 59), "右下角不得越界");

    pages.move_look_cursor(-5, 3, (80, 60));
    assert_eq!(pages.look_cursor(), (74, 59));
}

/// 不在 Look 页时光标不动（防止在游戏页偷偷改状态）。
#[test]
fn look_cursor_does_not_move_outside_the_look_page() {
    let mut pages = PageStack::new();
    pages.move_look_cursor(5, 5, (80, 60));
    assert_eq!(pages.look_cursor(), (0, 0));

    pages.open_quit_dialog();
    pages.move_look_cursor(5, 5, (80, 60));
    assert_eq!(pages.look_cursor(), (0, 0));
}

/// `MoveLookCursor` 不经 `apply` 落地（它需要世界尺寸），
/// 这条测试把该约定钉住：`apply` 返回 false，真正落地在 `apply_ui_intent`。
#[test]
fn move_look_cursor_is_not_handled_by_apply() {
    let mut pages = PageStack::new();
    pages.open_look(10, 10);
    assert!(!pages.apply(UiIntent::MoveLookCursor { dx: 1, dy: 0 }));
    assert_eq!(
        pages.look_cursor(),
        (10, 10),
        "apply 不得偷偷移动光标（它拿不到世界尺寸）"
    );
}

/// Look 页从**本帧快照**读地形：不可见/未探索的格也如实报告。
#[test]
fn look_view_reads_tile_details_from_the_frame() {
    let mut pages = PageStack::new();
    pages.open_look(3, 4);

    let mut frame = SceneFrame::empty();
    frame.map = render_api::MapView::new(80, 60);
    frame
        .map
        .set_tile(3, 4, map_tile(ecs_core::Tile::DeepWater));
    frame.map.set_visible(3, 4, true);
    frame.map.set_explored(3, 4, true);

    let view = pages.view(&config(), &frame);
    let UiView::Look(look) = view else {
        panic!("Look 页必须产出 UiView::Look");
    };
    assert_eq!(look.cursor, (3, 4));
    let tile = look.tile.expect("光标格必须有地形详情");
    assert_eq!(tile.visual, map_tile(ecs_core::Tile::DeepWater));
    assert!(tile.visible);
    assert!(tile.explored);
}

/// Look 页在 `Unknown` 地形上不产出详情（后端应画 fallback，而不是显示垃圾）。
#[test]
fn look_view_reports_no_tile_for_unknown_terrain() {
    let mut pages = PageStack::new();
    pages.open_look(0, 0);

    let mut frame = SceneFrame::empty();
    frame.map = render_api::MapView::new(4, 4); // 默认全是 VisualKey::Unknown

    let UiView::Look(look) = pages.view(&config(), &frame) else {
        panic!("Look 页必须产出 UiView::Look");
    };
    assert!(look.tile.is_none(), "未知地形不该给出详情");
}

/// Look 页能捡起光标处的实体。
#[test]
fn look_view_picks_up_the_entity_under_the_cursor() {
    let mut pages = PageStack::new();
    pages.open_look(2, 2);

    let mut frame = SceneFrame::empty();
    frame.map = render_api::MapView::new(8, 8);
    frame.map.set_tile(2, 2, map_tile(ecs_core::Tile::Floor));
    frame.entities.push(
        render_api::EntityView::new(
            render_api::EntityId::from_bits(9),
            (2, 2),
            crate::catalog::map_monster_kind(ecs_core::MonsterKindId::Rat),
        )
        .with_name("老鼠"),
    );

    let UiView::Look(look) = pages.view(&config(), &frame) else {
        panic!("Look 页必须产出 UiView::Look");
    };
    let entity = look.entity.expect("光标处应当捡到实体");
    assert_eq!(entity.name, "老鼠");
}

/// 关闭 Look 页后回到游戏页。
#[test]
fn closing_look_returns_to_game() {
    let mut pages = PageStack::new();
    pages.open_look(1, 1);
    assert!(pages.apply(UiIntent::ClosePage));
    assert!(pages.is_game());
}

/// 未实现页（R2 之前的 Inventory 等）不得 panic。
#[test]
fn unimplemented_pages_fall_back_to_game_view_without_panicking() {
    let pages = PageStack {
        kind: PageKind::Inventory,
        ..PageStack::new()
    };
    let view = pages.view(&config(), &SceneFrame::empty());
    assert_eq!(view, UiView::Game);
}

/// 未实现页上按 `ClosePage` 也必须能退出（否则玩家会被卡在空页面里）。
#[test]
fn unimplemented_pages_can_still_be_closed() {
    let mut pages = PageStack {
        kind: PageKind::Inventory,
        ..PageStack::new()
    };
    assert!(pages.apply(UiIntent::ClosePage));
    assert!(pages.is_game());
}

#[test]
fn entity_lines_report_name_and_hp() {
    let mut entity = render_api::EntityView::new(
        render_api::EntityId::from_bits(1),
        (0, 0),
        render_api::VisualKey::Player,
    )
    .with_name("冒险者");
    assert_eq!(entity_lines(&entity).len(), 1, "没有 HP 时只有名字行");

    entity = entity.with_hp(render_api::Meter::new(3.0, 10.0));
    let lines = entity_lines(&entity);
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0].text(), "冒险者");
    assert!(lines[1].text().contains("HP 3 / 10"));
}

#[test]
fn visual_text_mentions_the_category() {
    let line = visual_text(render_api::VisualKey::Stairs, "楼梯");
    let text = line.text();
    assert!(text.contains("Stairs"), "实际: {text}");
    assert!(text.contains("楼梯"), "实际: {text}");
}

/// 背包页网格（DsnA5）：`e` 打开后焦点在 Nav1，Tab 顺时针轮转，Esc 关闭。
#[test]
fn the_inventory_page_opens_cycles_focus_and_closes() {
    use render_api::{InventoryFocus, InventoryNavGroup};

    let mut pages = PageStack::new();
    assert_eq!(pages.kind(), render_api::PageKind::Game);

    assert!(pages.apply(UiIntent::OpenInventory));
    assert_eq!(pages.kind(), render_api::PageKind::Inventory);
    assert_eq!(pages.inventory_focus(), InventoryFocus::Nav1);

    // Nav1 聚焦时 ←/→ 在「身体」「行囊」间切页签。
    assert!(pages.apply(UiIntent::MoveInventoryCursor { dx: 1, dy: 0 }));
    assert_eq!(pages.inventory_nav1_selected(), 1);
    // 下界夹取：连按 ← 也只回到第一页签。
    assert!(pages.apply(UiIntent::MoveInventoryCursor { dx: -5, dy: 0 }));
    assert_eq!(pages.inventory_nav1_selected(), 0);

    // 脚下没有 Nav2 页签时，Tab 跳过 Nav2 / Context2。
    for expected in [InventoryFocus::Context1, InventoryFocus::Nav1] {
        assert!(pages.apply(UiIntent::CycleInventoryFocus));
        assert_eq!(pages.inventory_focus(), expected);
    }

    // 有了 Nav2 页签后：Nav1 → Nav2 → Context2 → Context1 → Nav1。
    pages.set_inventory_nav2_len(1);
    for expected in [
        InventoryFocus::Nav2,
        InventoryFocus::Context2,
        InventoryFocus::Context1,
        InventoryFocus::Nav1,
    ] {
        assert!(pages.apply(UiIntent::CycleInventoryFocus));
        assert_eq!(pages.inventory_focus(), expected);
    }

    // 内容栏聚焦时 ↑/↓ 只把光标夹在下界 0（上界留给提取时按行数夹取）。
    // 先把焦点从 Nav1 再走三步回到 Context1。
    pages.set_inventory_nav2_len(1);
    for _ in 0..3 {
        assert!(pages.apply(UiIntent::CycleInventoryFocus));
    }
    assert_eq!(pages.inventory_focus(), InventoryFocus::Context1);
    assert!(pages.apply(UiIntent::MoveInventoryCursor { dx: 0, dy: 5 }));
    assert_eq!(
        pages.inventory_context_cursor(InventoryNavGroup::Nav1),
        5
    );
    assert!(pages.apply(UiIntent::MoveInventoryCursor { dx: 0, dy: -99 }));
    assert_eq!(
        pages.inventory_context_cursor(InventoryNavGroup::Nav1),
        0
    );

    assert!(pages.apply(UiIntent::ClosePage));
    assert_eq!(pages.kind(), render_api::PageKind::Game);
}

/// Tab 焦点轮转是**顺时针**的：摄像机 → 信息 → 状态 → 事件 → 摄像机。
#[test]
fn tab_cycles_the_pane_focus_clockwise() {
    let mut pages = PageStack::new();
    assert_eq!(pages.pane_focus(), render_api::Pane::Camera);

    for expected in [
        render_api::Pane::Info,
        render_api::Pane::Status,
        render_api::Pane::Events,
        render_api::Pane::Camera,
    ] {
        assert!(pages.apply(UiIntent::CyclePaneFocus));
        assert_eq!(pages.pane_focus(), expected);
    }
}

/// 覆盖页打开时 Tab 不轮转四栏焦点：背包页有自己的焦点模型（Nav/Context）。
#[test]
fn pane_focus_does_not_cycle_while_a_page_is_open() {
    let mut pages = PageStack::new();
    pages.apply(UiIntent::OpenInventory);
    assert!(!pages.apply(UiIntent::CyclePaneFocus));
    assert_eq!(pages.pane_focus(), render_api::Pane::Camera);
}

/// 事件栏的滚动只在焦点位于事件栏时生效；向上累计、向下夹到 0（贴底）。
#[test]
fn events_scroll_only_moves_with_the_events_pane_focused() {
    let mut pages = PageStack::new();
    assert!(
        !pages.apply(UiIntent::ScrollEvents { dy: 1 }),
        "摄像机焦点不滚事件"
    );
    assert_eq!(pages.events_scroll(), 0);

    pages.apply(UiIntent::CyclePaneFocus); // → Info
    assert!(!pages.apply(UiIntent::ScrollEvents { dy: 1 }));
    pages.apply(UiIntent::CyclePaneFocus); // → Status
    assert!(!pages.apply(UiIntent::ScrollEvents { dy: 1 }));
    pages.apply(UiIntent::CyclePaneFocus); // → Events
    assert_eq!(pages.pane_focus(), render_api::Pane::Events);

    assert!(pages.apply(UiIntent::ScrollEvents { dy: 1 }));
    assert_eq!(pages.events_scroll(), 1);
    assert!(pages.apply(UiIntent::ScrollEvents { dy: 3 }));
    assert_eq!(pages.events_scroll(), 4);
    // 向下滚过头夹到 0：0 = 贴底/恢复跟随最新。
    assert!(pages.apply(UiIntent::ScrollEvents { dy: -9 }));
    assert_eq!(pages.events_scroll(), 0);

    // 覆盖页打开时，滚事件意图也不该被消费。
    assert!(pages.apply(UiIntent::ScrollEvents { dy: 2 }));
    pages.apply(UiIntent::OpenInventory);
    assert!(!pages.apply(UiIntent::ScrollEvents { dy: 1 }));
    assert_eq!(pages.events_scroll(), 2);
}

/// 信息栏选行只在焦点位于信息栏、且游戏页未开覆盖页时生效。
#[test]
fn info_cursor_moves_only_with_the_info_pane_focused() {
    let mut pages = PageStack::new();
    assert!(!pages.apply(UiIntent::MoveInfoCursor { dy: 1 }), "摄像机焦点不选行");
    assert_eq!(pages.info_cursor(), 0);

    pages.apply(UiIntent::CyclePaneFocus); // → Info
    assert_eq!(pages.pane_focus(), render_api::Pane::Info);
    assert!(pages.apply(UiIntent::MoveInfoCursor { dy: 3 }));
    assert_eq!(pages.info_cursor(), 3);
    assert!(pages.apply(UiIntent::MoveInfoCursor { dy: -1 }));
    assert_eq!(pages.info_cursor(), 2);

    // 覆盖页打开时，选行意图也不该被消费（焦点在页面内部）。
    pages.apply(UiIntent::OpenInventory);
    assert!(!pages.apply(UiIntent::MoveInfoCursor { dy: 1 }));
    assert_eq!(pages.info_cursor(), 2);
}