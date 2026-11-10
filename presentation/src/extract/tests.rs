//! `extract` 的测试：世界 → [`SceneFrame`] 的契约。
//!
//! 这里用 `ecs_core::world_loop::new_game` 造真实世界（而不是手搭组件），
//! 因为提取器要读的是**初始化后的**世界形状：地图、视野、记忆、日志都由
//! 初始化链路建立。手搭世界会漏掉"某资源其实没被初始化"这类真实问题。

use bevy_ecs::prelude::*;
use ecs_core::world_loop::new_game;
use render_api::{PageKind, UiView, VisualCategory, VisualKey, VisualLayer};

use super::*;
use crate::ui::{PageStack, UiIntent};

fn frame_for(seed: u64, pages: &PageStack) -> (World, SceneFrame) {
    let mut world = new_game(seed);
    let config = ExtractConfig::default().with_viewport(40, 20);
    let frame = extract_scene_frame(&mut world, &config, pages, 1);
    (world, frame)
}

/// 一帧提取出来的基本形状：地图填满、玩家在、HUD 就绪、相机可用。
#[test]
fn frame_has_map_player_hud_and_valid_camera() {    let (world, frame) = frame_for(11, &PageStack::new());

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

/// **视野内的怪物必须可见且能画出来**（SYN5 的正面断言）。
///
/// 这条与下面的"视野外不可见"成对：只有两条都在，才说明可见性判据**按实体类型分别裁定**。
/// 此前 `visible = is_stairs && explored`，怪物恒不可见——而当时那条"视野外不可见"的测试
/// 在一个永真命题上通过，等于没测。
#[test]
fn monsters_inside_the_viewshed_are_visible() {
    let mut world = new_game(2026);
    // 把一只怪物搬到玩家身边（视野内）。
    let player = {
        let mut q = world.query_filtered::<Entity, With<ecs_core::Player>>();
        q.iter(&world).next().expect("有玩家")
    };
    let player_pos = *world.get::<ecs_core::Position>(player).expect("玩家有位置");
    let monster = {
        let mut q = world.query_filtered::<Entity, With<ecs_core::Monster>>();
        q.iter(&world).next().expect("有怪物")
    };
    let adjacent = ecs_core::Position::new(player_pos.x, player_pos.y).offset(1, 0);
    {
        let mut position = world.get_mut::<ecs_core::Position>(monster).expect("怪物有位置");
        position.x = adjacent.0;
        position.y = adjacent.1;
    }
    // 让视野覆盖到它（直接给玩家一个足够大的视野，避免依赖地图几何）。
    if let Some(mut viewshed) = world.get_mut::<ecs_core::Viewshed>(player) {
        viewshed.visible_tiles.push(adjacent);
        viewshed.visible_tiles.push((player_pos.x, player_pos.y));
    }

    let config = ExtractConfig::default().with_viewport(40, 20);
    let frame = extract_scene_frame(&mut world, &config, &PageStack::new(), 1);

    // 按**位置**找那一只：帧里所有怪物都在（可见性只是标记），
    // 直接找第一只 `Monster` 会拿到别的怪物。
    let view = frame
        .entities
        .iter()
        .find(|entity| {
            matches!(entity.visual, VisualKey::Monster(_))
                && entity.position == (adjacent.0 as i32, adjacent.1 as i32)
        })
        .expect("视野内的怪物必须出现在帧里");
    assert!(
        view.visible,
        "视野内的怪物必须标为 visible，否则 tui 不会画它（SYN5）"
    );
}

/// 尸体（DsnE18 第②片）要能以**独立外观 + 名字**出现在帧里，且画在物品层。
///
/// 外观曾经复用怪物字形，玩家分不清"站着的哥布林"和"躺着的哥布林"——
/// 现在契约里有了 [`render_api::VisualKey::Corpse`]，这条测试钉住映射。
#[test]
fn corpses_are_drawn_with_their_own_visual_and_name() {
    let mut world = new_game(2026);
    let player = {
        let mut query = world.query_filtered::<Entity, With<ecs_core::Player>>();
        query.iter(&world).next().expect("有玩家")
    };
    let player_pos = *world.get::<ecs_core::Position>(player).expect("玩家有位置");

    // 玩家脚下放一具装着匕首的哥布林尸体。
    let dagger = world
        .spawn((
            ecs_core::Item::new(ecs_core::ItemId(24)),
            ecs_core::Amount::new(1),
        ))
        .id();
    let mut container = ecs_core::Container::new(&ecs_core::CORPSE);
    container
        .insert(0, dagger, &[ecs_core::ItemTag::Weapon])
        .expect("尸体格子无过滤");
    world.spawn((
        ecs_core::monster::Corpse::new(ecs_core::monster::MonsterKindId::Goblin, "哥布林"),
        ecs_core::Position::new(player_pos.x, player_pos.y),
        container,
    ));

    let config = ExtractConfig::default().with_viewport(40, 20);
    let frame = extract_scene_frame(&mut world, &config, &PageStack::new(), 1);
    let view = frame
        .entities
        .iter()
        .find(|entity| entity.name == "哥布林的尸体")
        .expect("尸体必须出现在帧里");
    assert_eq!(
        view.visual,
        render_api::VisualKey::Corpse,
        "尸体使用独立外观，不能与活着的哥布林同形"
    );
    assert_eq!(view.layer, VisualLayer::Item, "尸体画在物品层");
    assert!(view.visible, "探索过的格子上尸体必须可见");
}

/// 背包页要列出脚下的地面物品（DsnE21 阶段 4：拾取全显式）。
#[test]
fn the_inventory_page_lists_ground_items_underfoot() {
    let mut world = new_game(2026);
    let player = {
        let mut query = world.query_filtered::<Entity, With<ecs_core::Player>>();
        query.iter(&world).next().expect("有玩家")
    };
    let here = world
        .get::<ecs_core::Position>(player)
        .expect("玩家有位置")
        .to_tuple();
    world.spawn((
        ecs_core::Item::new(ecs_core::ItemId(14)),
        ecs_core::Amount::new(3),
        ecs_core::GroundItemAt::new(here.0, here.1),
    ));

    let mut pages = PageStack::new();
    pages.open_inventory();
    let nav2_len = inventory_nav2_tabs(&world).len();
    pages.set_inventory_nav2_len(nav2_len);
    // 顺时针：Nav1 → Nav2 → Context2；脚下内容栏只看 Nav2 选中的页签。
    pages.apply(UiIntent::CycleInventoryFocus);
    pages.apply(UiIntent::CycleInventoryFocus);
    assert_eq!(
        pages.inventory_focus(),
        render_api::InventoryFocus::Context2,
        "两次 Tab 后焦点应落在脚下内容栏"
    );

    let target = inventory_target(&world, &pages).expect("地面物品必须成为可选中行");
    assert!(
        matches!(target, InventoryTarget::Ground { index: 0 }),
        "第一行是地面物品的确定性排序第一件：{target:?}"
    );
    let view = inventory_view(&world, &pages);
    assert_eq!(
        view.nav2,
        vec![render_api::InventoryNav::Ground],
        "脚下只有地面物品时 Nav2 只有一个页签"
    );
    assert_eq!(view.context2.title, "地面");
    assert!(
        view.context2
            .items
            .iter()
            .any(|item| item.label.contains("黑色甲壳")),
        "行标签要写物品名：{:?}",
        view.context2.items.iter().map(|item| &item.label).collect::<Vec<_>>()
    );
    assert_eq!(view.context2.footer, "Enter 拿取");
    let detail = view.detail.expect("选中地面物品必须有详情");
    assert_eq!(detail.title, "黑色甲壳 ×3");
    assert!(detail.footer.contains("Enter 拾取"));
}

/// 同格多具尸体：Nav2 只有一个「尸体」页签，内容栏按实体编号分组，
/// 分组标题行不可操作，物品行带对应尸体的下标（DsnE21 决策 7）。
#[test]
fn multiple_corpses_are_grouped_under_headers() {
    let mut world = new_game(2026);
    let player = {
        let mut query = world.query_filtered::<Entity, With<ecs_core::Player>>();
        query.iter(&world).next().expect("有玩家")
    };
    let here = world
        .get::<ecs_core::Position>(player)
        .expect("玩家有位置")
        .to_tuple();
    // 两具尸体都踩在脚下；名字故意不同，便于断言分组标题。
    let (_first, _second) = {
        let mut add = |name: &'static str, id: u16| {
            let item = world
                .spawn((ecs_core::Item::new(ecs_core::ItemId(id)), ecs_core::Amount::new(1)))
                .id();
            let mut container = ecs_core::Container::new(&ecs_core::CORPSE);
            container.insert(0, item, &[]).expect("尸体格子无过滤");
            world
                .spawn((
                    ecs_core::monster::Corpse::new(
                        ecs_core::monster::MonsterKindId::Goblin,
                        name,
                    ),
                    ecs_core::Position::new(here.0, here.1),
                    container,
                ))
                .id()
        };
        (add("哥布林", 24), add("老鼠", 23))
    };

    let mut pages = PageStack::new();
    pages.open_inventory();
    let nav2_len = inventory_nav2_tabs(&world).len();
    pages.set_inventory_nav2_len(nav2_len);
    assert_eq!(
        pages.inventory_nav2_selected(),
        0,
        "只有一个「尸体」页签，两具尸体在内容栏里分组"
    );
    let view = inventory_view(&world, &pages);
    assert_eq!(view.nav2, vec![render_api::InventoryNav::Corpse]);
    assert!(
        view.context2.title.contains("2 具尸体"),
        "标题要说明脚下有几具：{}",
        view.context2.title
    );
    let labels: Vec<&str> = view
        .context2
        .items
        .iter()
        .map(|item| item.label.as_str())
        .collect();
    assert_eq!(
        labels,
        vec!["哥布林的尸体", "匕首", "老鼠的尸体", "石锤"],
        "多具尸体按实体编号分组列出"
    );
    assert!(view.context2.items[0].disabled, "分组标题不可操作");
    assert!(!view.context2.items[1].disabled, "物品行可操作");

    // 焦点移到脚下内容栏，逐行检查目标：标题行没有目标，物品行带尸体下标。
    pages.apply(UiIntent::CycleInventoryFocus);
    pages.apply(UiIntent::CycleInventoryFocus);
    assert_eq!(pages.inventory_focus(), render_api::InventoryFocus::Context2);
    assert_eq!(inventory_target(&world, &pages), None, "标题行不可操作");
    pages.apply(UiIntent::MoveInventoryCursor { dx: 0, dy: 1 });
    assert_eq!(
        inventory_target(&world, &pages),
        Some(InventoryTarget::Corpse {
            corpse: 0,
            slot: 0
        })
    );
    pages.apply(UiIntent::MoveInventoryCursor { dx: 0, dy: 1 });
    assert_eq!(inventory_target(&world, &pages), None, "第二具尸体的标题行");
    pages.apply(UiIntent::MoveInventoryCursor { dx: 0, dy: 1 });
    assert_eq!(
        inventory_target(&world, &pages),
        Some(InventoryTarget::Corpse {
            corpse: 1,
            slot: 0
        }),
        "第三件物品属于第二具尸体"
    );
}

/// 背包页详情面板（R2 收尾）：选中身体里的剑 → 名字/说明/修正/部位/动作提示。
#[test]
fn the_inventory_detail_describes_the_selected_item() {
    let mut world = new_game(2026);
    let player = {
        let mut query = world.query_filtered::<Entity, With<ecs_core::Player>>();
        query.iter(&world).next().expect("有玩家")
    };
    let sword_def = ecs_core::ItemId(0).def();
    let sword = world
        .spawn((ecs_core::Item::new(ecs_core::ItemId(0)), ecs_core::Amount::new(1)))
        .id();
    // 主手是哪一格由布局表说了算（不写常数）。
    let main_hand = ecs_core::BODY
        .slots
        .iter()
        .position(|slot| slot.accept.accepts(&[ecs_core::ItemTag::Weapon]))
        .expect("身体必须有武器槽");
    {
        let mut body = world.get_mut::<ecs_core::Container>(player).expect("有身体");
        body.insert(main_hand, sword, sword_def.tags)
            .expect("主手空着");
    }

    let mut pages = PageStack::new();
    pages.open_inventory();
    let view = inventory_view(&world, &pages);
    let detail = view.detail.expect("选中行必须有详情");
    assert_eq!(detail.title, sword_def.name, "标题是物品名");
    let lines: Vec<String> = detail.lines.iter().map(|line| line.text()).collect();
    assert!(
        lines.iter().any(|line| line == sword_def.desc),
        "详情要带注册表里的一句话说明：{lines:?}"
    );
    assert!(
        lines.iter().any(|line| line.contains(ecs_core::StatId::ATTACK.label())),
        "装备修正要出现在详情里：{lines:?}"
    );
    assert!(
        lines.iter().any(|line| line.starts_with("部位：")),
        "要告诉玩家它装在哪个部位：{lines:?}"
    );
    assert!(
        detail.footer.contains("Enter 卸下"),
        "身体行的动作提示：{}",
        detail.footer
    );
    assert!(
        lines.iter().any(|line| line.starts_with("重量：")),
        "详情要标出重量（DsnE20）：{lines:?}"
    );
    assert!(
        lines.iter().any(|line| line.starts_with("品质：")),
        "详情要标出品质（DsnE23）：{lines:?}"
    );
    let row = view
        .context1
        .items
        .iter()
        .find(|row| row.label.contains(sword_def.name))
        .expect("身体栏要有剑这一行");
    assert_eq!(
        row.style,
        render_api::UiTextStyle::Quality(sword_def.quality.rank()),
        "物品行按品质着色"
    );
    let tips: Vec<String> = view.tips.iter().map(|line| line.text()).collect();
    assert!(
        tips.iter().any(|line| line.contains("Enter 装/卸/拿")),
        "底部键位提示要完整：{tips:?}"
    );

    // Nav1 聚焦时 ←/→ 切页签——行囊页签的 Context1 底部显示占用与载重。
    pages.apply(UiIntent::MoveInventoryCursor { dx: 1, dy: 0 });
    let view = inventory_view(&world, &pages);
    assert_eq!(view.nav1_selected, 1, "→ 键切到行囊页签");
    assert_eq!(view.context1.title, "行囊");
    assert!(
        view.context1.footer.contains("载重"),
        "行囊栏要显示载重：{}",
        view.context1.footer
    );
}

/// 行囊里的消耗品：详情说效果，动作提示是 `r 使用`（而不是装/卸）。
#[test]
fn the_inventory_detail_for_a_consumable_says_how_to_use_it() {
    let mut world = new_game(2026);
    let player = {
        let mut query = world.query_filtered::<Entity, With<ecs_core::Player>>();
        query.iter(&world).next().expect("有玩家")
    };
    let mushroom_def = ecs_core::ItemId(25).def();
    let mushroom = world
        .spawn((ecs_core::Item::new(ecs_core::ItemId(25)), ecs_core::Amount::new(1)))
        .id();
    let pack = {
        let body = world.get::<ecs_core::Container>(player).expect("有身体");
        body.slot(ecs_core::item::pack_slot_index())
            .expect("身体背包槽里有行囊")
    };
    {
        let mut pack = world.get_mut::<ecs_core::Container>(pack).expect("行囊是容器");
        pack.insert(0, mushroom, mushroom_def.tags)
            .expect("行囊第 1 格空着");
    }

    let mut pages = PageStack::new();
    pages.open_inventory();
    // Nav1 聚焦时 ←/→ 在「身体」「行囊」间切页签；行囊内容进 Context1。
    pages.apply(UiIntent::MoveInventoryCursor { dx: 1, dy: 0 });
    let view = inventory_view(&world, &pages);
    assert_eq!(view.nav1_selected, 1);
    let detail = view.detail.expect("选中行必须有详情");
    assert_eq!(detail.title, mushroom_def.name);
    let lines: Vec<String> = detail.lines.iter().map(|line| line.text()).collect();
    assert!(
        lines.iter().any(|line| line.starts_with("使用：")),
        "消耗品要写清效果：{lines:?}"
    );
    assert!(
        lines.iter().any(|line| line == "行囊第 1 格"),
        "行囊行要标出格子：{lines:?}"
    );
    assert!(
        detail.footer.contains("r 使用"),
        "消耗品的动作提示：{}",
        detail.footer
    );
    assert!(
        !detail.footer.contains("Enter 装上"),
        "消耗品不能装到部位上：{}",
        detail.footer
    );
    assert!(
        lines.iter().any(|line| line == "重量：0.1"),
        "蘑菇每单位 0.1：{lines:?}"
    );
}

/// HUD 要显示行囊载重（DsnE20）：否则玩家不知道"为什么捡不起来"。
#[test]
fn the_hud_shows_the_pack_load() {
    let mut world = new_game(2026);
    let config = ExtractConfig::default().with_viewport(40, 20);
    let frame = extract_scene_frame(&mut world, &config, &PageStack::new(), 1);
    let lines: Vec<String> = frame
        .hud
        .extra_lines
        .iter()
        .map(|line| line.text())
        .collect();
    assert!(
        lines.iter().any(|line| line.starts_with("行囊载重：")),
        "HUD 必须有行囊载重：{lines:?}"
    );
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
    // 楼梯与地面物品的可见性口径是「**探索过**就画」（它们是导航/拾取目标，不是威胁），
    // 所以这里只对**怪物**断言"视野外不得可见"——这才是本测试要守的东西。
    assert!(
        frame.entities.iter().all(|entity| {
            !entity.visible
                || entity.visual == VisualKey::Stairs
                || matches!(entity.visual, VisualKey::Item(_))
        }),
        "视野清空后除楼梯与地面物品外不得有可见实体"
    );
    assert!(
        frame
            .entities
            .iter()
            .filter(|entity| matches!(entity.visual, VisualKey::Monster(_)))
            .all(|entity| !entity.visible),
        "怪物必须在视野外不可见（防止隔着墙看到怪物）"
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

/// 事件栏：已落定事实按时间正序保留最近 N 条。
#[test]
fn events_keep_the_most_recent_facts_in_chronological_order() {
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

    let texts: Vec<&str> = frame.events.iter().map(|line| line.text.as_str()).collect();
    assert_eq!(texts, vec!["第二条", "第三条"], "取最近 2 条且保持正序");
    assert_eq!(frame.events[0].level, render_api::LogLevel::Danger);
    assert_eq!(frame.events[1].level, render_api::LogLevel::System);
    assert!(frame.events.iter().all(|line| !line.running));
}

/// 事件栏滚动偏移属于 UI 状态：从页栈提取到帧里，后端按它裁窗口（DsnA6）。
#[test]
fn the_events_scroll_offset_comes_from_the_page_stack() {
    let mut world = new_game(11);
    let mut pages = PageStack::new();
    let config = ExtractConfig::default();

    // 焦点默认在摄像机：滚事件意图不生效，帧里偏移是 0。
    assert!(!pages.apply(UiIntent::ScrollEvents { dy: 1 }));
    assert_eq!(
        extract_scene_frame(&mut world, &config, &pages, 1).events_scroll,
        0
    );

    // Tab 三次到事件栏，再 ↑ 两次：偏移跟着页栈走。
    for _ in 0..3 {
        pages.apply(UiIntent::CyclePaneFocus);
    }
    pages.apply(UiIntent::ScrollEvents { dy: 2 });
    assert_eq!(pages.events_scroll(), 2);
    assert_eq!(
        extract_scene_frame(&mut world, &config, &pages, 2).events_scroll,
        2
    );

    // 覆盖页打开时滚事件不生效，偏移保持。
    pages.apply(UiIntent::OpenInventory);
    assert!(!pages.apply(UiIntent::ScrollEvents { dy: 1 }));
    assert_eq!(
        extract_scene_frame(&mut world, &config, &pages, 3).events_scroll,
        2
    );
}

/// 进行中的行动排在已落定事实之后，并按剩余时间降序（栏底即 0 刻度）。
#[test]
fn running_actions_are_listed_after_facts_with_remaining_time() {
    let mut world = new_game(11);
    let pages = PageStack::new();
    let player = ecs_core::player_entity(&world).expect("新游戏必须有玩家");
    let player_pos = world
        .get::<ecs_core::Position>(player)
        .expect("玩家有位置")
        .to_tuple();

    // 把一只怪物挪到玩家视野内，再手动挂两个"已起手、还没落"的行动。
    let monster = {
        let mut query = world.query_filtered::<Entity, With<ecs_core::Monster>>();
        query
            .iter(&world)
            .next()
            .expect("种子地图上应当有怪物")
    };
    world
        .entity_mut(monster)
        .insert(ecs_core::Position::new(player_pos.0, player_pos.1));
    world.spawn((
        ecs_core::ActionOf(monster),
        ecs_core::ActionName("BasicAttack"),
        ecs_core::ActionTimer {
            remaining_av: 200.0,
        },
        ecs_core::ActiveAction,
    ));
    world.spawn((
        ecs_core::ActionOf(monster),
        ecs_core::ActionName("Move"),
        ecs_core::ActionTimer {
            remaining_av: 50.0,
        },
        ecs_core::ActiveAction,
    ));
    world
        .resource_mut::<ecs_core::EventLog>()
        .push(ecs_core::EventMessage::combat("你击中了老鼠"));

    let config = ExtractConfig::default().with_log_lines(10);
    let frame = extract_scene_frame(&mut world, &config, &pages, 1);

    assert_eq!(frame.events[0].text, "你击中了老鼠");
    assert!(!frame.events[0].running, "落定事实不能标成进行中");

    let running: Vec<&render_api::EventLine> =
        frame.events.iter().filter(|line| line.running).collect();
    assert_eq!(running.len(), 2, "两个进行中行动各占一行");
    assert!(
        running[0].text.contains("200AV"),
        "剩余时间多的排在前面：{:?}",
        running[0].text
    );
    assert!(
        running[1].text.contains("50AV"),
        "剩余时间少的排在后面：{:?}",
        running[1].text
    );
    assert!(
        running[0].text.contains("举起了武器"),
        "行动标识要换成可读文案：{:?}",
        running[0].text
    );
}

/// 信息栏只列视野内、带 `Observable` 的实体，按"近→远；同距按方位顺时针（正上=0）"排序。
#[test]
fn the_info_pane_lists_visible_observable_objects_near_to_far() {
    let mut world = new_game(11);
    let player = ecs_core::player_entity(&world).expect("新游戏必须有玩家");
    let (px, py) = world
        .get::<ecs_core::Position>(player)
        .expect("玩家有位置")
        .to_tuple();

    let up = (px, py - 1);
    let right = (px + 1, py);
    let down = (px, py + 1);
    let far = (px + 2, py);
    let plain = (px.saturating_sub(1), py);

    let spawn_observable = |world: &mut World, at: (usize, usize)| {
        world
            .spawn((
                ecs_core::Position::new(at.0, at.1),
                ecs_core::EntityName(format!("{at:?}")),
                ecs_core::Observable,
            ))
            .id()
    };
    let up_id = spawn_observable(&mut world, up);
    let right_id = spawn_observable(&mut world, right);
    let down_id = spawn_observable(&mut world, down);
    let far_id = spawn_observable(&mut world, far);
    // 视野内但没有标记的实体不进信息栏。
    world.spawn((
        ecs_core::Position::new(plain.0, plain.1),
        ecs_core::EntityName("背景".into()),
    ));

    // 直接写视野集合，避免地图墙体让用例依赖种子布局。
    world
        .get_mut::<ecs_core::Viewshed>(player)
        .expect("玩家有视野")
        .visible_tiles = vec![up, right, down, plain]
            .into_iter()
            .chain([far])
            .collect();

    let mut pages = PageStack::new();
    pages.cycle_pane_focus();
    assert_eq!(
        extract_scene_frame(&mut world, &ExtractConfig::default(), &pages, 1)
            .info
            .entries,
        vec![
            render_api::EntityId::from_bits(up_id.to_bits()),
            render_api::EntityId::from_bits(right_id.to_bits()),
            render_api::EntityId::from_bits(down_id.to_bits()),
            render_api::EntityId::from_bits(far_id.to_bits()),
        ],
        "近的先排；同距从正上方起顺时针"
    );

    // 光标越界夹到最后一行。
    pages.apply(crate::ui::UiIntent::MoveInfoCursor { dy: 99 });
    let frame = extract_scene_frame(&mut world, &ExtractConfig::default(), &pages, 2);
    assert_eq!(frame.info.selected, Some(3), "光标应夹在最后一行");
    assert_eq!(
        frame.info.selected_id(),
        Some(render_api::EntityId::from_bits(far_id.to_bits()))
    );
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

/// **C2：泥潭必须看得见。**
///
/// 减速如果不可见，就是一次无声的惩罚——玩家会以为"我按了键但没动"。
/// 这条断言把"领域数据（`CellMoveCost`）→ 语义 key"这条链路钉住：
/// 只要 `overlay_cell_effects` 不再覆盖地形，它立刻失败。
#[test]
fn mud_cells_are_drawn_as_an_effect_overlay() {
    let mut world = new_game(2026);
    // 格索引里同时有泥潭与治疗泉，这里只取泥潭（`CellMoveCost`）。
    let mud: Vec<(usize, usize)> = {
        let mut query =
            world.query_filtered::<&ecs_core::CellEffectAt, With<ecs_core::CellMoveCost>>();
        query.iter(&world).map(|at| at.to_tuple()).collect()
    };
    let springs: Vec<(usize, usize)> = {
        let mut query =
            world.query_filtered::<&ecs_core::CellEffectAt, With<ecs_core::CellHeal>>();
        query.iter(&world).map(|at| at.to_tuple()).collect()
    };
    assert!(!mud.is_empty(), "F1 应当有泥潭（内容没上线，这条测试就没意义）");
    assert!(!springs.is_empty(), "F1 应当有治疗泉");

    let config = ExtractConfig::default().with_viewport(40, 20);
    let frame = extract_scene_frame(&mut world, &config, &PageStack::new(), 1);

    for (x, y) in &mud {
        assert_eq!(
            frame.map.tile(*x, *y),
            Some(VisualKey::Effect(crate::catalog::EFFECT_MUD)),
            "({x},{y}) 的泥潭没有画出来：玩家看不到减速，等于隐形惩罚"
        );
    }

    // 治疗泉是另一种效果，必须有自己的 key——否则玩家会把回血点当减速泥潭。
    for (x, y) in &springs {
        assert_eq!(
            frame.map.tile(*x, *y),
            Some(VisualKey::Effect(crate::catalog::EFFECT_SPRING)),
            "({x},{y}) 的治疗泉没有画出来：玩家看不到回复点"
        );
    }

    // 对照：没有效果的格子仍然是地形外观（别把整张图都盖掉）。
    let plain = (0..frame.map.len()).find(|idx| {
        let (x, y) = (idx % frame.map.width, idx / frame.map.width);
        !mud.contains(&(x, y)) && frame.map.tile(x, y).is_some()
    });
    let idx = plain.expect("地图上必须有普通格");
    let (px, py) = (idx % frame.map.width, idx / frame.map.width);
    assert!(
        !matches!(frame.map.tile(px, py), Some(VisualKey::Effect(_))),
        "没有效果的格子不得被画成效果"
    );
}

/// 投掷瞄准页：视图带光标、弹道与可投性；可达性由核心的 `throw_reachable` 决定。
#[test]
fn the_throw_aim_page_reports_the_path_and_reachability() {
    let mut world = new_game(2026);
    let player = player_position(&mut world).expect("新局要有玩家");

    // 找一个可走的相邻格作为"可达落点"；找不到就跳过（地图生成可能全被围住）。
    let reachable = {
        let map = world.resource::<ecs_core::Map>();
        [(1_isize, 0_isize), (0, 1), (-1, 0), (0, -1)]
            .into_iter()
            .filter_map(|(dx, dy)| {
                let x = player.0 as isize + dx;
                let y = player.1 as isize + dy;
                if x < 0 || y < 0 {
                    return None;
                }
                let cell = (x as usize, y as usize);
                map.tiles[cell.1][cell.0]
                    .walkable()
                    .then_some(cell)
            })
            .find(|cell| ecs_core::throw_reachable(map, player, *cell))
    };

    let mut pages = PageStack::new();
    let target = reachable.unwrap_or(player);
    pages.open_throw(target.0, target.1);
    let config = ExtractConfig::default().with_viewport(40, 20);
    let frame = extract_scene_frame(&mut world, &config, &pages, 1);

    let UiView::ThrowAim(aim) = &frame.ui else {
        panic!("瞄准页应当产出 ThrowAim 视图，实际：{:?}", frame.ui);
    };
    assert_eq!(aim.cursor, (target.0 as i32, target.1 as i32));
    assert_eq!(
        aim.path.len(),
        ecs_core::throw_path(player, target).len(),
        "弹道格数要与核心规则一致"
    );
    assert!(aim.valid, "可达落点应当标记为可投");
    assert!(aim.footer.contains("Enter"), "底栏要有确认提示");

    // 远到一定不可达的落点：视图必须明确标记不可投。
    let far = ((player.0 + 20).min(ecs_core::MAP_WIDTH - 1), player.1);
    pages.open_throw(far.0, far.1);
    let frame = extract_scene_frame(&mut world, &config, &pages, 2);
    let UiView::ThrowAim(aim) = &frame.ui else {
        panic!("瞄准页应当产出 ThrowAim 视图");
    };
    assert!(!aim.valid, "超出射程/视线的落点不得标记可投");
}
