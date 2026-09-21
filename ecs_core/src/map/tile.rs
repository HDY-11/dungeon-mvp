//! 地形的**种类与属性表**（`Tile` / `TileProps`）。
//!
//! 从 `map/mod.rs` 拆出（Phase E 的结构对齐，与 `map_gen.rs` 同级），
//! 目的是让"加一个地形变体要改哪几处"这件事变成**一处**（Phase H4 / ISSUES ECS31）。
//!
//! # 唯一数据源
//!
//! 地形的全部属性都写在 [`TILE_PROPS`] 这一张表里，包括序列化判别值。
//! `Serialize` / `Deserialize` / `glyph` / `walkable` / `blocks_vision` 全部**读表**，
//! 不再各自持有一份 `match`：
//!
//! | | H4 之前 | H4 之后 |
//! |---|---|---|
//! | 加一个变体要改 | `glyph` / `walkable` / `blocks_vision` / `From<u8>` / `Into<u8>` **五处** | 表里加**一行** |
//! | 漏改的后果 | 编译期就能报错（穷尽 match）→ 但改 5 处容易漏 | 表里少一行 → **穷举测试**立刻报错（见下） |
//! | 属性没有落点的新维度 | 只能继续往 5 个 match 里加 | 往 [`TileProps`] 加字段 |
//!
//! # 数据表的配套纪律（LESSONS `LECS22`）
//!
//! 换表的代价是**把"漏加一项"从编译期穷举检查推到了运行期静默**。所以这里配了
//! [`tests::table_covers_every_variant_and_round_trips`]：它**逐行枚举全表**并断言
//! 序列化/反序列化往返、判别值唯一、索引与判别值一致。漏一行、判别值重复、
//! 表与枚举顺序错位，都会在这里失败。

use serde::{Deserialize, Serialize};

/// Tile 使用自定义 Serde 以 u8 序列化（判别值见 [`TILE_PROPS`] 的 `id` 列）。
///
/// **兼容性约束（REFACTOR §10.7）：** 判别值是存档格式的一部分，
/// 新变体只能在末尾追加，不能插入或重排已有项。
///
/// 派生 `Ord` 只为测试快照排序。
///
/// **`#[repr(u8)]` + 声明顺序 = 序列化判别值**（见 [`Tile::id`]）：加变体只能在末尾追加，
/// 这条约束因此由语言保证，而不是靠手写 `match` 里的人工对齐。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum Tile {
    Wall,
    Floor,
    ShallowWater,
    DeepWater,
    Stalactite,
    Mycelium,
    FungalPatch,
    HangingVine,
    Sand,
    Seagrass,
    CoralReef,
}

/// 一种地形的全部静态属性。
///
/// **领域属性与显示属性同表但不同列**：`glyph` 是显示数据（长期应归属
/// `presentation` / `tui` 的 catalog，见 REFACTOR §10.7），放在这里只因为它是
/// "一个地形一份"的静态事实。新增**规则**属性（如 `move_cost`）请加领域列，
/// 不要把它读进渲染层。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TileProps {
    /// 该地形本身（表的第 i 行 = 判别值 i 的地形）。
    pub tile: Tile,
    /// 序列化判别值：**存档契约**，只能末尾追加。
    pub id: u8,
    /// 显示字形（显示数据，非领域契约）。
    pub glyph: char,
    /// 能否站立/走入。
    pub walkable: bool,
    /// 是否阻挡视线（FOV / LOS）。
    pub blocks_vision: bool,
    /// 移动代价倍率：**H8 预留的形状**，当前恒为 1.0。
    ///
    /// 本轮（H4）**只留位置，不填值、不接线**——移动 AV 仍然只由 `MoveSpeed` 决定
    /// （见 DESIGN DsnE8 ②）。具体数值口径属 GAME.md（待定）。
    /// 一旦接线，"无视地形减速"这类效果才有落点（ISSUES ECS27）。
    pub move_cost: f64,
}

/// 地形属性表：**唯一的种类→属性映射**，索引 = 判别值 = `tile as usize`。
///
/// 加一个地形变体 = 在 `Tile` 末尾加一个变体 + 在**本表末尾加一行**
/// （`id` 列填新值；`#[repr(u8)]` 会保证它等于"末尾追加"的那个数）。
/// 判别值必须保持 0..=10 连续（[`Tile::from_id`] 用索引查表）。
pub const TILE_PROPS: &[TileProps] = &[
    TileProps {
        tile: Tile::Wall,
        id: 0,
        glyph: '#',
        walkable: false,
        blocks_vision: true,
        move_cost: 1.0,
    },
    TileProps {
        tile: Tile::Floor,
        id: 1,
        glyph: '.',
        walkable: true,
        blocks_vision: false,
        move_cost: 1.0,
    },
    TileProps {
        tile: Tile::ShallowWater,
        id: 2,
        glyph: '~',
        walkable: true,
        blocks_vision: false,
        move_cost: 1.0,
    },
    TileProps {
        tile: Tile::DeepWater,
        id: 3,
        glyph: '≈',
        walkable: false,
        blocks_vision: false,
        move_cost: 1.0,
    },
    TileProps {
        tile: Tile::Stalactite,
        id: 4,
        glyph: '#',
        walkable: false,
        blocks_vision: true,
        move_cost: 1.0,
    },
    TileProps {
        tile: Tile::Mycelium,
        id: 5,
        glyph: ';',
        walkable: true,
        blocks_vision: false,
        move_cost: 1.0,
    },
    TileProps {
        tile: Tile::FungalPatch,
        id: 6,
        glyph: '♣',
        walkable: true,
        blocks_vision: false,
        move_cost: 1.0,
    },
    TileProps {
        tile: Tile::HangingVine,
        id: 7,
        glyph: '░',
        walkable: false,
        blocks_vision: true,
        move_cost: 1.0,
    },
    TileProps {
        tile: Tile::Sand,
        id: 8,
        glyph: ':',
        walkable: true,
        blocks_vision: false,
        move_cost: 1.0,
    },
    TileProps {
        tile: Tile::Seagrass,
        id: 9,
        glyph: ',',
        walkable: true,
        blocks_vision: false,
        move_cost: 1.0,
    },
    TileProps {
        tile: Tile::CoralReef,
        id: 10,
        glyph: '%',
        walkable: false,
        blocks_vision: true,
        move_cost: 1.0,
    },
];

/// 表的行数必须等于变体数；判别值必须与索引一一对应（由测试兜底）。
const _: () = assert!(TILE_PROPS.len() == 11);

impl Tile {
    /// 本变体在 [`TILE_PROPS`] 里的那一行。
    ///
    /// 用 `self as usize` 直接索引：判别值就是表索引，这是表能取代 5 处 match 的前提。
    pub const fn props(self) -> &'static TileProps {
        &TILE_PROPS[self as usize]
    }

    /// 序列化判别值（存档契约）。
    ///
    /// 直接取 `#[repr(u8)]` 判别值——**不再有一个存放判别值的 `match`**，
    /// 也不依赖表里 `id` 列的字面量。表里的 `id` 列因此只是"契约的书面记录"，
    /// 与实现是否一致由 [`tests::table_covers_every_variant_and_round_trips`] 看守。
    pub const fn id(self) -> u8 {
        self as u8
    }

    /// 显示字形（显示数据，见 [`TileProps`] 的说明）。
    pub const fn glyph(self) -> char {
        self.props().glyph
    }

    /// 能否站立/走入。
    pub const fn walkable(self) -> bool {
        self.props().walkable
    }

    /// 是否阻挡视线（FOV / LOS）。
    pub const fn blocks_vision(self) -> bool {
        self.props().blocks_vision
    }

    /// 移动代价倍率（H8 预留，当前恒为 1.0，尚无规则读取）。
    pub const fn move_cost(self) -> f64 {
        self.props().move_cost
    }

    /// 判别值 → 地形；未知值返回 `None`。
    ///
    /// 用**查表**而不是 `match`：加地形变体时这里不需要动
    /// （这正是 ECS31 要消除的那类中央分派）。
    pub const fn from_id(id: u8) -> Option<Tile> {
        let mut i = 0;
        while i < TILE_PROPS.len() {
            if TILE_PROPS[i].id == id {
                return Some(TILE_PROPS[i].tile);
            }
            i += 1;
        }
        None
    }
}

impl Serialize for Tile {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u8(self.id())
    }
}

impl<'de> Deserialize<'de> for Tile {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let v = u8::deserialize(deserializer)?;
        Tile::from_id(v).ok_or_else(|| {
            serde::de::Error::custom(format!("invalid Tile discriminant: {v}"))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 全表的变体清单：**加地形时必须在这里也加一项**，否则下面的穷举就漏了。
    ///
    /// 故意写成显式数组而不是"从表里推导"——推导出来的清单无法发现"表本身漏了一行"。
    const ALL_TILES: [Tile; 11] = [
        Tile::Wall,
        Tile::Floor,
        Tile::ShallowWater,
        Tile::DeepWater,
        Tile::Stalactite,
        Tile::Mycelium,
        Tile::FungalPatch,
        Tile::HangingVine,
        Tile::Sand,
        Tile::Seagrass,
        Tile::CoralReef,
    ];

    /// **数据表配套的穷举测试**（LESSONS LECS22）：漏一行、判别值重复、
    /// 索引与判别值错位，都必须在这里失败——因为换表之后"漏加一项"不再有编译期报错。
    #[test]
    fn table_covers_every_variant_and_round_trips() {
        assert_eq!(
            TILE_PROPS.len(),
            ALL_TILES.len(),
            "属性表与地形数量不一致：加地形必须同时加表里一行"
        );

        for (index, tile) in ALL_TILES.into_iter().enumerate() {
            let props = TILE_PROPS[index];
            assert_eq!(props.tile, tile, "表第 {index} 行的地形与枚举顺序不一致");
            assert_eq!(
                props.id,
                tile.id(),
                "{tile:?} 表里记的判别值与 #[repr(u8)] 判别值不一致"
            );
            assert_eq!(
                tile.id(),
                index as u8,
                "{tile:?} 的判别值必须等于表索引（新变体只能末尾追加）"
            );
            assert_eq!(
                tile.props().tile,
                tile,
                "{tile:?} 的 props() 必须取到表里自己那一行"
            );

            // 序列化往返：判别值就是存档里那个数。
            let serialized = serde_json::to_value(tile).expect("Tile 可序列化");
            assert_eq!(
                serialized.as_u64(),
                Some(u64::from(props.id)),
                "{tile:?} 的 serde 判别值必须等于表里的 id"
            );
            let back: Tile = serde_json::from_value(serialized).expect("Tile 可反序列化");
            assert_eq!(back, tile, "{tile:?} 序列化往返必须回到自己");
        }

        // 判别值唯一（重复会让 from_id 返回错的地形）。
        let mut ids: Vec<u8> = TILE_PROPS.iter().map(|props| props.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), TILE_PROPS.len(), "判别值必须两两不同");

        // 越界值不得 panic，且必须报错。
        assert_eq!(Tile::from_id(11), None, "未定义的判别值必须返回 None");
        assert_eq!(Tile::from_id(255), None);
        let bad: Result<Tile, _> = serde_json::from_value(serde_json::json!(200));
        assert!(bad.is_err(), "未定义的判别值必须反序列化失败");
    }

    /// H4 是"只碰形状"的一轮：新加的 `move_cost` 列必须**恒为 1.0**（= 不接线）。
    ///
    /// 这条断言的作用是防止"顺手填值"——数值口径属 GAME.md（待定），
    /// 一旦有人在这里填了非 1.0，必须同时改本条断言，从而无法悄悄改平衡。
    #[test]
    fn move_cost_is_still_a_reserved_slot() {
        for props in TILE_PROPS {
            assert_eq!(
                props.move_cost, 1.0,
                "{:?} 的 move_cost 在 H4 阶段必须保持预留值 1.0",
                props.tile
            );
        }
    }

    /// 判别值就是 H4 之前手写 `Serialize` 里那组数（0..=10）。
    ///
    /// 表的"分解"就是把这组数搬进 `id` 列，所以这里逐值钉住迁移前的结果：
    /// 一旦有人重排或改号，本用例先失败——**判别值是存档契约**（REFACTOR §10.7）。
    #[test]
    fn ids_match_the_pre_h4_serde_mapping() {
        let expected: [(Tile, u8); 11] = [
            (Tile::Wall, 0),
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
        for (tile, id) in expected {
            assert_eq!(tile.id(), id, "{tile:?} 的判别值变了（存档格式会变）");
        }
    }

    /// 可行走性与阻挡视线的既有口径（H4 之前散在 3 个 match 里，现在读同一张表）。
    ///
    /// 逐值钉住迁移前的结果，防止"搬家时抄错一格"。
    #[test]
    fn properties_match_the_pre_h4_values() {
        // (tile, glyph, walkable, blocks_vision)
        let expected: [(Tile, char, bool, bool); 11] = [
            (Tile::Wall, '#', false, true),
            (Tile::Floor, '.', true, false),
            (Tile::ShallowWater, '~', true, false),
            (Tile::DeepWater, '≈', false, false),
            (Tile::Stalactite, '#', false, true),
            (Tile::Mycelium, ';', true, false),
            (Tile::FungalPatch, '♣', true, false),
            (Tile::HangingVine, '░', false, true),
            (Tile::Sand, ':', true, false),
            (Tile::Seagrass, ',', true, false),
            (Tile::CoralReef, '%', false, true),
        ];
        for (tile, glyph, walkable, blocks_vision) in expected {
            assert_eq!(tile.glyph(), glyph, "{tile:?} 的字形变了");
            assert_eq!(tile.walkable(), walkable, "{tile:?} 的可行走性变了");
            assert_eq!(
                tile.blocks_vision(),
                blocks_vision,
                "{tile:?} 的阻挡视线变了"
            );
        }
    }
}
