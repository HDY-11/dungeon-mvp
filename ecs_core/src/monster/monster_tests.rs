//! `super`（`monster`）的测试：模板速度不变量与迁移排序，
//! 以及 Phase H6/H7 归位后的**权重等价**与**池序固定**。

use super::*;
use crate::balance::{MAX_SPEED, MIN_SPEED};
use crate::map::MapKind;
/// 全部怪物种类（与 [`monster_template`] 的 match 一一对应）。
const ALL_KINDS: [MonsterKindId; 8] = [
    MonsterKindId::Rat,
    MonsterKindId::Scorpion,
    MonsterKindId::Goblin,
    MonsterKindId::Sporeling,
    MonsterKindId::MushroomGolem,
    MonsterKindId::CaveFish,
    MonsterKindId::CaveCrab,
    MonsterKindId::DeepEel,
];

const ALL_MAP_KINDS: [MapKind; 3] = [MapKind::Cavern, MapKind::LushCavern, MapKind::Undersea];

/// D5：每个模板都必须给出可用的速度，且落在 clamp 区间内。
///
/// 这条测试的防的是「速度漏填/填成 0」：`0.0` 会让 AV 变成 `inf`
/// （`clamp_speed` 会兜住，但那是兜底不是设计），漏填则会让怪物
/// 直接从 AI 查询里消失（LESSONS.md LECS21 那一类静默失败）。
#[test]
fn every_monster_template_has_usable_speeds() {
    for kind in ALL_KINDS {
        let speeds = monster_template(kind).speeds;
        for (name, speed) in [
            ("move_speed", speeds.move_speed),
            ("attack_speed", speeds.attack_speed),
        ] {
            assert!(
                speed.is_finite(),
                "{kind:?}.{name} 必须是有限值，实际 {speed}"
            );
            assert!(
                (MIN_SPEED..=MAX_SPEED).contains(&speed),
                "{kind:?}.{name}={speed} 必须落在 [{MIN_SPEED}, {MAX_SPEED}]"
            );
        }
    }
}

/// D5：迁移映射保住了旧敏捷的**排序**——怪与怪之间的快慢关系不变。
///
/// 这是「先按旧敏捷保行为」的可验证含义：新速度的排序必须与 GAME.md
/// 里旧敏捷的排序一致（洞穴鱼 14 > 深鳗 10 > 孢子怪 8 > 老鼠 5 >
/// 蝎子/蘑菇傀儡 4 > 哥布林/洞穴蟹 3）。
#[test]
fn template_speeds_preserve_legacy_agility_ordering() {
    let speed_of = |kind: MonsterKindId| monster_template(kind).speeds.move_speed;

    let fast = speed_of(MonsterKindId::CaveFish);
    let mid = speed_of(MonsterKindId::DeepEel);
    let slow = speed_of(MonsterKindId::Goblin);

    assert!(
        fast > mid && mid > slow,
        "旧敏捷 14 > 10 > 3 的排序必须保住：{fast} > {mid} > {slow}"
    );
    // 同旧敏捷的怪必须仍然同速。
    assert_eq!(
        speed_of(MonsterKindId::Scorpion),
        speed_of(MonsterKindId::MushroomGolem),
        "旧敏捷同为 4 的两种怪必须同速"
    );
    assert_eq!(
        speed_of(MonsterKindId::Goblin),
        speed_of(MonsterKindId::CaveCrab),
        "旧敏捷同为 3 的两种怪必须同速"
    );
    // 玩家（1.25）应当比老鼠(5)/蝎子(4)/哥布林(3) 快，仅慢于洞穴鱼，
    // 与旧设计意图「玩家比所有怪物快…除洞穴鱼」一致（旧敏捷 14 的洞穴鱼
    // 在旧口径下反应时更短，本来就略快于玩家）。
    let player = crate::balance::PLAYER_MOVE_SPEED;
    assert!(player > speed_of(MonsterKindId::Rat));
    assert!(player > speed_of(MonsterKindId::Goblin));
    assert!(player < fast, "洞穴鱼在旧口径下也比玩家快，迁移后应保持");
}

/// D5：模板数值与 `stats()` 输出一致（防止有人改了模板却忘了走 stats）。
#[test]
fn stats_expose_template_speeds() {
    for kind in ALL_KINDS {
        let template = monster_template(kind);
        let stats = template.stats(3);
        assert_eq!(stats.move_speed.0, template.speeds.move_speed, "{kind:?}");
        assert_eq!(
            stats.attack_speed.0, template.speeds.attack_speed,
            "{kind:?}"
        );
    }
}

// ── H7：生成权重归位的等价性 ─────────────────────────

/// H7 的验收核心：权重搬进模板后，**逐点位级相等**于归位前 `spawn.rs` 的三层 `match`。
///
/// 期望值不是"按新公式再推一遍"，而是把归位前的三个 `match` **原样抄下来**。
/// `assert_eq!` 对 `f64` 是位级比较，所以任何一处把
/// `.max(底)` / `.min(顶)` 改写成不完全等价的形态（例如把下降曲线的下限
/// 写成 `clamp(0.0, cap)`，或把 `30.0 - f*2.0` 写成 `30.0 - (2.0*f)` 之类
/// 看起来一样的式子）都会在这里暴露。
#[test]
fn spawn_weights_match_the_legacy_match_arms() {
    fn legacy(map_kind: MapKind, kind: MonsterKindId, floor: u32) -> f64 {
        let f = floor as f64;
        match map_kind {
            MapKind::Cavern => match kind {
                MonsterKindId::Rat => (30.0 - f * 2.0).max(0.5),
                MonsterKindId::Scorpion => (20.0 + f * 1.0).min(40.0),
                MonsterKindId::Goblin => (5.0 + f * 2.5).min(45.0),
                _ => 0.0,
            },
            MapKind::LushCavern => match kind {
                MonsterKindId::Sporeling => (30.0 - f * 2.0).max(0.5),
                MonsterKindId::MushroomGolem => (5.0 + f * 2.5).min(40.0),
                MonsterKindId::Rat => (12.0 - f * 1.0).max(0.5),
                MonsterKindId::Goblin => (5.0 + f * 1.5).min(30.0),
                _ => 0.0,
            },
            MapKind::Undersea => match kind {
                MonsterKindId::CaveFish => (30.0 - f * 2.0).max(0.5),
                MonsterKindId::CaveCrab => (12.0 + f * 1.5).min(35.0),
                MonsterKindId::DeepEel => (8.0 + f * 2.0).min(35.0),
                MonsterKindId::Scorpion => (8.0 + f * 0.5).min(20.0),
                _ => 0.0,
            },
        }
    }

    // 楼层取到 50：下降曲线早被下限截住，上升曲线早被上限截住，
    // 因此 floor 1/2/3/5/10/50 覆盖了"未截住 / 刚截住 / 深截住"三种形态。
    for map_kind in ALL_MAP_KINDS {
        for kind in ALL_KINDS {
            for floor in [1u32, 2, 3, 5, 10, 50] {
                assert_eq!(
                    monster_spawn_weight(map_kind, kind, floor),
                    legacy(map_kind, kind, floor),
                    "{map_kind:?} / {kind:?} / F{floor}"
                );
            }
        }
    }
}

/// 种类池的**顺序是行为契约**，不是整洁度问题。
///
/// `roll_one_kind` 按池序累加权重：池序一换，同一颗种子就生成不同的怪，
/// 而这类差异不会出现在任何数值表上（`spawn_weights_match_the_legacy_match_arms`
/// 全绿也照样发生）。所以这里把三条顺序逐字钉住——**改它等于改行为**，
/// 要走 RULE §八 的判定，不是顺手"按枚举顺序排一下"。
#[test]
fn kind_pools_keep_their_declared_order() {
    assert_eq!(
        kinds_for(MapKind::Cavern),
        &[
            MonsterKindId::Rat,
            MonsterKindId::Scorpion,
            MonsterKindId::Goblin
        ],
        "洞穴池顺序被改动（RNG 敏感）"
    );
    assert_eq!(
        kinds_for(MapKind::LushCavern),
        &[
            MonsterKindId::Sporeling,
            MonsterKindId::MushroomGolem,
            MonsterKindId::Rat,
            MonsterKindId::Goblin
        ],
        "繁茂池顺序被改动（RNG 敏感）：注意它不是枚举顺序，别按枚举重排"
    );
    assert_eq!(
        kinds_for(MapKind::Undersea),
        &[
            MonsterKindId::CaveFish,
            MonsterKindId::CaveCrab,
            MonsterKindId::DeepEel,
            MonsterKindId::Scorpion
        ],
        "地海池顺序被改动（RNG 敏感）"
    );
}

/// 池 ⟺ 权重条目：池里有的物种必须在**该地图**有权重，有权重的必须在池里。
///
/// 归位把这两份数据放进了不同文件（池在 `spawn.rs`，权重在 `template.rs`），
/// 所以需要一条测试来维持"加物种时两处都改"的约束。只往模板加权重而忘了进池
/// （或反之）原本是**静默**的：滚权重时该项不可达，或池里一项权重恒 0 却占着一个下标。
#[test]
fn pools_and_weights_cover_the_same_species() {
    for map_kind in ALL_MAP_KINDS {
        let pool = kinds_for(map_kind);
        // 池内不允许重复：重复项会让同一物种有两个下标、权重翻倍。
        for (i, kind) in pool.iter().enumerate() {
            assert!(
                !pool[..i].contains(kind),
                "{map_kind:?} 池里 {kind:?} 出现两次"
            );
        }
        for kind in ALL_KINDS {
            let in_pool = pool.contains(&kind);
            let has_entry = monster_template(kind)
                .spawn
                .iter()
                .any(|entry| entry.map_kind == map_kind);
            assert_eq!(
                in_pool, has_entry,
                "{map_kind:?} / {kind:?}: 在池中={in_pool}，有权重条目={has_entry}（两处必须一致）"
            );
        }
    }
}

/// 加权挑选的语义：累加序（即池序）决定落点，越界回落第 0 个。
///
/// 这是 `roll_one_kind` 里唯一的**行为**部分，抽成纯函数后可以用精确的 `roll`
/// 断言，不必依赖某一版 `rand` 的内部流——换 `rand` 版本不该改变"谁被选中"的规则。
#[test]
fn pick_by_weight_follows_accumulation_order() {
    let weights = [30.0, 20.0, 5.0];
    // 区间 [0,30) → 0，[30,50) → 1，[50,55) → 2。
    assert_eq!(pick_by_weight(&weights, 0.0), 0);
    assert_eq!(pick_by_weight(&weights, 29.999), 0);
    assert_eq!(pick_by_weight(&weights, 30.0), 1);
    assert_eq!(pick_by_weight(&weights, 49.999), 1);
    assert_eq!(pick_by_weight(&weights, 50.0), 2);
    assert_eq!(pick_by_weight(&weights, 54.999), 2);
    // 浮点累加误差导致 roll >= total：回落第 0 个（与归位前一致）。
    assert_eq!(pick_by_weight(&weights, 55.0), 0);
    // 前导零权重的物种占着下标但取不到：这正是"权重为 0 却在池里"的形态。
    assert_eq!(pick_by_weight(&[0.0, 10.0], 0.0), 1);
}

// ── H6：能力归位的等价性 ─────────────────────────────

/// H6：八种怪物拿到与归位前**完全同一套** `Can*` 标记。
///
/// 归位前是 `spawn_monsters_system` 里写死的四条 `insert`；归位后是模板字段。
/// 这条测试锁住"归位没有顺手增删能力"——能力集合本身是行为（`CanFlee` 决定怪会不会跑）。
#[test]
fn every_template_grants_the_legacy_ability_set() {
    let expected = [
        Ability::Chase,
        Ability::Flee,
        Ability::Wander,
        Ability::Wait,
    ];
    for kind in ALL_KINDS {
        let abilities = monster_template(kind).abilities;
        assert_eq!(abilities, &expected, "{kind:?} 的能力集变了（归位只换位置）");
    }
}

/// `attach_abilities` 是 `Ability` → 组件的唯一映射，空列表必须什么都不挂。
#[test]
fn attach_abilities_maps_each_ability_to_its_marker() {
    use crate::components::{CanChase, CanFlee, CanWait, CanWander};
    use bevy_ecs::prelude::*;
    use bevy_ecs::world::CommandQueue;

    let mut world = World::new();
    let mut queue = CommandQueue::default();
    let (full, stub) = {
        let mut commands = Commands::new(&mut queue, &world);
        let mut full = commands.spawn_empty();
        let full_id = full.id();
        attach_abilities(
            &mut full,
            &[
                Ability::Chase,
                Ability::Flee,
                Ability::Wander,
                Ability::Wait,
            ],
        );
        let mut stub = commands.spawn_empty();
        let stub_id = stub.id();
        attach_abilities(&mut stub, &[]);
        (full_id, stub_id)
    };
    queue.apply(&mut world);

    assert!(world.get::<CanChase>(full).is_some(), "Chase 未挂");
    assert!(world.get::<CanFlee>(full).is_some(), "Flee 未挂");
    assert!(world.get::<CanWander>(full).is_some(), "Wander 未挂");
    assert!(world.get::<CanWait>(full).is_some(), "Wait 未挂");
    assert!(
        world.get::<CanChase>(stub).is_none()
            && world.get::<CanFlee>(stub).is_none()
            && world.get::<CanWander>(stub).is_none()
            && world.get::<CanWait>(stub).is_none(),
        "空能力列表不该挂任何标记"
    );
}


