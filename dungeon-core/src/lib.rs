pub mod action_types;
pub mod api;
pub mod components;
pub mod global;
pub mod items;
pub mod monster_def;
pub mod ops;
// pub mod pathfinding; // 已移除（find_path 未使用）
pub mod resources;
pub mod systems;


pub use action_types::*;
pub use api::*;
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
pub const MAP_HEIGHT: usize = 40;

/// 视窗尺寸（渲染时以玩家为中心截取此大小的区域）
pub const VIEWPORT_WIDTH: usize = 40;
pub const VIEWPORT_HEIGHT: usize = 20;

// ── Tile ──────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tile { Wall, Floor }

impl Tile {
    pub fn char(self) -> char {
        match self { Tile::Wall => '#', Tile::Floor => '.' }
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
        // room_accretion: Brogue 风格的有机洞穴
        if terrain_forge::ops::generate("room_accretion", &mut grid, Some(seed), None).is_err() {
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
        self.rooms = self.detect_cave_regions(12);
    }

    /// 从洞穴 Floor 中检测连通区域，返回按大小降序排列的房间列表。
    /// `max_rooms` 限制最大房间数。返回的房间用 Room 近似（bounding box）。
    /// 扩展：后续可加入区域分类（入口区/战斗区/Boss 区）。
    pub fn detect_cave_regions(&self, max_rooms: usize) -> Vec<Room> {
        let mut visited = [[false; MAP_WIDTH]; MAP_HEIGHT];
        let mut regions: Vec<Vec<(usize, usize)>> = Vec::new();

        for sy in 0..MAP_HEIGHT {
            for sx in 0..MAP_WIDTH {
                if visited[sy][sx] || self.tiles[sy][sx] != Tile::Floor { continue; }

                // BFS 找连通区
                let mut stack = vec![(sx, sy)];
                let mut region = Vec::new();
                while let Some((x, y)) = stack.pop() {
                    if visited[y][x] { continue; }
                    visited[y][x] = true;
                    region.push((x, y));

                    for (ny, nx) in [(y.wrapping_sub(1), x), (y + 1, x), (y, x.wrapping_sub(1)), (y, x + 1)] {
                        if nx < MAP_WIDTH && ny < MAP_HEIGHT && !visited[ny][nx] && self.tiles[ny][nx] == Tile::Floor {
                            stack.push((nx, ny));
                        }
                    }
                }

                // 过滤极小区域（噪声碎片），合并到结果
                if region.len() >= 6 {
                    regions.push(region);
                }
            }
        }

        // 按大小降序
        regions.sort_by(|a, b| b.len().cmp(&a.len()));

        regions.into_iter().take(max_rooms).map(|r| {
            let min_x = r.iter().map(|&(x, _)| x).min().unwrap_or(0);
            let max_x = r.iter().map(|&(x, _)| x).max().unwrap_or(0);
            let min_y = r.iter().map(|&(_, y)| y).min().unwrap_or(0);
            let max_y = r.iter().map(|&(_, y)| y).max().unwrap_or(0);
            Room {
                x: min_x, y: min_y,
                w: max_x - min_x + 1, h: max_y - min_y + 1,
                shape: RoomShape::Rect, // 统一用矩形近似
            }
        }).collect()
    }

    /// L 形走廊（保留供外部调用/未来使用）
    pub fn carve_corridor(&mut self, from: (usize, usize), to: (usize, usize)) {
        let (x1, y1) = from; let (x2, y2) = to;
        for x in x1.min(x2)..=x1.max(x2) { self.tiles[y1][x] = Tile::Floor; }
        for y in y1.min(y2)..=y1.max(y2) { self.tiles[y][x2] = Tile::Floor; }
    }

    pub fn render(&self) -> Vec<String> {
        (0..MAP_HEIGHT).map(|row| (0..MAP_WIDTH).map(|col| self.tiles[row][col].char()).collect()).collect()
    }
}
impl Default for Map { fn default() -> Self { Self::new() } }

#[cfg(test)]
mod tests;
