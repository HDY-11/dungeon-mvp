//! 相机：视口尺寸 + 跟随目标 → [`Camera2D`]。
//!
//! 相机是**视图状态**，不是游戏状态：它决定「看到哪一块」，不改变世界里任何
//! 东西。因此它住在 `presentation`（DsnX14），后端只消费算好的 `Camera2D`。
//!
//! # 为什么在这里夹取而不是让后端自己裁
//!
//! 世界是 80×60 的定长网格。若相机中心允许贴边越界，后端就要自己处理
//! 「视口比世界大」或「视口超出世界边界」两种退化情况——那是**每个后端各写一遍**
//! 的重复逻辑，且很容易两个后端裁得不一样。这里一次夹好：相机中心保证让可见
//! 矩形尽量落在世界内。

use render_api::Camera2D;

use crate::extract::ExtractConfig;

/// 相机跟随目标。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CameraFollow {
    /// 跟随玩家（默认）。找不到玩家时退化为世界中心。
    #[default]
    Player,
    /// 固定在世界中心（调试 / 观战视角）。
    WorldCenter,
    /// 锁定在指定世界坐标（Look 页面滚动时用）。
    Fixed { x: usize, y: usize },
}

/// 把「世界坐标点」夹到「视口尽量落在世界内」的位置，返回相机中心。
///
/// 视口比世界大时没有可夹的余地，直接返回世界中心——否则玩家贴边会把整张地图
/// 推出屏幕，露出大片空白。
pub fn center_on_player(x: usize, y: usize, viewport: (u16, u16), world: (usize, usize)) -> (f32, f32) {
    let world_w = world.0 as f32;
    let world_h = world.1 as f32;
    let view_w = viewport.0 as f32;
    let view_h = viewport.1 as f32;

    let center = |target: f32, view: f32, world_size: f32| -> f32 {
        if view >= world_size {
            // 视口不小于世界：唯一合理的位置是世界中心。
            return world_size * 0.5;
        }
        // tile 是**格子**，相机中心落在半个格子上才能让视口正好框住整数格。
        let half = view * 0.5;
        // 目标点也取格子中心（+0.5），否则玩家会偏在视口左上。
        (target + 0.5).clamp(half, world_size - half)
    };

    (
        center(x as f32, view_w, world_w),
        center(y as f32, view_h, world_h),
    )
}

/// 按跟随目标计算相机。
pub fn camera_for(
    follow: CameraFollow,
    player: Option<(usize, usize)>,
    viewport: (u16, u16),
    config: &ExtractConfig,
) -> Camera2D {
    let world = (config.world_width, config.world_height);
    let target = match follow {
        CameraFollow::Player => player.unwrap_or((world.0 / 2, world.1 / 2)),
        CameraFollow::WorldCenter => (world.0 / 2, world.1 / 2),
        CameraFollow::Fixed { x, y } => (x, y),
    };
    let (cx, cy) = center_on_player(target.0, target.1, viewport, world);
    Camera2D::new((cx, cy), viewport, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORLD: (usize, usize) = (80, 60);

    #[test]
    fn center_is_clamped_so_viewport_stays_inside_world() {
        // 视口 40×20：中心可动范围是 [20, 60] × [10, 50]。
        let viewport = (40, 20);
        let camera = |x, y| center_on_player(x, y, viewport, WORLD);
        assert_eq!(camera(0, 0), (20.0, 10.0), "贴左上角时必须夹住");
        assert_eq!(camera(79, 59), (60.0, 50.0), "贴右下角时必须夹住");
        // (15, 25)：x 想取 15.5 → 夹到 20；y 想取 25.5 在范围内（10..=50）→ 保留。
        assert_eq!(camera(15, 25), (20.0, 25.5), "只有 x 需要夹");
    }

    #[test]
    fn target_is_taken_as_cell_center() {
        // 玩家在 (30, 30)，视口够小不需要夹取 → 中心应当是 (30.5, 30.5)。
        let (cx, cy) = center_on_player(30, 30, (10, 10), WORLD);
        assert_eq!((cx, cy), (30.5, 30.5));
    }

    #[test]
    fn viewport_larger_than_world_falls_back_to_world_center() {
        let (cx, cy) = center_on_player(0, 0, (200, 200), WORLD);
        assert_eq!((cx, cy), (40.0, 30.0), "视口比世界大时固定在世界中心");
    }

    #[test]
    fn viewport_equal_to_world_maps_to_world_center() {
        // 边界情形：`view >= world` 走同一个分支，不能出现负的 clamp 区间。
        let (cx, cy) = center_on_player(10, 10, (80, 60), WORLD);
        assert_eq!((cx, cy), (40.0, 30.0));
    }

    #[test]
    fn player_follow_without_player_falls_back_to_world_center() {
        let config = ExtractConfig::new(80, 60);
        // 退化目标取世界正中（80/2, 60/2）= (40, 30) → 中心点 (40.5, 30.5)。
        let camera = camera_for(CameraFollow::Player, None, (40, 20), &config);
        assert_eq!(camera.center, (40.5, 30.5));
    }

    #[test]
    fn fixed_follow_ignores_player() {
        let config = ExtractConfig::new(80, 60);
        let camera = camera_for(
            CameraFollow::Fixed { x: 70, y: 5 },
            Some((0, 0)),
            (40, 20),
            &config,
        );
        // x=70 需要夹到 60，y=5 需要夹到 10。
        assert_eq!(camera.center, (60.0, 10.0));
    }

    #[test]
    fn world_center_follow_ignores_player() {
        let config = ExtractConfig::new(80, 60);
        let camera = camera_for(CameraFollow::WorldCenter, Some((0, 0)), (10, 10), &config);
        assert_eq!(camera.center, (40.5, 30.5));
    }

    #[test]
    fn camera_is_always_valid_for_non_zero_viewport() {
        let config = ExtractConfig::new(80, 60);
        for viewport in [(1, 1), (40, 20), (80, 60), (200, 200)] {
            let camera = camera_for(CameraFollow::Player, Some((5, 5)), viewport, &config);
            assert!(camera.is_valid(), "viewport={viewport:?} 的相机必须可用");
            assert_eq!(camera.cells_per_unit, 1.0, "TUI 格子与世界单位 1:1");
        }
    }
}
