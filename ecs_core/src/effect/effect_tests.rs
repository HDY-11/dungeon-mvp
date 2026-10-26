//! `effect` 的验收测试：**收桶 + 最小闭环一对**（H9 / H11）。
//!
//! 这里测的不是"某个效果算得对不对"，而是机制是否成立：
//! 同一条规则函数、同一个 actor、同一张地图，**格子侧多一条修正 + actor 侧声明无视**
//! 之后 AV 的变化方向与幅度是否符合设计（DsnE10 的五步、DsnE12 的三个落点）。

use super::*;
use crate::action::generation::PlayerCommand;
use crate::action::entity::{PlayerActionRequest};
use crate::balance::UNARMED_ATTACK_DURATION;
use crate::components::{ActionTimer, Position};
use crate::map::Tile;
use crate::schedule::PlayerMountSchedule;
use crate::test_util::{fill_map, spawn_test_player, test_world};

/// 平坦地面上的最小场景：玩家站在 `(10, 10)`，四周可走，没有行动、没有效果。
fn player_scene() -> (World, Entity) {
    let mut world = test_world();
    fill_map(&mut world, Tile::Floor);
    let player = spawn_test_player(&mut world, (10, 10));
    (world, player)
}

/// 让玩家发一次"向右走"，跑一次挂载调度，返回该行动的 AV。
///
/// 跑 `PlayerMountSchedule`（生成 + 仲裁）而不是整个行动链路：AV 在**生成期**固化，
/// 这条测试关心的正是那一刻读了什么。
fn move_av(world: &mut World) -> f64 {
    world
        .resource_mut::<PlayerActionRequest>()
        .command
        .replace(PlayerCommand::Move { dx: 1, dy: 0 });
    world.run_schedule(PlayerMountSchedule);

    let mut timers = world.query::<&ActionTimer>();
    let avs: Vec<f64> = timers.iter(world).map(|timer| timer.remaining_av).collect();
    assert_eq!(avs.len(), 1, "这一次挂载必须恰好产出一个行动");
    avs[0]
}

/// 清掉玩家上一次的行动，让下一次挂载能通过 `Without<Active>`。
fn clear_action(world: &mut World) {
    let mut actions = world.query_filtered::<Entity, With<crate::action::entity::ActiveAction>>();
    let entities: Vec<Entity> = actions.iter(world).collect();
    for action in entities {
        world.despawn(action);
    }
    if let Some(player) = crate::test_util::player_entity(world) {
        world.entity_mut(player).remove::<crate::components::Active>();
    }
}

/// 基线：没有效果时，AV 就是"基准耗时 / 速度"，与接入收桶之前逐值相等。
#[test]
fn no_effects_means_legacy_av() {
    let (mut world, _player) = player_scene();
    let av = move_av(&mut world);
    assert_eq!(
        av, UNARMED_ATTACK_DURATION,
        "速度 1.0 的玩家，无任何效果时 AV 必须等于基准耗时"
    );
}

/// **最小闭环（一）**：站在减速格上，移动 AV 按代价倍率变长。
#[test]
fn cell_move_cost_lengthens_the_move_av() {
    let (mut world, _player) = player_scene();
    let baseline = move_av(&mut world);
    clear_action(&mut world);

    // 玩家脚下一格放一个"两倍代价"的效果实体。
    world.spawn((CellEffectAt::new(10, 10), CellMoveCost::new(2.0)));
    // 格索引是每轮重建的派生数据，这里直接跑一次结算调度让它生效。
    world.run_schedule(crate::schedule::CoreSettleSchedule);
    assert!(
        world.resource::<CellEffects>().entity_at(10, 10).is_some(),
        "格索引没登记上，后面的断言就失去意义"
    );

    let slowed = move_av(&mut world);
    assert_eq!(
        slowed,
        baseline * 2.0,
        "两倍代价 → 速度减半 → AV 翻倍（baseline={baseline}, slowed={slowed}）"
    );
}

/// **最小闭环（二）**：actor 声明"无视地形"后，同一格上的减速被完全丢掉。
///
/// 关键在于**规则层一行没改**：`rules::modifier::apply_modifiers` 收到的是
/// `ignored: &[EffectSource::Terrain]`，它只做一次 `contains` 过滤。
#[test]
fn ignoring_the_terrain_bucket_cancels_the_cell_cost() {
    let (mut world, player) = player_scene();
    let baseline = move_av(&mut world);
    clear_action(&mut world);

    world.spawn((CellEffectAt::new(10, 10), CellMoveCost::new(2.0)));
    // 脚上那双"踏水靴"：不是修正，而是一条"无视 Terrain 桶"的效果。
    world.spawn((EffectOf(player), IgnoresSource::TERRAIN));
    world.run_schedule(crate::schedule::CoreSettleSchedule);

    let with_boots = move_av(&mut world);
    assert_eq!(
        with_boots, baseline,
        "无视地形之后 AV 必须回到平地值（格子修正被丢桶过滤掉了）"
    );
}

/// 无视是**按桶**生效的：无视状态效果不会连带无视地形。
///
/// 这条防的是"把 ignored 实现成一个布尔"这类退化——一旦退化成布尔，
/// 上一条测试照样全绿，但"只无视其中一类"就再也表达不出来。
#[test]
fn ignoring_one_bucket_leaves_the_other_untouched() {
    let (mut world, player) = player_scene();
    let baseline = move_av(&mut world);
    clear_action(&mut world);

    world.spawn((CellEffectAt::new(10, 10), CellMoveCost::new(2.0)));
    world.spawn((EffectOf(player), IgnoresSource(EffectSource::Status)));
    world.run_schedule(crate::schedule::CoreSettleSchedule);

    let av = move_av(&mut world);
    assert_eq!(
        av,
        baseline * 2.0,
        "无视 Status 不该减免 Terrain 的减速"
    );
}

/// 无效果实体 / 无效果实体但索引有残留时，`collect` 必须是干净的。
#[test]
fn collect_is_empty_without_effects() {
    let (mut world, player) = player_scene();
    let position = world
        .get::<Position>(player)
        .expect("玩家必须有 Position")
        .to_tuple();
    let mut system = bevy_ecs::system::SystemState::<EffectContext>::new(&mut world);
    let context = system.get_mut(&mut world);
    let collected = context.collect(player, position);
    assert!(
        collected.is_empty(),
        "没有任何效果时收桶结果必须为空（空桶 = 旧口径），实际 {collected:?}"
    );
}

/// 收桶把"脚下这一格"与"actor 名下效果"两个来源合到一起，且忽略表按去重收集。
#[test]
fn collect_reads_cell_and_actor_sides_separately() {
    let (mut world, player) = player_scene();
    world.spawn((CellEffectAt::new(10, 10), CellMoveCost::new(4.0)));
    world.spawn((EffectOf(player), IgnoresSource::TERRAIN));
    // 同一条效果重复声明的来源只收一次（去重）。
    world.spawn((EffectOf(player), IgnoresSource::TERRAIN));
    world.run_schedule(crate::schedule::CoreSettleSchedule);

    let position = world
        .get::<Position>(player)
        .expect("玩家必须有 Position")
        .to_tuple();
    let mut system = bevy_ecs::system::SystemState::<EffectContext>::new(&mut world);
    let collected = system.get_mut(&mut world).collect(player, position);

    assert_eq!(collected.cell.len(), 1, "脚下这一格贡献一条修正");
    assert_eq!(
        collected.cell[0].source,
        EffectSource::Terrain,
        "格子修正必须属于 Terrain 桶"
    );
    assert_eq!(collected.cell[0].multiplier, 0.25);
    assert_eq!(
        collected.ignored,
        vec![EffectSource::Terrain],
        "重复声明的无视来源必须去重"
    );
    assert!(collected.actor.is_empty(), "本轮没有 actor 侧修正效果");
}

/// 越界格子按"不可走"读，而不是 panic 或返回可走。
#[test]
fn out_of_bounds_cell_is_never_walkable() {
    let (mut world, _player) = player_scene();
    let mut system = bevy_ecs::system::SystemState::<EffectContext>::new(&mut world);
    let context = system.get_mut(&mut world);
    let attributes = context.cell_attributes(crate::map::MAP_WIDTH, 0);
    assert_eq!(attributes, CellAttributes::OUT_OF_BOUNDS);
    assert!(!attributes.walkable);
}
