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

    /// 随机生成地图：矩形/圆形/菱形/椭圆房间混合 + 走廊连接
    pub fn generate(&mut self, rng: &mut impl Rng) {
        use rand::RngExt;
        self.tiles = [[Tile::Wall; MAP_WIDTH]; MAP_HEIGHT];
        self.rooms.clear();
        let target = rng.random_range(8..=14);

        for _ in 0..target * 6 {
            if self.rooms.len() >= target { break; }

            // 加权随机选形状（矩形 45%，圆形 25%，菱形 20%，椭圆 10%）
            let shape = match rng.random_range(0..100) {
                0..=44 => RoomShape::Rect,
                45..=69 => RoomShape::Circle,
                70..=89 => RoomShape::Diamond,
                _ => RoomShape::Ellipse,
            };

            let (x, y, w, h) = match shape {
                RoomShape::Rect => {
                    let w = rng.random_range(5..=12);
                    let h = rng.random_range(4..=8);
                    let x = rng.random_range(1..(MAP_WIDTH - w - 1));
                    let y = rng.random_range(1..(MAP_HEIGHT - h - 1));
                    (x, y, w, h)
                }
                RoomShape::Circle | RoomShape::Diamond => {
                    let r = rng.random_range(4..=8);
                    let x = rng.random_range(r..(MAP_WIDTH - r));
                    let y = rng.random_range(r..(MAP_HEIGHT - r));
                    (x, y, r, r)
                }
                RoomShape::Ellipse => {
                    let a = rng.random_range(4..=8);
                    let b = rng.random_range(3..=6);
                    let x = rng.random_range(a..(MAP_WIDTH - a));
                    let y = rng.random_range(b..(MAP_HEIGHT - b));
                    (x, y, a, b)
                }
            };

            let room = Room { x, y, w, h, shape };
            let tiles = room.tiles();

            if !self.overlaps_existing(&tiles, 1) {
                for &(tx, ty) in &tiles {
                    self.tiles[ty][tx] = Tile::Floor;
                }
                self.rooms.push(room);
            }
        }

        // 走廊连接相邻房间（从第一个房间依次连接）
        for i in 1..self.rooms.len() {
            let prev = self.rooms[i - 1].center();
            let curr = self.rooms[i].center();
            self.carve_corridor(prev, curr);
        }
    }

    /// 格子级碰撞检测：检查 tiles 是否与已有 Floor 重叠（含 margin 格边距）
    fn overlaps_existing(&self, tiles: &[(usize, usize)], margin: usize) -> bool {
        for &(x, y) in tiles {
            if self.tiles[y][x] == Tile::Floor { return true; }
            for dy in -(margin as isize)..=margin as isize {
                for dx in -(margin as isize)..=margin as isize {
                    if dx == 0 && dy == 0 { continue; }
                    let nx = x.wrapping_add_signed(dx);
                    let ny = y.wrapping_add_signed(dy);
                    if nx < MAP_WIDTH && ny < MAP_HEIGHT && self.tiles[ny][nx] == Tile::Floor {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// L 形走廊
    fn carve_corridor(&mut self, from: (usize, usize), to: (usize, usize)) {
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
