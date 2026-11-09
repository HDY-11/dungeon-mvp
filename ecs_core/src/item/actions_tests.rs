//! 背包动作的测试（`item/actions.rs`）。
//!
//! 这一层的失败模式几乎都是"东西不见了"或"东西变两份了"，所以每条用例都从
//! **物品总数**与**两个容器各自的内容**两头断言，而不是只看返回值。

use super::*;
use crate::item::{Amount, BODY, Container, Item, ItemTag, TRAVELER_PACK, TRAVELER_PACK_ITEM, pack_slot_index};
use crate::map::Tile;
use crate::test_util::{fill_map, single_tile_scene};

/// 平地 + 玩家（带身体与行囊），与生产初始化同形。
fn world_with_player() -> (World, Entity) {
    let (mut world, player) = single_tile_scene();
    fill_map(&mut world, Tile::Floor);
    let pack = world
        .spawn((
            Item::new(TRAVELER_PACK_ITEM),
            Amount::new(1),
            Container::new(&TRAVELER_PACK),
        ))
        .id();
    let mut body = Container::new(&BODY);
    body.insert(pack_slot_index(), pack, &[ItemTag::Pack])
        .expect("身体必须有背包槽");
    world.entity_mut(player).insert(body);
    crate::system::run_settle_systems(&mut world);
    (world, player)
}

fn pack_of(world: &World, player: Entity) -> Entity {
    world
        .get::<Container>(player)
        .and_then(|body| body.slot(pack_slot_index()))
        .expect("玩家身上有行囊")
}

/// 往行囊里塞一件（走唯一装配桥，组件与生产一致）。
fn put_in_pack(world: &mut World, player: Entity, id: ItemId) -> (usize, Entity) {
    let pack = pack_of(world, player);
    let item = spawn_item_assembled(world, id, 1);
    let mut container = world.get_mut::<Container>(pack).expect("行囊");
    let slot = container.free_slot_for(&[]).expect("空格");
    container.insert(slot, item, &[]).expect("空格");
    (slot, item)
}

fn main_hand() -> usize {
    BODY
        .slots
        .iter()
        .position(|slot| slot.accept.accepts(&[ItemTag::Weapon]))
        .expect("身体有主手槽")
}

/// 从行囊穿上：物品从行囊消失、出现在主手槽。
#[test]
fn equipping_moves_the_item_from_pack_to_body() {
    let (mut world, player) = world_with_player();
    let (slot, sword) = put_in_pack(&mut world, player, ItemId(0));

    equip_from_pack(&mut world, slot).expect("可以穿");

    assert_eq!(
        world.get::<Container>(player).and_then(|body| body.slot(main_hand())),
        Some(sword)
    );
    let pack = pack_of(&world, player);
    assert!(!world.get::<Container>(pack).is_some_and(|pack| pack.contains(sword)));
}

/// 部位已占 → **交换**：新的穿上，旧的回到行囊（不是被丢掉）。
#[test]
fn equipping_into_an_occupied_slot_swaps() {
    let (mut world, player) = world_with_player();
    let (first_slot, first) = put_in_pack(&mut world, player, ItemId(0));
    equip_from_pack(&mut world, first_slot).expect("先穿一把");
    let (second_slot, second) = put_in_pack(&mut world, player, ItemId(24));

    equip_from_pack(&mut world, second_slot).expect("换一把");

    let body = world.get::<Container>(player).expect("身体");
    assert_eq!(body.slot(main_hand()), Some(second), "新剑穿上");
    let pack = pack_of(&world, player);
    assert!(
        world.get::<Container>(pack).is_some_and(|pack| pack.contains(first)),
        "旧剑必须回到行囊"
    );
    // 两件都还在世界上：总量没有变化。
    assert!(world.get_entity(first).is_ok());
    assert!(world.get_entity(second).is_ok());
}

/// 卸下：从身体回到行囊；行囊满则**放回去**并报错。
#[test]
fn unequipping_returns_to_pack_and_refuses_when_full() {
    let (mut world, player) = world_with_player();
    let (slot, sword) = put_in_pack(&mut world, player, ItemId(0));
    equip_from_pack(&mut world, slot).expect("穿上");

    // 先把行囊填满（用不可堆叠的剑；换成轻的填充物，避免先撞载重上限）。
    let pack = pack_of(&world, player);
    {
        let fillers: Vec<Entity> = (0..TRAVELER_PACK.len())
            .map(|_| spawn_item_assembled(&mut world, ItemId(29), 1))
            .collect();
        let mut container = world.get_mut::<Container>(pack).expect("行囊");
        for (index, filler) in fillers.into_iter().enumerate() {
            let _ = container.insert(index, filler, &[]);
        }
    }

    assert_eq!(
        unequip_to_pack(&mut world, main_hand()),
        Err(ItemActionError::NoRoom)
    );
    assert_eq!(
        world.get::<Container>(player).and_then(|body| body.slot(main_hand())),
        Some(sword),
        "失败时装备必须还在身上"
    );
}

/// 丢弃：物品落到玩家脚下，并带着 `GroundItemAt`。
#[test]
fn dropping_puts_the_item_on_the_ground() {
    let (mut world, player) = world_with_player();
    let (slot, item) = put_in_pack(&mut world, player, ItemId(10));

    drop_from_pack(&mut world, slot).expect("可以丢");

    let player_pos = world.get::<Position>(player).unwrap().to_tuple();
    assert_eq!(
        world.get::<GroundItemAt>(item).map(|at| at.to_tuple()),
        Some(player_pos),
        "丢下的东西必须落在玩家脚下"
    );
    let pack = pack_of(&world, player);
    assert!(!world.get::<Container>(pack).is_some_and(|pack| pack.contains(item)));
}

/// 使用：消耗品用掉 1 个；数量归零时实体被销毁。
#[test]
fn using_a_consumable_spends_one_and_despawns_at_zero() {
    let (mut world, player) = world_with_player();
    let max = world.get::<Health>(player).unwrap().max;
    world.get_mut::<Health>(player).unwrap().current = max - 10.0;

    let pack = pack_of(&world, player);
    let mushroom = spawn_item_assembled(&mut world, ItemId(25), 2);
    {
        let mut container = world.get_mut::<Container>(pack).expect("行囊");
        let slot = container.free_slot_for(&[]).expect("空格");
        container.insert(slot, mushroom, &[]).expect("空格");
    }

    use_from_pack(&mut world, 0).expect("吃一个");
    assert_eq!(
        world.get::<Amount>(mushroom).map(|amount| amount.0),
        Some(1),
        "吃掉一个，还剩一个"
    );
    assert!(world.get::<Health>(player).unwrap().current > max - 10.0);

    use_from_pack(&mut world, 0).expect("再吃一个");
    assert!(world.get_entity(mushroom).is_err(), "吃完就该销毁");
}

/// 非装备不能穿、非消耗品不能用、空槽位报 EmptySlot——每条都给一句话。
#[test]
fn wrong_targets_report_clear_errors() {
    let (mut world, player) = world_with_player();
    let (slot, _) = put_in_pack(&mut world, player, ItemId(10)); // 材料

    assert_eq!(
        equip_from_pack(&mut world, slot),
        Err(ItemActionError::NotEquippable)
    );
    assert_eq!(
        use_from_pack(&mut world, slot),
        Err(ItemActionError::NotUsable)
    );
    assert_eq!(
        drop_from_pack(&mut world, slot + 5),
        Err(ItemActionError::EmptySlot)
    );
    // 行囊那一格本身不能卸下（会把自己塞进自己）。
    assert_eq!(
        unequip_to_pack(&mut world, pack_slot_index()),
        Err(ItemActionError::CannotNestPack)
    );
}

// ── 尸体显式搜刮（玩家反馈后从"踩上自动全拾取"改成背包页动作） ──

/// 在玩家脚下放一具装着 `id` 的尸体，返回（尸体，物品）。
fn corpse_underfoot_with(world: &mut World, player: Entity, id: ItemId) -> (Entity, Entity) {
    let at = world.get::<Position>(player).expect("玩家有位置").to_tuple();
    let item = spawn_item_assembled(world, id, 1);
    let mut container = Container::new(&crate::item::CORPSE);
    container.insert(0, item, &[]).expect("尸体格子无过滤");
    let corpse = world
        .spawn((
            crate::monster::Corpse::new(crate::monster::MonsterKindId::Goblin, "哥布林"),
            Position::new(at.0, at.1),
            container,
        ))
        .id();
    (corpse, item)
}

/// 显式拿取：尸体那一格清空、物品出现在行囊，实体不重建（同一个 Entity）。
#[test]
fn taking_from_a_corpse_moves_the_item_into_the_pack() {
    let (mut world, player) = world_with_player();
    let (corpse, dagger) = corpse_underfoot_with(&mut world, player, ItemId(24));

    let taken = take_from_corpse(&mut world, 0, 0).expect("可以拿");

    assert_eq!(taken, ItemId(24));
    assert_eq!(
        world.get::<Container>(corpse).expect("尸体").used(),
        0,
        "拿走之后尸体该格是空的"
    );
    let pack = pack_of(&world, player);
    assert!(
        world
            .get::<Container>(pack)
            .is_some_and(|pack| pack.contains(dagger)),
        "匕首必须进同一个行囊实体（不是复制一份）"
    );
    assert!(world.get_entity(dagger).is_ok(), "物品实体不该被销毁重建");
    assert!(
        world.get::<crate::monster::Corpse>(corpse).is_some(),
        "尸体本身留在原地"
    );
}

/// 脚下没尸体 → `NoCorpse`；尸体空格 → `EmptySlot`。
#[test]
fn taking_from_a_corpse_reports_missing_corpse_and_empty_slot() {
    let (mut world, player) = world_with_player();
    assert_eq!(
        take_from_corpse(&mut world, 0, 0),
        Err(ItemActionError::NoCorpse)
    );
    let _ = corpse_underfoot_with(&mut world, player, ItemId(24));
    assert_eq!(
        take_from_corpse(&mut world, 0, 3),
        Err(ItemActionError::EmptySlot),
        "尸体只有 4 格，第 4 格（下标 3）是空的"
    );
}

/// 行囊满 → 拿不动，东西**必须还留在尸体上**（不能凭空消失）。
#[test]
fn a_full_pack_keeps_the_corpse_item_in_place() {
    let (mut world, player) = world_with_player();
    let (corpse, dagger) = corpse_underfoot_with(&mut world, player, ItemId(24));
    let pack = pack_of(&world, player);
    {
        let fillers: Vec<Entity> = (0..TRAVELER_PACK.len())
            .map(|_| spawn_item_assembled(&mut world, ItemId(29), 1))
            .collect();
        let mut container = world.get_mut::<Container>(pack).expect("行囊");
        for (index, filler) in fillers.into_iter().enumerate() {
            let _ = container.insert(index, filler, &[]);
        }
    }

    assert_eq!(
        take_from_corpse(&mut world, 0, 0),
        Err(ItemActionError::NoRoom)
    );
    assert!(
        world
            .get::<Container>(corpse)
            .is_some_and(|container| container.slot(0) == Some(dagger)),
        "拿不动时匕首必须回到尸体原位"
    );
    assert!(world.get_entity(dagger).is_ok());
}

/// 同格多具尸体：下标按实体编号升序，拿第二具不会动第一具（DsnE21 决策 7）。
#[test]
fn multiple_corpses_underfoot_are_addressed_by_stable_index() {
    let (mut world, player) = world_with_player();
    let (first, dagger) = corpse_underfoot_with(&mut world, player, ItemId(24));
    let (second, hammer) = corpse_underfoot_with(&mut world, player, ItemId(23));

    assert_eq!(
        take_from_corpse(&mut world, 2, 0),
        Err(ItemActionError::NoCorpse),
        "没有第三具尸体"
    );
    assert_eq!(
        take_from_corpse(&mut world, 1, 0).expect("第二具尸体"),
        ItemId(23)
    );
    assert_eq!(
        world.get::<Container>(second).expect("第二具").used(),
        0,
        "被翻的那具清空"
    );
    assert_eq!(
        world.get::<Container>(first).expect("第一具").used(),
        1,
        "另一具尸体不该被动"
    );
    let pack = pack_of(&world, player);
    assert!(
        world
            .get::<Container>(pack)
            .is_some_and(|pack| pack.contains(hammer)),
        "石锤进同一个行囊实体"
    );
    assert!(
        !world
            .get::<Container>(pack)
            .expect("行囊")
            .contains(dagger),
        "没被选中的尸体物品不动"
    );
}

// ── 载重（DsnE20）：槽位空着也可能背不动 ──

/// 往行囊前 `count` 格塞锈铁剑（每把 3.0，共 18.0），留出空槽但接近上限 20.0。
fn fill_pack_with_swords(world: &mut World, pack: Entity, count: usize) -> Vec<Entity> {
    let swords: Vec<Entity> = (0..count)
        .map(|_| spawn_item_assembled(world, ItemId(0), 1))
        .collect();
    let mut container = world.get_mut::<Container>(pack).expect("行囊");
    for (index, sword) in swords.iter().copied().enumerate() {
        container.insert(index, sword, &[]).expect("空格");
    }
    swords
}

/// 卸下会让行囊超重 → `Overweight`，装备**不脱下来**（原子失败）。
#[test]
fn unequipping_that_would_overweight_the_pack_is_refused() {
    let (mut world, player) = world_with_player();
    let (slot, sword) = put_in_pack(&mut world, player, ItemId(0));
    equip_from_pack(&mut world, slot).expect("穿上");
    let pack = pack_of(&world, player);
    fill_pack_with_swords(&mut world, pack, 6); // 18.0，还剩空槽

    assert_eq!(
        unequip_to_pack(&mut world, main_hand()),
        Err(ItemActionError::Overweight)
    );
    assert_eq!(
        world
            .get::<Container>(player)
            .and_then(|body| body.slot(main_hand())),
        Some(sword),
        "背不动就继续穿着"
    );
    assert!(
        world
            .get::<Container>(pack)
            .is_some_and(|pack| pack.used() == 6),
        "行囊不该被改动"
    );
}

/// 尸体上的重物会让行囊超重 → `Overweight`，东西留在尸体上。
#[test]
fn taking_a_corpse_item_that_would_overweight_the_pack_is_refused() {
    let (mut world, player) = world_with_player();
    let (corpse, hammer) = corpse_underfoot_with(&mut world, player, ItemId(23));
    let pack = pack_of(&world, player);
    fill_pack_with_swords(&mut world, pack, 6); // 18.0；石锤 6.0 → 24.0 > 20.0

    assert_eq!(
        take_from_corpse(&mut world, 0, 0),
        Err(ItemActionError::Overweight)
    );
    assert!(
        world
            .get::<Container>(corpse)
            .is_some_and(|container| container.slot(0) == Some(hammer)),
        "背不动时东西必须留在尸体原位"
    );
}

// ── 地面显式拾取（DsnE21 阶段 4） ──

/// 在玩家脚下生成一件地面物品。
fn ground_underfoot_with(
    world: &mut World,
    player: Entity,
    id: ItemId,
    count: u32,
) -> Entity {
    let position = world
        .get::<Position>(player)
        .expect("玩家有位置")
        .to_tuple();
    let item = spawn_item_assembled(world, id, count);
    world
        .entity_mut(item)
        .insert(GroundItemAt::new(position.0, position.1));
    item
}

/// 地面材料：拿起来进行囊，实体被搬走、不再带地面位置。
#[test]
fn taking_from_the_ground_moves_the_item_into_the_pack() {
    let (mut world, player) = world_with_player();
    let flesh = ground_underfoot_with(&mut world, player, ItemId(10), 2);
    let pack = pack_of(&world, player);

    assert_eq!(take_from_ground(&mut world, 0), Ok(ItemId(10)));
    assert!(
        world
            .get::<Container>(pack)
            .is_some_and(|pack| pack.contains(flesh)),
        "拿起来的东西必须进行囊"
    );
    assert!(
        world.get::<GroundItemAt>(flesh).is_none(),
        "进了容器就不能还带着地面位置"
    );
}

/// 地面装备且对应部位空着 → 直接穿上（沿用旧拾取的路由）。
#[test]
fn taking_ground_equipment_with_a_free_slot_equips_it() {
    let (mut world, player) = world_with_player();
    let sword = ground_underfoot_with(&mut world, player, ItemId(0), 1);

    assert_eq!(take_from_ground(&mut world, 0), Ok(ItemId(0)));
    assert_eq!(
        world
            .get::<Container>(player)
            .and_then(|body| body.slot(main_hand())),
        Some(sword),
        "剑必须进主手槽"
    );
}

/// 脚下没东西、或下标越界 → `NoGround`（不 panic、不误拿别的东西）。
#[test]
fn taking_from_the_ground_reports_missing_items_and_bad_indices() {
    let (mut world, player) = world_with_player();

    assert_eq!(
        take_from_ground(&mut world, 0),
        Err(ItemActionError::NoGround),
        "脚下没东西"
    );
    ground_underfoot_with(&mut world, player, ItemId(10), 1);
    assert_eq!(
        take_from_ground(&mut world, 1),
        Err(ItemActionError::NoGround),
        "只有一件，下标 1 越界"
    );
    assert!(take_from_ground(&mut world, 0).is_ok());
}

/// 行囊超重 → `Overweight`，东西留在原地（事件栏要说清楚）。
#[test]
fn an_overweight_ground_item_stays_where_it_is() {
    let (mut world, player) = world_with_player();
    let pack = pack_of(&world, player);
    fill_pack_with_swords(&mut world, pack, 6); // 18.0；甲壳 1.0×3 → 21.0 > 20.0
    // 用**非装备**的重物：装备会被①空部位直接穿上，测不到行囊载重。
    let shell = ground_underfoot_with(&mut world, player, ItemId(14), 3);

    assert_eq!(
        take_from_ground(&mut world, 0),
        Err(ItemActionError::Overweight)
    );
    assert!(
        world.get::<GroundItemAt>(shell).is_some(),
        "背不动就留在地上"
    );
    let log = world.resource::<EventLog>();
    assert!(
        log.messages
            .iter()
            .any(|message| message.text.contains("行囊超重")),
        "事件栏必须区分超重与满"
    );
}
