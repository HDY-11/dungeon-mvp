//! 后端外观目录的**契约侧**一半：把 `ecs_core` 的枚举映射成语义 key。
//!
//! 后端的另一半（`tui::TuiCatalog` / 未来 `gpu::GpuCatalog`）负责把
//! [`VisualKey`] 变成真正的 glyph / 颜色 / 纹理。两者故意分开：
//!
//! - 这里只回答「这是什么」，住在本 crate（认识 `ecs_core`）；
//! - 那里只回答「长什么样」，住在后端（不认识 `ecs_core`）。
//!
//! 于是换后端不用碰 `ecs_core`，改地形样式不用碰集成层。

use render_api::VisualKey;

/// 地形外观目录：`ecs_core::Tile` ↔ `VisualKey::Tile(u16)`。
///
/// `u16` 是**不透明映射**，只要求「同一版本内稳定」。当前实现直接沿用
/// `ecs_core::Tile` 的 serde 判别值（0..=10），理由有二：
///
/// 1. 那份判别值本身已经有「只能末尾追加」的兼容性约束（REFACTOR §10.7），
///    复用它等于复用了同一条约束，不会引入第二套需要同步的编号；
/// 2. 后端做 golden 测试时，编号与存档里的 tile 编号一致，排查问题少一层转换。
///
/// 代价是「契约编号跟着 `core` 的 serde 编号走」——如果哪天地形要重排判别值
/// （被 §10.7 禁止），这里必须同时改。该耦合是**有意**的，故写在此处。
pub type TileCatalog = fn(ecs_core::Tile) -> u16;

/// 把地形映射成语义 key。
pub fn map_tile(tile: ecs_core::Tile) -> VisualKey {
    VisualKey::Tile(tile_id(tile))
}

/// 地形的稳定编号（见 [`TileCatalog`] 的说明）。
pub fn tile_id(tile: ecs_core::Tile) -> u16 {
    // `Tile` 的判别值定义在 `ecs_core/src/map/mod.rs` 的手写 `Serialize` 里；
    // 这里用一次穷尽 match 固化下来，避免依赖「枚举声明顺序恰好等于 serde 顺序」。
    match tile {
        ecs_core::Tile::Wall => 0,
        ecs_core::Tile::Floor => 1,
        ecs_core::Tile::ShallowWater => 2,
        ecs_core::Tile::DeepWater => 3,
        ecs_core::Tile::Stalactite => 4,
        ecs_core::Tile::Mycelium => 5,
        ecs_core::Tile::FungalPatch => 6,
        ecs_core::Tile::HangingVine => 7,
        ecs_core::Tile::Sand => 8,
        ecs_core::Tile::Seagrass => 9,
        ecs_core::Tile::CoralReef => 10,
    }
}

/// 把怪物种类映射成语义 key。
///
/// 编号来源同 [`tile_id`]：`MonsterKindId` 的声明顺序（该枚举同样受
/// 「只能末尾追加」约束）。
pub fn map_monster_kind(kind: ecs_core::MonsterKindId) -> VisualKey {
    VisualKey::Monster(monster_kind_id(kind))
}

/// 怪物种类的稳定编号。
pub fn monster_kind_id(kind: ecs_core::MonsterKindId) -> u16 {
    use ecs_core::MonsterKindId as K;
    match kind {
        K::Rat => 0,
        K::Scorpion => 1,
        K::Goblin => 2,
        K::Sporeling => 3,
        K::MushroomGolem => 4,
        K::CaveFish => 5,
        K::CaveCrab => 6,
        K::DeepEel => 7,
    }
}

/// 语义类别 → 该类别下第一个可用的 key。
///
/// 给「只知道类别、不知道具体种类」的调用方用（例如日志里要画一个怪物图标）。
/// 找不到合适 key 时返回 [`VisualKey::Unknown`]，后端会走 fallback 外观。
pub fn map_visual_category(category: render_api::VisualCategory) -> VisualKey {
    use render_api::VisualCategory as C;
    match category {
        C::Player => VisualKey::Player,
        C::Monster => VisualKey::Monster(0),
        C::Stairs => VisualKey::Stairs,
        C::Item => VisualKey::Item(0),
        C::Tile => VisualKey::Tile(0),
        C::Effect => VisualKey::Effect(0),
        C::Ui => VisualKey::UiIcon(render_api::UiIcon::Unknown),
        C::Unknown => VisualKey::Unknown(0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ecs_core::{MonsterKindId, Tile};

    /// 地形的 serde 判别值是存档格式的一部分（只能末尾追加），
    /// 契约编号跟着它走，所以这里把两者钉在一起：
    /// 万一有人重排 `Tile` 的手写 `Serialize`，这条测试会先炸。
    #[test]
    fn tile_ids_match_serde_discriminants() {
        let all = [
            (Tile::Wall, 0u8),
            (Tile::Floor, 1),
            (Tile::ShallowWater, 2),
            (Tile::DeepWater, 3),
            (Tile::Stalactite, 4),
            (Tile::Mycelium, 5),
            (Tile::FungalPatch, 6),
            (Tile::HangingVine, 7),
            (Tile::Sand, 8),
            (Tile::Seagrass, 9),
            (Tile::CoralReef, 10),
        ];
        for (tile, expected) in all {
            let serialized = serde_json::to_value(tile).expect("Tile 可序列化");
            assert_eq!(
                serialized.as_u64(),
                Some(u64::from(expected)),
                "{tile:?} 的 serde 判别值变了"
            );
            assert_eq!(
                tile_id(tile),
                u16::from(expected),
                "{tile:?} 的契约编号必须与 serde 判别值一致"
            );
            assert_eq!(map_tile(tile), VisualKey::Tile(u16::from(expected)));
        }
    }

    /// 怪物种类编号必须两两不同，且覆盖全部 8 种。
    #[test]
    fn monster_kind_ids_are_unique_and_cover_all_kinds() {
        let all = [
            MonsterKindId::Rat,
            MonsterKindId::Scorpion,
            MonsterKindId::Goblin,
            MonsterKindId::Sporeling,
            MonsterKindId::MushroomGolem,
            MonsterKindId::CaveFish,
            MonsterKindId::CaveCrab,
            MonsterKindId::DeepEel,
        ];
        let mut ids: Vec<u16> = all.iter().map(|kind| monster_kind_id(*kind)).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), all.len(), "怪物种类编号必须唯一");
        assert_eq!(ids, (0..8).collect::<Vec<u16>>(), "编号应当是 0..8 的排列");

        assert_eq!(
            map_monster_kind(MonsterKindId::CaveFish),
            VisualKey::Monster(5)
        );
    }

    #[test]
    fn category_fallback_returns_matching_key_kind() {
        use render_api::VisualCategory as C;
        for category in [
            C::Player,
            C::Monster,
            C::Stairs,
            C::Item,
            C::Tile,
            C::Effect,
            C::Ui,
            C::Unknown,
        ] {
            assert_eq!(
                map_visual_category(category).category(),
                category,
                "{category:?} 的 fallback key 类别必须一致"
            );
        }
    }
}
