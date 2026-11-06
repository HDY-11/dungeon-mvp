//! 属性词表（`rules::stat`）的用例：身份稳定、名字序列化、加载期拒绝未知项、区间可用。

use super::*;

#[test]
fn stat_ids_match_their_table_positions() {
    // 编号必须与表顺序一致：`STATS[i].id == StatId(i)`。
    // 这样"加一项属性"只需在表里追加一行，不会出现编号与顺序错位。
    for (index, def) in STATS.iter().enumerate() {
        assert_eq!(def.id, StatId(index as u16), "第 {index} 行的 id 与表位置不一致");
    }
    assert_eq!(STATS.len(), StatId::ALL.len());
}

#[test]
fn every_stat_has_a_unique_name_and_a_label() {
    for (index, def) in STATS.iter().enumerate() {
        assert!(!def.name.is_empty(), "{} 没有名字", def.name);
        assert!(!def.label.is_empty(), "{} 没有中文名", def.name);
        assert!(def.name.chars().all(|c| c.is_ascii_lowercase() || c == '_'), "{} 不是 snake_case", def.name);
        assert!(def.min < def.max, "{} 的区间反了", def.name);
        assert!(def.max.is_finite() && def.min.is_finite(), "{} 的区间必须是有限数", def.name);
        // 名字与编号都不允许重复。
        for other in &STATS[index + 1..] {
            assert_ne!(def.name, other.name, "属性名字重复：{}", def.name);
            assert_ne!(def.id, other.id, "属性编号重复：{}", def.name);
        }
    }
}

#[test]
fn the_known_speed_stats_are_ratios() {
    assert!(StatId::MOVE_SPEED.is_ratio());
    assert!(StatId::ATTACK_SPEED.is_ratio());
    assert!(!StatId::ATTACK.is_ratio());
    assert!(!StatId::DEFENSE.is_ratio());
}

#[test]
fn every_stat_round_trips_through_its_data_name() {
    for def in STATS {
        let json = serde_json::to_string(&def.id).expect("属性应当能序列化");
        assert_eq!(json, format!("\"{}\"", def.name));
        let parsed: StatId = serde_json::from_str(&json).expect("属性名应当能反序列化");
        assert_eq!(parsed, def.id);
        assert_eq!(stat_by_name(def.name), Some(def.id));
    }
}

#[test]
fn an_unknown_stat_name_is_rejected() {
    // 数据里写错名字必须炸在加载期，而不是静默变成某个默认属性。
    let error = serde_json::from_str::<StatId>("\"luck\"").expect_err("未知属性必须报错");
    let text = error.to_string();
    assert!(text.contains("未知属性"), "错误信息应说明未知属性：{text}");
    assert!(text.contains("luck"), "错误信息应带上写错的名字：{text}");
    assert!(text.contains("attack"), "错误信息应列出可用名字：{text}");
}

#[test]
fn unknown_ids_do_not_exist() {
    assert!(StatId::ATTACK.exists());
    assert!(!StatId(999).exists());
}
