//! 核心领域层：地图、组件、物品、属性公式、视野、寻路等纯数据/纯查询逻辑。
//!
//! 该层不依赖 dungeon-action / dungeon-world，保持可独立测试。

pub mod color;
pub mod components;
pub mod ext;
pub mod fov;
pub mod items;
pub mod logger;
pub mod map_gen;
pub mod monster_def;
pub mod ops;
pub mod pathfinding;
pub mod resources;
pub mod systems;

use serde::{Deserialize, Serialize};

pub use components::*;
pub use ext::*;
pub use items::*;
pub use logger::*;
pub use monster_def::*;
pub use ops::*;
// pub use pathfinding::*; // 已移除
pub use resources::*;
pub use systems::*;

pub use log::*;

use rand::Rng;

pub use components::EntityName;

// ── 行动成本常量（已移除—硬编码在 action.rs 中） ──

// ── 常量 ──────────────────────────────────────────────

pub const MAP_WIDTH: usize = 80;
pub const MAP_HEIGHT: usize = 60;

/// 视窗尺寸（渲染时以玩家为中心截取此大小的区域）
pub const VIEWPORT_WIDTH: usize = 40;
pub const VIEWPORT_HEIGHT: usize = 20;

// ── 地图类型（Dsn24 多类型地图） ──────────────────────

/// 地图类型：骨架均为 room_accretion 洞穴，环境修饰差异化。
/// F1 固定 Cavern（新手层）；之后由 (seed, floor) 确定性派生。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub enum MapKind {
    /// 标准洞穴（现有地形）
    Cavern,
    /// 繁茂洞穴：真菌植物生态，水极少，垂藤替代钟乳石
    LushCavern,
    /// 地海：大面积水域，珊瑚礁替代钟乳石
    Undersea,
}

/// 环境修饰参数（Dsn24）：generate_water / generate_stalactites / 装饰方块共用
#[derive(Clone, Copy, Debug)]
pub struct MapEnvParams {
    /// 深水种子概率（‰；Cavern 基准 2‰）
    pub water_seed_per_mille: u32,
    /// 深水种子与房间中心的最小曼哈顿距离（Cavern=3 保持原样；Undersea=1 让水域贴近活动区）
    pub water_seed_min_dist: u32,
    /// 深水扩散概率增量（相对基准 0.25 - 面积×0.002 的额外加成）
    pub water_expand_bonus: f32,
    /// 浅水扩散概率（%）
    pub shallow_expand_chance: u32,
    /// 障碍密度（%）：Cavern/LushCavern 为钟乳石或垂藤，Undersea 为珊瑚礁
    pub obstacle_chance: u32,
    /// 装饰方块概率（%）：LushCavern 菌丝/蘑菇丛，Undersea 沙岸/海草
    pub decor_chance: u32,
}

impl MapKind {
    /// 环境修饰参数表（Dsn24；数值均为 [⃞试调]）
    pub fn env_params(self) -> MapEnvParams {
        match self {
            // 标准洞穴：现有参数（水 2‰、钟乳石 7%）
            MapKind::Cavern => MapEnvParams {
                water_seed_per_mille: 2,
                water_seed_min_dist: 3,
                water_expand_bonus: 0.0,
                shallow_expand_chance: 10,
                obstacle_chance: 7,
                decor_chance: 0,
            },
            // 繁茂洞穴：几乎无水，垂藤密集，菌丝铺地
            MapKind::LushCavern => MapEnvParams {
                water_seed_per_mille: 0,
                water_seed_min_dist: 3,
                water_expand_bonus: -0.02,
                shallow_expand_chance: 2,
                obstacle_chance: 10,
                decor_chance: 25,
            },
            // 地海：水域显著扩大（种子 20‰≈10 倍 + 贴近房间 + 扩散加成），障碍为珊瑚礁
            // [⃞试调: 8‰ 在真实地图（~400 Floor）期望种子仅 3，方差大易出现 0 水域；20‰ 期望 8 种子]
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

/// 由 (seed, floor) 确定性派生地图类型（Dsn24）。
/// F1 固定 Cavern（新手层）；之后按哈希取模三分均分 [⃞试调]。
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

// ── Tile ──────────────────────────────────────────────

/// Tile 用自定义 Serde 以 u8 序列化（兼容 bincode Vec<u8> 存档格式）。
/// 数值映射：Wall=0, Floor=1, ShallowWater=2, DeepWater=3, Stalactite=4,
/// Mycelium=5, FungalPatch=6, HangingVine=7, Sand=8, Seagrass=9, CoralReef=10。
/// 如需添加新变体请在末尾追加，不要插入或重排已有项——否则旧存档无声损坏。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tile {
    Wall,
    Floor,
    ShallowWater, // ~ 浅蓝，可行走
    DeepWater,    // ≈ 深蓝，不可行走
    Stalactite,   // # 黄色，不可行走（装饰性墙壁）
    // Dsn24 多类型地图方块（5-10；繁茂洞穴 / 地海）
    Mycelium,    // ; 菌丝地面，可行走（繁茂洞穴）
    FungalPatch, // ♣ 蘑菇丛，可行走（繁茂洞穴）
    HangingVine, // ░ 垂藤，不可行走挡视线（繁茂洞穴，替代钟乳石）
    Sand,        // : 沙岸，可行走（地海）
    Seagrass,    // , 海草，可行走（地海）
    CoralReef,   // % 珊瑚礁，不可行走挡视线（地海，替代钟乳石）
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
                "invalid Tile discriminant: {}",
                v
            ))),
        }
    }
}

impl Tile {
    pub fn glyph(self) -> char {
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

    /// 是否可通行（用于移动逻辑）
    pub fn walkable(self) -> bool {
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

    /// 是否阻挡视线（用于 FOV）
    pub fn blocks_vision(self) -> bool {
        matches!(
            self,
            Tile::Wall | Tile::Stalactite | Tile::HangingVine | Tile::CoralReef
        )
    }

    /// 渲染前景色（正常可见时）
    pub fn fg_color(self) -> (u8, u8, u8) {
        match self {
            Tile::Wall => (180, 180, 180),
            Tile::Stalactite => (255, 255, 0),
            Tile::Floor => (200, 200, 200),
            Tile::ShallowWater => (220, 240, 255),
            Tile::DeepWater => (80, 150, 220),
            Tile::Mycelium => (140, 190, 120),
            Tile::FungalPatch => (90, 220, 110),
            Tile::HangingVine => (40, 130, 70),
            Tile::Sand => (230, 215, 160),
            Tile::Seagrass => (70, 170, 110),
            Tile::CoralReef => (240, 150, 90),
        }
    }

    /// 渲染背景色（所有 Tile 均有背景色，为半块字符叠加铺路）
    pub fn bg_color(self) -> Option<(u8, u8, u8)> {
        match self {
            Tile::Wall => Some((50, 50, 60)),
            Tile::Floor => Some((20, 22, 25)),
            Tile::ShallowWater => Some((120, 190, 250)),
            Tile::DeepWater => Some((20, 60, 140)),
            Tile::Stalactite => Some((60, 55, 20)),
            Tile::Mycelium => Some((25, 45, 25)),
            Tile::FungalPatch => Some((20, 55, 25)),
            Tile::HangingVine => Some((15, 40, 25)),
            Tile::Sand => Some((60, 55, 35)),
            Tile::Seagrass => Some((25, 55, 35)),
            Tile::CoralReef => Some((70, 35, 25)),
        }
    }
}

// ── Room ──────────────────────────────────────────────

/// 房间形状
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RoomShape {
    #[default]
    Rect,
    Circle,
    Diamond,
    Ellipse,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Room {
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
    #[serde(default)]
    pub shape: RoomShape,
}
impl Room {
    /// 房间中心坐标（矩形=左上角+半宽/半高，其他形状=x,y 就是中心）
    pub fn center(&self) -> (usize, usize) {
        match self.shape {
            RoomShape::Rect => (self.x + self.w / 2, self.y + self.h / 2),
            _ => (self.x, self.y),
        }
    }
}

// ── Map（ECS Resource）────────────────────────────────

#[derive(bevy_ecs::prelude::Resource)]
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
    /// 保留 `self.rooms` 给外部使用（玩家出生、怪物/物品放置）。
    /// 扩展：将来可按 biome 切换算法（bsp / cellular / room_accretion）。
    pub fn generate(&mut self, kind: MapKind, rng: &mut impl Rng) {
        use rand::RngExt;
        let seed: u64 = rng.random();
        self.tiles = [[Tile::Wall; MAP_WIDTH]; MAP_HEIGHT];
        self.rooms.clear();

        // ── 用 terrain-forge 生成洞穴 ──
        let mut grid = terrain_forge::Grid::new(MAP_WIDTH, MAP_HEIGHT);
        // room_accretion: Brogue 风格的有机洞穴（缩小模板尺寸）
        let mut params = terrain_forge::ops::Params::new();
        params.insert(
            "templates".into(),
            serde_json::json!([
                {"Rectangle": {"min": 4, "max": 9}},
                {"Circle": {"min_radius": 2, "max_radius": 4}},
                {"Blob": {"size": 6, "smoothing": 2}},
            ]),
        );
        params.insert("max_rooms".into(), serde_json::json!(14));
        params.insert("loop_chance".into(), serde_json::json!(0.05));
        if terrain_forge::ops::generate("room_accretion", &mut grid, Some(seed), Some(&params))
            .is_err()
        {
            // 如果算法失败，回退到简单的噪声+CA
            let _ = terrain_forge::ops::generate(
                "cellular",
                &mut grid,
                Some(seed.wrapping_add(1)),
                None,
            );
        }

        // ── 转换到我的 Tile ──
        for y in 0..MAP_HEIGHT {
            for x in 0..MAP_WIDTH {
                self.tiles[y][x] = if grid[(x, y)].is_floor() {
                    Tile::Floor
                } else {
                    Tile::Wall
                };
            }
        }

        // ── 从洞穴中检测连通区域 → 房间列表（用于怪物/物品放置） ──
        self.rooms = crate::map_gen::detect_cave_regions(self, 12);
        if self.rooms.is_empty() {
            // 极端情况：无足够大区域，放一个默认房间在地图中央
            self.rooms.push(Room {
                x: MAP_WIDTH / 2 - 5,
                y: MAP_HEIGHT / 2 - 5,
                w: 10,
                h: 10,
                shape: RoomShape::Rect,
            });
            for y in self.rooms[0].y..self.rooms[0].y + self.rooms[0].h {
                for x in self.rooms[0].x..self.rooms[0].x + self.rooms[0].w {
                    if x < MAP_WIDTH && y < MAP_HEIGHT {
                        self.tiles[y][x] = Tile::Floor;
                    }
                }
            }
        }

        // ── 环境修饰：水域 + 障碍 + 地形装饰 + 连通性（Dsn24: 参数按 MapKind 分派） ──
        let params = kind.env_params();
        crate::map_gen::generate_water(self, rng, seed.wrapping_add(100), &params);
        crate::map_gen::carve_expand(self, rng, seed.wrapping_add(150));
        crate::map_gen::generate_obstacles(self, rng, seed.wrapping_add(200), &params, kind);
        crate::map_gen::generate_terrain_decor(self, rng, seed.wrapping_add(250), &params, kind);
        crate::map_gen::ensure_connectivity(self, rng, seed.wrapping_add(300));
        crate::map_gen::ensure_spawn_accessible(self, rng, seed.wrapping_add(350));
    }

    // ── 工具函数 ──

    /// 统计地图中某种 tile 的数量
    pub fn count_tile(&self, tile: Tile) -> usize {
        self.tiles.iter().flatten().filter(|&&t| t == tile).count()
    }

    /// 判断 (x,y) 是否远离出生点（不破坏玩家出生区）
    pub fn is_away_from_spawn(&self, x: usize, y: usize, min_dist: usize) -> bool {
        self.rooms
            .first()
            .map(|r| {
                let (sx, sy) = r.center();
                x.abs_diff(sx) + y.abs_diff(sy) >= min_dist
            })
            .unwrap_or(true)
    }

    /// 获取玩家出生点（第一个房间的中心）。
    /// 若中心点不可行走，螺旋向外搜索最近的可行走格作为兜底。
    pub fn spawn_point(&self) -> (usize, usize) {
        let center = self
            .rooms
            .first()
            .map(|r| r.center())
            .unwrap_or((MAP_WIDTH / 2, MAP_HEIGHT / 2));
        self.nearest_walkable(center.0, center.1)
    }

    /// 从 (x, y) 螺旋向外搜索最近的可行走格（G22：楼梯/物品等落点兜底）。
    /// 极端情况（全图无可走格）返回地图中心。
    pub fn nearest_walkable(&self, x: usize, y: usize) -> (usize, usize) {
        if x < MAP_WIDTH && y < MAP_HEIGHT && self.tiles[y][x].walkable() {
            return (x, y);
        }
        // 螺旋搜索：从半径 1 开始向外扩展，找最近的可行走格
        for r in 1..=40 {
            for dy in -(r as isize)..=r as isize {
                for dx in -(r as isize)..=r as isize {
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
        // 极端情况：整个地图没有可行走格（不应发生）
        (MAP_WIDTH / 2, MAP_HEIGHT / 2)
    }

    /// 找一个距给定点最远的房间中心（用于楼梯放置）
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
