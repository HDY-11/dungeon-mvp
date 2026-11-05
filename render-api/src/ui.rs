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

/// 背包页导航栏里的一个页签。
///
/// 导航栏 1（Nav1）放「身体 / 行囊」，导航栏 2（Nav2）放「尸体 / 地面」；
/// 每个页签决定它下方 Context 栏显示哪个容器的内容（DsnA5）。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum InventoryNav {
    /// 身体七部位（Nav1 第一个页签）。
    #[default]
    Body,
    /// 旅者行囊（Nav1 第二个页签）。
    Pack,
    /// 脚下尸体（Nav2；有尸体时才出现）。
    Corpse,
    /// 脚下地面物品（Nav2；有地面物品时才出现）。
    Ground,
}

impl InventoryNav {
    /// 页签文字。渲染层只负责画，不自己造词。
    pub const fn label(self) -> &'static str {
        match self {
            Self::Body => "身体",
            Self::Pack => "行囊",
            Self::Corpse => "尸体",
            Self::Ground => "地面",
        }
    }

    /// 属于导航栏 1 还是导航栏 2。
    pub const fn group(self) -> InventoryNavGroup {
        match self {
            Self::Body | Self::Pack => InventoryNavGroup::Nav1,
            Self::Corpse | Self::Ground => InventoryNavGroup::Nav2,
        }
    }
}

/// 背包页的两条导航栏。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum InventoryNavGroup {
    Nav1,
    Nav2,
}

impl InventoryNavGroup {
    pub const fn context(self) -> InventoryFocus {
        match self {
            Self::Nav1 => InventoryFocus::Context1,
            Self::Nav2 => InventoryFocus::Context2,
        }
    }
}

/// 背包页的 4 个焦点位，顺时针 `Nav1 → Nav2 → Context2 → Context1`。
///
/// `Detail` 不是焦点位：详情跟随当前 Context 的选中行（DsnA5）。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum InventoryFocus {
    #[default]
    Nav1,
    Nav2,
    Context2,
    Context1,
}

impl InventoryFocus {
    /// 顺时针的下一个焦点位。
    pub const fn next(self) -> Self {
        match self {
            Self::Nav1 => Self::Nav2,
            Self::Nav2 => Self::Context2,
            Self::Context2 => Self::Context1,
            Self::Context1 => Self::Nav1,
        }
    }
}

/// 背包页视图（DsnA5）：3×3 网格 + 右侧详情。
///
/// ```text
/// Nav1     Nav2     Detail
/// Context1 Context2 Detail
/// Tip      Tip      Tip
/// ```
///
/// `nav2` 为空表示脚下既没有尸体也没有地面物品，此时 Nav2 / Context2 不参与渲染
/// 与焦点轮转。行与动作的对应关系由 `presentation` 决定，渲染层只画 `ListView`。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InventoryView {
    pub title: String,
    /// 导航栏 1 的页签（身体、行囊）。
    pub nav1: Vec<InventoryNav>,
    pub nav1_selected: usize,
    /// 导航栏 2 的页签（尸体、地面）；空 = 脚下什么都没有。
    pub nav2: Vec<InventoryNav>,
    pub nav2_selected: usize,
    pub context1: ListView,
    pub context2: ListView,
    pub detail: Option<DetailView>,
    /// 底部按键提示，逐行画。
    pub tips: Vec<UiTextLine>,
    pub focus: InventoryFocus,
}

impl InventoryView {
    /// 某条导航栏的页签列表。
    pub fn nav_tabs(&self, group: InventoryNavGroup) -> &[InventoryNav] {
        match group {
            InventoryNavGroup::Nav1 => &self.nav1,
            InventoryNavGroup::Nav2 => &self.nav2,
        }
    }

    /// 某条导航栏当前选中的页签。
    pub fn nav_selected(&self, group: InventoryNavGroup) -> Option<InventoryNav> {
        self.nav_tabs(group)
            .get(match group {
                InventoryNavGroup::Nav1 => self.nav1_selected,
                InventoryNavGroup::Nav2 => self.nav2_selected,
            })
            .copied()
    }

    /// 某条导航栏对应的 Context 栏。
    pub fn context(&self, group: InventoryNavGroup) -> &ListView {
        match group {
            InventoryNavGroup::Nav1 => &self.context1,
            InventoryNavGroup::Nav2 => &self.context2,
        }
    }

    /// 焦点所在的 Context 栏（焦点在导航栏时返回它的 Context）。
    pub fn focused_context(&self) -> &ListView {
        match self.focus {
            InventoryFocus::Nav1 | InventoryFocus::Context1 => &self.context1,
            InventoryFocus::Nav2 | InventoryFocus::Context2 => &self.context2,
        }
    }

    /// 焦点所在的导航栏（焦点在 Context 时返回它上方的导航栏）。
    pub fn focused_nav_group(&self) -> InventoryNavGroup {
        match self.focus {
            InventoryFocus::Nav1 | InventoryFocus::Context1 => InventoryNavGroup::Nav1,
            InventoryFocus::Nav2 | InventoryFocus::Context2 => InventoryNavGroup::Nav2,
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
    /// 背包页是网格视图，比其它页大得多；装箱避免把整个枚举撑到 368 字节
    /// （`SceneFrame` 每帧克隆一次）。
    Inventory(Box<InventoryView>),
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

        let inventory = UiView::Inventory(Box::default());
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
    fn inventory_grid_exposes_navs_contexts_and_focus_cycle() {
        let pack = ListView::new("行囊").with_items(vec![ListItemView::new("蘑菇 ×2")]);
        let inventory = InventoryView {
            title: "背包".to_string(),
            nav1: vec![InventoryNav::Body, InventoryNav::Pack],
            nav1_selected: 1,
            nav2: vec![InventoryNav::Ground],
            nav2_selected: 0,
            context1: pack,
            context2: ListView::new("地面").with_items(vec![ListItemView::new("石子 ×1")]),
            ..InventoryView::default()
        };

        assert_eq!(
            inventory.nav_selected(InventoryNavGroup::Nav1),
            Some(InventoryNav::Pack)
        );
        assert_eq!(
            inventory.nav_selected(InventoryNavGroup::Nav2),
            Some(InventoryNav::Ground)
        );
        // 焦点默认 Nav1 → 它对应 Context1。
        assert_eq!(inventory.focused_nav_group(), InventoryNavGroup::Nav1);
        assert_eq!(inventory.focused_context().len(), 1);
        // Nav1 的 Context1 里是行囊内容（身体页签的列表由 presentation 换）。
        assert_eq!(inventory.context(InventoryNavGroup::Nav1).items[0].label, "蘑菇 ×2");
        assert_eq!(inventory.nav_tabs(InventoryNavGroup::Nav2), &[InventoryNav::Ground]);
        assert_eq!(InventoryNav::Corpse.group(), InventoryNavGroup::Nav2);
        assert_eq!(InventoryNav::Body.label(), "身体");

        assert_eq!(
            [
                InventoryFocus::Nav1,
                InventoryFocus::Nav2,
                InventoryFocus::Context2,
                InventoryFocus::Context1,
            ]
            .map(InventoryFocus::next),
            [
                InventoryFocus::Nav2,
                InventoryFocus::Context2,
                InventoryFocus::Context1,
                InventoryFocus::Nav1,
            ],
            "焦点位顺时针轮转"
        );
        let mut focused = InventoryView {
            focus: InventoryFocus::Context2,
            ..inventory
        };
        assert_eq!(focused.focused_nav_group(), InventoryNavGroup::Nav2);
        focused.focus = InventoryFocus::Nav1;
        assert_eq!(focused.focused_context().items[0].label, "蘑菇 ×2");
    }
}
