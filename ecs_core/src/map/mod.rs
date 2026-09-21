//! 地图、Tile 与地图类型定义。

use bevy_ecs::prelude::*;
use rand::Rng;
use serde::{Deserialize, Serialize};

pub mod map_gen;
pub mod tile;

pub use map_gen::*;
// `Tile` / `TileProps` / `TILE_PROPS` 从 `tile` 子模块转出，公共路径 `map::Tile`
// 与拆分前完全一致（`map/mod.rs` 只留地图相关的类型与 `Map` 本身）。
pub use tile::{TILE_PROPS, Tile, TileProps};

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
        crate::map::map_gen::generate_map(self, kind, rng);
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
