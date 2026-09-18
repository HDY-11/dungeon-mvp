//! `super`（`monster`）的测试：模板速度不变量与迁移排序。
//!
//! 通过 `mod.rs` 末尾的 `#[cfg(test)] #[path = "monster_tests.rs"] mod tests;` 引入。

use super::*;
use crate::balance::{MAX_SPEED, MIN_SPEED};
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

