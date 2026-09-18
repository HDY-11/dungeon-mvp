//! `SceneFrame` → ratatui 绘制。
//!
//! # 这层的硬约束
//!
//! 只消费 [`SceneFrame`]，**不认识 `ecs_core`**。这条边界由 `Cargo.toml` 强制
//! （`tui` 的依赖表里没有 `ecs_core`），所以"顺手查一下 core 组件"在编译期就
//! 不可能。这正是 Dsn28 要的可替换性：`GpuPlugin` 消费同一个 `SceneFrame`。
//!
//! # 相机
//!
//! 视口位置由 `frame.camera` 决定（`presentation` 算好的，含世界边界夹取），
//! 这里只做「世界格 → 终端格」的平移。**不要**在这层重新算相机：那会让
//! TUI 与 GPU 的取景不一致，而且把世界尺寸的知识漏进后端。

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};
use render_api::{EntityView, SceneFrame, UiView};

use crate::catalog::{TuiCatalog, log_color};
use crate::state::DevLogBuffer;

/// 调试面板高度（终端行）。
const DEBUG_PANEL_HEIGHT: u16 = 8;
/// 侧栏最小宽度。
const SIDE_PANEL_MIN_WIDTH: u16 = 26;

/// 一整帧的区块划分。
///
/// **抽成独立类型是因为相机需要它**：地图只占终端的一部分，`presentation`
/// 必须知道**地图区**多大才能正确夹取相机（`Camera2D.viewport` 要的是后端表面
/// 上留给世界的尺寸，不是整个终端）。曾经把整个终端尺寸传给相机，结果是
/// 视口被当成 80 宽、大于世界的一半，夹取失效，玩家一走出中心就滚出画面。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameAreas {
    /// 全部可用区域。
    pub full: Rect,
    /// 游戏主区（地图 + 侧栏）。
    pub main: Rect,
    /// 地图区（含边框）。
    pub map: Rect,
    /// 侧栏（含边框）。
    pub side: Rect,
    /// 调试面板。
    pub debug: Rect,
}

/// 计算区块划分。**这是布局的唯一权威**——渲染与相机都从这里取。
pub fn frame_areas(area: Rect) -> FrameAreas {
    let [main, debug] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(DEBUG_PANEL_HEIGHT)]).areas(area);
    let [map, side] = Layout::horizontal([
        Constraint::Percentage(60),
        Constraint::Min(SIDE_PANEL_MIN_WIDTH),
    ])
    .areas(main);
    FrameAreas {
        full: area,
        main,
        map,
        side,
        debug,
    }
}

/// 地图区里**可画世界格**的尺寸（去掉边框）。
///
/// 这就是 [`Camera2D`] 该收到的视口。返回 `(0, 0)` 表示"太小/未知"，
/// 相机会退回世界中心而不是 panic。
///
/// [`Camera2D`]: render_api::Camera2D
pub fn map_viewport(area: Rect) -> (u16, u16) {
    let map = frame_areas(area).map;
    (map.width.saturating_sub(2), map.height.saturating_sub(2))
}

/// 画一整帧。
pub fn render_frame(terminal_frame: &mut Frame, scene: &SceneFrame, dev_log: Option<&DevLogBuffer>) {
    let areas = frame_areas(terminal_frame.area());

    match &scene.ui {
        // 背包页是全屏页（Dsn21），其余页面叠加在游戏画面上。
        UiView::Inventory(_) => render_placeholder(
            terminal_frame,
            areas.full,
            "背包",
            "物品系统尚未迁移到 ecs_core（Dsn25 S4），此页为占位。",
        ),
        ui => {
            render_map(terminal_frame, areas.full, scene);
            render_side(terminal_frame, areas.full, scene);

            match ui {
                UiView::Look(look) => render_look_overlay(terminal_frame, areas.full, look),
                UiView::Dialog(dialog) => render_dialog_overlay(terminal_frame, areas.full, dialog),
                UiView::ThrowSelect(_) | UiView::ThrowAim(_) => render_placeholder(
                    terminal_frame,
                    areas.full,
                    "投掷",
                    "投掷系统尚未迁移到 ecs_core（Dsn25 S4），此页为占位。",
                ),
                _ => {}
            }
        }
    }

    render_debug_panel(terminal_frame, areas.debug, dev_log);
}

/// 画地图。
///
/// 收的是**整帧区域**而不是地图区：布局必须由 [`frame_areas`] 算**一次**。
/// 曾经这里收地图区、又调一次 `map_viewport`（它内部再算一次 `frame_areas`），
/// 于是调试面板的高度被减了两次，地图区缩水成 22×14，玩家被裁到画面外。
fn render_map(frame: &mut Frame, full: Rect, scene: &SceneFrame) {
    let area = frame_areas(full).map;
    let (viewport_w, viewport_h) = map_viewport(full);
    let (viewport_w, viewport_h) = (viewport_w as usize, viewport_h as usize);

    let lines = build_map_lines(scene, viewport_w, viewport_h);

    let title = if scene.game_over {
        " 游戏结束 "
    } else {
        " Dungeon MVP (ecs_core) "
    };
    let block = Block::default()
        .title(title)
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL);
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

/// 把可见区域渲染成终端行。
///
/// 抽成公开函数是为了让 golden 测试**不经过 `Terminal`** 就能断言画面内容
/// （`TestBackend` 也能测，但拿 `Vec<Line>` 更适合断言"这一格画了什么"）。
pub fn build_map_lines(scene: &SceneFrame, viewport_w: usize, viewport_h: usize) -> Vec<Line<'static>> {
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
        return Span::styled(
            TuiCatalog::glyph(entity.visual).to_string(),
            Style::default().fg(TuiCatalog::fg(entity.visual)).bg(tile_bg),
        );
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

fn render_side(frame: &mut Frame, full: Rect, scene: &SceneFrame) {
    let area = frame_areas(full).side;
    let mut lines = Vec::new();

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
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "可见实体",
        Style::default().fg(Color::Cyan),
    )));
    for entity in scene.visible_entities() {
        if entity.visual == render_api::VisualKey::Player {
            continue;
        }
        let hp = match entity.hp {
            Some(hp) if hp.max > 0.0 => format!(" {:.0}/{:.0}", hp.current, hp.max),
            _ => String::new(),
        };
        lines.push(Line::from(format!(
            "{} {}{hp}",
            TuiCatalog::glyph(entity.visual),
            entity.name
        )));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "日志",
        Style::default().fg(Color::Cyan),
    )));
    for line in &scene.log {
        lines.push(Line::from(Span::styled(
            line.text.clone(),
            Style::default().fg(log_color(line.level)),
        )));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "方向键/hjkl：移动或攻击    .：等待",
        Style::default().fg(Color::DarkGray),
    )));
    lines.push(Line::from(Span::styled(
        "?：查看    q/Esc：退出",
        Style::default().fg(Color::DarkGray),
    )));

    if scene.game_over {
        lines.push(Line::from(Span::styled(
            "你死了。按 q 退出。",
            Style::default().fg(Color::Red),
        )));
    }

    let block = Block::default().borders(Borders::ALL).title("状态");
    frame.render_widget(
        Paragraph::new(lines).block(block).wrap(Wrap { trim: true }),
        area,
    );
}

fn render_look_overlay(frame: &mut Frame, full: Rect, look: &render_api::LookView) {
    let area = frame_areas(full).map;
    // 光标位置换算到终端格：相机已经把世界原点定好了，这里只做平移。
    // 换算失败的唯一原因是视口尺寸为 0（终端太小），此时不画光标。
    let mut lines = Vec::new();
    lines.push(Line::from(format!(
        "光标：({}, {})",
        look.cursor.0, look.cursor.1
    )));
    match &look.tile {
        Some(tile) => {
            lines.push(Line::from(format!("地形：{}", tile.name)));
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
    frame.render_widget(
        Paragraph::new(lines).block(block).centered(),
        inner,
    );
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

fn render_debug_panel(frame: &mut Frame, area: Rect, dev_log: Option<&DevLogBuffer>) {
    let lines = dev_log
        .map(|buffer| {
            buffer
                .lines
                .iter()
                .rev()
                .take(area.height.saturating_sub(2) as usize)
                .rev()
                .map(|entry| {
                    let level_color = match entry.level.as_str() {
                        "ERROR" => Color::Red,
                        "WARN" => Color::Yellow,
                        "INFO" => Color::Cyan,
                        _ => Color::DarkGray,
                    };
                    Line::from(vec![
                        Span::styled(
                            format!("[{}] ", entry.level),
                            Style::default().fg(level_color),
                        ),
                        Span::styled(
                            format!("{}: ", entry.target),
                            Style::default().fg(Color::DarkGray),
                        ),
                        Span::raw(entry.message.clone()),
                    ])
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let block = Block::default().borders(Borders::ALL).title("Debug");
    frame.render_widget(Paragraph::new(lines).block(block), area);
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
