//! TUI 的 golden 测试：给定 [`SceneFrame`]，断言终端画面。
//!
//! # 两种断言手段
//!
//! - [`build_map_lines`] 直接产出 `Vec<Line>`：适合断言"这一格画了什么"，
//!   不需要终端、不需要 `TestBackend`，失败信息也最直观；
//! - `TestBackend`：适合断言"整帧布局没崩"（尺寸、边框、各区块是否都在）。
//!
//! 两者都要，因为它们防的是不同的问题：前者防"画错格子"，后者防"布局炸了"。
//!
//! # 为什么不经过 `presentation`
//!
//! `tui` 的依赖表里**没有** `ecs_core`/`presentation`（那是 Dsn28 的边界），
//! 所以这里的 `SceneFrame` 全部手工构造。这反而是好事：这一层只对**契约**
//! 负责，不依赖提取逻辑，契约用例（render-api 的 `tests/contract.rs`）改了之后
//! 这些测试就是后端侧的第一道警报。

use ratatui::{Terminal, backend::TestBackend, layout::Rect, style::Color};
use render_api::{
    Camera2D, DetailView, DialogView, EntityId, EntityView, HudView, InventoryFocus,
    InventoryPanel, InventoryPanelView, InventoryView, ListItemView, ListView, LogLevel, LogLine,
    MapView, Meter, SceneFrame, TileInfo, UiTextLine, UiTextStyle, UiView, VisualKey, VisualLayer,
};

use super::*;

const W: usize = 8;
const H: usize = 4;

/// 一个 8×4 的世界：全部地板、全部可见、全部已探索，玩家在 (2, 2)。
///
/// 相机固定在能让 8×4 整张地图可见的位置（视口大于世界时相机取世界中心）。
fn base_scene() -> SceneFrame {
    let mut frame = SceneFrame {
        camera: Camera2D::new((4.0, 2.0), (8, 4), 1.0),
        map: MapView::new(W, H),
        ..SceneFrame::default()
    };
    for y in 0..H {
        for x in 0..W {
            frame.map.set_tile(x, y, VisualKey::Tile(1)); // Floor
            frame.map.set_visible(x, y, true);
            frame.map.set_explored(x, y, true);
        }
    }
    frame.player = Some(
        EntityView::new(EntityId::from_bits(1), (2, 2), VisualKey::Player)
            .with_name("冒险者 Lv.1")
            .with_hp(Meter::new(20.0, 20.0)),
    );
    frame.hud = HudView {
        floor: 1,
        map_kind: "Cavern".to_string(),
        player_name: "冒险者".to_string(),
        level: 1,
        hp: Meter::new(20.0, 20.0),
        mp: Meter::new(8.0, 8.0),
        exp: Meter::new(0.0, 100.0),
        attack: 8.0,
        defense: 4.0,
        extra_lines: Vec::new(),
    };
    frame
}

/// 把渲染出的行拼成字符串（每行一个 `String`），便于断言。
fn lines_to_text(lines: &[ratatui::text::Line<'_>]) -> Vec<String> {
    lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect()
}

/// 把 `TestBackend` 的缓冲逐行拼成字符串。
///
/// **不要**把 `buffer.content` 直接拍平成一个 `String`：宽字符（CJK、`≈`、`░`）
/// 在缓冲里占多个格子，拍平后每个格子都会吐出一个符号，中文就会被拆散、
/// `contains("中文")` 永远失败。逐行拼、按行断言才是稳的。
fn buffer_rows(terminal: &Terminal<TestBackend>) -> Vec<String> {
    let buffer = terminal.backend().buffer();
    (0..buffer.area.height)
        .map(|y| {
            let mut row = String::new();
            for x in 0..buffer.area.width {
                let cell = &buffer[(x, y)];
                // 宽字符的续格是空符号，跳过以免出现重复/空洞。
                let symbol = cell.symbol();
                if symbol.is_empty() {
                    continue;
                }
                row.push_str(symbol);
            }
            row
        })
        .collect()
}

/// 整帧文本（行间用 `\n` 连接），用于 `contains` 断言。
fn buffer_text(terminal: &Terminal<TestBackend>) -> String {
    buffer_rows(terminal).join("\n")
}

/// 去掉所有空白后的整帧文本。
///
/// `Paragraph` 的 `Wrap { trim: true }` 会在 CJK 字符之间插入空格（换行算法的
/// 副作用），所以断言中文时必须先去掉空白，否则 `contains("对老鼠造成")` 恒假。
/// 去空白只影响"看起来怎样"，不影响"画了什么"，对内容断言是安全的。
fn buffer_text_compact(terminal: &Terminal<TestBackend>) -> String {
    buffer_text(terminal)
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect()
}


/// 整张地图可见时，应当逐格画出地形 glyph，且不出现空白。
#[test]
fn full_view_map_renders_every_cell() {
    let scene = base_scene();
    let text = lines_to_text(&build_map_lines(&scene, W, H));

    assert_eq!(text.len(), H);
    for (y, row) in text.iter().enumerate() {
        assert_eq!(row.chars().count(), W, "第 {y} 行宽度不对: {row:?}");
    }
    // 玩家在 (2,2)，其余是地板 '.'。
    assert_eq!(text[2], "..@.....", "实际行: {:?}", text);
    assert_eq!(text[0], "........");
}

/// 相机偏移必须生效：世界坐标 → 终端格的平移要对。
///
/// 用**非整数**中心（`x.75`）才能真正考验平移：`visible_rect().min` 会落在非整数
/// 上，实现必须 `floor` 到正确的格。整数中心下"平移对不对"是测不出来的。
#[test]
fn camera_offset_shifts_the_viewport() {
    let mut scene = base_scene();
    // 中心 (2.75, 1.75)、视口 4×2 → min_x = 0.75、min_y = 0.75 → floor 后起点 (0,0)。
    scene.camera = Camera2D::new((2.75, 1.75), (4, 2), 1.0);
    let text = lines_to_text(&build_map_lines(&scene, 4, 2));

    assert_eq!(text.len(), 2);
    assert_eq!(text[0], "....", "起点 (0,0) 的一行");
    assert_eq!(text[1], "....");

    // 中心右移一格 → 起点变成 (1,0)：(1,1) 的老鼠应当出现在第一列。
    scene.entities.push(
        EntityView::new(EntityId::from_bits(2), (1, 1), VisualKey::Monster(0)).with_name("老鼠"),
    );
    scene.camera = Camera2D::new((3.75, 1.75), (4, 2), 1.0);
    let text = lines_to_text(&build_map_lines(&scene, 4, 2));
    assert_eq!(
        text[1].chars().next(),
        Some('r'),
        "视口起点右移后，(1,1) 的老鼠应当出现在第一列，实际: {:?}",
        text[1]
    );
}

/// 实体优先于地形；玩家优先于同格的怪物。
#[test]
fn entities_are_drawn_over_terrain_and_player_wins_ties() {
    let mut scene = base_scene();
    // (1,1) 放一只老鼠；(2,2) 与玩家同格放一只哥布林。
    scene.entities.push(
        EntityView::new(EntityId::from_bits(2), (1, 1), VisualKey::Monster(0))
            .with_name("老鼠")
            .with_hp(Meter::new(5.0, 10.0)),
    );
    scene
        .entities
        .push(EntityView::new(EntityId::from_bits(3), (2, 2), VisualKey::Monster(2)).with_name("哥布林"));

    let text = lines_to_text(&build_map_lines(&scene, W, H));
    assert_eq!(text[1].chars().nth(1), Some('r'), "老鼠应当压过地板");
    assert_eq!(text[2].chars().nth(2), Some('@'), "同格时玩家优先于怪物");
}

/// **不可见的实体一律不画**（防"隔着墙看到怪物"）。
#[test]
fn invisible_entities_are_not_drawn() {
    let mut scene = base_scene();
    scene.entities.push(
        EntityView::new(EntityId::from_bits(2), (5, 1), VisualKey::Monster(0))
            .with_name("老鼠")
            .with_visibility(false, false),
    );

    let text = lines_to_text(&build_map_lines(&scene, W, H));
    assert_eq!(
        text[1].chars().nth(5),
        Some('.'),
        "不可见实体所在格应当画地形"
    );
}

/// 地形三态：可见（正常）、已探索（压暗）、未知（空白）。
#[test]
fn terrain_has_three_states() {
    let mut scene = base_scene();
    // (0,0) 可见墙；(1,0) 已探索但不可见；(2,0) 未知。
    scene.map.set_tile(0, 0, VisualKey::Tile(0)); // Wall
    scene.map.set_tile(1, 0, VisualKey::Tile(0));
    scene.map.set_visible(1, 0, false);
    scene.map.set_visible(2, 0, false);
    scene.map.set_explored(2, 0, false);

    let lines = build_map_lines(&scene, W, H);
    let text = lines_to_text(&lines);

    assert_eq!(text[0].chars().next(), Some('#'), "可见的墙画 #");

    let dimmed = &lines[0].spans[1];
    let visible = &lines[0].spans[0];
    assert_ne!(
        dimmed.style.fg, visible.style.fg,
        "已探索但不可见的格子必须用不同的前景色（压暗）"
    );
    assert_eq!(text[0].chars().nth(1), Some('#'));

    assert_eq!(
        text[0].chars().nth(2),
        Some(' '),
        "未知格子必须留空，不能泄露地形"
    );
}

/// 世界外的格子留空，不 panic。
#[test]
fn out_of_world_cells_are_blank() {
    let mut scene = base_scene();
    // 视口比世界大：右下角会落到世界外。
    scene.camera = Camera2D::new((4.0, 2.0), (16, 8), 1.0);
    let text = lines_to_text(&build_map_lines(&scene, 16, 8));

    assert_eq!(text.len(), 8);
    assert_eq!(text[7].chars().count(), 16);
    assert!(
        text[7].trim().is_empty(),
        "世界外的行应当全空白，实际: {:?}",
        text[7]
    );
}

/// **回归测试**：布局只能算一次。
///
/// 曾经 `render_map` 收的是"地图区"，却又调了一次 `map_viewport`（它内部会再算一次
/// `frame_areas`）——于是调试面板的高度被减了两次，地图区从 48×22 缩水成 22×14，
/// 而相机仍按 46×20 夹取，玩家一走出中心就被裁到画面外（`mvp_loop_test` 抓到了它）。
///
/// 这条测试钉住两个不变量：
///
/// 1. **自洽**：`map_viewport(整帧)` 必须等于 `frame_areas(整帧).camera` 去掉边框后的
///    尺寸——相机以为的格数 = 实际画的格数；
/// 2. **不可嵌套**：把地图区当输入再算一次布局会得到**不同**的结果，说明"拿子区域
///    当输入"是明确的错误用法，而不是碰巧能用。
#[test]
fn layout_is_computed_once_and_viewport_matches_the_drawn_area() {
    for terminal in [
        ratatui::layout::Rect::new(0, 0, 80, 30),
        ratatui::layout::Rect::new(0, 0, 120, 40),
        ratatui::layout::Rect::new(0, 0, 100, 30),
    ] {
        let areas = frame_areas(terminal);
        let inner_w = areas.camera.width.saturating_sub(2);
        let inner_h = areas.camera.height.saturating_sub(2);

        assert_eq!(
            map_viewport(terminal),
            (inner_w, inner_h),
            "整帧 {terminal:?}: 相机视口必须等于实际绘制的地图区尺寸"
        );
    }

    // 「不可嵌套」：120×40 下地图区本身仍然是四栏尺寸，再拿它当输入会得到
    // **不同**的地图矩形——说明"拿子区域当输入"是明确的错误用法，而不是碰巧能用。
    let full = frame_areas(ratatui::layout::Rect::new(0, 0, 120, 40));
    assert_ne!(
        frame_areas(full.camera).camera,
        full.camera,
        "对子区域再算布局应当得到不同结果（所以它必须只算一次）"
    );
}

/// 需求外的实体（Unknown key）画成 `?`，而不是静默当空气。
#[test]
fn unknown_visual_keys_render_as_question_mark() {
    let mut scene = base_scene();
    scene.map.set_tile(0, 0, VisualKey::Unknown(42));
    let text = lines_to_text(&build_map_lines(&scene, W, H));
    assert_eq!(text[0].chars().next(), Some('?'));
}

/// 整帧布局：用 `TestBackend` 走一遍真实绘制路径，确认不 panic 且四栏都在。
#[test]
fn full_frame_renders_without_panicking_and_shows_all_panels() {
    let scene = base_scene();
    let mut terminal = Terminal::new(TestBackend::new(80, 30)).expect("测试终端");

    terminal
        .draw(|frame| render_frame(frame, &scene, None))
        .expect("绘制不得失败");

    let text = buffer_text_compact(&terminal);

    assert!(text.contains("DungeonMVP"), "地图标题应当出现");
    assert!(text.contains("摄像机") || text.contains("DungeonMVP"), "地图栏");
    assert!(text.contains("信息"), "信息栏标题应当出现");
    assert!(text.contains("状态"), "状态栏标题应当出现");
    assert!(text.contains("事件"), "事件栏标题应当出现");
    // 调试面板在**没有调试输出时高度为 0**（SYN9：80×30 下它白占 7 行）。
    // 这里传的是 `None`，所以它**不该**出现——出现才是 bug。
    assert!(
        !text.contains("Debug"),
        "没有调试输出时不应给 Debug 面板留位置（SYN9）"
    );
    assert!(text.contains('@'), "玩家 glyph 应当出现");
    assert!(text.contains("HP"), "状态栏应当有 HP");
}

/// 四栏布局：80×30 下地图占左上、信息右上、事件左下、状态右下。
///
/// 比例由契约层定；这里断言后端**真的按契约画**：四栏互不重叠、拼起来铺满终端。
#[test]
fn four_panes_tile_the_surface_without_overlap() {
    let areas = frame_areas(Rect::new(0, 0, 80, 30));
    let panes = [areas.camera, areas.info, areas.status, areas.events];
    assert!(
        panes.iter().all(|pane| pane.width > 0 && pane.height > 0),
        "四栏都必须有面积"
    );

    assert_eq!(areas.camera.x, 0, "地图在顶行左侧");
    assert_eq!(areas.camera.y, 0, "地图在顶行");
    assert_eq!(areas.info.y, 0, "信息在顶行右侧");
    assert_eq!(areas.camera.right(), areas.info.x, "顶行两栏不留缝");
    assert_eq!(areas.info.right(), 80, "信息栏贴右边缘");

    assert_eq!(areas.events.y, areas.camera.bottom(), "事件在地图下方");
    assert_eq!(areas.status.y, areas.events.y, "状态与事件同排");
    assert_eq!(areas.status.x, areas.info.x, "右栏上下对齐");
    assert_eq!(areas.events.right(), areas.status.x, "底行两栏不留缝");
    assert_eq!(areas.events.bottom(), 30, "底行贴下边缘");
    assert_eq!(areas.camera.bottom(), areas.events.y, "顶行贴底行");
    assert_eq!(areas.events.height, areas.status.height, "底行两栏同高");
    assert_eq!(areas.camera.width, areas.events.width, "左列两栏同宽");
}

/// 焦点所在栏用青色描边（Tab 的视觉反馈）。
#[test]
fn the_focused_pane_gets_a_cyan_border() {
    let mut scene = base_scene();
    scene.focus = Pane::Events;
    let mut terminal = Terminal::new(TestBackend::new(80, 30)).expect("测试终端");
    terminal
        .draw(|frame| render_frame(frame, &scene, None))
        .expect("绘制不得失败");

    let areas = frame_areas(Rect::new(0, 0, 80, 30));
    let buffer = terminal.backend().buffer();
    let corner = &buffer[(areas.events.x, areas.events.y)];
    assert_eq!(corner.fg, Color::Cyan, "焦点栏的边框应当高亮");
    let other = &buffer[(areas.camera.x, areas.camera.y)];
    assert_ne!(other.fg, Color::Cyan, "非焦点栏不抢焦");
}

/// 退出对话框叠加在游戏画面上，且捕获输入。
#[test]
fn dialog_overlay_is_drawn_over_the_game() {
    let mut scene = base_scene();
    scene.ui = UiView::Dialog(DialogView::confirm("退出", "确认退出游戏？"));

    let mut terminal = Terminal::new(TestBackend::new(80, 30)).expect("测试终端");
    terminal
        .draw(|frame| render_frame(frame, &scene, None))
        .expect("绘制不得失败");

    let text = buffer_text_compact(&terminal);


    assert!(text.contains("确认退出游戏"), "对话框正文应当出现");
    // 游戏画面仍在（对话框是叠加，不是替换）。
    assert!(text.contains('@'), "对话框不该盖掉整个游戏画面");
}

/// Look 页叠加：显示光标坐标与地形名。
#[test]
fn look_overlay_shows_cursor_and_tile() {
    let mut scene = base_scene();
    scene.ui = UiView::Look(render_api::LookView {
        cursor: (3, 1),
        tile: Some(TileInfo::new(VisualKey::Tile(1), "地板").with_visibility(true, true)),
        entity: Some(
            EntityView::new(EntityId::from_bits(9), (3, 1), VisualKey::Monster(0)).with_name("老鼠"),
        ),
        footer: "方向键移动  Esc 关闭".to_string(),
    });

    let mut terminal = Terminal::new(TestBackend::new(80, 30)).expect("测试终端");
    terminal
        .draw(|frame| render_frame(frame, &scene, None))
        .expect("绘制不得失败");

    let text = buffer_text_compact(&terminal);


    assert!(text.contains("光标"), "应当显示光标坐标");
    assert!(text.contains("地板"), "应当显示地形名");
    assert!(text.contains("老鼠"), "应当显示光标处实体");
}

/// 极小终端不得 panic（布局里全是 `saturating_sub`）。
#[test]
fn tiny_terminal_does_not_panic() {
    let scene = base_scene();
    for (width, height) in [(1u16, 1u16), (3, 2), (10, 3), (20, 5)] {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("测试终端");
        terminal
            .draw(|frame| render_frame(frame, &scene, None))
            .unwrap_or_else(|err| panic!("{width}×{height} 下绘制失败: {err}"));
    }
}

/// 游戏结束的提示必须出现。
#[test]
fn game_over_notice_is_shown() {
    let mut scene = base_scene();
    scene.game_over = true;

    let mut terminal = Terminal::new(TestBackend::new(80, 30)).expect("测试终端");
    terminal
        .draw(|frame| render_frame(frame, &scene, None))
        .expect("绘制不得失败");

    let text = buffer_text_compact(&terminal);


    assert!(text.contains("游戏结束"), "标题应当变成游戏结束");
    assert!(text.contains("你死了"), "应当提示玩家已死");
}

/// **死亡提示不能被侧栏内容挤掉**（用眼睛玩才发现的问题）。
///
/// 侧栏先画死亡提示、再画楼层与"可见实体"；同屏实体很多时提示仍必须在
/// 面板内可见。这条测试把「拥挤侧栏 + 已死」这个最坏情况钉住。
#[test]
fn death_notice_survives_a_crowded_side_panel() {
    let mut scene = base_scene();
    scene.game_over = true;
    scene.entities = (0..40)
        .map(|i| {
            EntityView::new(EntityId::from_bits(100 + i), (i as i32 % 8, i as i32 % 4), VisualKey::Monster(0))
                .with_name(format!("噪声实体 {i}"))
                .with_visibility(true, true)
        })
        .collect();

    let mut terminal = Terminal::new(TestBackend::new(80, 30)).expect("测试终端");
    terminal
        .draw(|frame| render_frame(frame, &scene, None))
        .expect("绘制不得失败");

    let text = buffer_text_compact(&terminal);
    assert!(
        text.contains("你死了"),
        "侧栏再拥挤，死亡提示也必须在面板内可见"
    );
}

/// 日志面板已按玩家反馈去掉，改为**事件栏**：`scene.log` 从状态侧栏搬到
/// 左下的事件栏，旧事件在上、新事件在下。
#[test]
fn the_events_panel_renders_the_log_in_timeline_order() {
    let mut scene = base_scene();
    scene.log = vec![
        LogLine::new(LogLevel::Combat, "你击中了老鼠"),
        LogLine::new(LogLevel::Combat, "对老鼠造成 3 点伤害"),
    ];

    let mut terminal = Terminal::new(TestBackend::new(80, 30)).expect("测试终端");
    terminal
        .draw(|frame| render_frame(frame, &scene, None))
        .expect("绘制不得失败");

    // 宽字符在缓冲里占两格（续格是空格），所以先按行去掉空白再做 contains/顺序比较。
    let rows: Vec<String> = buffer_rows(&terminal)
        .iter()
        .map(|row| row.chars().filter(|ch| !ch.is_whitespace()).collect())
        .collect();
    let old_row = rows
        .iter()
        .position(|row| row.contains("你击中了老鼠"))
        .expect("旧事件应当出现在画面里");
    let new_row = rows
        .iter()
        .position(|row| row.contains("对老鼠造成3点伤害"))
        .expect("新事件应当出现在画面里");
    assert!(
        old_row < new_row,
        "事件栏是时间轴：旧事件在上、新事件在下（old={old_row}, new={new_row}）"
    );
    let text = buffer_text_compact(&terminal);
    assert!(!text.contains("日志"), "不应再有「日志」标题");
}

/// 未实现页给占位提示，而不是空白或崩溃。
#[test]
fn unimplemented_pages_show_a_placeholder() {
    let mut scene = base_scene();
    // 投掷页仍未迁移（S4），走占位分支。
    scene.ui = UiView::ThrowSelect(render_api::ListView::new("投掷"));

    let mut terminal = Terminal::new(TestBackend::new(80, 30)).expect("测试终端");
    terminal
        .draw(|frame| render_frame(frame, &scene, None))
        .expect("绘制不得失败");

    let text = buffer_text_compact(&terminal);

    assert!(text.contains("投掷系统尚未迁移"), "应当说明为何是空的");
}

/// 背包页是**真页面**：画出「身体 / 行囊」两栏与底部提示，且不再有占位文案（R2）。
#[test]
fn the_inventory_page_renders_two_panels() {
    use render_api::{InventoryPanel, InventoryPanelView, InventoryView, ListView};

    let mut scene = base_scene();
    let mut equipment = ListView::new("身体");
    equipment.items.push(render_api::ListItemView::new("主手：锈铁剑"));
    let mut backpack = ListView::new("行囊");
    backpack.items.push(render_api::ListItemView::new("生物血肉 ×2"));
    let mut inventory = InventoryView::default();
    inventory
        .panels
        .push(InventoryPanelView::new(InventoryPanel::Equipment, equipment));
    inventory
        .panels
        .push(InventoryPanelView::new(InventoryPanel::Backpack, backpack));
    inventory.footer = "↑↓ 选择    e/Esc 关闭".to_string();
    scene.ui = UiView::Inventory(inventory);

    let mut terminal = Terminal::new(TestBackend::new(80, 30)).expect("测试终端");
    terminal
        .draw(|frame| render_frame(frame, &scene, None))
        .expect("绘制不得失败");
    let text = buffer_text_compact(&terminal);

    assert!(text.contains("身体"), "左栏标题");
    assert!(text.contains("行囊"), "右栏标题");
    assert!(text.contains("主手：锈铁剑"), "装备行必须显示部位与物品名");
    assert!(text.contains("生物血肉"), "行囊行必须显示物品名");
    assert!(text.contains("关闭"), "底部提示（footer 画在页面最后一行）");
    assert!(
        !text.contains("尚未迁移"),
        "背包页已经不是占位页了（占位文案会误导玩家）"
    );
}

/// 构造一个典型背包视图：身体一栏 + 行囊一栏（带占用数与物品描述）。
fn inventory_view() -> InventoryView {
    let mut equipment = ListView::new("身体");
    equipment
        .items
        .push(ListItemView::new("主手：锈铁剑").with_detail("×1"));

    let mut backpack = ListView::new("行囊");
    backpack
        .items
        .push(ListItemView::new("生物血肉 ×2").with_detail("腥臭的肉块"));
    backpack.footer = "1/12".to_string();

    InventoryView {
        title: "背包".to_string(),
        panels: vec![
            InventoryPanelView::new(InventoryPanel::Equipment, equipment),
            InventoryPanelView::new(InventoryPanel::Backpack, backpack),
        ],
        focus: InventoryFocus::Panel(InventoryPanel::Equipment),
        detail: None,
        footer: "↑↓ 选择    e/Esc 关闭".to_string(),
    }
}

/// 把背包视图塞进场景，走真正的 [`render_frame`] 入口而不是直接调渲染函数。
fn inventory_scene(inventory: InventoryView) -> SceneFrame {
    let mut scene = base_scene();
    scene.ui = UiView::Inventory(inventory);
    scene
}

/// 带样式的详情视图：契约里 `Accent` 的行必须真的换颜色。
fn item_detail_view() -> DetailView {
    let mut detail = DetailView::new("物品详情");
    detail.push_line(UiTextLine::plain("锈铁剑"));
    detail.push_line(UiTextLine::styled("攻击 +1", UiTextStyle::Accent));
    detail.footer = "Enter 装卸".to_string();
    detail
}

/// 画一帧背包页并返回终端（缓冲留给测试断言格子颜色/符号）。
fn draw_inventory(
    inventory: InventoryView,
    width: u16,
    height: u16,
) -> Terminal<TestBackend> {
    let scene = inventory_scene(inventory);
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("测试终端");
    terminal
        .draw(|frame| render_frame(frame, &scene, None))
        .expect("绘制不得失败");
    terminal
}

/// 面板内区（边框内缩 1 列/行）。
fn panel_inner(panel: Rect) -> Rect {
    Rect {
        x: panel.x + 1,
        y: panel.y + 1,
        width: panel.width.saturating_sub(2),
        height: panel.height.saturating_sub(2),
    }
}

/// 选中行必须按**终端列宽**铺满整行反色：中文标签后要补空格到栏尾，
/// 只反色到字尾会让"选中"看起来像裁了一半。
#[test]
fn inventory_selected_row_highlights_every_column_of_the_panel() {
    let inventory = inventory_view();
    let terminal = draw_inventory(inventory.clone(), 80, 30);
    let layout = super::inventory::inventory_layout(Rect::new(0, 0, 80, 30), &inventory);
    let inner = panel_inner(layout.panels[0]);
    let buffer = terminal.backend().buffer();
    let row = inner.y;

    // 宽字符的续格在缓冲里是空符号（背景被重置），那一格由终端用字形自己的
    // 样式铺满，不能按普通格子断言；其余有字形的格子都必须反色。
    for x in inner.x..inner.x + inner.width {
        let cell = &buffer[(x, row)];
        if cell.symbol() == " " {
            continue;
        }
        assert_eq!(
            cell.bg,
            Color::Cyan,
            "选中行第 {x} 列的可见字符没有反色：整行宽度必须按终端列算"
        );
    }
    assert_eq!(buffer[(inner.x, row)].symbol(), "▸", "选中行要有可辨认的标记");
    assert_eq!(buffer[(inner.x, row)].bg, Color::Cyan);
    assert_eq!(
        buffer[(inner.x + inner.width - 1, row)].symbol(),
        " ",
        "行尾应当是补出来的空格（说明按显示宽度补齐，而不是 chars().count()）"
    );
    assert_eq!(
        buffer[(inner.x + inner.width - 1, row)].bg,
        Color::Cyan,
        "补出来的空格也必须反色，否则选中行会短一截"
    );
}

/// 中文宽度按终端列算：全角 2 列，歧义宽度字符与空格按 1 列。
#[test]
fn cjk_width_is_measured_in_terminal_columns() {
    use super::inventory::{display_width, truncate_to_width};

    assert_eq!(display_width("主手：锈铁剑"), 12);
    assert_eq!("主手：锈铁剑".chars().count(), 6, "字符数不等于列宽");
    assert_eq!(display_width("↑↓ 选择"), 7, "箭头/空格 1 列，汉字 2 列");
    assert_eq!(display_width("abc"), 3);

    let clipped = truncate_to_width("锈铁剑", 5);
    assert_eq!(clipped, "锈铁…", "截断必须落在字符边界并留省略号");
    assert!(display_width(&clipped) <= 5);
    assert_eq!(truncate_to_width("abc", 3), "abc", "放得下就不动");
    assert_eq!(truncate_to_width("锈铁剑", 0), "", "0 列不画任何东西");
}

/// 详情从「右侧栏」到「底部区块」再到「不画」的三档降级；窄屏两栏改上下堆叠。
#[test]
fn inventory_layout_degrades_from_side_detail_to_stacked_without_detail() {
    let mut inventory = inventory_view();
    inventory.detail = Some(item_detail_view());

    let wide = super::inventory::inventory_layout(Rect::new(0, 0, 120, 40), &inventory);
    assert_eq!(wide.panels.len(), 2);
    assert_eq!(wide.panels[0].y, wide.panels[1].y, "宽屏两栏并排");
    assert!(
        wide.panels[0].right() < wide.panels[1].x,
        "两栏之间留 1 列间隔"
    );
    let side = wide.detail.expect("120×40 应当画右侧详情");
    assert!(side.x > wide.panels[1].right(), "详情在行囊栏右侧");
    assert_eq!(side.height, wide.panels[0].height, "详情与两栏同高");
    assert!(wide.footer.is_some(), "宽屏保留底部提示行");

    let medium = super::inventory::inventory_layout(Rect::new(0, 0, 80, 30), &inventory);
    let bottom = medium.detail.expect("80×30 应当画底部详情");
    assert!(bottom.y >= medium.panels[0].bottom(), "底部详情不压住两栏");
    assert_eq!(bottom.width, medium.content.width, "底部详情占满可用宽度");

    let narrow = super::inventory::inventory_layout(Rect::new(0, 0, 40, 30), &inventory);
    assert_eq!(narrow.panels.len(), 2);
    assert!(
        narrow.panels[0].bottom() < narrow.panels[1].y,
        "40 列放不下并排两栏，应当上下堆叠"
    );
    assert!(narrow.detail.is_none(), "40 列放不下详情，宁可降级也不挤");
    assert!(
        narrow.panels.iter().all(|rect| rect.width == 38),
        "堆叠时两栏都占满内容宽度"
    );

    let mut empty = inventory.clone();
    empty.detail = Some(DetailView::new("空"));
    let layout = super::inventory::inventory_layout(Rect::new(0, 0, 120, 40), &empty);
    assert!(layout.detail.is_none(), "lines 为空的详情不占版面");
}

/// 底部提示按「≥2 个连续空格」分组，组间画分隔符；组数是契约决定的，不写死。
#[test]
fn inventory_footer_groups_hint_clusters_with_separators() {
    let mut inventory = inventory_view();
    inventory.footer = "↑↓ 选择    Enter 装卸 · r 使用 · d 丢弃    e/Esc 关闭".to_string();
    let terminal = draw_inventory(inventory, 80, 30);
    let rows = buffer_rows(&terminal);
    // 宽字符续格/普通空格在缓冲里都是空格符号，先滤掉再断言文本。
    let footer: String = rows[rows.len() - 2]
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect();

    assert!(footer.contains("选择"), "第一组提示: {footer:?}");
    assert!(footer.contains("装卸"), "第二组提示: {footer:?}");
    assert!(footer.contains("关闭"), "第三组提示: {footer:?}");
    // 行首/行尾各有一个页面边框的 `│`，分隔符只数中间那段。
    let hint = footer
        .strip_prefix('│')
        .and_then(|text| text.strip_suffix('│'))
        .unwrap_or(&footer);
    assert_eq!(
        hint.matches('│').count(),
        2,
        "三组之间各一个分隔符: {footer:?}"
    );
}

/// 详情页：标题/正文/样式/底部提示都要画，焦点边框要压在详情上。
#[test]
fn inventory_detail_panel_draws_lines_and_focus_border() {
    let mut inventory = inventory_view();
    inventory.detail = Some(item_detail_view());
    inventory.focus = InventoryFocus::Detail;
    let terminal = draw_inventory(inventory.clone(), 120, 40);
    let layout = super::inventory::inventory_layout(Rect::new(0, 0, 120, 40), &inventory);
    let detail = layout.detail.expect("120×40 应当有右侧详情");
    let buffer = terminal.backend().buffer();
    let middle = detail.y + detail.height / 2;

    assert_eq!(
        buffer[(detail.x, middle)].fg,
        Color::Cyan,
        "焦点在详情：详情边框高亮"
    );
    assert_ne!(
        buffer[(layout.panels[0].x, middle)].fg,
        Color::Cyan,
        "非焦点栏的边框不抢焦"
    );

    let text = buffer_text_compact(&terminal);
    assert!(text.contains("物品详情"), "详情标题");
    assert!(text.contains("攻击+1"), "契约里的 Accent 行必须画出来");
    assert!(text.contains("Enter装卸"), "详情底部提示");
}

/// 80×30（常见终端）的详情预算：真实详情（说明+类型+攻速+修正+部位+可装+动作）
/// 要全部放得下。这是"底部详情占 45% 高度"这条取舍的验收点。
#[test]
fn inventory_detail_at_80x30_fits_typical_content() {
    let mut inventory = inventory_view();
    let mut detail = DetailView::new("锈铁剑");
    for text in [
        "剑身布满锈迹，但仍能割开血肉",
        "类型：装备",
        "主手攻速：1200ms",
        "攻击 +1",
        "移速 -5%",
        "无视：浅水",
        "部位：主手",
        "可装：主手",
    ] {
        detail.push_line(UiTextLine::plain(text));
    }
    detail.footer = "Enter 卸下    d 丢弃".to_string();
    inventory.detail = Some(detail);

    let terminal = draw_inventory(inventory, 80, 30);
    let text = buffer_text_compact(&terminal);
    for expected in [
        "类型：装备",
        "主手攻速：1200ms",
        "攻击+1",
        "移速-5%",
        "无视：浅水",
        "部位：主手",
        "可装：主手",
        "Enter卸下",
    ] {
        assert!(
            text.contains(expected),
            "80×30 的详情应当完整显示 {expected}，实际画面：{text}"
        );
    }
}

/// 占位行（"（…）"）要弱化；焦点不在行囊时，行囊选中行只留标记不铺底色。
#[test]
fn inventory_placeholder_rows_are_dimmed_and_unfocused_selection_stays_marked() {
    let mut inventory = inventory_view();
    inventory.panels[1].list.items = vec![
        ListItemView::new("生物血肉 ×2"),
        ListItemView::new("（行囊是空的）"),
    ];
    let terminal = draw_inventory(inventory.clone(), 80, 30);
    let layout = super::inventory::inventory_layout(Rect::new(0, 0, 80, 30), &inventory);
    let panel = layout.panels[1];
    let buffer = terminal.backend().buffer();

    let selected = &buffer[(panel.x + 1, panel.y + 1)];
    assert_eq!(selected.symbol(), "▸", "行囊的选中项仍有标记");
    assert_ne!(selected.bg, Color::Cyan, "焦点在身体栏，行囊选中行不铺底色");

    let placeholder = &buffer[(panel.x + 3, panel.y + 2)];
    assert_eq!(placeholder.fg, Color::DarkGray, "占位行要弱化，不能和真物品一样亮");
}

/// 选中项超出可视行数时列表必须滚动：按 ↓ 不能选到屏幕外。
#[test]
fn inventory_panel_scrolls_to_keep_the_selected_item_visible() {
    let mut inventory = inventory_view();
    let items = (0..20)
        .map(|index| ListItemView::new(format!("道具{index:02}")))
        .collect::<Vec<_>>();
    inventory.panels[1].list = ListView::new("行囊").with_items(items);
    inventory.panels[1].list.selected = 15;
    inventory.focus = InventoryFocus::Panel(InventoryPanel::Backpack);

    let terminal = draw_inventory(inventory, 80, 20);
    let text = buffer_text_compact(&terminal);

    assert!(text.contains("道具15"), "选中项必须可见");
    assert!(!text.contains("道具00"), "列表应当滚动，不能永远从第 0 项开始画");
}

/// 40 列窄屏：两栏上下堆叠，正文与底部提示都不能丢。
#[test]
fn inventory_narrow_terminal_stacks_panels_and_keeps_footer() {
    let terminal = draw_inventory(inventory_view(), 40, 30);
    let text = buffer_text_compact(&terminal);

    assert!(text.contains("身体"), "窄屏也要有身体栏");
    assert!(text.contains("行囊"), "窄屏也要有行囊栏");
    assert!(text.contains("关闭"), "窄屏也要保留底部提示");

    let rows = buffer_rows(&terminal);
    let compact: Vec<String> = rows
        .iter()
        .map(|row| row.chars().filter(|ch| !ch.is_whitespace()).collect())
        .collect();
    let body_row = compact
        .iter()
        .position(|row| row.contains("身体"))
        .expect("身体标题行");
    let pack_row = compact
        .iter()
        .position(|row| row.contains("行囊"))
        .expect("行囊标题行");
    assert!(body_row < pack_row, "40 列放不下两栏，应当上下堆叠");
}

/// 极小/极窄终端只要求「不 panic、不越界」，不要求好看。
#[test]
fn inventory_page_survives_tiny_and_narrow_terminals() {
    let mut inventory = inventory_view();
    inventory.detail = Some(item_detail_view());
    let scene = inventory_scene(inventory);

    for (width, height) in [
        (1u16, 1u16),
        (2, 2),
        (3, 2),
        (10, 3),
        (20, 5),
        (40, 10),
        (40, 30),
        (48, 16),
        (64, 20),
        (100, 12),
        (120, 40),
    ] {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("测试终端");
        terminal
            .draw(|frame| render_frame(frame, &scene, None))
            .unwrap_or_else(|err| panic!("{width}×{height} 下画背包页失败: {err}"));
    }
}

/// 空场景（首帧前）不得 panic。
#[test]
fn empty_scene_renders_without_panicking() {
    let scene = SceneFrame::empty();
    let mut terminal = Terminal::new(TestBackend::new(40, 12)).expect("测试终端");
    terminal
        .draw(|frame| render_frame(frame, &scene, None))
        .expect("空场景也应当能画");
}

/// 实体分层的绘制顺序：地形层先画、actor 后画（保证怪物压在地形上）。
#[test]
fn layer_order_puts_actors_above_terrain() {
    let mut scene = base_scene();
    // 楼梯（Terrain 层）与怪物（Actor 层）同格：actor 应当赢。
    scene
        .entities
        .push(EntityView::new(EntityId::from_bits(4), (4, 0), VisualKey::Stairs).with_layer(VisualLayer::Terrain));
    scene
        .entities
        .push(EntityView::new(EntityId::from_bits(5), (4, 0), VisualKey::Monster(1)).with_layer(VisualLayer::Actor));

    let text = lines_to_text(&build_map_lines(&scene, W, H));
    assert_eq!(
        text[0].chars().nth(4),
        Some('s'),
        "同格时 actor 层的怪物应当压过 Terrain 层的楼梯"
    );
}
