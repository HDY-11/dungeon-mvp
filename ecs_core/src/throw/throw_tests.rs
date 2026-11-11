//! 投掷规则与结算的测试（Gm9 / DsnE28）。
//!
//! 覆盖三件事：
//! - 可达性只由「射程 + 视线 + 界内」决定（与瞄准界面共用同一函数）；
//! - 落点必中：格上有人就打人，没人就落地；
//! - 每次投掷只从副手消耗一颗，堆空后移除。

use super::*;
use crate::item::{BODY, GroundItemAt, ItemId};
use crate::map::Tile;
use crate::monster::MonsterKindId;
use crate::test_util::{fill_map, spawn_test_monster, spawn_test_player, test_world};
use bevy_ecs::system::RunSystemOnce;

fn scene() -> (bevy_ecs::world::World, Entity) {
    let mut world = test_world();
    fill_map(&mut world, Tile::Floor);
    let player = spawn_test_player(&mut world, (10, 10));
    (world, player)
}

/// 给玩家装一个身体容器，副手放一堆石子（id 18，`Throw.damage = 3`）。
fn give_off_hand_stones(world: &mut bevy_ecs::world::World, player: Entity, amount: u32) -> Entity {
    let stone = world
        .spawn((
            Item::new(ItemId(18)),
            Amount::new(amount),
            Throw { damage: 3.0 },
        ))
        .id();
    let tags = ItemId(18).def().tags;
    let mut body = Container::new(&BODY);
    body.insert(off_hand_slot_index(), stone, tags)
        .expect("副手槽必须接受可投掷物");
    world.entity_mut(player).insert(body);
    stone
}

fn ground_items(world: &mut bevy_ecs::world::World, at: (usize, usize)) -> Vec<ItemId> {
    let mut query = world.query::<(&Item, &GroundItemAt)>();
    query
        .iter(world)
        .filter(|(_, ground)| ground.to_tuple() == at)
        .map(|(item, _): (&Item, &GroundItemAt)| item.id)
        .collect()
}

fn send_throw(world: &mut bevy_ecs::world::World, thrower: Entity, target: (usize, usize)) {
    world
        .resource_mut::<bevy_ecs::message::Messages<ThrowIntentEvent>>()
        .write(ThrowIntentEvent { thrower, target });
}

#[test]
fn throw_reachability_is_range_plus_line_of_sight() {
    let mut world = test_world();
    fill_map(&mut world, Tile::Floor);
    let map = world.resource::<Map>();
    let from = (10, 10);
    assert!(throw_reachable(map, from, (10, 15)), "正射程内可达");
    assert!(!throw_reachable(map, from, (10, 16)), "超出射程不可达");
    assert!(
        !throw_reachable(map, from, (MAP_WIDTH, 10)),
        "界外格子不可达"
    );

    // 中间挡一堵墙：路径上除终点外任何阻挡都会断视线。
    world.resource_mut::<Map>().tiles[10][11] = Tile::Wall;
    let map = world.resource::<Map>();
    assert!(!throw_reachable(map, from, (12, 10)), "视线被墙阻断");
    assert!(
        throw_reachable(map, from, (11, 10)),
        "砸在墙面上仍然可达（终点格本身豁免）"
    );

    // 弹道不含起点、含终点。
    let path = throw_path(from, (13, 10));
    assert_eq!(path.first(), Some(&(11, 10)));
    assert_eq!(path.last(), Some(&(13, 10)));
    assert!(!path.contains(&from));
}

#[test]
fn throwing_at_a_monster_hurts_it_and_drops_the_stone() {
    let (mut world, player) = scene();
    let monster = spawn_test_monster(&mut world, MonsterKindId::Rat, (11, 10), 30.0, 4.0);
    give_off_hand_stones(&mut world, player, 1);
    send_throw(&mut world, player, (11, 10));
    world.run_system_once(resolve_throw_system).expect("system should run");
    world.run_system_once(consume_thrown_item_system).expect("system should run");

    // 基值 3，怪物防御 0 → 护甲域 1，无暴击/增伤/抗性。
    let health = *world.get::<Health>(monster).expect("怪物还活着");
    assert_eq!(health.current, 27.0);
    let log = &world.resource::<EventLog>().messages;
    let last = log.last().expect("投掷应写日志");
    assert!(last.text.contains("投出"), "{}", last.text);
    assert!(last.text.contains("石子"), "{}", last.text);

    // 落点必中并落地一颗；副手一颗已耗光。
    assert_eq!(ground_items(&mut world, (11, 10)), vec![ItemId(18)]);
    let body = world.get::<Container>(player).expect("玩家有身体容器");
    assert!(body.slot(off_hand_slot_index()).is_none(), "副手应当空了");
}

#[test]
fn a_throw_consumes_only_one_stone_from_the_off_hand_stack() {
    let (mut world, player) = scene();
    let monster = spawn_test_monster(&mut world, MonsterKindId::Rat, (11, 10), 30.0, 4.0);
    let stone = give_off_hand_stones(&mut world, player, 3);
    send_throw(&mut world, player, (11, 10));
    world.run_system_once(resolve_throw_system).expect("system should run");
    world.run_system_once(consume_thrown_item_system).expect("system should run");

    assert_eq!(world.get::<Amount>(stone).unwrap().0, 2);
    assert_eq!(
        world.get::<Container>(player).unwrap().slot(off_hand_slot_index()),
        Some(stone)
    );
    assert_eq!(*world.get::<Health>(monster).expect("怪物还活着"), Health::new(30.0).damage(3.0));
}

#[test]
fn throwing_at_an_empty_cell_just_lands_there() {
    let (mut world, player) = scene();
    give_off_hand_stones(&mut world, player, 1);
    send_throw(&mut world, player, (13, 10));
    world.run_system_once(resolve_throw_system).expect("system should run");
    world.run_system_once(consume_thrown_item_system).expect("system should run");

    assert_eq!(ground_items(&mut world, (13, 10)), vec![ItemId(18)]);
    let body = world.get::<Container>(player).expect("玩家有身体容器");
    assert!(body.slot(off_hand_slot_index()).is_none(), "空投也要消耗");
}

#[test]
fn a_throw_without_an_off_hand_item_is_a_no_op() {
    let (mut world, player) = scene();
    let body = Container::new(&BODY);
    world.entity_mut(player).insert(body);
    let monster = spawn_test_monster(&mut world, MonsterKindId::Rat, (11, 10), 30.0, 4.0);
    send_throw(&mut world, player, (11, 10));
    world.run_system_once(resolve_throw_system).expect("system should run");
    world.run_system_once(consume_thrown_item_system).expect("system should run");

    assert_eq!(world.get::<Health>(monster).unwrap().current, 30.0);
    assert!(ground_items(&mut world, (11, 10)).is_empty());
    let log = &world.resource::<EventLog>().messages;
    assert!(log.is_empty(), "没有可投物就不该写伤害日志");
}
