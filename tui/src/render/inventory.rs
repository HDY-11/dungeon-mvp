//! 背包页（全屏页，Dsn21）的绘制。
//!
//! # 只画契约
//!
//! 收的是 [`render_api::InventoryView`]：每栏有哪些行、选中哪一行、详情面板有什么、
//! 底部提示写什么，全部由 `presentation` 决定（DsnX14：显示数据归 presentation，
//! 后端只负责画）。这一层**不写死键位文案**，也不重排内容。
//!
//! # 为什么要自己算显示宽度
//!
//! 终端的「列」不等于 `char`：一个汉字或全角标点占 2 列。用 `chars().count()` 当
//! 宽度去补空格，选中行的高亮会短一半（中文标签），窄屏截断还会把宽字符切成半个。
//! `tui` 的依赖表里没有 `unicode-width`（本任务的写域只允许 `tui/src`），所以
//! [`display_width`] 用一张 East Asian Wide/Fullwidth 表近似它，口径与 ratatui
//! 内部的 `unicode-width`（默认非 CJK：全角 2 列、歧义宽度 1 列、组合记号 0 列）
//! 保持一致，保证「补空格铺满整行」与 ratatui 实际写进缓冲的宽度对得上。
//!
//! # 布局降级
//!
//! 1. 宽屏（内容 ≥ 64 列）：身体 / 行囊左右两栏（40% : 60%，中间 1 列间隔）；
//! 2. 窄屏（< 64 列，例如 40 列终端）：上下堆叠，宁可滚动也不压扁标签；
//! 3. 详情面板：超宽（≥ 100 列且 ≥ 12 行）放右侧；普通宽屏（≥ 48 列且 ≥ 16 行）
//!    放底部；再小就**不画**详情——降级时先保证两栏可用；
//! 4. 极小终端（1×1、10×3）：所有尺寸用 saturating 运算，画不下就少画，绝不 panic。

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};
use render_api::{
    DetailView, InventoryFocus, InventoryPanelView, InventoryView, ListItemView, ListView,
    UiTextLine, UiTextStyle,
};

/// 页面内容至少这么宽时，身体 / 行囊左右并排；否则上下堆叠。
const COLUMNS_MIN_WIDTH: u16 = 64;
/// 两栏并排时每栏的最小宽度（含边框）。
const PANEL_MIN_WIDTH: u16 = 18;
/// 栏与栏之间的间隔列数。
const GAP: u16 = 1;
/// 内容高度低于这个值时不留底部提示行（否则两栏只剩边框）。
const FOOTER_MIN_HEIGHT: u16 = 6;

/// 详情放右侧所需的最小内容尺寸。
const DETAIL_SIDE_MIN_WIDTH: u16 = 100;
const DETAIL_SIDE_MIN_HEIGHT: u16 = 12;
/// 详情放底部所需的最小内容尺寸。
const DETAIL_BOTTOM_MIN_WIDTH: u16 = 48;
const DETAIL_BOTTOM_MIN_HEIGHT: u16 = 16;
/// 详情栏宽 / 高的占比与夹取范围。
///
/// 高度按 45% 给：80×30 的常见终端里内容区 27 行，详情拿 12 行 = 边框 2 + 正文 9 + 提示 1，
/// 刚好放得下"说明 + 类型 + 攻速 + 修正 + 部位 + 可装"这类最长的一行内容；两栏各剩 12 行
/// （身体 7 格一屏放满，行囊 12 格正好一屏）。再矮一档就优先保列表，正文允许截断。
const DETAIL_WIDTH_PERCENT: u16 = 34;
const DETAIL_WIDTH_MIN: u16 = 24;
const DETAIL_WIDTH_MAX: u16 = 44;
const DETAIL_HEIGHT_PERCENT: u16 = 45;
const DETAIL_HEIGHT_MIN: u16 = 5;
const DETAIL_HEIGHT_MAX: u16 = 12;

/// 页面布局结果。**抽成纯函数是为了能测**：测试直接断言矩形几何，
/// 不依赖 `TestBackend` 的像素去反推布局。
#[derive(Debug, Clone)]
pub(super) struct InventoryLayout {
    /// 页面外框内的内容区（不含底部提示行）。
    pub(super) content: Rect,
    /// 每栏区域（含栏自己的边框），顺序与 `inventory.panels` 一致。
    pub(super) panels: Vec<Rect>,
    /// 详情面板区域（含边框）；`None` = 放不下或没有详情。
    pub(super) detail: Option<Rect>,
    /// 底部提示行区域；`None` = 高度不够，不画。
    pub(super) footer: Option<Rect>,
}

/// 计算背包页布局。纯函数，不做 I/O，测试可直接调用。
pub(super) fn inventory_layout(area: Rect, inventory: &InventoryView) -> InventoryLayout {
    let frame_inner = Block::default().borders(Borders::ALL).inner(area);
    let footer = if frame_inner.height >= FOOTER_MIN_HEIGHT && !inventory.footer.is_empty() {
        Some(Rect {
            x: frame_inner.x,
            y: frame_inner.y + frame_inner.height - 1,
            width: frame_inner.width,
            height: 1,
        })
    } else {
        None
    };
    let content = Rect {
        x: frame_inner.x,
        y: frame_inner.y,
        width: frame_inner.width,
        height: frame_inner
            .height
            .saturating_sub(u16::from(footer.is_some())),
    };

    let mut layout = InventoryLayout {
        content,
        panels: Vec::new(),
        detail: None,
        footer,
    };
    if content.width == 0 || content.height == 0 {
        return layout;
    }

    let has_detail = inventory
        .detail
        .as_ref()
        .is_some_and(|detail| !detail.is_empty());

    // 没有列表栏（未来页面可能只有详情）：详情占满内容区。
    if inventory.panels.is_empty() {
        if has_detail {
            layout.detail = Some(content);
        }
        return layout;
    }

    // 详情放右侧：超宽屏。
    if has_detail
        && content.width >= DETAIL_SIDE_MIN_WIDTH
        && content.height >= DETAIL_SIDE_MIN_HEIGHT
    {
        let panel_count = inventory.panels.len() as u16;
        let max_detail_width = content
            .width
            .saturating_sub(PANEL_MIN_WIDTH * panel_count + GAP);
        let detail_width = ((u32::from(content.width) * u32::from(DETAIL_WIDTH_PERCENT) / 100) as u16)
            .clamp(DETAIL_WIDTH_MIN, DETAIL_WIDTH_MAX)
            .min(max_detail_width);
        if detail_width >= DETAIL_WIDTH_MIN {
            let panels_area = Rect {
                x: content.x,
                y: content.y,
                width: content.width - detail_width - GAP,
                height: content.height,
            };
            layout.panels = split_panels(panels_area, inventory.panels.len());
            layout.detail = Some(Rect {
                x: content.x + content.width - detail_width,
                y: content.y,
                width: detail_width,
                height: content.height,
            });
            return layout;
        }
    }

    // 详情放底部：普通宽屏且够高。
    if has_detail
        && content.width >= DETAIL_BOTTOM_MIN_WIDTH
        && content.height >= DETAIL_BOTTOM_MIN_HEIGHT
    {
        let detail_height =
            ((u32::from(content.height) * u32::from(DETAIL_HEIGHT_PERCENT) / 100) as u16)
                .clamp(DETAIL_HEIGHT_MIN, DETAIL_HEIGHT_MAX);
        let panels_height = content.height.saturating_sub(detail_height + GAP);
        if panels_height > 0 {
            let panels_area = Rect {
                x: content.x,
                y: content.y,
                width: content.width,
                height: panels_height,
            };
            layout.panels = split_panels(panels_area, inventory.panels.len());
            layout.detail = Some(Rect {
                x: content.x,
                y: content.y + panels_height + GAP,
                width: content.width,
                height: detail_height,
            });
            return layout;
        }
    }

    // 放不下详情：降级为两栏（或堆叠），优先保证列表可用。
    layout.panels = split_panels(content, inventory.panels.len());
    layout
}

/// 按宽度决定两栏并排还是上下堆叠，并把区域切分给每栏。
fn split_panels(area: Rect, count: usize) -> Vec<Rect> {
    if count == 0 || area.width == 0 || area.height == 0 {
        return Vec::new();
    }
    if area.width >= COLUMNS_MIN_WIDTH {
        split_columns(area, count)
    } else {
        split_rows(area, count)
    }
}

/// 左右并排：两栏按 40% : 60% 分，第一栏（身体）窄一些；栏间留 [`GAP`] 列。
fn split_columns(area: Rect, count: usize) -> Vec<Rect> {
    let count_u16 = count as u16;
    let usable = area.width.saturating_sub(GAP * (count_u16 - 1));
    let widths: Vec<u16> = if count == 2 {
        let first = ((u32::from(usable) * 40 / 100) as u16)
            .max(PANEL_MIN_WIDTH)
            .min(usable.saturating_sub(PANEL_MIN_WIDTH));
        vec![first, usable - first]
    } else {
        let each = usable / count_u16;
        (0..count)
            .map(|index| {
                if index + 1 == count {
                    usable - each * (count_u16 - 1)
                } else {
                    each
                }
            })
            .collect()
    };

    let mut x = area.x;
    widths
        .into_iter()
        .map(|width| {
            let rect = Rect {
                x,
                y: area.y,
                width,
                height: area.height,
            };
            x = x.saturating_add(width).saturating_add(GAP);
            rect
        })
        .collect()
}

/// 上下堆叠：两栏按 40% : 60% 分行，行间留 [`GAP`] 行。
fn split_rows(area: Rect, count: usize) -> Vec<Rect> {
    let count_u16 = count as u16;
    let usable = area.height.saturating_sub(GAP * (count_u16 - 1));
    let heights: Vec<u16> = if count == 2 && usable >= 6 {
        let first = ((u32::from(usable) * 40 / 100) as u16)
            .max(3)
            .min(usable - 3);
        vec![first, usable - first]
    } else {
        let each = usable / count_u16;
        (0..count)
            .map(|index| {
                if index + 1 == count {
                    usable - each * (count_u16 - 1)
                } else {
                    each
                }
            })
            .collect()
    };

    let mut y = area.y;
    heights
        .into_iter()
        .map(|height| {
            let rect = Rect {
                x: area.x,
                y,
                width: area.width,
                height,
            };
            y = y.saturating_add(height).saturating_add(GAP);
            rect
        })
        .collect()
}

/// 画背包页（全屏）。
pub(super) fn render_inventory_page(frame: &mut Frame, area: Rect, inventory: &InventoryView) {
    let layout = inventory_layout(area, inventory);
    frame.render_widget(page_block(inventory), area);

    if layout.content.width > 0 && layout.content.height > 0 {
        for (panel_view, panel_area) in inventory.panels.iter().zip(&layout.panels) {
            let focused = inventory.focus == InventoryFocus::Panel(panel_view.panel);
            render_panel(frame, *panel_area, panel_view, focused);
        }
        if let (Some(detail_view), Some(detail_area)) = (inventory.detail.as_ref(), layout.detail) {
            render_detail(
                frame,
                detail_area,
                detail_view,
                inventory.focus == InventoryFocus::Detail,
            );
        }
    }

    if let Some(footer_area) = layout.footer {
        render_footer(frame, footer_area, &inventory.footer);
    }
}

/// 页面外框：青色边框 + 标题（标题也是契约数据，不在这里编）。
fn page_block(inventory: &InventoryView) -> Block<'static> {
    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    if !inventory.title.is_empty() {
        block = block.title_top(Line::from(Span::styled(
            format!(" {} ", inventory.title),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));
    }
    block
}

/// 一栏：标题（+ 右对齐的占用数）、边框、可滚动列表。
fn render_panel(
    frame: &mut Frame,
    area: Rect,
    panel: &InventoryPanelView,
    focused: bool,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let list = &panel.list;
    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style(focused));
    if !list.title.is_empty() {
        block = block.title_top(Line::from(Span::styled(
            format!(" {} ", list.title),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )));
    }
    if !list.footer.is_empty() {
        block = block.title_top(
            Line::from(Span::styled(
                format!(" {} ", list.footer),
                Style::default().fg(Color::DarkGray),
            ))
            .right_aligned(),
        );
    }
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 || list.items.is_empty() {
        return;
    }

    let capacity = inner.height as usize;
    let start = visible_start(list, capacity);
    for (row, (index, item)) in list
        .items
        .iter()
        .enumerate()
        .skip(start)
        .take(capacity)
        .enumerate()
    {
        let row_area = Rect {
            x: inner.x,
            y: inner.y + row as u16,
            width: inner.width,
            height: 1,
        };
        let line = item_line(item, index == list.selected, focused, inner.width as usize);
        frame.render_widget(line, row_area);
    }
}

/// 计算列表可见窗口的起点：至少让 `selected` 落在可视范围内。
///
/// 契约里的 `scroll` 是**建议值**（说明写在后端可自行分页），这里在它基础上
/// 做一次最小修正：选中项在窗口上方就上移、下方就下移，绝不让选中行画到栏外
/// ——否则玩家按 `↓` 时看不到自己选到哪。
fn visible_start(list: &ListView, capacity: usize) -> usize {
    let len = list.items.len();
    if len == 0 || capacity == 0 {
        return 0;
    }
    let max_start = len.saturating_sub(capacity);
    let selected = list.selected.min(len - 1);
    let mut start = list.scroll.min(max_start);
    if selected < start {
        start = selected;
    } else if selected >= start + capacity {
        start = selected + 1 - capacity;
    }
    start.min(max_start)
}

/// 一行列表项的绘制。
///
/// 三种视觉层次：焦点栏的选中行**整行反色**（用显示宽度补满，中文才不会短一截）；
/// 非焦点栏的选中行保留 `▸` 标记但不铺底色（焦点在哪一眼能看出来）；其余行按
/// 契约的 [`UiTextStyle`] 上色，占位行（`（…）`）与 disabled 行弱化。
fn item_line(item: &ListItemView, selected: bool, focused: bool, width: usize) -> Line<'static> {
    if selected {
        let body = item_body(item);
        let body = truncate_to_width(&body, width.saturating_sub(2));
        let text = if focused {
            pad_to_width(&format!("▸ {body}"), width)
        } else {
            format!("▸ {body}")
        };
        let style = if focused {
            Style::default()
                .bg(Color::Cyan)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        };
        return Line::from(Span::styled(text, style));
    }

    let body_style = if item.disabled || is_placeholder(&item.label) {
        Style::default().fg(Color::DarkGray)
    } else {
        text_style(item.style)
    };
    let available = width.saturating_sub(2);
    let mut spans = vec![Span::raw("  ")];
    match item.detail.as_deref().filter(|detail| !detail.is_empty()) {
        Some(detail) if display_width(&item.label) + 2 + display_width(detail) <= available => {
            spans.push(Span::styled(item.label.clone(), body_style));
            spans.push(Span::raw("  "));
            spans.push(Span::styled(
                detail.to_string(),
                Style::default().fg(Color::DarkGray),
            ));
        }
        Some(detail) => {
            let combined = format!("{}  {detail}", item.label);
            spans.push(Span::styled(
                truncate_to_width(&combined, available),
                body_style,
            ));
        }
        None => spans.push(Span::styled(
            truncate_to_width(&item.label, available),
            body_style,
        )),
    }
    Line::from(spans)
}

/// 详情面板：标题、契约给的文本行、可选底部提示。
fn render_detail(frame: &mut Frame, area: Rect, detail: &DetailView, focused: bool) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style(focused));
    if !detail.title.is_empty() {
        block = block.title_top(Line::from(Span::styled(
            format!(" {} ", detail.title),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )));
    }
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let footer_height = u16::from(!detail.footer.is_empty() && inner.height >= 2);
    let lines_height = inner.height.saturating_sub(footer_height);
    let lines: Vec<Line<'static>> = detail
        .lines
        .iter()
        .take(lines_height as usize)
        .map(detail_line)
        .collect();
    frame.render_widget(
        Paragraph::new(lines),
        Rect {
            x: inner.x,
            y: inner.y,
            width: inner.width,
            height: lines_height,
        },
    );

    if footer_height > 0 {
        frame.render_widget(
            Line::from(Span::styled(
                truncate_to_width(&detail.footer, inner.width as usize),
                Style::default().fg(Color::DarkGray),
            )),
            Rect {
                x: inner.x,
                y: inner.y + lines_height,
                width: inner.width,
                height: 1,
            },
        );
    }
}

/// 页面底部提示：契约给的一整行文本，按 **2 个及以上连续空格**分组后
/// 用 `│` 分隔显示。分组只是排版，键位文案仍全来自 `presentation`（LTUI3）。
fn render_footer(frame: &mut Frame, area: Rect, footer: &str) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let text = truncate_to_width(&footer_groups(footer).join(" │ "), area.width as usize);
    frame.render_widget(
        Line::from(Span::styled(text, Style::default().fg(Color::DarkGray))),
        area,
    );
}

/// 把 footer 拆成若干组：2 个及以上连续空格视为组边界。
fn footer_groups(footer: &str) -> Vec<&str> {
    footer
        .split("  ")
        .map(str::trim)
        .filter(|group| !group.is_empty())
        .collect()
}

/// 契约语义样式 → 终端样式（外观定义仍集中在渲染层，契约层不出现颜色）。
fn text_style(style: UiTextStyle) -> Style {
    match style {
        UiTextStyle::Normal => Style::default(),
        UiTextStyle::Title => Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
        UiTextStyle::Accent => Style::default().fg(Color::Cyan),
        UiTextStyle::Muted => Style::default().fg(Color::DarkGray),
        UiTextStyle::Warning => Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
        UiTextStyle::Danger => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        UiTextStyle::Success => Style::default().fg(Color::Green),
        UiTextStyle::Selected => Style::default().bg(Color::Cyan).fg(Color::Black),
        UiTextStyle::Disabled => Style::default().fg(Color::DarkGray),
    }
}

fn border_style(focused: bool) -> Style {
    if focused {
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    }
}

fn detail_line(line: &UiTextLine) -> Line<'static> {
    let spans: Vec<Span<'static>> = line
        .spans
        .iter()
        .map(|span| Span::styled(span.text.clone(), text_style(span.style)))
        .collect();
    Line::from(spans)
}

fn item_body(item: &ListItemView) -> String {
    match item.detail.as_deref().filter(|detail| !detail.is_empty()) {
        Some(detail) => format!("{}  {detail}", item.label),
        None => item.label.clone(),
    }
}

/// 占位行的启发式识别：契约里没有"占位"标记，`presentation` 约定的写法是
/// 全角括号包起来的空状态文案（`（身上没穿东西）`）。
fn is_placeholder(label: &str) -> bool {
    let trimmed = label.trim();
    trimmed.starts_with('（') && trimmed.ends_with('）')
}

/// 一个字符占的终端列数。
///
/// 只覆盖会影响 UI 宽度的范围：控制/组合记号 0 列，East Asian Wide/Fullwidth
/// （含 CJK、全角标点、假名、谚文、常见 emoji）2 列，其余 1 列。歧义宽度
/// （`▸`、`│`、`×` 等）按 1 列——与 `unicode-width` 的默认口径一致。
pub(super) fn char_width(ch: char) -> usize {
    let code = ch as u32;
    match code {
        // 控制字符与组合记号不占位。
        0x00..=0x1F
        | 0x7F..=0x9F
        | 0x0300..=0x036F
        | 0x1AB0..=0x1AFF
        | 0x1DC0..=0x1DFF
        | 0x20D0..=0x20FF
        | 0x200B..=0x200F
        | 0x2060..=0x2064
        | 0xFE00..=0xFE0F
        | 0xFE20..=0xFE2F
        | 0xE0100..=0xE01EF => 0,
        // East Asian Wide / Fullwidth。
        0x1100..=0x115F
        | 0x231A..=0x231B
        | 0x2329..=0x232A
        | 0x23E9..=0x23EC
        | 0x23F0
        | 0x23F3
        | 0x25FD..=0x25FE
        | 0x2614..=0x2615
        | 0x2648..=0x2653
        | 0x267F
        | 0x2693
        | 0x26A1
        | 0x26AA..=0x26AB
        | 0x26BD..=0x26BE
        | 0x26C4..=0x26C5
        | 0x26CE
        | 0x26D4
        | 0x26EA
        | 0x26F2..=0x26F3
        | 0x26F5
        | 0x26FA
        | 0x26FD
        | 0x2705
        | 0x270A..=0x270B
        | 0x2728
        | 0x274C
        | 0x274E
        | 0x2753..=0x2755
        | 0x2757
        | 0x2795..=0x2797
        | 0x27B0
        | 0x27BF
        | 0x2B1B..=0x2B1C
        | 0x2B50
        | 0x2B55
        | 0x2E80..=0x303E
        | 0x3041..=0x33FF
        | 0x3400..=0x4DBF
        | 0x4E00..=0x9FFF
        | 0xA000..=0xA4CF
        | 0xA960..=0xA97F
        | 0xAC00..=0xD7A3
        | 0xF900..=0xFAFF
        | 0xFE10..=0xFE19
        | 0xFE30..=0xFE6F
        | 0xFF00..=0xFF60
        | 0xFFE0..=0xFFE6
        | 0x16FE0..=0x16FE4
        | 0x17000..=0x187F7
        | 0x18800..=0x18CD5
        | 0x1B000..=0x1B2FB
        | 0x1F004
        | 0x1F0CF
        | 0x1F18E
        | 0x1F191..=0x1F19A
        | 0x1F200..=0x1F320
        | 0x1F32D..=0x1F335
        | 0x1F337..=0x1F37C
        | 0x1F37E..=0x1F393
        | 0x1F3A0..=0x1F3CA
        | 0x1F3CF..=0x1F3D3
        | 0x1F3E0..=0x1F3F0
        | 0x1F3F4
        | 0x1F3F8..=0x1F43E
        | 0x1F440
        | 0x1F442..=0x1F4FC
        | 0x1F4FF..=0x1F53D
        | 0x1F54B..=0x1F54E
        | 0x1F550..=0x1F567
        | 0x1F57A
        | 0x1F595..=0x1F596
        | 0x1F5A4
        | 0x1F5FB..=0x1F64F
        | 0x1F680..=0x1F6C5
        | 0x1F6CC
        | 0x1F6D0..=0x1F6D2
        | 0x1F6D5..=0x1F6D7
        | 0x1F6DC..=0x1F6DF
        | 0x1F6EB..=0x1F6EC
        | 0x1F6F4..=0x1F6FC
        | 0x1F7E0..=0x1F7EB
        | 0x1F7F0
        | 0x1F90C..=0x1F93A
        | 0x1F93C..=0x1F945
        | 0x1F947..=0x1F9FF
        | 0x1FA70..=0x1FAFF
        | 0x20000..=0x2FFFD
        | 0x30000..=0x3FFFD => 2,
        _ => 1,
    }
}

/// 文本的终端显示宽度（列数），**不是**字符数。
pub(super) fn display_width(text: &str) -> usize {
    text.chars().map(char_width).sum()
}

/// 按显示宽度截断，放不下时保留 `max_width - 1` 列再加 `…`。
///
/// 只在字符边界切，宽字符不会被切成半个。
pub(super) fn truncate_to_width(text: &str, max_width: usize) -> String {
    if display_width(text) <= max_width {
        return text.to_string();
    }
    if max_width == 0 {
        return String::new();
    }
    let budget = max_width - 1;
    let mut out = String::new();
    let mut used = 0;
    for ch in text.chars() {
        let width = char_width(ch);
        if used + width > budget {
            break;
        }
        out.push(ch);
        used += width;
    }
    out.push('…');
    out
}

/// 用空格补到指定显示宽度（已超宽则原样返回，交给 ratatui 裁剪）。
fn pad_to_width(text: &str, width: usize) -> String {
    let current = display_width(text);
    if current >= width {
        return text.to_string();
    }
    let mut padded = String::with_capacity(text.len() + width - current);
    padded.push_str(text);
    padded.push_str(&" ".repeat(width - current));
    padded
}
