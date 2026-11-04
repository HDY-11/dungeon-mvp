//! 容器与槽位的测试（`item/container.rs`）。
//!
//! 全是纯数据操作（不碰 `World`），所以用例写得比 ECS 层细：过滤矩阵、容量、满的语义、
//! 原子操作失败的三种原因。容器是物品系统的地基，这里错一步，后面所有"物品去哪了"都不可查。

use super::*;

/// 一件"虚拟物品"的实体：本模块不关心它带什么组件，只关心身份。
fn item() -> Entity {
    let mut world = World::new();
    world.spawn_empty().id()
}

fn body() -> Container {
    Container::new(&BODY)
}

fn pack() -> Container {
    Container::new(&TRAVELER_PACK)
}

// ── 过滤矩阵 ─────────────────────────────────────────

/// `Any` 收一切（包括没有任何标签的物品）。
#[test]
fn any_slot_accepts_everything_including_untagged_items() {
    assert!(SlotAccept::Any.accepts(&[]));
    assert!(SlotAccept::Any.accepts(&[ItemTag::Weapon]));
    assert!(SlotAccept::Any.accepts(&[ItemTag::Ring, ItemTag::Feet]));
}

/// `Only` 是白名单求交：有任何一枚标签在名单里就成立。
#[test]
fn only_slots_intersect_with_the_item_tags() {
    let weapons = SlotAccept::Only(&[ItemTag::Weapon]);
    assert!(weapons.accepts(&[ItemTag::Weapon]));
    assert!(weapons.accepts(&[ItemTag::Weapon, ItemTag::Shield]), "多标签取交");
    assert!(!weapons.accepts(&[ItemTag::Ring]));
    assert!(!weapons.accepts(&[]), "没有标签的物品不能进有过滤的槽位");
}

/// 身体各部位的过滤器与物品标签一一对上（内容表的交叉校验）。
///
/// 这条守的是"表写错了不会有报错"那类问题：把「脚」槽写成 `Only(&[ItemTag::Head])`，
/// 编译与运行都不报错，只表现成"靴子永远装不上"。
#[test]
fn body_slots_accept_exactly_one_slot_family_each() {
    let expected = [
        (0usize, ItemTag::Head),
        (1, ItemTag::Chest),
        (2, ItemTag::Weapon),
        (3, ItemTag::Shield),
        (4, ItemTag::Feet),
        (5, ItemTag::Ring),
        (6, ItemTag::Pack),
    ];
    for (index, tag) in expected {
        let slot = &BODY.slots[index];
        assert!(
            slot.accept.accepts(&[tag]),
            "第 {index} 槽「{}」必须接受 {:?}",
            slot.name,
            tag
        );
        // 只接受这一族：换一枚别的标签就应当被拒。
        let other = if tag == ItemTag::Ring {
            ItemTag::Weapon
        } else {
            ItemTag::Ring
        };
        assert!(
            !slot.accept.accepts(&[other]),
            "第 {index} 槽「{}」不该接受 {:?}",
            slot.name,
            other
        );
    }
}

/// 每一枚标签都至少被一个槽位接受（否则这件东西永远装不上，且没有任何报错）。
#[test]
fn every_tag_has_a_slot_that_accepts_it() {
    let all = [
        ItemTag::Head,
        ItemTag::Chest,
        ItemTag::Weapon,
        ItemTag::Shield,
        ItemTag::Feet,
        ItemTag::Ring,
        ItemTag::Pack,
    ];
    for tag in all {
        let accepted_anywhere = BODY
            .slots
            .iter()
            .any(|slot| slot.accept.accepts(&[tag]));
        assert!(accepted_anywhere, "{:?} 没有任何槽位接受它", tag);
    }
}

// ── 容量与「满」 ─────────────────────────────────────

/// 空容器：所有槽位都空，`free_slot_for` 给出**第一个**可用的槽。
#[test]
fn an_empty_body_offers_the_first_matching_slot() {
    let body = body();
    assert_eq!(body.used(), 0);
    assert_eq!(body.len(), BODY.len());
    assert_eq!(body.free_slot_for(&[ItemTag::Weapon]), Some(2));
    assert_eq!(body.free_slot_for(&[ItemTag::Head]), Some(0));
    // 行囊的格子什么都能接，所以第一个空位就是 0。
    assert_eq!(pack().free_slot_for(&[ItemTag::Weapon]), Some(0));
}

/// 「满」是**相对于物品**的：主手被占之后，剑满了、头盔没满。
#[test]
fn fullness_is_relative_to_what_you_try_to_put_in() {
    let mut body = body();
    let sword = item();
    body.insert(2, sword, &[ItemTag::Weapon]).expect("主手空着");

    assert!(body.is_full_for(&[ItemTag::Weapon]), "主手只有一格，已被占");
    assert!(!body.is_full_for(&[ItemTag::Head]), "头还空着");
}

/// 无标签的物品进不了有过滤的槽位，因此对身体而言永远是"满"的。
#[test]
fn untagged_items_can_never_enter_body_slots() {
    let body = body();
    assert_eq!(body.free_slot_for(&[]), None);
    assert!(body.is_full_for(&[]));
    // 但行囊（Any）收得下。
    assert_eq!(pack().free_slot_for(&[]), Some(0));
}

/// 行囊满了（12 格全占）之后 `free_slot_for` 返回 `None`。
#[test]
fn a_full_pack_has_no_free_slot() {
    let mut pack = pack();
    for index in 0..pack.len() {
        pack.insert(index, item(), &[ItemTag::Ring]).expect("空格");
    }
    assert_eq!(pack.used(), pack.len());
    assert!(pack.is_full_for(&[ItemTag::Weapon]));
    assert_eq!(pack.free_slot_for(&[]), None);
}

// ── 原子操作 ─────────────────────────────────────────

/// 插入成功会记下"谁在第几格"，取出会清空并返回槽位号。
#[test]
fn insert_then_remove_round_trips() {
    let mut body = body();
    let helmet = item();
    assert_eq!(body.insert(0, helmet, &[ItemTag::Head]), Ok(0));
    assert_eq!(body.slot(0), Some(helmet));
    assert!(body.contains(helmet));
    assert_eq!(body.slot_of(helmet), Some(0));
    assert_eq!(body.used(), 1);

    assert_eq!(body.remove(helmet), Some(0));
    assert!(!body.contains(helmet));
    assert_eq!(body.used(), 0);
    assert_eq!(body.remove(helmet), None, "重复取出返回 None 而不是 panic");
}

/// 三种失败原因各一条：越界、已占、过滤。
#[test]
fn insert_reports_the_three_failure_modes() {
    let mut body = body();
    let first = item();
    let second = item();

    assert_eq!(
        body.insert(BODY.len(), first, &[ItemTag::Head]),
        Err(InsertError::NoSuchSlot(BODY.len()))
    );

    body.insert(0, first, &[ItemTag::Head]).expect("头空着");
    assert_eq!(
        body.insert(0, second, &[ItemTag::Head]),
        Err(InsertError::Occupied)
    );

    assert_eq!(
        body.insert(2, second, &[ItemTag::Head]),
        Err(InsertError::Filtered),
        "头盔不能进主手"
    );
    // 失败的插入不得留下痕迹。
    assert_eq!(body.slot(2), None);
    assert_eq!(body.used(), 1);
}

/// `iter` 按槽位顺序给出非空项（UI 依赖稳定顺序，不能是"插入顺序"）。
#[test]
fn iteration_follows_slot_order_not_insertion_order() {
    let mut body = body();
    let helmet = item();
    let sword = item();
    body.insert(2, sword, &[ItemTag::Weapon]).expect("主手");
    body.insert(0, helmet, &[ItemTag::Head]).expect("头");

    let order: Vec<(usize, Entity)> = body.iter().collect();
    assert_eq!(order, vec![(0, helmet), (2, sword)]);
}

/// `take_slot` 直接拿第 N 格，越界返回 `None`。
#[test]
fn take_slot_is_index_based() {
    let mut body = body();
    let ring = item();
    body.insert(5, ring, &[ItemTag::Ring]).expect("手指");
    assert_eq!(body.take_slot(5), Some(ring));
    assert_eq!(body.take_slot(5), None);
    assert_eq!(body.take_slot(BODY.len() + 3), None);
}

/// 槽位容量目前恒为 1（形状留了字段，数值没放开）。
///
/// 等到引入"重量/堆叠"，这条用例会连同 `SlotDef.capacity` 的语义一起改——它是那道改动
/// 的**入口断言**：现在谁把容量写成 2 而没有配套的 `Amount` 组件，这条立刻红。
#[test]
fn slot_capacity_is_one_for_now() {
    for def in BODY
        .slots
        .iter()
        .chain(TRAVELER_PACK.slots.iter())
        .chain(CORPSE.slots.iter())
    {
        assert_eq!(
            def.capacity, 1,
            "槽位「{}」的容量放开了，但还没有堆叠（Amount）支持",
            def.name
        );
    }
}

/// 布局表的自检：名字非空、槽位不重名（重名会让日志与 UI 分不清是哪一个）。
#[test]
fn container_defs_are_well_formed() {
    for def in [&BODY, &TRAVELER_PACK, &CORPSE] {
        let mut names: Vec<&str> = Vec::new();
        for slot in def.slots {
            assert!(!slot.name.is_empty(), "{} 有空名字的槽位", def.name);
            assert!(!names.contains(&slot.name), "{} 有重名槽位", def.name);
            names.push(slot.name);
        }
        assert!(!def.is_empty(), "{} 是 0 格容器", def.name);
    }
}

/// 尸体没有过滤（DsnE18 第②片）：谁身上的东西都装得下。
#[test]
fn the_corpse_layout_accepts_anything() {
    for slot in CORPSE.slots {
        assert!(
            matches!(slot.accept, SlotAccept::Any),
            "尸体槽位「{}」不该有过滤",
            slot.name
        );
    }
    let mut corpse = Container::new(&CORPSE);
    let whatever = item();
    assert_eq!(
        corpse.insert(0, whatever, &[]).expect("战利品无 tag 也得放得下"),
        0
    );
}
