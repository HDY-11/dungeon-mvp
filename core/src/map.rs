//! 地图、Tile 与地图类型定义。

use bevy_ecs::prelude::*;
use rand::Rng;
use serde::{Deserialize, Serialize};

pub const MAP_WIDTH: usize = 80;
pub const MAP_HEIGHT: usize = 60;

/// 地图类型：骨架统一为 room_accretion 洞穴，环境修饰差异化。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub enum MapKind {
    Cavern,
    LushCavern,
    Undersea,
}

/// 环境修饰参数。
#[derive(Clone, Copy, Debug)]
pub struct MapEnvParams {
    pub water_seed_per_mille: u32,
    pub water_seed_min_dist: u32,
    pub water_expand_bonus: f64,
    pub shallow_expand_chance: u32,
    pub obstacle_chance: u32,
    pub decor_chance: u32,
}

impl MapKind {
    pub const fn env_params(self) -> MapEnvParams {
        match self {
            MapKind::Cavern => MapEnvParams {
                water_seed_per_mille: 2,
                water_seed_min_dist: 3,
                water_expand_bonus: 0.0,
                shallow_expand_chance: 10,
                obstacle_chance: 7,
                decor_chance: 0,
            },
            MapKind::LushCavern => MapEnvParams {
                water_seed_per_mille: 0,
                water_seed_min_dist: 3,
                water_expand_bonus: -0.02,
                shallow_expand_chance: 2,
                obstacle_chance: 10,
                decor_chance: 25,
            },
            MapKind::Undersea => MapEnvParams {
                water_seed_per_mille: 20,
                water_seed_min_dist: 1,
                water_expand_bonus: 0.08,
                shallow_expand_chance: 18,
                obstacle_chance: 3,
                decor_chance: 15,
            },
        }
    }
}

/// 由 `(seed, floor)` 确定性派生地图类型；F1 固定 Cavern。
pub fn map_kind_for(seed: u64, floor: u32) -> MapKind {
    if floor <= 1 {
        return MapKind::Cavern;
    }
    let h = seed
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(floor as u64 * 31) as usize;
    match h % 3 {
        0 => MapKind::Cavern,
        1 => MapKind::LushCavern,
        _ => MapKind::Undersea,
    }
}

/// Tile 使用自定义 Serde 以 u8 序列化。
/// 数值映射：Wall=0, Floor=1, ShallowWater=2, DeepWater=3, Stalactite=4,
/// Mycelium=5, FungalPatch=6, HangingVine=7, Sand=8, Seagrass=9, CoralReef=10。
/// 新变体只能在末尾追加，不能插入或重排已有项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

impl serde::Serialize for Tile {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u8(match self {
            Tile::Wall => 0,
            Tile::Floor => 1,
            Tile::ShallowWater => 2,
            Tile::DeepWater => 3,
            Tile::Stalactite => 4,
            Tile::Mycelium => 5,
            Tile::FungalPatch => 6,
            Tile::HangingVine => 7,
            Tile::Sand => 8,
            Tile::Seagrass => 9,
            Tile::CoralReef => 10,
        })
    }
}

impl<'de> serde::Deserialize<'de> for Tile {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let v = u8::deserialize(deserializer)?;
        match v {
            0 => Ok(Tile::Wall),
            1 => Ok(Tile::Floor),
            2 => Ok(Tile::ShallowWater),
            3 => Ok(Tile::DeepWater),
            4 => Ok(Tile::Stalactite),
            5 => Ok(Tile::Mycelium),
            6 => Ok(Tile::FungalPatch),
            7 => Ok(Tile::HangingVine),
            8 => Ok(Tile::Sand),
            9 => Ok(Tile::Seagrass),
            10 => Ok(Tile::CoralReef),
            _ => Err(serde::de::Error::custom(format!(
                "invalid Tile discriminant: {v}"
            ))),
        }
    }
}

impl Tile {
    pub const fn glyph(self) -> char {
        match self {
            Tile::Wall | Tile::Stalactite => '#',
            Tile::Floor => '.',
            Tile::ShallowWater => '~',
            Tile::DeepWater => '≈',
            Tile::Mycelium => ';',
            Tile::FungalPatch => '♣',
            Tile::HangingVine => '░',
            Tile::Sand => ':',
            Tile::Seagrass => ',',
            Tile::CoralReef => '%',
        }
    }

    pub const fn walkable(self) -> bool {
        matches!(
            self,
            Tile::Floor
                | Tile::ShallowWater
                | Tile::Mycelium
                | Tile::FungalPatch
                | Tile::Sand
                | Tile::Seagrass
        )
    }

    pub const fn blocks_vision(self) -> bool {
        matches!(
            self,
            Tile::Wall | Tile::Stalactite | Tile::HangingVine | Tile::CoralReef
        )
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RoomShape {
    #[default]
    Rect,
    Circle,
    Diamond,
    Ellipse,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Room {
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
    #[serde(default)]
    pub shape: RoomShape,
}

impl Room {
    pub const fn center(&self) -> (usize, usize) {
        match self.shape {
            RoomShape::Rect => (self.x + self.w / 2, self.y + self.h / 2),
            _ => (self.x, self.y),
        }
    }
}

#[derive(Resource)]
pub struct Map {
    pub tiles: [[Tile; MAP_WIDTH]; MAP_HEIGHT],
    pub rooms: Vec<Room>,
}

impl Map {
    pub fn new() -> Self {
        Self {
            tiles: [[Tile::Wall; MAP_WIDTH]; MAP_HEIGHT],
            rooms: Vec::new(),
        }
    }

    /// 使用 terrain-forge 生成洞穴地图。
    pub fn generate(&mut self, kind: MapKind, rng: &mut impl Rng) {
        crate::map_gen::generate_map(self, kind, rng);
    }

    pub fn count_tile(&self, tile: Tile) -> usize {
        self.tiles.iter().flatten().filter(|&&t| t == tile).count()
    }

    pub fn is_away_from_spawn(&self, x: usize, y: usize, min_dist: usize) -> bool {
        self.rooms
            .first()
            .map(|r| {
                let (sx, sy) = r.center();
                x.abs_diff(sx) + y.abs_diff(sy) >= min_dist
            })
            .unwrap_or(true)
    }

    pub fn spawn_point(&self) -> (usize, usize) {
        let center = self
            .rooms
            .first()
            .map(Room::center)
            .unwrap_or((MAP_WIDTH / 2, MAP_HEIGHT / 2));
        self.nearest_walkable(center.0, center.1)
    }

    pub fn nearest_walkable(&self, x: usize, y: usize) -> (usize, usize) {
        if x < MAP_WIDTH && y < MAP_HEIGHT && self.tiles[y][x].walkable() {
            return (x, y);
        }
        for r in 1..=MAP_WIDTH.max(MAP_HEIGHT) as isize {
            for dy in -r..=r {
                for dx in -r..=r {
                    if dx == 0 && dy == 0 {
                        continue;
                    }
                    let nx = x.wrapping_add_signed(dx);
                    let ny = y.wrapping_add_signed(dy);
                    if nx < MAP_WIDTH && ny < MAP_HEIGHT && self.tiles[ny][nx].walkable() {
                        return (nx, ny);
                    }
                }
            }
        }
        (MAP_WIDTH / 2, MAP_HEIGHT / 2)
    }

    pub fn farthest_room_from(&self, point: (usize, usize)) -> Option<(usize, usize)> {
        let (px, py) = point;
        self.rooms
            .iter()
            .map(|r| {
                (
                    r.center(),
                    r.center().0.abs_diff(px) + r.center().1.abs_diff(py),
                )
            })
            .max_by_key(|(_, d)| *d)
            .map(|(p, _)| p)
    }
}

impl Default for Map {
    fn default() -> Self {
        Self::new()
    }
}
