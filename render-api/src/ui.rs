//! 页面级 UI 视图模型。
//!
//! 这里只描述“页面上有什么”，不包含 ratatui / GPU 类型。
//! 后端根据 [`UiView`] 变体决定布局与绘制：
//!
//! - TUI 把 [`ListView`] 渲染成 ratatui `List`，把 [`UiTextLine`] 渲染成 `Line`；
//! - 未来的 GPU 后端把同样的数据渲染成文本 / 面板。
//!
//! 页面状态（页栈、光标、选中项）由 `presentation` 维护；
//! 本模块只承载“当前帧应该画什么”。

use crate::scene::{EntityView, TileInfo};

/// 当前页面类型。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum PageKind {
    #[default]
    Game,
    Look,
    Dialog,
    Inventory,
    ThrowSelect,
    ThrowAim,
}

/// 文本语义样式。
///
/// 后端把语义映射到自己的调色板；契约层不出现具体颜色值。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum UiTextStyle {
    #[default]
    Normal,
    Title,
    Accent,
    Muted,
    Warning,
    Danger,
    Success,
    Selected,
    Disabled,
}

/// 一段带样式的文本。
#[derive(Clone, Debug, PartialEq)]
pub struct UiSpan {
    pub text: String,
    pub style: UiTextStyle,
}

impl UiSpan {
    pub fn new(text: impl Into<String>, style: UiTextStyle) -> Self {
        Self {
            text: text.into(),
            style,
        }
    }

    pub fn normal(text: impl Into<String>) -> Self {
        Self::new(text, UiTextStyle::Normal)
    }
}

/// 一行文本，由若干 [`UiSpan`] 组成。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UiTextLine {
    pub spans: Vec<UiSpan>,
}

impl UiTextLine {
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            spans: vec![UiSpan::normal(text)],
        }
    }

    pub fn styled(text: impl Into<String>, style: UiTextStyle) -> Self {
        Self {
            spans: vec![UiSpan::new(text, style)],
        }
    }

    pub fn from_spans(spans: Vec<UiSpan>) -> Self {
        Self { spans }
    }

    /// 拼接后的纯文本，用于日志 / 测试 / 文本后端。
    pub fn text(&self) -> String {
        let mut out = String::new();
        for span in &self.spans {
            out.push_str(&span.text);
        }
        out
    }

    pub fn is_empty(&self) -> bool {
        self.spans.iter().all(|span| span.text.is_empty())
    }
}

/// 列表项。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ListItemView {
    pub label: String,
    pub detail: Option<String>,
    pub style: UiTextStyle,
    pub disabled: bool,
}

impl ListItemView {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            ..Self::default()
        }
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub fn with_style(mut self, style: UiTextStyle) -> Self {
        self.style = style;
        self
    }

    pub fn with_disabled(mut self) -> Self {
        self.disabled = true;
        self
    }
}

/// 可选列表。
///
/// `selected` 是逻辑选中项，`scroll` 是建议的滚动起点；
/// 具体分页 / 滚动策略由后端根据自身视口决定。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ListView {
    pub title: String,
    pub items: Vec<ListItemView>,
    pub selected: usize,
    pub scroll: usize,
    pub footer: String,
}

impl ListView {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            ..Self::default()
        }
    }

    pub fn with_items(mut self, items: Vec<ListItemView>) -> Self {
        self.items = items;
        self.clamp_selection();
        self
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn selected_item(&self) -> Option<&ListItemView> {
        self.items.get(self.selected)
    }

    /// 把 `selected` / `scroll` 限制在合法范围内。
    pub fn clamp_selection(&mut self) {
        if self.items.is_empty() {
            self.selected = 0;
            self.scroll = 0;
            return;
        }
        self.selected = self.selected.min(self.items.len() - 1);
        self.scroll = self.scroll.min(self.selected);
    }
}

/// 详情面板。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DetailView {
    pub title: String,
    pub lines: Vec<UiTextLine>,
    pub footer: String,
}

impl DetailView {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            ..Self::default()
        }
    }

    pub fn push_line(&mut self, line: UiTextLine) {
        self.lines.push(line);
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
}

/// 确认 / 信息对话框。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DialogView {
    pub title: String,
    pub message: String,
    /// 确认键提示；`None` 表示无确认动作。
    pub confirm: Option<String>,
    /// 取消键提示；`None` 表示无取消动作。
    pub cancel: Option<String>,
    /// 危险操作（退出 / 丢弃等），后端可用警示样式。
    pub danger: bool,
}

impl DialogView {
    pub fn confirm(title: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            message: message.into(),
            confirm: Some("y".to_string()),
            cancel: Some("n".to_string()),
            danger: false,
        }
    }

    pub fn danger(title: impl Into<String>, message: impl Into<String>) -> Self {
        let mut dialog = Self::confirm(title, message);
        dialog.danger = true;
        dialog
    }
}

/// 查看模式（Look）视图。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LookView {
    pub cursor: (i32, i32),
    pub tile: Option<TileInfo>,
    pub entity: Option<EntityView>,
    pub footer: String,
}

impl LookView {
    pub fn new(cursor: (i32, i32)) -> Self {
        Self {
            cursor,
            ..Self::default()
        }
    }

    pub fn with_tile(mut self, tile: TileInfo) -> Self {
        self.tile = Some(tile);
        self
    }

    pub fn with_entity(mut self, entity: EntityView) -> Self {
        self.entity = Some(entity);
        self
    }

    pub fn with_footer(mut self, footer: impl Into<String>) -> Self {
        self.footer = footer.into();
        self
    }
}

/// 背包页中的面板类型。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum InventoryPanel {
    #[default]
    Equipment,
    Backpack,
    Ground,
}

/// 背包页当前焦点。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum InventoryFocus {
    Panel(InventoryPanel),
    Detail,
}

impl Default for InventoryFocus {
    fn default() -> Self {
        Self::Panel(InventoryPanel::default())
    }
}

/// 背包页中的一个面板。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InventoryPanelView {
    pub panel: InventoryPanel,
    pub list: ListView,
}

impl InventoryPanelView {
    pub fn new(panel: InventoryPanel, list: ListView) -> Self {
        Self { panel, list }
    }
}

/// 背包 / 装备页视图。
///
/// 物品系统迁移到 `core` 之前，这是目标形状；`presentation` 按
/// [`InventoryPanel`] 填充装备 / 背包 / 地面面板，详情面板可选。
/// 用面板列表而不是三个固定字段，方便未来追加“合成 / 商店”等页面。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InventoryView {
    pub title: String,
    pub panels: Vec<InventoryPanelView>,
    pub focus: InventoryFocus,
    pub detail: Option<DetailView>,
    pub footer: String,
}

impl InventoryView {
    /// 按面板类型查找列表。
    pub fn panel(&self, panel: InventoryPanel) -> Option<&ListView> {
        self.panels
            .iter()
            .find(|entry| entry.panel == panel)
            .map(|entry| &entry.list)
    }

    pub fn panel_mut(&mut self, panel: InventoryPanel) -> Option<&mut ListView> {
        self.panels
            .iter_mut()
            .find(|entry| entry.panel == panel)
            .map(|entry| &mut entry.list)
    }

    /// 当前焦点对应的列表；焦点在详情页时返回 `None`。
    pub fn focused_panel(&self) -> Option<&ListView> {
        match self.focus {
            InventoryFocus::Panel(panel) => self.panel(panel),
            InventoryFocus::Detail => None,
        }
    }
}

/// 投掷瞄准视图。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ThrowAimView {
    pub cursor: (i32, i32),
    /// 弹道经过的格子，按顺序排列。
    pub path: Vec<(i32, i32)>,
    /// 当前光标是否落在合法目标上。
    pub valid: bool,
    pub target: Option<EntityView>,
    pub footer: String,
}

impl ThrowAimView {
    pub fn new(cursor: (i32, i32)) -> Self {
        Self {
            cursor,
            ..Self::default()
        }
    }

    pub fn with_path(mut self, path: Vec<(i32, i32)>) -> Self {
        self.path = path;
        self
    }

    pub fn with_valid(mut self, valid: bool) -> Self {
        self.valid = valid;
        self
    }

    pub fn with_target(mut self, target: EntityView) -> Self {
        self.target = Some(target);
        self
    }
}

/// 当前帧的页面视图。
///
/// `Game` 表示没有页面覆盖；其他变体由后端叠加或替换游戏画面。
#[derive(Clone, Debug, Default, PartialEq)]
pub enum UiView {
    #[default]
    Game,
    Look(LookView),
    Dialog(DialogView),
    Inventory(InventoryView),
    ThrowSelect(ListView),
    ThrowAim(ThrowAimView),
}

impl UiView {
    pub const fn kind(&self) -> PageKind {
        match self {
            Self::Game => PageKind::Game,
            Self::Look(_) => PageKind::Look,
            Self::Dialog(_) => PageKind::Dialog,
            Self::Inventory(_) => PageKind::Inventory,
            Self::ThrowSelect(_) => PageKind::ThrowSelect,
            Self::ThrowAim(_) => PageKind::ThrowAim,
        }
    }

    /// 是否全屏替换游戏画面（Dsn21：背包页是全屏页）。
    pub const fn is_fullscreen(&self) -> bool {
        matches!(self, Self::Inventory(_))
    }

    /// 是否叠加在游戏画面上。
    pub const fn is_overlay(&self) -> bool {
        matches!(
            self,
            Self::Look(_) | Self::Dialog(_) | Self::ThrowSelect(_) | Self::ThrowAim(_)
        )
    }

    /// 是否捕获输入（非 Game 页都捕获）。
    pub const fn captures_input(&self) -> bool {
        !matches!(self, Self::Game)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::EntityId;
    use crate::visual::VisualKey;

    #[test]
    fn page_kind_and_flags() {
        assert_eq!(UiView::Game.kind(), PageKind::Game);
        assert!(!UiView::Game.captures_input());
        assert!(!UiView::Game.is_fullscreen());
        assert!(!UiView::Game.is_overlay());

        let dialog = UiView::Dialog(DialogView::confirm("退出", "确认退出？"));
        assert_eq!(dialog.kind(), PageKind::Dialog);
        assert!(dialog.captures_input());
        assert!(!dialog.is_fullscreen());
        assert!(dialog.is_overlay());

        let inventory = UiView::Inventory(InventoryView::default());
        assert_eq!(inventory.kind(), PageKind::Inventory);
        assert!(inventory.is_fullscreen());
        assert!(!inventory.is_overlay());
    }

    #[test]
    fn text_line_joins_spans() {
        let line = UiTextLine::from_spans(vec![
            UiSpan::new("HP: ", UiTextStyle::Muted),
            UiSpan::new("10/20", UiTextStyle::Danger),
        ]);
        assert_eq!(line.text(), "HP: 10/20");
        assert!(!line.is_empty());

        let empty = UiTextLine::default();
        assert!(empty.is_empty());
        assert_eq!(empty.text(), "");
    }

    #[test]
    fn list_view_clamps_selection() {
        let mut list =
            ListView::new("背包").with_items(vec![ListItemView::new("a"), ListItemView::new("b")]);
        assert_eq!(list.len(), 2);
        assert_eq!(
            list.selected_item().map(|item| item.label.as_str()),
            Some("a")
        );

        list.selected = 99;
        list.scroll = 99;
        list.clamp_selection();
        assert_eq!(list.selected, 1);
        assert_eq!(list.scroll, 1);

        list.items.clear();
        list.selected = 5;
        list.scroll = 5;
        list.clamp_selection();
        assert_eq!(list.selected, 0);
        assert_eq!(list.scroll, 0);
        assert!(list.is_empty());
    }

    #[test]
    fn dialog_builders() {
        let confirm = DialogView::confirm("退出", "确认退出？");
        assert_eq!(confirm.confirm.as_deref(), Some("y"));
        assert!(!confirm.danger);

        let danger = DialogView::danger("下楼", "确认下楼？");
        assert!(danger.danger);
        assert_eq!(danger.cancel.as_deref(), Some("n"));
    }

    #[test]
    fn look_view_builder() {
        let entity = EntityView::new(EntityId(1), (1, 1), VisualKey::Monster(1));
        let view = LookView::new((1, 1))
            .with_entity(entity)
            .with_footer("x/Esc 退出");
        assert_eq!(view.cursor, (1, 1));
        assert!(view.entity.is_some());
        assert_eq!(view.footer, "x/Esc 退出");
    }

    #[test]
    fn throw_aim_view_builder() {
        let view = ThrowAimView::new((2, 3))
            .with_path(vec![(1, 1), (2, 2), (3, 3)])
            .with_valid(true);
        assert_eq!(view.path.len(), 3);
        assert!(view.valid);
    }

    #[test]
    fn inventory_view_panels_and_focus() {
        let backpack = ListView::new("背包").with_items(vec![ListItemView::new("药水")]);
        let mut inventory = InventoryView {
            title: "背包".to_string(),
            panels: vec![InventoryPanelView::new(InventoryPanel::Backpack, backpack)],
            ..InventoryView::default()
        };
        assert_eq!(
            inventory.panel(InventoryPanel::Backpack).map(ListView::len),
            Some(1)
        );
        assert!(inventory.panel(InventoryPanel::Equipment).is_none());
        assert!(
            inventory.focused_panel().is_none(),
            "默认焦点是装备面板，但这里没有装备面板"
        );

        inventory.focus = InventoryFocus::Panel(InventoryPanel::Backpack);
        assert_eq!(inventory.focused_panel().map(ListView::len), Some(1));
        inventory.focus = InventoryFocus::Detail;
        assert!(inventory.focused_panel().is_none());

        assert!(inventory.panel_mut(InventoryPanel::Backpack).is_some());
        assert!(inventory.panel_mut(InventoryPanel::Ground).is_none());
    }
}
