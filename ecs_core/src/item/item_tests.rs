//! 物品系统的**链路级**测试：捡起 → 去哪 → 生效（DsnE16 / DsnE18）。
//!
//! 三层各钉一条：
//!
//! 1. **拾取路由**：装备进身体槽位、材料进行囊、消耗品在需要时立刻生效、行囊满了留在原地；
//! 2. **装备生效**：身体槽位里的物品决定加成与攻速；
//! 3. **掉落落地**：怪物死亡按表掉东西在地面，且位置正确。
//!
//! 这些用例能写出来，靠的是"容器的槽位是唯一权威、物品就是实体"这个形状：
//! 断言只需要"看槽里有没有它"，不必追某个关系实体有没有被正确同步。

use super::*;
use crate::components::{Health, Position};
use crate::effect::EffectContext;
use crate::events::{AttackEvent, AttackIntentEvent};
use crate::map::Tile;
use crate::rules::{StatId, apply_modifiers};
use crate::test_util::{fill_map, single_tile_scene, spawn_test_monster};
use bevy_ecs::message::Messages;
use bevy_ecs::system::{RunSystemOnce, SystemState};

// ── 场景脚手架 ───────────────────────────────────────

/// 全平地 + 玩家站在 `(10, 10)`，并给玩家**身体容器 + 行囊**。
///
/// `single_tile_scene` 走的是 `test_world`（只注册资源），没有跑 `spawn_player_system`，
/// 所以这里手工装上与生产一致的两件东西：身体容器（`BODY`）与塞在「背包」槽里的行囊。
fn open_world() -> (World, Entity) {
    let (mut world, player) = single_tile_scene();
    fill_map(&mut world, Tile::Floor);
    world.entity_mut(player).insert(Position::new(10, 10));
    let pack = world
        .spawn((Item::new(TRAVELER_PACK_ITEM), Amount::new(1), Container::new(&TRAVELER_PACK)))
        .id();
    let mut body = Container::new(&BODY);
    body.insert(pack_slot_index(), pack, &[ItemTag::Pack])
        .expect("身体必须有背包槽");
    world.entity_mut(player).insert(body);
    crate::system::run_settle_systems(&mut world);
    (world, player)
}

/// 在某一格放一堆物品（走唯一装配桥，组件与生产一致）。
fn place(world: &mut World, pos: (usize, usize), id: ItemId, count: u32) -> Entity {
    let entity = spawn_item_assembled(world, id, count);
    world
        .entity_mut(entity)
        .insert(GroundItemAt::new(pos.0, pos.1));
    entity
}

/// 显式拾取一次：拿脚下第 0 件（旧测试的"踩上自动捡"已改成动作调用）。
fn pickup(world: &mut World) {
    let _ = take_from_ground(world, 0);
}

/// 玩家的身体容器里第 `index` 槽占了谁。
fn body_slot(world: &World, player: Entity, index: usize) -> Option<Entity> {
    world.get::<Container>(player).and_then(|body| body.slot(index))
}

/// 主手槽的下标（从布局表推出，不手写常数）。
fn main_hand() -> usize {
    BODY.slots
        .iter()
        .position(|slot| slot.accept.accepts(&[ItemTag::Weapon]))
        .expect("身体必须有主手槽")
}

/// 行囊实体（身体「背包」槽里的那一件）。
fn pack_of(world: &World, player: Entity) -> Entity {
    body_slot(world, player, pack_slot_index()).expect("玩家身上必须有行囊")
}

/// 行囊里某个编号一共几件（同一编号的 `Amount` 相加）。
fn pack_count(world: &mut World, player: Entity, id: ItemId) -> u32 {
    let pack_entity = pack_of(world, player);
    let occupants: Vec<Entity> = world
        .get::<Container>(pack_entity)
        .map(|pack| pack.iter().map(|(_, item)| item).collect())
        .unwrap_or_default();
    occupants
        .into_iter()
        .filter_map(|item| {
            let item_comp = world.get::<Item>(item)?;
            if item_comp.id != id {
                return None;
            }
            Some(world.get::<Amount>(item).map(|a| a.0).unwrap_or(0))
        })
        .sum()
}

/// 求值侧的有效数值（与 `resolve_attack_system` 用的是同一条路径）。
fn effective(world: &mut World, actor: Entity, stat: StatId, base: f64) -> f64 {
    let mut state = SystemState::<EffectContext>::new(world);
    let mods = {
        let context = state.get(world).unwrap();
        context.collect_actor(actor)
    };
    apply_modifiers(stat, base, &mods, &[])
}

// ── 1. 拾取路由 ──────────────────────────────────────

/// 装备：对应部位空着 → 进身体槽位（= 穿上），并离开地面。
#[test]
fn equipment_goes_into_the_body_slot() {
    let (mut world, player) = open_world();
    let sword = place(&mut world, (10, 10), ItemId(0), 1);

    pickup(&mut world);

    assert_eq!(
        body_slot(&world, player, main_hand()),
        Some(sword),
        "剑必须进主手槽"
    );
    assert!(
        world.get::<GroundItemAt>(sword).is_none(),
        "进了容器就不能还带着地面位置"
    );
}

/// 装备：部位已占 → 进行囊（不顶掉、不丢失）。
#[test]
fn a_second_weapon_goes_into_the_pack() {
    let (mut world, player) = open_world();
    let first = place(&mut world, (10, 10), ItemId(0), 1);
    pickup(&mut world);
    let second = place(&mut world, (10, 10), ItemId(24), 1);
    pickup(&mut world);

    assert_eq!(
        body_slot(&world, player, main_hand()),
        Some(first),
        "已占的部位不得被顶掉"
    );
    let pack = pack_of(&world, player);
    assert!(
        world
            .get::<Container>(pack)
            .is_some_and(|pack| pack.contains(second)),
        "第二把武器必须进行囊"
    );
}

/// 材料：进行囊，同一编号先并堆（`Amount` 叠加），不占两格。
#[test]
fn materials_merge_into_one_slot() {
    let (mut world, player) = open_world();
    place(&mut world, (10, 10), ItemId(10), 2);
    pickup(&mut world);
    place(&mut world, (10, 10), ItemId(10), 3);
    pickup(&mut world);

    assert_eq!(pack_count(&mut world, player, ItemId(10)), 5, "血肉 ×5");
    let pack = pack_of(&world, player);
    assert_eq!(
        world.get::<Container>(pack).map(|pack| pack.used()),
        Some(1),
        "同种材料必须并进同一格"
    );
}

/// 消耗品：生命不满时立刻用掉 1 个。
#[test]
fn a_consumable_is_used_immediately_when_it_helps() {
    let (mut world, player) = open_world();
    let max = world.get::<Health>(player).unwrap().max;
    world.get_mut::<Health>(player).unwrap().current = max - 10.0;
    let mushroom = place(&mut world, (10, 10), ItemId(25), 1);

    pickup(&mut world);

    assert!(world.get_entity(mushroom).is_err(), "用掉的消耗品必须消失");
    assert_eq!(
        world.get::<Health>(player).unwrap().current,
        max - 4.0,
        "蘑菇回复 6 点"
    );
}

/// 消耗品：满血时不消耗，进行囊留着。
#[test]
fn a_consumable_is_stored_when_it_would_be_wasted() {
    let (mut world, player) = open_world();
    let mushroom = place(&mut world, (10, 10), ItemId(25), 1);

    pickup(&mut world);

    let pack = pack_of(&world, player);
    assert!(
        world
            .get::<Container>(pack)
            .is_some_and(|pack| pack.contains(mushroom)),
        "满血时蘑菇应当进行囊"
    );
}

/// 一堆消耗品只吃掉 1 个，剩下的仍在地上（`Amount` 减一）。
#[test]
fn using_one_of_a_pile_leaves_the_rest_on_the_ground() {
    let (mut world, player) = open_world();
    let max = world.get::<Health>(player).unwrap().max;
    world.get_mut::<Health>(player).unwrap().current = max - 10.0;
    let pile = place(&mut world, (10, 10), ItemId(25), 3);

    pickup(&mut world);

    assert_eq!(
        world.get::<Amount>(pile).map(|amount| amount.0),
        Some(2),
        "吃一个、剩两个"
    );
}

/// 行囊满：物品留在原地并记日志（不许静默失败，SYN6）。
#[test]
fn a_full_pack_leaves_the_item_on_the_ground_and_says_so() {
    let (mut world, player) = open_world();
    let pack_entity = pack_of(&world, player);
    {
        // 用**不可堆叠**的东西填满（剑）：否则"破布"还能并堆，满得不彻底。
        let fillers: Vec<Entity> = (0..TRAVELER_PACK.len())
            .map(|_| world.spawn((Item::new(ItemId(0)), Amount::new(1))).id())
            .collect();
        let mut pack = world.get_mut::<Container>(pack_entity).expect("行囊是容器");
        for (index, filler) in fillers.into_iter().enumerate() {
            pack.insert(index, filler, &[]).expect("空格");
        }
        assert_eq!(pack.used(), pack.len(), "前置条件：行囊已满");
    }
    let rag = place(&mut world, (10, 10), ItemId(11), 1);

    pickup(&mut world);

    assert!(world.get_entity(rag).is_ok(), "行囊满时物品必须留在原地");
    let log = world.resource::<EventLog>();
    assert!(
        log.messages.iter().any(|m| m.text.contains("行囊满了")),
        "必须告诉玩家为什么没捡起来：{:?}",
        log.messages.iter().map(|m| &m.text).collect::<Vec<_>>()
    );
}

/// 行囊**有空槽但超重**：物品留在原地，日志说"超重"而不是"满了"（DsnE20）。
#[test]
fn an_overweight_pack_leaves_the_item_on_the_ground_and_says_so() {
    let (mut world, player) = open_world();
    let pack_entity = pack_of(&world, player);
    {
        // 6 把锈铁剑 = 18.0，行囊还空着 6 格，但只剩 2.0 的余量。
        let heavy: Vec<Entity> = (0..6)
            .map(|_| world.spawn((Item::new(ItemId(0)), Amount::new(1))).id())
            .collect();
        let mut pack = world.get_mut::<Container>(pack_entity).expect("行囊是容器");
        for (index, sword) in heavy.into_iter().enumerate() {
            pack.insert(index, sword, &[]).expect("空格");
        }
    }
    // 黑色甲壳 1.0 ×3：18.0 + 3.0 = 21.0 > 20.0（材料走行囊路由，不会被装备）。
    let shell = place(&mut world, (10, 10), ItemId(14), 3);

    pickup(&mut world);

    assert!(
        world.get::<GroundItemAt>(shell).is_some(),
        "背不动的东西必须留在原地"
    );
    let log = world.resource::<EventLog>();
    assert!(
        log.messages.iter().any(|m| m.text.contains("行囊超重")),
        "日志要区分超重与满：{:?}",
        log.messages.iter().map(|m| &m.text).collect::<Vec<_>>()
    );
}

/// 不站在物品上就什么也不捡。
#[test]
fn items_on_other_cells_are_not_picked_up() {
    let (mut world, player) = open_world();
    let far = place(&mut world, (14, 10), ItemId(10), 1);

    pickup(&mut world);

    assert!(world.get_entity(far).is_ok());
    assert_eq!(
        world
            .get::<Container>(pack_of(&world, player))
            .map(|pack| pack.used()),
        Some(0)
    );
}

// ── 2. 装备生效 ──────────────────────────────────────

/// 攻击加成：主手槽里放剑 → 有效攻击 = 8 + 3。
#[test]
fn a_weapon_in_the_body_raises_the_effective_attack() {
    let (mut world, player) = open_world();
    assert_eq!(effective(&mut world, player, StatId::ATTACK, 8.0), 8.0);

    place(&mut world, (10, 10), ItemId(0), 1);
    pickup(&mut world);

    assert_eq!(
        effective(&mut world, player, StatId::ATTACK, 8.0),
        11.0,
        "锈铁剑 +3"
    );
    // 攻击加成**不许**漏进速度。
    assert_eq!(effective(&mut world, player, StatId::MOVE_SPEED, 1.0), 1.0);
}

/// 防御加成：皮甲（躯干）→ 有效防御 = 4 + 4。
#[test]
fn armor_in_the_body_raises_the_effective_defense() {
    let (mut world, player) = open_world();
    place(&mut world, (10, 10), ItemId(2), 1);
    pickup(&mut world);

    assert_eq!(effective(&mut world, player, StatId::DEFENSE, 4.0), 8.0);
}

/// 多件装备叠加：剑 + 戒指 = +5 攻击（两个不同部位）。
#[test]
fn equipment_bonuses_stack_across_slots() {
    let (mut world, player) = open_world();
    place(&mut world, (10, 10), ItemId(0), 1);
    pickup(&mut world);
    place(&mut world, (10, 10), ItemId(3), 1);
    pickup(&mut world);

    assert_eq!(effective(&mut world, player, StatId::ATTACK, 8.0), 13.0);
}

/// 把物品移出身体槽位 → 加成**立刻**消失（槽位是唯一权威，不读副本）。
#[test]
fn removing_the_item_from_the_body_removes_its_bonus() {
    let (mut world, player) = open_world();
    place(&mut world, (10, 10), ItemId(0), 1);
    pickup(&mut world);
    assert_eq!(effective(&mut world, player, StatId::ATTACK, 8.0), 11.0);

    let sword = body_slot(&world, player, main_hand()).expect("主手有剑");
    world
        .get_mut::<Container>(player)
        .expect("玩家有身体")
        .remove(sword);

    assert_eq!(effective(&mut world, player, StatId::ATTACK, 8.0), 8.0);
}

/// 踏水靴：装进身体后求值侧收到"无视地形"那一桶。
#[test]
fn trail_boots_still_ignore_the_terrain_bucket() {
    let (mut world, player) = open_world();
    place(&mut world, (10, 10), ItemId(40), 1);
    pickup(&mut world);

    let mut state = SystemState::<EffectContext>::new(&mut world);
    let collected = {
        let context = state.get(&world).unwrap();
        context.collect(player, (10, 10))
    };
    assert!(
        collected
            .ignored
            .contains(&crate::rules::EffectSource::Terrain),
        "靴子必须让地形那一桶被无视，实得 {:?}",
        collected.ignored
    );
}

/// 武器改的是**攻击耗时**（GAME.md Gm7「武器攻速」）。
#[test]
fn the_weapon_replaces_the_attack_duration() {
    let (mut world, player) = open_world();
    let unarmed = {
        let mut state = SystemState::<EffectContext>::new(&mut world);
        state.get(&world).unwrap().main_hand_weapon_speed(player)
    };
    assert_eq!(unarmed, None, "空手时用空手基准");

    place(&mut world, (10, 10), ItemId(23), 1); // 石锤 450ms
    pickup(&mut world);

    let hammer = {
        let mut state = SystemState::<EffectContext>::new(&mut world);
        state.get(&world).unwrap().main_hand_weapon_speed(player)
    };
    assert_eq!(hammer, Some(450.0), "石锤 450ms");
}

/// 力量上限来自主手武器的蓝图（DsnE24）：空手没有上限，装备后读武器声明值。
#[test]
fn the_weapon_supplies_the_strength_cap() {
    let (mut world, player) = open_world();
    let unarmed = {
        let mut state = SystemState::<EffectContext>::new(&mut world);
        state.get(&world).unwrap().main_hand_strength_cap(player)
    };
    assert_eq!(unarmed, None, "空手没有武器上限，用 balance 的默认值");

    place(&mut world, (10, 10), ItemId(0), 1); // 锈铁剑 6
    pickup(&mut world);

    let sword = {
        let mut state = SystemState::<EffectContext>::new(&mut world);
        state.get(&world).unwrap().main_hand_strength_cap(player)
    };
    assert_eq!(sword, Some(6.0), "锈铁剑的力量上限是 6");
}

// ── 3. 掉落落地 ──────────────────────────────────────

/// 怪物死亡 → 按模板掉落表掉东西在它倒下的那一格。
#[test]
fn a_dead_monster_drops_loot_on_its_cell() {
    let (mut world, _player) = open_world();
    let rat = spawn_test_monster(
        &mut world,
        crate::monster::MonsterKindId::Rat,
        (6, 6),
        10.0,
        4.0,
    );
    world.get_mut::<Health>(rat).unwrap().current = 0.0;

    let _ = world.run_system_once(crate::system::death::check_death_system);

    assert!(world.get_entity(rat).is_err(), "死者必须被回收");
    let drops: Vec<((usize, usize), ItemId, u32)> = {
        let mut query = world.query::<(&GroundItemAt, &Item, &Amount)>();
        query
            .iter(&world)
            .map(|(at, item, amount)| (at.to_tuple(), item.id, amount.0))
            .collect()
    };
    assert!(!drops.is_empty(), "老鼠必掉血肉");
    for (pos, id, count) in &drops {
        assert_eq!(*pos, (6, 6), "掉落必须落在尸体那一格");
        assert_eq!(*id, ItemId(10), "老鼠只掉生物血肉");
        assert!((1..=2).contains(count), "血肉数量 1-2，实得 {count}");
    }
}

/// 玩家死亡不掉东西。
#[test]
fn the_players_death_does_not_drop_loot() {
    let (mut world, player) = open_world();
    world.get_mut::<Health>(player).unwrap().current = 0.0;

    let _ = world.run_system_once(crate::system::death::check_death_system);

    let mut query = world.query_filtered::<&Item, With<GroundItemAt>>();
    assert_eq!(query.iter(&world).count(), 0, "玩家死亡不该掉东西");
    assert!(world.resource::<crate::resources::TurnManager>().game_over);
}

/// 攻击链路：身体槽位里的装备加成真的进了伤害计算。
#[test]
fn equipment_bonus_reaches_the_damage_pipeline() {
    let (mut world, player) = open_world();
    let target = spawn_test_monster(
        &mut world,
        crate::monster::MonsterKindId::Rat,
        (11, 10),
        100.0,
        4.0,
    );
    crate::system::run_settle_systems(&mut world);

    fn swing(world: &mut World, player: Entity, target: Entity) -> f64 {
        world.write_message(AttackIntentEvent {
            attacker: player,
            target,
        });
        let _ = world.run_system_once(crate::system::combat::resolve_attack_system);
        world
            .resource_mut::<Messages<AttackEvent>>()
            .drain()
            .next()
            .expect("必须产生一次攻击事件")
            .damage
    }

    let unarmed = swing(&mut world, player, target);
    place(&mut world, (10, 10), ItemId(0), 1);
    pickup(&mut world);
    let armed = swing(&mut world, player, target);

    assert!(
        armed > unarmed,
        "持剑的伤害必须更高（空手 {unarmed}，持剑 {armed}）"
    );
}

// ── 4. 尸体容器（DsnE18 第②片）───────────────────────

/// 哥布林倒下 → 原地留一具**容器**，装着它身上的匕首；战利品仍落在地上。
#[test]
fn a_dead_monster_leaves_a_corpse_holding_its_equipment() {
    let (mut world, _player) = open_world();
    let goblin = spawn_test_monster(
        &mut world,
        crate::monster::MonsterKindId::Goblin,
        (6, 6),
        10.0,
        4.0,
    );
    world.get_mut::<Health>(goblin).unwrap().current = 0.0;

    let _ = world.run_system_once(crate::system::death::check_death_system);

    let corpses: Vec<(Entity, (usize, usize))> = {
        let mut query =
            world.query_filtered::<(Entity, &Position), With<crate::monster::Corpse>>();
        query
            .iter(&world)
            .map(|(entity, position)| (entity, position.to_tuple()))
            .collect()
    };
    assert_eq!(corpses.len(), 1, "一只怪倒下一具尸体");
    let (corpse, at) = corpses[0];
    assert_eq!(at, (6, 6), "尸体留在它倒下的那一格");
    assert!(
        world.get::<crate::components::Blocks>(corpse).is_none(),
        "尸体不占格，玩家能踩上去"
    );

    let held: Vec<ItemId> = {
        let container = world.get::<Container>(corpse).expect("尸体必须是容器");
        container
            .iter()
            .filter_map(|(_, item)| world.get::<Item>(item).map(|item| item.id))
            .collect()
    };
    assert_eq!(held, vec![ItemId(24)], "哥布林身上的匕首进尸体");

    // 用户选的 (a) 档：战利品（模板 `loot`）仍然落在地上。
    let mut ground = world.query_filtered::<&Item, With<GroundItemAt>>();
    assert!(
        ground.iter(&world).count() > 0,
        "战利品仍按 GAME.md Gm8 落地"
    );
}

/// 不配装备的物种（老鼠）留下**空**尸体，不掉多余的东西。
#[test]
fn a_monster_without_equipment_leaves_an_empty_corpse() {
    let (mut world, _player) = open_world();
    let rat = spawn_test_monster(&mut world, crate::monster::MonsterKindId::Rat, (6, 6), 10.0, 4.0);
    world.get_mut::<Health>(rat).unwrap().current = 0.0;

    let _ = world.run_system_once(crate::system::death::check_death_system);

    let mut query = world.query_filtered::<&Container, With<crate::monster::Corpse>>();
    let containers: Vec<&Container> = query.iter(&world).collect();
    assert_eq!(containers.len(), 1, "老鼠也有尸体");
    assert_eq!(containers[0].used(), 0, "老鼠身上没装备，尸体是空的");
}

/// 踩到尸体格**不再自动搜刮**（玩家反馈）：尸体保持原样，东西由背包页「脚下」面板
/// 显式拿取（`actions::take_from_corpse`，见 actions_tests.rs）。
#[test]
fn stepping_on_a_corpse_does_not_auto_loot() {
    let (mut world, player) = open_world();
    let dagger = world
        .spawn((Item::new(ItemId(24)), Amount::new(1)))
        .id();
    let mut container = Container::new(&CORPSE);
    container
        .insert(0, dagger, &[ItemTag::Weapon])
        .expect("尸体格子无过滤");
    let corpse = world
        .spawn((
            crate::monster::Corpse::new(crate::monster::MonsterKindId::Goblin, "哥布林"),
            Position::new(10, 10),
            container,
        ))
        .id();

    // 玩家这一格没有地面物品：显式拾取也拿不到任何东西。
    assert_eq!(
        take_from_ground(&mut world, 0),
        Err(ItemActionError::NoGround)
    );
    let pack_entity = world
        .get::<Container>(player)
        .and_then(|body| body.slot(pack_slot_index()))
        .expect("玩家必须有行囊");
    let in_pack: Vec<ItemId> = {
        let pack = world.get::<Container>(pack_entity).expect("行囊实体");
        pack.iter()
            .filter_map(|(_, item)| world.get::<Item>(item).map(|item| item.id))
            .collect()
    };
    assert!(in_pack.is_empty(), "踩上尸体不该自动拿走任何东西");
    assert_eq!(
        world.get::<Container>(corpse).expect("尸体还在").used(),
        1,
        "匕首必须还在尸体上，等玩家自己翻"
    );

    let log = world.resource::<EventLog>();
    assert!(
        !log.messages
            .iter()
            .any(|message| message.text.contains("搜出了")),
        "不再有自动搜刮的日志"
    );
}
use std::collections::HashSet;

/// 一条**全世界**的不变量：每件物品要么在地上（`GroundItemAt`），
/// 要么在某个容器槽位里——不能两边都有，也不能两边都没有。
///
/// 存放在哪是两条互斥的路径（`item/mod.rs` 模块文档）：拾取做"从地面摘标记、插进槽位"，
/// 掉落做"生成实体 + 挂 GroundItemAt"。漏改一条路径就会出现"既在地上又在背包里"的
/// 幽灵物品（同一件东西能被捡两次），或"无位置无主人"的孤儿实体。
#[test]
fn every_item_is_either_on_the_ground_or_in_a_container() {
    use crate::components::Position;

    let (mut world, player) = open_world();
    // 玩家挪到与哥布林同一格：拾取系统会收走落地的战利品，尸体里的匕首留在容器里
    // （踩尸体不自动搜刮，正好让这条不变量同时覆盖「地面→行囊」与「尸体容器」两种存法）。
    world.get_mut::<Position>(player).unwrap().x = 10;
    world.get_mut::<Position>(player).unwrap().y = 10;

    // 远处留一件地面物品（不会被捡）。
    world.spawn((
        Item::new(ItemId(25)),
        Amount::new(1),
        GroundItemAt::new(40, 40),
    ));
    let goblin = spawn_test_monster(
        &mut world,
        crate::monster::MonsterKindId::Goblin,
        (10, 10),
        10.0,
        1.0,
    );
    world.get_mut::<Health>(goblin).unwrap().current = 0.0;
    let _ = world.run_system_once(crate::system::death::check_death_system);
    // 显式把同格地面战利品全部拿走（踩上不再自动捡）。
    while take_from_ground(&mut world, 0).is_ok() {}

    let mut in_container: HashSet<bevy_ecs::entity::Entity> = HashSet::new();
    let mut containers = world.query::<&Container>();
    for container in containers.iter(&world) {
        in_container.extend(container.iter().map(|(_, item)| item));
    }
    let ground_items: HashSet<bevy_ecs::entity::Entity> = {
        let mut query = world.query_filtered::<bevy_ecs::entity::Entity, With<GroundItemAt>>();
        query.iter(&world).collect()
    };
    let all_items: HashSet<bevy_ecs::entity::Entity> = {
        let mut query = world.query_filtered::<bevy_ecs::entity::Entity, With<Item>>();
        query.iter(&world).collect()
    };

    assert!(!all_items.is_empty(), "场景里必须有物品，否则这条测试没意义");
    for item in &all_items {
        let on_ground = ground_items.contains(item);
        let stored = in_container.contains(item);
        assert!(
            on_ground != stored,
            "物品 {item:?} 的存放方式不唯一：在地上={on_ground}、在容器里={stored}"
        );
    }
}

#[test]
fn assembly_materializes_parts_as_components() {
    let mut world = World::new();
    let sword = spawn_item_assembled(&mut world, ItemId(0), 1);
    assert!(world.get::<Equippable>(sword).is_some(), "剑是可装备物品");
    assert!(world.get::<Weapon>(sword).is_some(), "剑是主手武器");
    assert!(
        world.get::<Modifiers>(sword).is_some(),
        "剑的 +3 攻击应装配成组件"
    );

    let mushroom = spawn_item_assembled(&mut world, ItemId(25), 3);
    assert!(world.get::<Consumable>(mushroom).is_some());
    assert!(world.get::<Behaviors>(mushroom).is_some(), "蘑菇带 use 行为");
    assert!(world.get::<Equippable>(mushroom).is_none(), "蘑菇不是装备");

    let pack = spawn_item_assembled(&mut world, ItemId(41), 1);
    assert!(world.get::<Container>(pack).is_some(), "行囊自带容器");

    let material = spawn_item_assembled(&mut world, ItemId(10), 2);
    assert!(world.get::<Equippable>(material).is_none());
    assert!(world.get::<Modifiers>(material).is_none());
    assert!(world.get::<Behaviors>(material).is_none());
}

#[test]
fn weapons_are_assembled_at_level_one_with_no_experience() {
    let mut world = World::new();
    let sword = spawn_item_assembled(&mut world, ItemId(0), 1);
    let progress = *world
        .get::<WeaponProgress>(sword)
        .expect("有成长曲线的武器装配时应带养成进度");
    assert_eq!((progress.level, progress.exp), (1, 0.0));

    let mushroom = spawn_item_assembled(&mut world, ItemId(25), 3);
    assert!(
        world.get::<WeaponProgress>(mushroom).is_none(),
        "非武器没有养成进度"
    );
}

/// `Weapon` 的 required component：绕过装配桥直接插 `Weapon` 也会自动带进度。
///
/// 这保证的是「武器实体一定带 `WeaponProgress`」属于**类型层面**的结构约束，
/// 而不是只有装配桥记得插。
#[test]
fn weapon_requires_its_progress_component() {
    let mut world = World::new();
    let spec = ItemId(0).def().weapon().expect("锈铁剑有武器部件");
    let weapon = world
        .spawn(Weapon {
            speed_ms: spec.speed_ms,
            strength_cap: spec.strength_cap,
            growth: spec.growth,
        })
        .id();
    let progress = *world
        .get::<WeaponProgress>(weapon)
        .expect("插入 Weapon 必须自动带 WeaponProgress（required component）");
    assert_eq!((progress.level, progress.exp), (1, 0.0));
}

#[test]
fn weapon_experience_levels_up_across_the_curved_threshold_and_caps() {
    let growth = ItemId(0)
        .def()
        .weapon()
        .expect("锈铁剑有武器部件")
        .growth;
    let mut progress = WeaponProgress::new();

    // 差一点不升。
    let first = growth.exp_to_next(1);
    assert_eq!(progress.add_exp(first - 0.1, &growth), 0);
    assert_eq!(progress.level, 1);

    // 补上最后一点 → 恰好升 1 级，剩余经验 = 多出来的部分。
    assert_eq!(progress.add_exp(0.1, &growth), 1);
    assert_eq!(progress.level, 2);
    assert!(progress.exp.abs() < 1e-9, "刚好跨过门槛，不该有剩余经验");

    // 一次给一大笔可以连升；到上限后停住、经验清零、再给也不再涨。
    let huge = growth.exp_to_next(2) + growth.exp_to_next(3) + growth.exp_to_next(4) + 1.0;
    progress.add_exp(huge, &growth);
    assert_eq!(progress.level, growth.max_level, "等级上限是 5");
    assert_eq!(progress.exp, 0.0, "满级后经验清零");
    assert_eq!(progress.add_exp(999.0, &growth), 0, "满级不再升级");

    // 非正/非有限经验一律忽略。
    let mut fresh = WeaponProgress::new();
    assert_eq!(fresh.add_exp(0.0, &growth), 0);
    assert_eq!(fresh.add_exp(-5.0, &growth), 0);
    assert_eq!(fresh.add_exp(f64::NAN, &growth), 0);
    assert_eq!((fresh.level, fresh.exp), (1, 0.0));
}
