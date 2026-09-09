//! `render-api` 公共 API 冒烟测试。
//!
//! 这里只依赖 crate 的公开接口，验证 TUI / GPU 后端在编译期需要的类型
//! 都能被正常构造和消费。

use render_api::prelude::*;

#[test]
fn scene_frame_public_api_smoke() {
    let mut frame = SceneFrame::empty();
    assert!(!frame.is_ready());
    frame.bump_revision();
    assert_eq!(frame.revision, 1);

    frame.map = MapView::new(3, 2);
    assert!(frame.is_ready());
    assert!(frame.map.set_tile(1, 1, VisualKey::Tile(4)));
    assert_eq!(frame.map.tile(1, 1), Some(VisualKey::Tile(4)));

    frame.camera = Camera2D::new((1.0, 1.0), (80, 24), 1.0);
    frame.player = Some(
        EntityView::new(EntityId::from_bits(1), (1, 1), VisualKey::Player)
            .with_name("冒险者")
            .with_hp(Meter::new(20.0, 20.0)),
    );
    frame.hud = HudView {
        floor: 1,
        map_kind: "洞穴".to_string(),
        player_name: "冒险者".to_string(),
        level: 1,
        hp: Meter::new(20.0, 20.0),
        mp: Meter::new(5.0, 5.0),
        exp: Meter::new(0.0, 20.0),
        attack: 5.0,
        defense: 1.0,
        extra_lines: vec![UiTextLine::plain("测试")],
    };
    frame.log.push(LogLine::new(LogLevel::Combat, "命中"));
    frame.ui = UiView::Dialog(DialogView::confirm("退出", "确认退出？"));

    assert!(frame.hud.is_ready());
    assert_eq!(frame.ui.kind(), PageKind::Dialog);
    assert!(frame.ui.captures_input());
    assert!(frame.player.as_ref().is_some_and(EntityView::is_alive));
}

#[test]
fn input_contract_smoke() {
    let mut queue = InputQueue::default();
    queue.push(InputEvent::key(Key::Up));
    queue.push(InputEvent::resize(80, 24));
    queue.push(InputEvent::Quit);

    let events = queue.take();
    assert_eq!(events.len(), 3);
    assert_eq!(events[0], InputEvent::key(Key::Up));
    assert!(events[2].is_quit());

    let surface = SurfaceInfo::new(1280, 720, 32, 32);
    assert_eq!(surface.cells(), (40, 22));
}

#[test]
fn ui_contract_smoke() {
    let mut list = ListView::new("背包").with_items(vec![
        ListItemView::new("短剑").with_detail("攻击 +3"),
        ListItemView::new("药水").with_style(UiTextStyle::Success),
    ]);
    list.selected = 1;
    list.clamp_selection();
    assert_eq!(
        list.selected_item().map(|item| item.label.as_str()),
        Some("药水")
    );

    let inventory = InventoryView {
        title: "背包".to_string(),
        panels: vec![InventoryPanelView::new(InventoryPanel::Backpack, list)],
        focus: InventoryFocus::Panel(InventoryPanel::Backpack),
        ..InventoryView::default()
    };
    assert_eq!(
        inventory.panel(InventoryPanel::Backpack).map(ListView::len),
        Some(2)
    );
    assert!(inventory.focused_panel().is_some());

    let view = UiView::Inventory(inventory);
    assert!(view.is_fullscreen());
    assert_eq!(view.kind(), PageKind::Inventory);

    let aim = ThrowAimView::new((3, 3))
        .with_path(vec![(1, 1), (2, 2), (3, 3)])
        .with_valid(true);
    assert_eq!(aim.path.len(), 3);
}

#[test]
fn visual_contract_smoke() {
    let keys = [
        VisualKey::Player,
        VisualKey::Monster(2),
        VisualKey::Stairs,
        VisualKey::Item(9),
        VisualKey::Tile(3),
        VisualKey::Effect(1),
        VisualKey::UiIcon(UiIcon::Heart),
        VisualKey::Unknown(0),
    ];
    for key in keys {
        assert_eq!(VisualKey::from_u64(key.as_u64()), key, "{key:?}");
    }
    assert!(VisualKey::Unknown(0).is_unknown());
}
