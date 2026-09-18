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

use ratatui::{Terminal, backend::TestBackend};
use render_api::{
    Camera2D, DialogView, EntityId, EntityView, HudView, LogLevel, LogLine, MapView, Meter,
    SceneFrame, TileInfo, UiView, VisualKey, VisualLayer,
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
/// 1. **自洽**：`map_viewport(整帧)` 必须等于 `frame_areas(整帧).map` 去掉边框后的
///    尺寸——相机以为的格数 = 实际画的格数；
/// 2. **不可嵌套**：把地图区当输入再算一次布局会得到**不同**的结果，说明"拿子区域
///    当输入"是明确的错误用法，而不是碰巧能用。
#[test]
fn layout_is_computed_once_and_viewport_matches_the_drawn_area() {
    for terminal in [
        ratatui::layout::Rect::new(0, 0, 80, 30),
        ratatui::layout::Rect::new(0, 0, 120, 40),
        ratatui::layout::Rect::new(0, 0, 60, 24),
    ] {
        let areas = frame_areas(terminal);
        let inner_w = areas.map.width.saturating_sub(2);
        let inner_h = areas.map.height.saturating_sub(2);

        assert_eq!(
            map_viewport(terminal),
            (inner_w, inner_h),
            "整帧 {terminal:?}: 相机视口必须等于实际绘制的地图区尺寸"
        );

        assert_ne!(
            frame_areas(areas.map).map,
            areas.map,
            "对子区域再算布局应当得到不同结果（所以它必须只算一次）"
        );
    }
}

/// 需求外的实体（Unknown key）画成 `?`，而不是静默当空气。
#[test]
fn unknown_visual_keys_render_as_question_mark() {
    let mut scene = base_scene();
    scene.map.set_tile(0, 0, VisualKey::Unknown(42));
    let text = lines_to_text(&build_map_lines(&scene, W, H));
    assert_eq!(text[0].chars().next(), Some('?'));
}

/// 整帧布局：用 `TestBackend` 走一遍真实绘制路径，确认不 panic 且各区块都在。
#[test]
fn full_frame_renders_without_panicking_and_shows_all_panels() {
    let scene = base_scene();
    let mut terminal = Terminal::new(TestBackend::new(80, 30)).expect("测试终端");

    terminal
        .draw(|frame| render_frame(frame, &scene, None))
        .expect("绘制不得失败");

    let text = buffer_text_compact(&terminal);


    assert!(text.contains("DungeonMVP"), "标题应当出现");
    assert!(text.contains("状态"), "侧栏标题应当出现");
    assert!(text.contains("Debug"), "调试面板应当出现");
    assert!(text.contains('@'), "玩家 glyph 应当出现");
    assert!(text.contains("HP"), "状态栏应当有 HP");
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

/// 日志按等级上色（颜色来自 catalog，不散在渲染代码里）。
#[test]
fn log_lines_use_catalog_colors() {
    let mut scene = base_scene();
    scene.log = vec![
        LogLine::new(LogLevel::Combat, "对老鼠造成 3 点伤害"),
        LogLine::new(LogLevel::Danger, "你死了"),
    ];

    let mut terminal = Terminal::new(TestBackend::new(60, 30)).expect("测试终端");
    terminal
        .draw(|frame| render_frame(frame, &scene, None))
        .expect("绘制不得失败");

    let text = buffer_text_compact(&terminal);

    assert!(text.contains("对老鼠造成3点伤害"), "日志正文应当出现");
    assert!(text.contains("你死了"));
}

/// 未实现页给占位提示，而不是空白或崩溃。
#[test]
fn unimplemented_pages_show_a_placeholder() {
    let mut scene = base_scene();
    scene.ui = UiView::Inventory(render_api::InventoryView::default());

    let mut terminal = Terminal::new(TestBackend::new(80, 30)).expect("测试终端");
    terminal
        .draw(|frame| render_frame(frame, &scene, None))
        .expect("绘制不得失败");

    let text = buffer_text_compact(&terminal);

    assert!(text.contains("物品系统尚未迁移"), "应当说明为何是空的");
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
