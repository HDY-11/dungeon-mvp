pub mod components;
pub mod fov;
pub mod items;
pub mod map_gen;
pub mod monster_def;
pub mod ops;
pub mod pathfinding;
// pub mod pathfinding; // 已移除（find_path 未使用）
pub mod resources;
pub mod systems;


pub use components::*;
pub use items::*;
pub use ops::*;
pub use monster_def::*;
// pub use pathfinding::*; // 已移除
pub use resources::*;
pub use systems::*;

use rand::Rng;

pub use components::EntityName;

// ── 行动成本常量（已移除—硬编码在 action.rs 中） ──

// ── 常量 ──────────────────────────────────────────────

pub const MAP_WIDTH: usize = 80;
pub const MAP_HEIGHT: usize = 60;

/// 视窗尺寸（渲染时以玩家为中心截取此大小的区域）
pub const VIEWPORT_WIDTH: usize = 40;
pub const VIEWPORT_HEIGHT: usize = 20;

// ── Tile ──────────────────────────────────────────────

/// Tile 用自定义 Serde 以 u8 序列化（兼容 bincode Vec<u8> 存档格式）。
/// 数值映射：Wall=0, Floor=1, ShallowWater=2, DeepWater=3, Stalactite=4。
/// 如需添加新变体请在末尾追加，不要插入或重排已有项——否则旧存档无声损坏。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tile {
    Wall,
    Floor,
    ShallowWater,   // ~ 浅蓝，可行走
    DeepWater,      // ≈ 深蓝，不可行走
    Stalactite,     // # 黄色，不可行走（装饰性墙壁）
}

impl serde::Serialize for Tile {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u8(match self {
            Tile::Wall => 0,
            Tile::Floor => 1,
            Tile::ShallowWater => 2,
            Tile::DeepWater => 3,
            Tile::Stalactite => 4,
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
            _ => Err(serde::de::Error::custom(format!("invalid Tile discriminant: {}", v))),
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
        }
    }

    /// 是否可通行（用于移动逻辑）
    pub fn walkable(self) -> bool {
        matches!(self, Tile::Floor | Tile::ShallowWater)
    }

    /// 是否阻挡视线（用于 FOV）
    pub fn blocks_vision(self) -> bool {
        matches!(self, Tile::Wall | Tile::Stalactite)
    }

    /// 渲染前景色（正常可见时）
    pub fn fg_color(self) -> (u8, u8, u8) {
        match self {
            Tile::Wall => (180, 180, 180),
            Tile::Stalactite => (255, 255, 0),
            Tile::Floor => (200, 200, 200),
            Tile::ShallowWater => (220, 240, 255),
            Tile::DeepWater => (80, 150, 220),
        }
    }

    /// 渲染背景色（仅水域有特殊背景）
    pub fn bg_color(self) -> Option<(u8, u8, u8)> {
        match self {
            Tile::ShallowWater => Some((120, 190, 250)),
            Tile::DeepWater => Some((20, 60, 140)),
            _ => None,
        }
    }
}

// ── Room ──────────────────────────────────────────────

/// 房间形状
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RoomShape {
    Rect,
    Circle,
    Diamond,
    Ellipse,
}
impl Default for RoomShape { fn default() -> Self { Self::Rect } }

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Room {
    pub x: usize, pub y: usize,
    pub w: usize, pub h: usize,
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

    /// 生成房间占据的所有格子坐标
    pub fn tiles(&self) -> Vec<(usize, usize)> {
        match self.shape {
            RoomShape::Rect => {
                let mut t = Vec::with_capacity(self.w * self.h);
                for y in self.y..self.y + self.h {
                    for x in self.x..self.x + self.w {
                        if x < MAP_WIDTH && y < MAP_HEIGHT { t.push((x, y)); }
                    }
                }
                t
            }
            RoomShape::Circle => {
                let mut t = Vec::new();
                let cx = self.x as isize; let cy = self.y as isize; let r = self.w as isize;
                for dy in -r..=r {
                    for dx in -r..=r {
                        if dx * dx + dy * dy <= r * r {
                            let x = cx + dx; let y = cy + dy;
                            if x >= 0 && x < MAP_WIDTH as isize && y >= 0 && y < MAP_HEIGHT as isize {
                                t.push((x as usize, y as usize));
                            }
                        }
                    }
                }
                t
            }
            RoomShape::Diamond => {
                let mut t = Vec::new();
                let cx = self.x as isize; let cy = self.y as isize; let r = self.w as isize;
                for dy in -r..=r {
                    for dx in -r..=r {
                        if dx.abs() + dy.abs() <= r {
                            let x = cx + dx; let y = cy + dy;
                            if x >= 0 && x < MAP_WIDTH as isize && y >= 0 && y < MAP_HEIGHT as isize {
                                t.push((x as usize, y as usize));
                            }
                        }
                    }
                }
                t
            }
            RoomShape::Ellipse => {
                let mut t = Vec::new();
                let cx = self.x as isize; let cy = self.y as isize;
                let a = self.w.max(1) as f32; let b = self.h.max(1) as f32;
                for dy in -(self.h as isize)..=self.h as isize {
                    for dx in -(self.w as isize)..=self.w as isize {
                        let fx = dx as f32 / a;
                        let fy = dy as f32 / b;
                        if fx * fx + fy * fy <= 1.0 {
                            let x = cx + dx; let y = cy + dy;
                            if x >= 0 && x < MAP_WIDTH as isize && y >= 0 && y < MAP_HEIGHT as isize {
                                t.push((x as usize, y as usize));
                            }
                        }
                    }
                }
                t
            }
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
    pub fn new() -> Self { Self { tiles: [[Tile::Wall; MAP_WIDTH]; MAP_HEIGHT], rooms: Vec::new() } }

    /// 使用 terrain-forge 生成洞穴地图。
    /// 保留 `self.rooms` 给外部使用（玩家出生、怪物/物品放置）。
    /// 扩展：将来可按 biome 切换算法（bsp / cellular / room_accretion）。
    pub fn generate(&mut self, rng: &mut impl Rng) {
        use rand::RngExt;
        let seed: u64 = rng.random();
        self.tiles = [[Tile::Wall; MAP_WIDTH]; MAP_HEIGHT];
        self.rooms.clear();

        // ── 用 terrain-forge 生成洞穴 ──
        let mut grid = terrain_forge::Grid::new(MAP_WIDTH, MAP_HEIGHT);
        // room_accretion: Brogue 风格的有机洞穴（缩小模板尺寸）
        let mut params = terrain_forge::ops::Params::new();
        params.insert("templates".into(), serde_json::json!([
            {"Rectangle": {"min": 4, "max": 9}},
            {"Circle": {"min_radius": 2, "max_radius": 4}},
            {"Blob": {"size": 6, "smoothing": 2}},
        ]));
        params.insert("max_rooms".into(), serde_json::json!(14));
        params.insert("loop_chance".into(), serde_json::json!(0.05));
        if terrain_forge::ops::generate("room_accretion", &mut grid, Some(seed), Some(&params)).is_err() {
            // 如果算法失败，回退到简单的噪声+CA
            let _ = terrain_forge::ops::generate("cellular", &mut grid, Some(seed.wrapping_add(1)), None);
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
                x: MAP_WIDTH / 2 - 5, y: MAP_HEIGHT / 2 - 5,
                w: 10, h: 10, shape: RoomShape::Rect,
            });
            for y in self.rooms[0].y..self.rooms[0].y + self.rooms[0].h {
                for x in self.rooms[0].x..self.rooms[0].x + self.rooms[0].w {
                    if x < MAP_WIDTH && y < MAP_HEIGHT { self.tiles[y][x] = Tile::Floor; }
                }
            }
        }

        // ── 环境修饰：水域 + 钟乳石 + 连通性 ──
        crate::map_gen::generate_water(self, rng, seed.wrapping_add(100));
        crate::map_gen::carve_expand(self, rng, seed.wrapping_add(150));
        crate::map_gen::generate_stalactites(self, rng, seed.wrapping_add(200));
        crate::map_gen::ensure_connectivity(self, rng, seed.wrapping_add(300));
        crate::map_gen::ensure_spawn_accessible(self, rng, seed.wrapping_add(350));
    }

    // ── 工具函数 ──

    /// 统计地图中某种 tile 的数量
    pub fn count_tile(&self, tile: Tile) -> usize {
        self.tiles.iter().flatten().filter(|&&t| t == tile).count()
    }



    /// 统计 (x,y) 的 8 邻域中某种 tile 的数量
    pub fn count_neighbor_tile(&self, x: usize, y: usize, tile: Tile) -> usize {
        let mut n = 0;
        for dy in -1isize..=1 {
            for dx in -1isize..=1 {
                if dx == 0 && dy == 0 { continue; }
                let nx = x.wrapping_add_signed(dx);
                let ny = y.wrapping_add_signed(dy);
                if nx < MAP_WIDTH && ny < MAP_HEIGHT && self.tiles[ny][nx] == tile {
                    n += 1;
                }
            }
        }
        n
    }

    /// 判断 (x,y) 是否远离出生点（不破坏玩家出生区）
    pub fn is_away_from_spawn(&self, x: usize, y: usize, min_dist: usize) -> bool {
        self.rooms.first().map(|r| {
            let (sx, sy) = r.center();
            x.abs_diff(sx) + y.abs_diff(sy) >= min_dist
        }).unwrap_or(true)
    }

    /// L 形走廊（保留供外部调用/未来使用）
    pub fn carve_corridor(&mut self, from: (usize, usize), to: (usize, usize)) {
        let (x1, y1) = from; let (x2, y2) = to;
        for x in x1.min(x2)..=x1.max(x2) { self.tiles[y1][x] = Tile::Floor; }
        for y in y1.min(y2)..=y1.max(y2) { self.tiles[y][x2] = Tile::Floor; }
    }

    /// 获取玩家出生点（第一个房间的中心）。
    /// 若中心点不可行走，螺旋向外搜索最近的可行走格作为兜底。
    pub fn spawn_point(&self) -> (usize, usize) {
        let center = self.rooms.first().map(|r| r.center()).unwrap_or((MAP_WIDTH / 2, MAP_HEIGHT / 2));
        if center.0 < MAP_WIDTH && center.1 < MAP_HEIGHT && self.tiles[center.1][center.0].walkable() {
            return center;
        }
        // 螺旋搜索：从半径 1 开始向外扩展，找最近的可行走格
        for r in 1..=20 {
            for dy in -(r as isize)..=r as isize {
                for dx in -(r as isize)..=r as isize {
                    if dx == 0 && dy == 0 { continue; }
                    let nx = center.0.wrapping_add_signed(dx);
                    let ny = center.1.wrapping_add_signed(dy);
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
        self.rooms.iter()
            .map(|r| (r.center(), r.center().0.abs_diff(px) + r.center().1.abs_diff(py)))
            .max_by_key(|(_, d)| *d)
            .map(|(p, _)| p)
    }

    pub fn render(&self) -> Vec<String> {
        (0..MAP_HEIGHT).map(|row| (0..MAP_WIDTH).map(|col| self.tiles[row][col].glyph()).collect()).collect()
    }
}
impl Default for Map { fn default() -> Self { Self::new() } }


