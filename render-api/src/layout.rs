//! 后端无关的窗口布局：四栏划分与焦点轮转。
//!
//! # 为什么布局在契约层
//!
//! 布局只有一份权威：**焦点轮转**（Tab 顺时针）、**鼠标命中**（点哪一栏）
//! 与**相机视口**（地图栏多大）都读同一份矩形。曾经布局只存在于 TUI 的
//! `frame_areas` 里，于是 `presentation` 要把终端尺寸喂给相机、又要把
//! 键盘焦点和鼠标坐标换算回栏位——每个消费方都得复制一遍划分规则，
//! 迟早漂移。现在划分是 [`game_layout`]，后端只负责把 [`Rect`] 换成自己的
//! 矩形类型（TUI 的 ratatui `Rect`、未来的 GPU 视口）。
//!
//! # 固定 2×2 四栏
//!
//! ```text
//! ┌─────────────┬────────┐
//! │  摄像机      │  信息   │
//! ├─────────────┼────────┤
//! │  事件        │  状态   │
//! └─────────────┴────────┘
//! ```
//!
//! 焦点顺序按**顺时针**：摄像机 → 信息 → 状态 → 事件（[`Pane::next`]）。
//! 顺序是行为约定，不是绘制细节：TUI 高亮、鼠标点击、键盘 Tab 都按它走。

/// 后端无关的矩形（终端格 / 表面单位）。
///
/// 原点在左上角；`x` / `y` 是相对整个表面的偏移。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

impl Rect {
    /// 空矩形：没有面积。
    pub const EMPTY: Self = Self {
        x: 0,
        y: 0,
        width: 0,
        height: 0,
    };

    pub const fn new(x: u16, y: u16, width: u16, height: u16) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// 是否没有面积（宽或高为 0）。
    pub const fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0
    }

    /// 右边界（不含）。
    pub const fn right(self) -> u16 {
        self.x.saturating_add(self.width)
    }

    /// 下边界（不含）。
    pub const fn bottom(self) -> u16 {
        self.y.saturating_add(self.height)
    }

    /// 点是否落在矩形内（右/下边界不含）。
    pub const fn contains(self, x: u16, y: u16) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }

    /// 去掉四周边框后的内容区；边框大于尺寸时退化为空矩形。
    pub const fn inner(self, border: u16) -> Self {
        let shrink_x = border.saturating_mul(2);
        let shrink_y = border.saturating_mul(2);
        if self.width <= shrink_x || self.height <= shrink_y {
            return Self::EMPTY;
        }
        Self {
            x: self.x.saturating_add(border),
            y: self.y.saturating_add(border),
            width: self.width - shrink_x,
            height: self.height - shrink_y,
        }
    }
}

/// 游戏画面的四个栏位。
///
/// 变体顺序就是焦点顺时针顺序；[`Pane::next`] 依赖这个顺序，新增栏位时
/// 要同步 [`game_layout`] 与 [`Pane::next`]。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Pane {
    /// 地图（摄像机）。
    #[default]
    Camera,
    /// 可交互物 / 怪物列表（信息）。
    Info,
    /// 玩家状态。
    Status,
    /// 事件时间轴。
    Events,
}

impl Pane {
    /// 顺时针的下一个栏位；四个栏位一圈后回到自身。
    pub const fn next(self) -> Self {
        match self {
            Self::Camera => Self::Info,
            Self::Info => Self::Status,
            Self::Status => Self::Events,
            Self::Events => Self::Camera,
        }
    }

    /// 全部栏位，按焦点顺序。
    pub const ALL: [Self; 4] = [Self::Camera, Self::Info, Self::Status, Self::Events];
}

/// 四栏布局结果；矩形的坐标系与传入的表面相同。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GameLayout {
    /// 地图栏（含边框）。
    pub camera: Rect,
    /// 信息栏（含边框）。
    pub info: Rect,
    /// 状态栏（含边框）。
    pub status: Rect,
    /// 事件栏（含边框）。
    pub events: Rect,
}

impl GameLayout {
    /// 指定栏位的矩形。
    pub const fn rect(self, pane: Pane) -> Rect {
        match pane {
            Pane::Camera => self.camera,
            Pane::Info => self.info,
            Pane::Status => self.status,
            Pane::Events => self.events,
        }
    }

    /// 鼠标命中：坐标落在哪一栏。坐标不在任何栏位内时返回 `None`。
    pub const fn pane_at(self, x: u16, y: u16) -> Option<Pane> {
        if self.camera.contains(x, y) {
            Some(Pane::Camera)
        } else if self.info.contains(x, y) {
            Some(Pane::Info)
        } else if self.status.contains(x, y) {
            Some(Pane::Status)
        } else if self.events.contains(x, y) {
            Some(Pane::Events)
        } else {
            None
        }
    }
}

/// 支持的最小表面尺寸（终端 80×24）。小于它时只画地图，其它栏位为空。
pub const MIN_SURFACE: (u16, u16) = (80, 24);
/// 四栏布局的最低门槛：低于它（宽 < 60 或高 < 16）退化为全屏地图。
const LAYOUT_FLOOR: (u16, u16) = (60, 16);
/// 右栏最小宽度：低于它时信息/状态放不下两三个汉字 + 数值。
const SIDE_MIN_WIDTH: u16 = 26;
/// 地图栏最小宽度；右栏再宽也不能把地图挤没。
const CAMERA_MIN_WIDTH: u16 = 24;
/// 事件栏最小高度（行）；顶部栏再高也要给事件留几行。
const EVENTS_MIN_HEIGHT: u16 = 6;
/// 顶部栏最小高度。
const TOP_MIN_HEIGHT: u16 = 8;

/// 计算四栏布局。
///
/// - 大于等于 [`MIN_SURFACE`] 时按 42% 右栏 / 60% 顶部划分（地图优先）；
/// - 60×16 到 80×24 之间仍出四栏，只是更挤；
/// - 小于 60×16 时只保留地图（`camera` 占满表面，其余为空矩形），
///   后端据此降级画一张全屏地图，而不是画一堆 0 宽面板。
pub fn game_layout(surface: (u16, u16)) -> GameLayout {
    let (width, height) = surface;
    if width < LAYOUT_FLOOR.0 || height < LAYOUT_FLOOR.1 {
        return GameLayout {
            camera: Rect::new(0, 0, width, height),
            info: Rect::EMPTY,
            status: Rect::EMPTY,
            events: Rect::EMPTY,
        };
    }

    let right = ((u32::from(width) * 42 + 50) / 100) as u16;
    let right = right
        .max(SIDE_MIN_WIDTH)
        .min(width.saturating_sub(CAMERA_MIN_WIDTH));
    let left = width.saturating_sub(right);

    let top = ((u32::from(height) * 60 + 50) / 100) as u16;
    let top = top
        .max(TOP_MIN_HEIGHT)
        .min(height.saturating_sub(EVENTS_MIN_HEIGHT));

    let left = left.max(1);
    let right = width.saturating_sub(left);
    let top = top.max(1);
    let bottom = height.saturating_sub(top);

    GameLayout {
        camera: Rect::new(0, 0, left, top),
        info: Rect::new(left, 0, right, top),
        events: Rect::new(0, top, left, bottom),
        status: Rect::new(left, top, right, bottom),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focus_cycles_clockwise_and_returns() {
        assert_eq!(Pane::Camera.next(), Pane::Info);
        assert_eq!(Pane::Info.next(), Pane::Status);
        assert_eq!(Pane::Status.next(), Pane::Events);
        assert_eq!(Pane::Events.next(), Pane::Camera);
        assert_eq!(Pane::ALL.len(), 4);
    }

    #[test]
    fn rect_geometry_helpers() {
        let rect = Rect::new(2, 3, 10, 5);
        assert_eq!(rect.right(), 12);
        assert_eq!(rect.bottom(), 8);
        assert!(rect.contains(2, 3));
        assert!(rect.contains(11, 7));
        assert!(!rect.contains(12, 7));
        assert!(!rect.contains(2, 8));
        assert!(!rect.is_empty());

        let inner = rect.inner(1);
        assert_eq!(inner, Rect::new(3, 4, 8, 3));
        assert_eq!(Rect::new(0, 0, 2, 2).inner(1), Rect::EMPTY);
        assert_eq!(Rect::new(0, 0, 1, 1).inner(1), Rect::EMPTY);
    }

    #[test]
    fn panes_tile_the_whole_surface_without_gaps_or_overlap() {
        for surface in [(80, 24), (100, 30), (120, 40), (61, 17)] {
            let layout = game_layout(surface);
            let panes = Pane::ALL.map(|pane| layout.rect(pane));

            // 面积之和 = 整个表面，没有缝隙也没有重叠。
            let total: u32 = panes
                .iter()
                .map(|rect| u32::from(rect.width) * u32::from(rect.height))
                .sum();
            assert_eq!(
                total,
                u32::from(surface.0) * u32::from(surface.1),
                "{surface:?} 的栏位没有铺满"
            );

            // 四角各落在一个栏位里。
            assert_eq!(layout.pane_at(0, 0), Some(Pane::Camera), "{surface:?}");
            assert_eq!(
                layout.pane_at(surface.0 - 1, 0),
                Some(Pane::Info),
                "{surface:?}"
            );
            assert_eq!(
                layout.pane_at(0, surface.1 - 1),
                Some(Pane::Events),
                "{surface:?}"
            );
            assert_eq!(
                layout.pane_at(surface.0 - 1, surface.1 - 1),
                Some(Pane::Status),
                "{surface:?}"
            );
            assert_eq!(layout.pane_at(surface.0, surface.1), None);
        }
    }

    #[test]
    fn camera_takes_the_larger_share() {
        let layout = game_layout((120, 40));
        assert_eq!(layout.camera, Rect::new(0, 0, 70, 24));
        assert_eq!(layout.info, Rect::new(70, 0, 50, 24));
        assert_eq!(layout.events, Rect::new(0, 24, 70, 16));
        assert_eq!(layout.status, Rect::new(70, 24, 50, 16));
    }

    #[test]
    fn minimum_surface_still_has_four_panes() {
        let layout = game_layout(MIN_SURFACE);
        for pane in Pane::ALL {
            let rect = layout.rect(pane);
            assert!(!rect.is_empty(), "{pane:?} 在最小终端上不该为空");
            assert!(!rect.inner(1).is_empty(), "{pane:?} 去掉边框后要有内容区");
        }
    }

    #[test]
    fn tiny_surface_degrades_to_fullscreen_map() {
        let layout = game_layout((40, 12));
        assert_eq!(layout.camera, Rect::new(0, 0, 40, 12));
        assert!(layout.info.is_empty());
        assert!(layout.status.is_empty());
        assert!(layout.events.is_empty());
        assert_eq!(layout.pane_at(0, 0), Some(Pane::Camera));
    }
}
