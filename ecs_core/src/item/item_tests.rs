//! 物品系统的**链路级**测试：捡起 → 去哪 → 生效。
//!
//! 三层各钉一条：
//!
//! 1. **拾取路由**：装备进槽位、材料进背包、消耗品在需要时立刻生效、背包满了留在原地；
//! 2. **装备生效**：穿上剑后攻击变高、穿上甲后受伤变少、武器改攻击耗时；
//! 3. **掉落落地**：怪物死亡按表掉东西在地面，且位置正确。
//!
//! 这些用例能写出来，靠的是"装备的数值只有注册表一处、求值侧按关系收"这个形状：
//! 于是断言只需要"穿上 → 读有效值"，不必去追某个效果实体有没有被正确同步。

use super::*;
use crate::effect::EffectContext;
use crate::events::{AttackEvent, AttackIntentEvent};
use crate::map::Tile;
use crate::rules::{Stat, apply_modifiers};
use crate::test_util::{fill_map, single_tile_scene, spawn_test_monster};
use bevy_ecs::event::Events;
use bevy_ecs::system::{RunSystemOnce, SystemState};

// ── 场景脚手架 ───────────────────────────────────────

/// 全平地 + 玩家站在 `(10, 10)`（带空背包）。
fn open_world() -> (World, Entity) {
    let (mut world, player) = single_tile_scene();
    fill_map(&mut world, Tile::Floor);
    world.entity_mut(player).insert(Position::new(10, 10));
    if world.get::<Inventory>(player).is_none() {
        world.entity_mut(player).insert(Inventory::new());
    }
    crate::system::run_settle_systems(&mut world);
    (world, player)
}

/// 在某一格放一件东西。
fn place(world: &mut World, pos: (usize, usize), id: ItemId, count: u32) -> Entity {
    world
        .spawn((GroundItemAt::new(pos.0, pos.1), GroundItem::of(id, count)))
        .id()
}

/// 跑一次拾取。
fn pickup(world: &mut World) {
    let _ = world.run_system_once(pickup_system);
}

/// 玩家装备着的物品（槽位, 编号）。
fn equipped_of(world: &mut World, owner: Entity) -> Vec<(EquipSlot, ItemId)> {
    let mut query = world.query::<(&EquippedBy, &EquippedItem)>();
    let mut list: Vec<(EquipSlot, ItemId)> = query
        .iter(world)
        .filter(|(of, _)| of.owner() == owner)
        .map(|(_, item)| (item.slot, item.id))
        .collect();
    list.sort_by_key(|(slot, _)| EquipSlot::ALL.iter().position(|s| s == slot));
    list
}

/// 求值侧的有效攻击/防御（与 `resolve_attack_system` 用的是同一条路径）。
fn effective(world: &mut World, actor: Entity, stat: Stat, base: f64) -> f64 {
    let mut state = SystemState::<EffectContext>::new(world);
    let mods = {
        let context = state.get(world);
        context.collect_actor(actor)
    };
    apply_modifiers(stat, base, &mods, &[])
}

// ── 1. 拾取路由 ──────────────────────────────────────

/// 装备：槽位空 → 直接穿上（不占背包），并从地上消失。
#[test]
fn equipment_is_worn_when_the_slot_is_free() {
    let (mut world, player) = open_world();
    let sword = place(&mut world, (10, 10), ItemId(0), 1);

    pickup(&mut world);

    assert!(world.get_entity(sword).is_err(), "捡起后地面物品必须消失");
    assert_eq!(
        equipped_of(&mut world, player),
        vec![(EquipSlot::MainHand, ItemId(0))],
        "锈铁剑必须进主手槽"
    );
    assert_eq!(
        world.get::<Inventory>(player).map(Inventory::used),
        Some(0),
        "装备不占背包空间"
    );
}

/// 装备：槽位已占 → 进背包（不替换、不丢失）。
#[test]
fn a_second_weapon_goes_to_the_bag() {
    let (mut world, player) = open_world();
    place(&mut world, (10, 10), ItemId(0), 1); // 锈铁剑先穿
    pickup(&mut world);
    place(&mut world, (10, 10), ItemId(24), 1); // 匕首后到
    pickup(&mut world);

    assert_eq!(
        equipped_of(&mut world, player),
        vec![(EquipSlot::MainHand, ItemId(0))],
        "已占用的槽位不得被顶掉"
    );
    let inventory = world.get::<Inventory>(player).expect("玩家有背包");
    assert_eq!(inventory.count_of(ItemId(24)), 1, "第二把武器必须进背包");
}

/// 材料：进背包并按 `max_stack` 堆叠。
#[test]
fn materials_stack_in_the_bag() {
    let (mut world, player) = open_world();
    place(&mut world, (10, 10), ItemId(10), 2); // 血肉 ×2
    pickup(&mut world);
    place(&mut world, (10, 10), ItemId(10), 3); // 血肉 ×3
    pickup(&mut world);

    let inventory = world.get::<Inventory>(player).expect("玩家有背包");
    assert_eq!(inventory.count_of(ItemId(10)), 5);
    assert_eq!(inventory.used(), 1, "同种材料必须并进同一堆");
}

/// 消耗品：生命不满时立刻生效并消耗掉（临时口径，背包页落地前）。
#[test]
fn a_consumable_is_used_immediately_when_it_helps() {
    let (mut world, player) = open_world();
    let health = world.get::<Health>(player).unwrap();
    let max = health.max;
    world.get_mut::<Health>(player).unwrap().current = max - 10.0;
    let mushroom = place(&mut world, (10, 10), ItemId(25), 1);

    pickup(&mut world);

    assert!(world.get_entity(mushroom).is_err(), "用掉的消耗品必须消失");
    assert_eq!(
        world.get::<Health>(player).unwrap().current,
        max - 4.0,
        "蘑菇回复 6 点（6.0 精确可表示）"
    );
    assert_eq!(
        world.get::<Inventory>(player).map(Inventory::used),
        Some(0),
        "没进背包"
    );
}

/// 消耗品：满血时**不消耗**，进背包留着。
#[test]
fn a_consumable_is_stored_when_it_would_be_wasted() {
    let (mut world, player) = open_world();
    let mushroom = place(&mut world, (10, 10), ItemId(25), 1);

    pickup(&mut world);

    assert!(world.get_entity(mushroom).is_err(), "捡起后进背包，所以仍离地");
    assert_eq!(
        world.get::<Inventory>(player).map(|inv| inv.count_of(ItemId(25))),
        Some(1),
        "满血时蘑菇进背包"
    );
}

/// 消耗品按堆消耗：一堆 3 个，只吃掉 1 个，地上还剩 2 个。
#[test]
fn using_one_of_a_pile_leaves_the_rest_on_the_ground() {
    let (mut world, player) = open_world();
    let max = world.get::<Health>(player).unwrap().max;
    world.get_mut::<Health>(player).unwrap().current = max - 10.0;
    let pile = place(&mut world, (10, 10), ItemId(25), 3);

    pickup(&mut world);

    let remaining = world
        .get::<GroundItem>(pile)
        .map(|ground| ground.stack.count)
        .expect("剩下的蘑菇必须还在地上");
    assert_eq!(remaining, 2, "吃一个、剩两个");
}

/// 背包满：物品**留在原地**并记一条日志（不许静默失败，ISSUES SYN6）。
#[test]
fn a_full_bag_leaves_the_item_on_the_ground_and_says_so() {
    let (mut world, player) = open_world();
    {
        let mut inventory = world.get_mut::<Inventory>(player).expect("玩家有背包");
        for _ in 0..INVENTORY_SLOTS {
            inventory.push_slot(ItemStack::new(ItemId(10), 64));
        }
        assert!(inventory.is_full());
    }
    let rag = place(&mut world, (10, 10), ItemId(11), 1);

    pickup(&mut world);

    assert!(
        world.get_entity(rag).is_ok(),
        "背包满时物品必须留在原地"
    );
    let log = world.resource::<EventLog>();
    assert!(
        log.messages.iter().any(|message| message.text.contains("背包满了")),
        "必须告诉玩家为什么没捡起来：{:?}",
        log.messages.iter().map(|m| &m.text).collect::<Vec<_>>()
    );
}

/// 不站在物品上就什么也不捡（拾取是按格判定的，不是按地图）。
#[test]
fn items_on_other_cells_are_not_picked_up() {
    let (mut world, player) = open_world();
    let far = place(&mut world, (14, 10), ItemId(10), 1);

    pickup(&mut world);

    assert!(world.get_entity(far).is_ok());
    assert_eq!(world.get::<Inventory>(player).map(Inventory::used), Some(0));
}

// ── 2. 装备生效 ──────────────────────────────────────

/// 攻击加成：穿上锈铁剑后有效攻击 = 8 + 3。
#[test]
fn a_weapon_raises_the_effective_attack() {
    let (mut world, player) = open_world();
    assert_eq!(effective(&mut world, player, Stat::Attack, 8.0), 8.0);

    place(&mut world, (10, 10), ItemId(0), 1);
    pickup(&mut world);

    assert_eq!(
        effective(&mut world, player, Stat::Attack, 8.0),
        11.0,
        "锈铁剑 +3"
    );
    // 关键：攻击加成**不许**漏进速度（`Stat` 维度存在的意义）。
    assert_eq!(
        effective(&mut world, player, Stat::MoveSpeed, 1.0),
        1.0,
        "武器不该改移动速度"
    );
}

/// 防御加成：穿上皮甲后有效防御 = 4 + 4。
#[test]
fn armor_raises_the_effective_defense() {
    let (mut world, player) = open_world();
    place(&mut world, (10, 10), ItemId(2), 1);
    pickup(&mut world);

    assert_eq!(
        effective(&mut world, player, Stat::Defense, 4.0),
        8.0,
        "皮甲 +4 防御"
    );
}

/// 多件装备叠加：剑 + 戒指 = +5 攻击。
#[test]
fn equipment_bonuses_stack_across_slots() {
    let (mut world, player) = open_world();
    place(&mut world, (10, 10), ItemId(0), 1); // 锈铁剑 +3
    pickup(&mut world);
    place(&mut world, (10, 10), ItemId(3), 1); // 攻击戒指 +2
    pickup(&mut world);

    assert_eq!(effective(&mut world, player, Stat::Attack, 8.0), 13.0);
}

/// 卸下（直接 `despawn` 装备实体）之后加成**立刻**消失。
///
/// 这条是"求值侧按关系收、不在实体上抄一份修正"的直接红利：
/// 不需要任何 owner 系统去清理效果实体，删掉装备就完事。
#[test]
fn removing_the_equipment_removes_its_bonus() {
    let (mut world, player) = open_world();
    place(&mut world, (10, 10), ItemId(0), 1);
    pickup(&mut world);
    assert_eq!(effective(&mut world, player, Stat::Attack, 8.0), 11.0);

    let equipped: Vec<Entity> = {
        let mut query = world.query::<(Entity, &EquippedBy)>();
        query
            .iter(&world)
            .filter(|(_, of)| of.owner() == player)
            .map(|(entity, _)| entity)
            .collect()
    };
    for entity in equipped {
        world.entity_mut(entity).despawn();
    }

    assert_eq!(
        effective(&mut world, player, Stat::Attack, 8.0),
        8.0,
        "装备没了，加成必须归零"
    );
}

/// 踏水靴：装备之后求值侧收到"无视地形"那一桶（C4/C5 的语义在物品系统里活下来）。
#[test]
fn trail_boots_still_ignore_the_terrain_bucket() {
    let (mut world, player) = open_world();
    place(&mut world, (10, 10), ItemId(40), 1);
    pickup(&mut world);

    let mut state = SystemState::<EffectContext>::new(&mut world);
    let collected = {
        let context = state.get(&world);
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

/// 武器改的是**攻击耗时**（GAME.md Gm7「武器攻速」），不是攻击速度倍率。
#[test]
fn the_weapon_replaces_the_attack_duration() {
    let (mut world, player) = open_world();
    let unarmed = {
        let mut state = SystemState::<EffectContext>::new(&mut world);
        let context = state.get(&world);
        context.main_hand_weapon_speed(player)
    };
    assert_eq!(unarmed, None, "空手时用空手基准");

    place(&mut world, (10, 10), ItemId(23), 1); // 石锤 450ms
    pickup(&mut world);

    let hammer = {
        let mut state = SystemState::<EffectContext>::new(&mut world);
        let context = state.get(&world);
        context.main_hand_weapon_speed(player)
    };
    assert_eq!(hammer, Some(450.0), "石锤 450ms");
}

// ── 3. 掉落落地 ──────────────────────────────────────

/// 怪物死亡 → 按模板掉落表掉东西在他倒下的那一格。
///
/// 用"必掉"的老鼠做断言（血肉 100% 1-2），把随机性排除在外。
#[test]
fn a_dead_monster_drops_loot_on_its_cell() {
    let (mut world, _player) = open_world();
    let rat = spawn_test_monster(&mut world, crate::monster::MonsterKindId::Rat, (6, 6), 10.0, 4.0);
    world.get_mut::<Health>(rat).unwrap().current = 0.0;

    let _ = world.run_system_once(crate::system::death::check_death_system);

    assert!(world.get_entity(rat).is_err(), "死者必须被回收");
    let drops: Vec<((usize, usize), ItemId, u32)> = {
        let mut query = world.query::<(&GroundItemAt, &GroundItem)>();
        query
            .iter(&world)
            .map(|(at, item)| (at.to_tuple(), item.stack.id, item.stack.count))
            .collect()
    };
    assert!(!drops.is_empty(), "老鼠必掉血肉");
    for (pos, id, count) in &drops {
        assert_eq!(*pos, (6, 6), "掉落必须落在尸体那一格");
        assert_eq!(*id, ItemId(10), "老鼠只掉生物血肉");
        assert!((1..=2).contains(count), "血肉数量 1-2，实得 {count}");
    }
}

/// 玩家死亡不掉落（也不发经验）——掉落逻辑只对怪物生效。
#[test]
fn the_players_death_does_not_drop_loot() {
    let (mut world, player) = open_world();
    world.get_mut::<Health>(player).unwrap().current = 0.0;

    let _ = world.run_system_once(crate::system::death::check_death_system);

    let mut query = world.query::<&GroundItem>();
    assert_eq!(query.iter(&world).count(), 0, "玩家死亡不该掉东西");
    assert!(
        world.resource::<crate::resources::TurnManager>().game_over,
        "玩家死亡要标记 game_over"
    );
}

/// 攻击链路：装备加成真的进了伤害计算（用固定暴击率 0 的场景读伤害）。
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

    // 空手打一次，读伤害。
    world.send_event(AttackIntentEvent {
        attacker: player,
        target,
    });
    let _ = world.run_system_once(crate::system::combat::resolve_attack_system);
    let unarmed = world
        .resource_mut::<Events<AttackEvent>>()
        .drain()
        .next()
        .expect("必须产生一次攻击事件")
        .damage;

    // 穿上锈铁剑（+3）再打一次。
    place(&mut world, (10, 10), ItemId(0), 1);
    pickup(&mut world);
    world.send_event(AttackIntentEvent {
        attacker: player,
        target,
    });
    let _ = world.run_system_once(crate::system::combat::resolve_attack_system);
    let armed = world
        .resource_mut::<Events<AttackEvent>>()
        .drain()
        .next()
        .expect("必须产生一次攻击事件")
        .damage;

    assert!(
        armed > unarmed,
        "拿剑的伤害必须更高（空手 {unarmed}，持剑 {armed}）"
    );
}
