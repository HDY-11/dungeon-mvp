//! `extract` 的测试：世界 → [`SceneFrame`] 的契约。
//!
//! 这里用 `ecs_core::world_loop::new_game` 造真实世界（而不是手搭组件），
//! 因为提取器要读的是**初始化后的**世界形状：地图、视野、记忆、日志都由
//! 初始化链路建立。手搭世界会漏掉"某资源其实没被初始化"这类真实问题。

use bevy_ecs::prelude::*;
use ecs_core::world_loop::new_game;
use render_api::{PageKind, VisualCategory, VisualKey, VisualLayer};

use super::*;
use crate::ui::PageStack;

fn frame_for(seed: u64, pages: &PageStack) -> (World, SceneFrame) {
    let mut world = new_game(seed);
    let config = ExtractConfig::default().with_viewport(40, 20);
    let frame = extract_scene_frame(&mut world, &config, pages, 1);
    (world, frame)
}

/// 一帧提取出来的基本形状：地图填满、玩家在、HUD 就绪、相机可用。
#[test]
fn frame_has_map_player_hud_and_valid_camera() {
    let (world, frame) = frame_for(11, &PageStack::new());

    assert_eq!(frame.revision, 1);
    assert!(frame.is_ready(), "初始化后的世界必须产出可渲染的帧");

    let map = &frame.map;
    assert_eq!((map.width, map.height), (80, 60));
    assert_eq!(map.len(), 80 * 60, "地形必须全量填充");

    // 视野非空，且玩家所在格必定可见。
    assert!(map.visible_count() > 0, "FOV 之后必须有可见格");
    assert!(map.explored_count() >= map.visible_count());

    let player = frame.player.as_ref().expect("必须提取到玩家");
    assert_eq!(player.visual, VisualKey::Player);
    assert_eq!(player.layer, VisualLayer::Actor);
    assert!(player.visible);
    assert!(
        map.is_visible(player.position.0 as usize, player.position.1 as usize),
        "玩家所在格必须在可见位图里"
    );

    assert!(frame.hud.is_ready(), "HUD 必须有玩家名");
    assert_eq!(frame.hud.floor, 1);
    assert!(frame.hud.hp.max > 0.0);
    assert!(frame.camera.is_valid());

    // 世界确实存在（防止"忘了把 world 还回来"的低级写法）。
    assert!(world.get_resource::<ecs_core::Map>().is_some());
}

/// 提取必须是**只读**的：跑两次拿到同样的帧（除了 revision）。
#[test]
fn extraction_is_read_only_and_deterministic() {
    let pages = PageStack::new();
    let mut world = new_game(7);
    let config = ExtractConfig::default().with_viewport(40, 20);

    let first = extract_scene_frame(&mut world, &config, &pages, 1);
    let second = extract_scene_frame(&mut world, &config, &pages, 2);

    assert_eq!(
        SceneFrame {
            revision: 0,
            ..first.clone()
        },
        SceneFrame {
            revision: 0,
            ..second.clone()
        },
        "同一世界连续提取两次，除 revision 外必须完全一致"
    );
    assert_eq!(first.revision, 1);
    assert_eq!(second.revision, 2);
}

/// 实体视图：怪物被计入、玩家不重复出现、楼梯在探索后被标为可见。
#[test]
fn entities_exclude_player_and_include_stairs_once_explored() {
    let pages = PageStack::new();
    let (world, frame) = frame_for(11, &pages);

    // 玩家只在 `frame.player` 里，不在 entities 里（后端要给它特殊样式）。
    let player_bits = frame.player.as_ref().unwrap().id.to_bits();
    assert!(
        frame.entities.iter().all(|e| e.id.to_bits() != player_bits),
        "玩家不得重复出现在 entities 里"
    );

    // 世界里有怪物（初始化会生成），所以 entities 非空。
    assert!(!frame.entities.is_empty(), "初始化后应当有怪物实体");
    assert!(
        frame
            .entities
            .iter()
            .any(|e| e.visual.category() == VisualCategory::Monster),
        "至少有实体的视觉键是怪物"
    );

    // 楼梯：一旦所在格已探索就必须可见（它是导航目标，不该随视野闪烁）。
    let stairs = frame
        .entities
        .iter()
        .find(|e| e.visual == VisualKey::Stairs)
        .expect("初始化会放置楼梯");
    let (sx, sy) = (stairs.position.0 as usize, stairs.position.1 as usize);
    // 开局楼梯通常还在视野外（未被探索），所以先断言"可见性与探索位一致"，
    // 再人为把它标为已探索、复提一帧，验证"探索即可见"这条规则。
    assert_eq!(
        stairs.visible,
        frame.map.is_explored(sx, sy),
        "楼梯可见性必须与探索位一致"
    );

    let mut world = world;
    world.resource_mut::<ecs_core::MapMemory>().explored[sy][sx] = true;
    let frame = extract_scene_frame(
        &mut world,
        &ExtractConfig::default().with_viewport(40, 20),
        &pages,
        2,
    );
    let stairs = frame
        .entities
        .iter()
        .find(|e| e.visual == VisualKey::Stairs)
        .expect("楼梯必须仍在");
    assert!(
        stairs.visible,
        "已探索的楼梯必须可见（否则玩家找不到下楼的路）"
    );

    // 实体顺序稳定：同层内按 (y, x) 递增。
    //
    // 排序键**不含 entity id**：Bevy 会复用 entity 槽位，按 id 打破平局会让
    // 同格两个实体的顺序随分配历史变化。提取器里也只按 (layer, y, x, bits)
    // 排——对同格实体，位置相同时 bits 只作最后的稳定兜底。
    let mut sorted = frame.entities.clone();
    sorted.sort_by_key(|e| {
        (
            e.layer,
            e.position.1,
            e.position.0,
            e.id.to_bits(),
        )
    });
    assert_eq!(frame.entities, sorted, "实体顺序必须稳定可复现");

    // world 未受影响。
    drop(world);
}

/// 视野外的怪物不得出现在帧里（防止"隔着墙看到怪物"）。
#[test]
fn monsters_outside_viewshed_are_not_rendered() {
    let mut world = new_game(11);
    let pages = PageStack::new();
    let config = ExtractConfig::default().with_viewport(40, 20);

    // 把玩家视野清空：此时除了玩家自己，不应再有"可见"的实体。
    {
        let player = {
            let mut query = world.query_filtered::<Entity, With<ecs_core::Player>>();
            query.iter(&world).next().expect("有玩家")
        };
        let mut viewshed = world.get_mut::<ecs_core::Viewshed>(player).unwrap();
        viewshed.visible_tiles.clear();
    }

    let frame = extract_scene_frame(&mut world, &config, &pages, 1);
    assert_eq!(frame.map.visible_count(), 0, "视野清空后可见格为 0");
    assert!(
        frame
            .entities
            .iter()
            .all(|entity| entity.visual == VisualKey::Stairs || !entity.visible),
        "视野清空后除楼梯外不得有可见实体"
    );
}

/// 小世界配置：世界外的实体会被丢弃而不是画到地图外。
#[test]
fn entities_outside_configured_world_are_dropped() {
    let mut world = new_game(11);
    let pages = PageStack::new();
    // 3×3 的小世界：绝大多数实体都在范围外。
    let config = ExtractConfig::new(3, 3).with_viewport(3, 3);
    let frame = extract_scene_frame(&mut world, &config, &pages, 1);

    assert_eq!((frame.map.width, frame.map.height), (3, 3));
    assert_eq!(frame.map.len(), 9);
    assert!(
        frame
            .entities
            .iter()
            .all(|e| (0..3).contains(&e.position.0) && (0..3).contains(&e.position.1)),
        "世界外的实体必须被丢弃"
    );
}

/// 日志按时间正序保留最近 N 条。
#[test]
fn log_keeps_the_most_recent_lines_in_chronological_order() {
    let mut world = new_game(11);
    let pages = PageStack::new();

    {
        let mut log = world.resource_mut::<ecs_core::EventLog>();
        log.push(ecs_core::EventMessage::combat("第一条"));
        log.push(ecs_core::EventMessage::danger("第二条"));
        log.push(ecs_core::EventMessage::system("第三条"));
    }

    let config = ExtractConfig::default().with_log_lines(2);
    let frame = extract_scene_frame(&mut world, &config, &pages, 1);

    let texts: Vec<&str> = frame.log.iter().map(|line| line.text.as_str()).collect();
    assert_eq!(texts, vec!["第二条", "第三条"], "取最近 2 条且保持正序");
    assert_eq!(frame.log[0].level, render_api::LogLevel::Danger);
    assert_eq!(frame.log[1].level, render_api::LogLevel::System);
}

/// 页栈状态进入帧：Game 页对应 `UiView::Game`，退出确认对应 Dialog。
#[test]
fn page_stack_state_is_reflected_in_the_frame() {
    let mut world = new_game(11);
    let config = ExtractConfig::default().with_viewport(40, 20);

    let mut pages = PageStack::new();
    let frame = extract_scene_frame(&mut world, &config, &pages, 1);
    assert_eq!(frame.ui, render_api::UiView::Game);
    assert_eq!(frame.ui.kind(), PageKind::Game);

    pages.open_quit_dialog();
    let frame = extract_scene_frame(&mut world, &config, &pages, 2);
    assert_eq!(frame.ui.kind(), PageKind::Dialog);
    assert!(frame.ui.captures_input());
}

/// 游戏结束与退出请求必须透传（后端据此画提示）。
#[test]
fn game_over_and_quit_flags_are_forwarded() {
    let mut world = new_game(11);
    let pages = PageStack::new();
    let config = ExtractConfig::default().with_viewport(40, 20);

    world.resource_mut::<ecs_core::TurnManager>().game_over = true;
    let frame = extract_scene_frame(&mut world, &config, &pages, 1);
    assert!(frame.game_over);
    assert!(!frame.quit_requested);
}

/// 帧号自增器：单调递增，且后端可据此跳过未变化的帧。
#[test]
fn frame_source_bumps_revision_monotonically() {
    let mut world = new_game(11);
    let pages = PageStack::new();
    let config = ExtractConfig::default().with_viewport(40, 20);

    let mut source = SceneFrameSource::new();
    assert_eq!(source.revision(), 0);

    let first = source.next_frame(&mut world, &config, &pages);
    let second = source.next_frame(&mut world, &config, &pages);
    assert_eq!(source.revision(), 2);
    assert_eq!(first.revision, 1);
    assert_eq!(second.revision, 2);
    assert!(second.revision > first.revision);
}

/// 相机跟随玩家：中心应当落在玩家附近（视口 40×20 时允许被世界边界夹取）。
#[test]
fn camera_follows_the_player() {
    let (_, frame) = frame_for(11, &PageStack::new());
    let player = frame.player.as_ref().unwrap();
    let rect = frame.camera.visible_rect();

    assert!(
        rect.contains(player.position.0 as f32 + 0.5, player.position.1 as f32 + 0.5),
        "玩家必须在相机可见矩形内：player={:?} rect={rect:?}",
        player.position
    );
}

#[test]
fn log_level_mapping_covers_every_core_level() {
    use ecs_core::EventLevel;
    use render_api::LogLevel;
    let cases = [
        (EventLevel::Combat, LogLevel::Combat),
        (EventLevel::Item, LogLevel::Item),
        (EventLevel::Skill, LogLevel::Skill),
        (EventLevel::System, LogLevel::System),
        (EventLevel::Danger, LogLevel::Danger),
    ];
    for (core_level, expected) in cases {
        assert_eq!(map_log_level(core_level), expected, "{core_level:?}");
    }
}
