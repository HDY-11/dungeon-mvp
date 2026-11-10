//! `SceneFrame` → ratatui 绘制。
//!
//! # 这层的硬约束
//!
//! 只消费 [`SceneFrame`]，**不认识 `ecs_core`**。这条边界由 `Cargo.toml` 强制
//! （`tui` 的依赖表里没有 `ecs_core`），所以"顺手查一下 core 组件"在编译期就
//! 不可能。这正是 Dsn28 要的可替换性：`GpuPlugin` 消费同一个 `SceneFrame`。
//!
//! # 四栏布局
//!
//! 栏目划分**不在本文件**：它是契约层的 [`render_api::game_layout`]，焦点轮转
//! 与鼠标命中读同一份矩形。这里只把 [`render_api::Rect`] 换成 ratatui 的
//! `Rect`，然后往四个栏位里画内容。
//!
//! # 相机
//!
//! 视口位置由 `frame.camera` 决定（`presentation` 算好的，含世界边界夹取），
//! 这里只做「世界格 → 终端格」的平移。**不要**在这层重新算相机：那会让
//! TUI 与 GPU 的取景不一致，而且把世界尺寸的知识漏进后端。

use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};
use render_api::{EntityView, Pane, SceneFrame, UiView};

use crate::catalog::TuiCatalog;
use crate::layout::inner_rect;

mod inventory;

use inventory::render_inventory_page;

/// 一整帧的区块划分（由契约层的四栏布局换算而来）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameAreas {
    /// 全部可用区域。
    pub full: Rect,
    /// 地图栏（含边框）。
    pub camera: Rect,
    /// 信息栏（含边框）。
    pub info: Rect,
    /// 状态栏（含边框）。
    pub status: Rect,
    /// 事件栏（含边框）。
    pub events: Rect,
}

/// 计算区块划分。**这是 TUI 侧布局的唯一入口**——它只是契约布局的换算，
/// 不重新决定比例。
pub fn frame_areas(area: Rect) -> FrameAreas {
    let layout = render_api::game_layout((area.width, area.height));
    let offset = |rect: render_api::Rect| Rect {
        x: area.x.saturating_add(rect.x),
        y: area.y.saturating_add(rect.y),
        width: rect.width,
        height: rect.height,
    };
    FrameAreas {
        full: area,
        camera: offset(layout.camera),
        info: offset(layout.info),
        status: offset(layout.status),
        events: offset(layout.events),
    }
}

/// 地图栏里**可画世界格**的尺寸（去掉边框）。
///
/// 这就是 [`render_api::Camera2D`] 该收到的视口。返回 `(0, 0)` 表示"太小/未知"，
/// 相机会退回世界中心而不是 panic。
pub fn map_viewport(area: Rect) -> (u16, u16) {
    let camera = inner_rect(frame_areas(area).camera, 1);
    (camera.width, camera.height)
}

/// 画一整帧。
pub fn render_frame(terminal_frame: &mut Frame, scene: &SceneFrame) {
    let areas = frame_areas(terminal_frame.area());

    match &scene.ui {
        // 背包页是全屏页（Dsn21）：身体与行囊两栏并排，其余页面叠加在游戏画面上。
        UiView::Inventory(inventory) => render_inventory_page(terminal_frame, areas.full, inventory),
        ui => {
            render_map(terminal_frame, areas.camera, scene);
            render_info(terminal_frame, areas.info, scene);
            render_status(terminal_frame, areas.status, scene);
            render_events(terminal_frame, areas.events, scene);

            match ui {
                UiView::Look(look) => render_look_overlay(terminal_frame, areas.camera, look),
                UiView::Dialog(dialog) => render_dialog_overlay(terminal_frame, areas.full, dialog),
                UiView::ThrowAim(aim) => {
                    render_throw_aim_overlay(terminal_frame, areas.camera, scene, aim)
                }
                UiView::ThrowSelect(_) => render_placeholder(
                    terminal_frame,
                    areas.full,
                    "投掷",
                    "投掷选择页尚未迁移（当前直接进入瞄准页）。",
                ),
                _ => {}
            }
        }
    }
}

/// 栏位外框：聚焦时用青色描边，让"Tab 现在在哪一栏"一眼可见。
fn pane_block(title: &str, focused: bool) -> Block<'_> {
    let block = Block::default().borders(Borders::ALL).title(title);
    if focused {
        block.border_style(Style::default().fg(Color::Cyan))
    } else {
        block
    }
}

/// 画地图。
fn render_map(frame: &mut Frame, area: Rect, scene: &SceneFrame) {
    let inner = inner_rect(area, 1);
    let viewport = (inner.width as usize, inner.height as usize);
    let lines = build_map_lines(scene, viewport.0, viewport.1);

    let title = if scene.game_over {
        " 游戏结束 "
    } else {
        " Dungeon MVP (ecs_core) "
    };
    let block = pane_block(title, scene.focus == Pane::Camera).title_alignment(Alignment::Center);
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

/// 画信息栏：**当前可见的观察候选**（名字 + 类型 + 血量条 + 距离），以及键位提示。
///
/// 顺序由契约层的 [`render_api::InfoView`] 给出（近→远，同距按方位顺时针），
/// 后端不再自己排序。焦点在信息栏时 `↑/↓` 选中一行，选中对象在地图上高亮。
/// 每个候选画两行（名字 / 细节），可见行不够时窗口跟着选中行走——否则"选中了
/// 却看不见"比没有这一栏更糟。
fn render_info(frame: &mut Frame, area: Rect, scene: &SceneFrame) {
    let focused = scene.focus == Pane::Info;
    let selected = scene.info.selected_id();
    let player_pos = scene.player.as_ref().map(|player| player.position);
    let mut rows: Vec<Line> = Vec::new();
    for id in &scene.info.entries {
        let Some(entity) = scene.entities.iter().find(|entity| entity.id == *id) else {
            // 候选来自同一帧的实体列表；这里找不到说明提取器产出了悬空 id。
            continue;
        };
        let is_selected = selected == Some(*id);
        let marker = if is_selected { '>' } else { ' ' };
        let style = if is_selected {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };
        rows.push(Line::from(Span::styled(
            format!(
                "{marker}{} {}",
                TuiCatalog::glyph(entity.visual),
                entity.name
            ),
            style,
        )));
        rows.push(Line::from(Span::styled(
            entity_detail(entity, player_pos),
            Style::default().fg(Color::DarkGray),
        )));
    }
    if rows.is_empty() {
        rows.push(Line::from(Span::styled(
            "（视野内没有可关注的东西）",
            Style::default().fg(Color::DarkGray),
        )));
    }

    // 边框内可用行数减去底部两行提示。
    let inner_height = usize::from(area.height.saturating_sub(2)).max(1);
    let visible = inner_height.saturating_sub(2).max(1);
    let selected_row = scene
        .info
        .selected
        .map(|index| index.min(scene.info.entries.len().saturating_sub(1)) * 2);
    let start = match selected_row {
        Some(row) if row + 2 > visible => row + 2 - visible,
        _ => 0,
    };
    let end = (start + visible).min(rows.len());
    let mut lines: Vec<Line> = rows[start..end].to_vec();
    lines.push(Line::from(""));
    let hint = if focused {
        "↑↓：选择对象      Tab：切换焦点"
    } else {
        "Tab：切换焦点"
    };
    lines.push(Line::from(Span::styled(
        hint,
        Style::default().fg(Color::DarkGray),
    )));
    frame.render_widget(
        Paragraph::new(lines)
            .block(pane_block("信息", focused))
            .wrap(ratatui::widgets::Wrap { trim: true }),
        area,
    );
}

/// 信息栏候选的类型标签（只用于显示，不是领域分类）。
fn info_kind(visual: render_api::VisualKey) -> &'static str {
    use render_api::VisualKey as V;
    match visual {
        V::Monster(_) => "怪物",
        V::Stairs => "楼梯",
        V::Item(_) => "物品",
        V::Corpse => "尸体",
        V::Effect(_) => "效果",
        _ => "对象",
    }
}

/// 信息栏的细节行：类型 · 血量条（有血才有） · 距离。
///
/// 距离与排序用同一口径（欧氏距离平方排序后取整数距离），避免"列表顺序是近→远
/// 但距离数字看起来跳序"。玩家位置缺失时（死亡/切页中途）省略距离。
fn entity_detail(entity: &EntityView, player_pos: Option<(i32, i32)>) -> String {
    let mut parts: Vec<String> = vec![info_kind(entity.visual).to_string()];
    if let Some(hp) = entity.hp.filter(|hp| hp.max > 0.0) {
        parts.push(format!(
            "HP {} {:.0}/{:.0}",
            health_bar(hp.current, hp.max),
            hp.current,
            hp.max
        ));
    }
    if let Some((px, py)) = player_pos {
        let dx = f64::from(entity.position.0 - px);
        let dy = f64::from(entity.position.1 - py);
        let distance = dx.hypot(dy).round() as usize;
        parts.push(format!("{distance} 格"));
    }
    format!("  {}", parts.join(" · "))
}

/// 纯文本血量条：10 格宽，满血 `[##########]`、空血 `[----------]`。
fn health_bar(current: f32, max: f32) -> String {
    const WIDTH: usize = 10;
    let ratio = (current / max).clamp(0.0, 1.0);
    let filled = ((ratio * WIDTH as f32).round() as usize).min(WIDTH);
    format!(
        "[{}{}]",
        "#".repeat(filled),
        "-".repeat(WIDTH - filled)
    )
}

/// 画状态栏：楼层、玩家属性、扩展状态与键位提示。
fn render_status(frame: &mut Frame, area: Rect, scene: &SceneFrame) {
    let mut lines = Vec::new();

    // 死亡提示必须排在最前：面板再挤也不能让"你死了"被挤出可视区。
    if scene.game_over {
        lines.push(Line::from(Span::styled(
            "你死了。按 q 退出。",
            Style::default().fg(Color::Red),
        )));
        lines.push(Line::from(""));
    }

    lines.push(Line::from(format!("楼层：{}", scene.hud.floor)));
    if !scene.hud.map_kind.is_empty() {
        lines.push(Line::from(format!("地图类型：{}", scene.hud.map_kind)));
    }

    if scene.hud.is_ready() {
        lines.push(Line::from(Span::styled(
            format!("{} Lv.{}", scene.hud.player_name, scene.hud.level),
            Style::default().fg(Color::Yellow),
        )));
        lines.push(Line::from(format!(
            "HP：{:.0} / {:.0}",
            scene.hud.hp.current, scene.hud.hp.max
        )));
        lines.push(Line::from(format!(
            "MP：{:.0} / {:.0}",
            scene.hud.mp.current, scene.hud.mp.max
        )));
        lines.push(Line::from(format!(
            "EXP：{:.0} / {:.0}",
            scene.hud.exp.current, scene.hud.exp.max
        )));
        // 扩展信息（C3 起：装备加成、中毒、行囊载重等）。
        for line in &scene.hud.extra_lines {
            let text: String = line.spans.iter().map(|span| span.text.as_str()).collect();
            if !text.is_empty() {
                lines.push(Line::from(Span::styled(text, Style::default().fg(Color::Red))));
            }
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "方向键/hjkl：移动或攻击    .：等待",
        Style::default().fg(Color::DarkGray),
    )));
    lines.push(Line::from(Span::styled(
        "1/2/3：施法（法术见扩展行）",
        Style::default().fg(Color::DarkGray),
    )));
    lines.push(Line::from(Span::styled(
        "e：背包    t：投掷    ?：查看    q/Esc：退出",
        Style::default().fg(Color::DarkGray),
    )));

    frame.render_widget(
        Paragraph::new(lines)
            .block(pane_block("状态", scene.focus == Pane::Status))
            .wrap(ratatui::widgets::Wrap { trim: true }),
        area,
    );
}

/// 画事件栏：一条"截止现在"的时间轴（DsnE21）。
///
/// 已落定事实在前（旧→新）、进行中的行动在后（剩余时间降序），栏底即 0 刻度；
/// 默认自动贴底显示最新（`events_scroll == 0`）。焦点在事件栏时可以向上翻
/// 历史（DsnA6）：`events_scroll` 是从底部往上裁掉的行数，标题上标出当前偏移，
/// 滚回 0 就恢复跟随最新。
///
/// 可读性（DsnE25）：每条带等级前缀符号；聚焦时底部留一行翻页提示。
/// 事实与进行中是**同一条时间轴**（DsnE21 统一语义），不插分隔行——已落定的
/// 事实在上、未落定的行动在下，由排序本身表达先后，不靠装饰性标题。
fn render_events(frame: &mut Frame, area: Rect, scene: &SceneFrame) {
    let focused = scene.focus == Pane::Events;
    let inner_height = usize::from(area.height.saturating_sub(2)).max(1);
    // 聚焦时底部留一行提示；太矮就不留，优先给事件本身。
    let hint_rows = usize::from(focused && inner_height >= 4);
    let visible = inner_height.saturating_sub(hint_rows).max(1);

    let mut rows: Vec<Line> = Vec::new();
    for line in &scene.events {
        let prefix = crate::catalog::log_prefix(line.level);
        let color = if line.running {
            // 进行中：暗一点，表示"还没落定"；剩余 AV 已经写在文案里。
            Color::DarkGray
        } else {
            crate::catalog::log_color(line.level)
        };
        rows.push(Line::from(Span::styled(
            format!("{prefix} {}", line.text),
            Style::default().fg(color),
        )));
    }
    if rows.is_empty() {
        rows.push(Line::from(Span::styled(
            "（暂无事件）",
            Style::default().fg(Color::DarkGray),
        )));
    }

    // 偏移超过可滚范围时按上限夹取（页栈只记"想滚多少"，上限由可见高度决定）。
    let max_scroll = rows.len().saturating_sub(visible);
    let scroll = scene.events_scroll.min(max_scroll);
    let title = if scroll > 0 {
        format!("事件 ↑{scroll}")
    } else {
        "事件".to_string()
    };

    let end = rows.len() - scroll;
    let start = end.saturating_sub(visible);
    let mut lines: Vec<Line> = rows[start..end].to_vec();
    if hint_rows > 0 {
        lines.push(Line::from(Span::styled(
            "↑↓：翻历史    ↓ 到底：跟随最新",
            Style::default().fg(Color::DarkGray),
        )));
    }

    frame.render_widget(
        Paragraph::new(lines).block(pane_block(&title, focused)),
        area,
    );
}

/// 把可见区域渲染成终端行。
///
/// 抽成公开函数是为了让 golden 测试**不经过 `Terminal`** 就能断言画面内容
/// （`TestBackend` 也能测，但拿 `Vec<Line>` 更适合断言"这一格画了什么"）。
pub fn build_map_lines(
    scene: &SceneFrame,
    viewport_w: usize,
    viewport_h: usize,
) -> Vec<Line<'static>> {
    let rect = scene.camera.visible_rect();
    // 相机中心是格子中心（x.5），可见矩形左上角因此落在半格处；
    // `floor` 得到第一个完整可见格。相机已由 `presentation` 夹在世界内，
    // 所以这里不需要再防越界（`MapView::tile` 本身也返回 Option）。
    let origin_x = rect.min_x.floor() as i32;
    let origin_y = rect.min_y.floor() as i32;

    let mut lines = Vec::with_capacity(viewport_h);
    for row in 0..viewport_h {
        let mut spans = Vec::with_capacity(viewport_w);
        for col in 0..viewport_w {
            let (x, y) = (origin_x + col as i32, origin_y + row as i32);
            spans.push(cell_span(scene, x, y));
        }
        lines.push(Line::from(spans));
    }
    lines
}

/// 单格的 span：实体 > 地形（可见 > 记忆）> 空白。
///
/// 顺序与取舍都在这里，说明如下：
///
/// - 实体压过地形：怪物站在地板上时看到的是怪物；
/// - **看不见的实体一律不画**——否则会出现"隔着墙看到怪物"，或者看到怪物上一帧
///   的位置；
/// - 地形三态：可见（正常配色）> 已探索（压暗）> 未知（空白）。
fn cell_span(scene: &SceneFrame, x: i32, y: i32) -> Span<'static> {
    if x < 0 || y < 0 {
        return blank_span();
    }
    let (ux, uy) = (x as usize, y as usize);
    let Some(tile) = scene.map.tile(ux, uy) else {
        return blank_span();
    };
    // 地形的底色先算好：实体站在水面上时背景仍然是水。
    let tile_bg = if scene.map.is_visible(ux, uy) {
        TuiCatalog::bg(tile).unwrap_or(Color::Reset)
    } else {
        TuiCatalog::dim_bg(tile).unwrap_or(Color::Reset)
    };

    if let Some(entity) = visible_entity_at(scene, x, y) {
        let mut style = Style::default()
            .fg(TuiCatalog::fg(entity.visual))
            .bg(tile_bg);
        // 信息栏选中的对象在地图上反色，玩家不用在列表和地图之间来回找。
        if scene.info.selected_id() == Some(entity.id) {
            style = style.fg(Color::Black).bg(Color::Yellow);
        }
        return Span::styled(TuiCatalog::glyph(entity.visual).to_string(), style);
    }

    if scene.map.is_visible(ux, uy) {
        Span::styled(
            TuiCatalog::glyph(tile).to_string(),
            Style::default().fg(TuiCatalog::fg(tile)).bg(tile_bg),
        )
    } else if scene.map.is_explored(ux, uy) {
        Span::styled(
            TuiCatalog::glyph(tile).to_string(),
            Style::default().fg(TuiCatalog::dim_fg(tile)).bg(tile_bg),
        )
    } else {
        blank_span()
    }
}

fn blank_span() -> Span<'static> {
    Span::styled(" ", Style::default().bg(Color::Reset))
}

/// 该格上"当前可见"的实体。
///
/// 三条优先级规则，**都不能靠 `entities` 的排列顺序实现**（那个顺序是
/// `presentation` 给的稳定顺序，不是绘制优先级）：
///
/// 1. **层级优先**：`Actor` 压过 `Terrain`——楼梯与怪物同格时应当看到怪物，
///    否则玩家会以为怪物站在了楼梯上；
/// 2. **玩家优先于同层**：玩家与怪物同格时看到玩家（这种情况只在传送/生成异常
///    时出现，但若画成怪物，玩家会以为自己的角色消失了）；
/// 3. 同层同格时取先出现者，保证结果可复现。
fn visible_entity_at(scene: &SceneFrame, x: i32, y: i32) -> Option<&EntityView> {
    if let Some(player) = &scene.player
        && player.position == (x, y)
        && player.visible
    {
        return Some(player);
    }
    scene
        .entities
        .iter()
        .filter(|entity| entity.position == (x, y) && entity.visible)
        .max_by_key(|entity| entity.layer)
}

/// 投掷瞄准的叠加层：在已画好的地图上标出弹道与落点，并在相机内底部给一行提示。
///
/// 地图格坐标到终端格只做平移（相机原点由 `presentation` 算好），越界的弹道格直接跳过。
fn render_throw_aim_overlay(
    frame: &mut Frame,
    area: Rect,
    scene: &SceneFrame,
    aim: &render_api::ThrowAimView,
) {
    let inner = inner_rect(area, 1);
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    let visible = scene.camera.visible_rect();
    let origin_x = visible.min_x.floor() as i32;
    let origin_y = visible.min_y.floor() as i32;
    let color = if aim.valid { Color::Cyan } else { Color::Red };
    let style = Style::default().fg(color);

    let to_screen = |x: i32, y: i32| -> Option<(u16, u16)> {
        let col = x - origin_x;
        let row = y - origin_y;
        if col < 0 || row < 0 {
            return None;
        }
        let (col, row) = (col as u16, row as u16);
        (col < inner.width && row < inner.height).then(|| (inner.x + col, inner.y + row))
    };

    {
        let buffer = frame.buffer_mut();
        for &(x, y) in &aim.path {
            if let Some(cell) = to_screen(x, y) {
                buffer[cell].set_symbol("*");
                buffer[cell].set_style(style);
            }
        }
        if let Some(cell) = to_screen(aim.cursor.0, aim.cursor.1) {
            buffer[cell].set_symbol("X");
            buffer[cell].set_style(style);
        }
    }

    let target = aim
        .target
        .as_ref()
        .map_or_else(|| "空地".to_string(), |entity| entity.name.clone());
    let hint = format!(
        "投掷 光标({},{}) 落点：{} 弹道 {} 格 {}",
        aim.cursor.0,
        aim.cursor.1,
        target,
        aim.path.len(),
        if aim.valid { "可投" } else { "不可投" }
    );
    let hint_area = Rect {
        x: inner.x,
        y: inner.y + inner.height - 1,
        width: inner.width,
        height: 1,
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(hint, Style::default().fg(Color::Black).bg(color)))),
        hint_area,
    );
}

fn render_look_overlay(frame: &mut Frame, area: Rect, look: &render_api::LookView) {    // 光标位置换算到终端格：相机已经把世界原点定好了，这里只做平移。
    // 换算失败的唯一原因是视口尺寸为 0（终端太小），此时不画光标。
    let mut lines = Vec::new();
    lines.push(Line::from(format!(
        "光标：({}, {})",
        look.cursor.0, look.cursor.1
    )));
    match &look.tile {
        Some(tile) => {
            // 名称由后端目录决定（显示数据归后端），不用 `TileInfo.name`：
            // 那份字段是 presentation 从 `{:?}` 生成的占位，会把 `Tile(1)` 打给玩家。
            lines.push(Line::from(format!(
                "地形：{}",
                crate::catalog::describe(tile.visual)
            )));
            lines.push(Line::from(format!(
                "可见：{}    已探索：{}",
                yes_no(tile.visible),
                yes_no(tile.explored)
            )));
        }
        None => lines.push(Line::from("地形：未知")),
    }
    if let Some(entity) = &look.entity {
        lines.push(Line::from(format!("实体：{}", entity.name)));
    }
    lines.push(Line::from(Span::styled(
        look.footer.clone(),
        Style::default().fg(Color::DarkGray),
    )));

    let block = Block::default()
        .borders(Borders::ALL)
        .title("查看")
        .border_style(Style::default().fg(Color::Cyan));
    // 叠加在地图的下半部分，避免盖住状态栏。
    let overlay = Rect {
        x: area.x,
        y: area.y + area.height.saturating_sub(8),
        width: area.width,
        height: area.height.min(8),
    };
    frame.render_widget(Paragraph::new(lines).block(block), overlay);
}

fn render_dialog_overlay(frame: &mut Frame, area: Rect, dialog: &render_api::DialogView) {
    let inner = centered_rect(area, 40, 7);
    let mut lines = vec![Line::from(dialog.message.clone())];
    if let Some(confirm) = &dialog.confirm {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("Enter：{confirm}    Esc：取消"),
            Style::default().fg(Color::Yellow),
        )));
    }
    let style = if dialog.danger {
        Style::default().fg(Color::Red)
    } else {
        Style::default().fg(Color::Cyan)
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(dialog.title.clone())
        .title_alignment(Alignment::Center)
        .border_style(style);
    frame.render_widget(Paragraph::new(lines).block(block).centered(), inner);
}

fn render_placeholder(frame: &mut Frame, area: Rect, title: &str, message: &str) {
    let block = Block::default().borders(Borders::ALL).title(title);
    let inner = Rect {
        x: area.x + 1,
        y: area.y + area.height / 2,
        width: area.width.saturating_sub(2),
        height: 1,
    };
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(message).centered(), inner);
}

/// 居中的固定尺寸矩形（对话框用）。
fn centered_rect(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    }
}

fn yes_no(value: bool) -> &'static str {
    if value { "是" } else { "否" }
}

#[cfg(test)]
mod tests;
