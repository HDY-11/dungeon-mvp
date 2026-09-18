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
