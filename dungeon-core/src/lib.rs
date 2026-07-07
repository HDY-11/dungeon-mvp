pub mod action_types;
pub mod components;
pub mod items;
pub mod monster_def;
pub mod ops;
// pub mod pathfinding; // 已移除（find_path 未使用）
pub mod resources;
pub mod systems;


pub use action_types::*;
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
        self.rooms = self.detect_cave_regions(12);
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
        self.generate_water(rng, seed.wrapping_add(100));
        self.carve_expand(rng, seed.wrapping_add(150));
        self.generate_stalactites(rng, seed.wrapping_add(200));
        self.ensure_connectivity(rng, seed.wrapping_add(300));
        self.ensure_spawn_accessible(rng, seed.wrapping_add(350));
    }

    /// 用噪声在水域放置深水种子 → 元胞扩散（75% 浅水 / 25% 深水）
    pub fn generate_water(&mut self, _rng: &mut impl Rng, seed: u64) {
        use rand::{RngExt, SeedableRng};
        let mut rng2 = rand::rngs::SmallRng::seed_from_u64(seed);

        // ── Phase 1: 噪声深水种子（2% 概率） ──
        for y in 0..MAP_HEIGHT {
            for x in 0..MAP_WIDTH {
                if self.tiles[y][x] == Tile::Floor && rng2.random_range(0..1000) < 20
                    && self.is_away_from_rooms(x, y, 6)
                {
                    self.tiles[y][x] = Tile::DeepWater;
                }
            }
        }

        // ── Phase 2: 深水→扩散（8 方向独立判定） ──
        let deep_count = self.count_tile(Tile::DeepWater) as f32;
        let expand_chance = (0.25 - deep_count * 0.002).max(0.02);
        {
            let mut next = self.tiles;
            for y in 0..MAP_HEIGHT {
                for x in 0..MAP_WIDTH {
                    if self.tiles[y][x] != Tile::DeepWater { continue; }
                    for (dx, dy) in &[(-1,-1),(0,-1),(1,-1),(-1,0),(1,0),(-1,1),(0,1),(1,1)] {
                        let nx = x.wrapping_add_signed(*dx);
                        let ny = y.wrapping_add_signed(*dy);
                        if nx >= MAP_WIDTH || ny >= MAP_HEIGHT || self.tiles[ny][nx] != Tile::Floor { continue; }
                        next[ny][nx] = if rng2.random_range(0.0..1.0) < expand_chance {
                            Tile::DeepWater
                        } else {
                            Tile::ShallowWater
                        };
                    }
                }
            }
            self.tiles = next;
        }

        // ── Phase 3: 浅水→扩散（8 方向 10% 概率） ──
        {
            let mut next = self.tiles;
            for y in 0..MAP_HEIGHT {
                for x in 0..MAP_WIDTH {
                    if self.tiles[y][x] != Tile::ShallowWater { continue; }
                    for (dx, dy) in &[(-1,-1),(0,-1),(1,-1),(-1,0),(1,0),(-1,1),(0,1),(1,1)] {
                        let nx = x.wrapping_add_signed(*dx);
                        let ny = y.wrapping_add_signed(*dy);
                        if nx >= MAP_WIDTH || ny >= MAP_HEIGHT { continue; }
                        // 浅水不覆盖深水和墙体
                        if next[ny][nx] != Tile::Floor { continue; }
                        if rng2.random_range(0..100) < 10 {
                            next[ny][nx] = Tile::ShallowWater;
                        }
                    }
                }
            }
            self.tiles = next;
        }
    }

    /// 元胞扩张：对每格墙，若邻接可行走格则 25% 概率挖成 Floor（拓宽通道）
    pub fn carve_expand(&mut self, _rng: &mut impl Rng, seed: u64) {
        use rand::{RngExt, SeedableRng};
        let mut rng2 = rand::rngs::SmallRng::seed_from_u64(seed);
        let mut next = self.tiles;
        for y in 0..MAP_HEIGHT {
            for x in 0..MAP_WIDTH {
                if self.tiles[y][x].walkable() { continue; }
                let walkable_near = self.count_walkable_neighbors(x, y);
                if walkable_near >= 1 && rng2.random_range(0..100) < 25 {
                    next[y][x] = Tile::Floor;
                }
            }
        }
        self.tiles = next;
    }

    /// 在每个房间中随机放置钟乳石（# 黄色，约 7% 密度）
    pub fn generate_stalactites(&mut self, _rng: &mut impl Rng, seed: u64) {
        use rand::{RngExt, SeedableRng};
        let mut rng2 = rand::rngs::SmallRng::seed_from_u64(seed);
        for room in &self.rooms.clone() {
            for y in room.y..room.y + room.h {
                for x in room.x..room.x + room.w {
                    if self.tiles[y][x] == Tile::Floor && rng2.random_range(0..100) < 7 {
                        self.tiles[y][x] = Tile::Stalactite;
                    }
                }
            }
        }
    }

    /// 检查最大连通区是否覆盖大部分可行走区域；若不连通，用醉汉游走挖 2-3 条通道
    pub fn ensure_connectivity(&mut self, _rng: &mut impl Rng, seed: u64) {
        use rand::{RngExt, SeedableRng};
        let mut rng2 = rand::rngs::SmallRng::seed_from_u64(seed);
        let regions = self.collect_walkable_regions();
        if regions.len() <= 1 { return; }

        let passages = rng2.random_range(2..=3);
        for p in 0..passages {
            let from_idx = p % regions.len();
            let to_idx = (p + 1) % regions.len();
            if from_idx >= regions.len() || to_idx >= regions.len() { break; }

            let from = regions[from_idx][regions[from_idx].len() / 2];
            let to = regions[to_idx][0];

            // 醉汉游走（宽度 2）
            let (mut cx, mut cy) = (from.0 as isize, from.1 as isize);
            let (tx, ty) = (to.0 as isize, to.1 as isize);
            for _ in 0..500 {
                if (cx - tx).abs() + (cy - ty).abs() < 3 { break; }
                let dx = if rng2.random_range(0..100) < 50 { (tx - cx).signum() } else { rng2.random_range(-1i32..2) as isize };
                let dy = if rng2.random_range(0..100) < 50 { (ty - cy).signum() } else { rng2.random_range(-1i32..2) as isize };
                cx = (cx + dx).clamp(0, MAP_WIDTH as isize - 1);
                cy = (cy + dy).clamp(0, MAP_HEIGHT as isize - 1);
                // 挖 2 格宽通道
                for (ox, oy) in &[(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let (ux, uy) = ((cx + ox) as usize, (cy + oy) as usize);
                    if ux < MAP_WIDTH && uy < MAP_HEIGHT && !self.tiles[uy][ux].walkable() {
                        self.tiles[uy][ux] = Tile::Floor;
                    }
                }
            }
        }
    }

    /// 确保出生点不会被封闭在墙壁中。
    /// 如果出生点 8 方向都没有可行走格，就用醉汉游走凿一条路出去。
    pub fn ensure_spawn_accessible(&mut self, _rng: &mut impl Rng, seed: u64) {
        use rand::{RngExt, SeedableRng};
        if self.rooms.is_empty() { return; }
        let (sx, sy) = self.rooms[0].center();
        // 检查 8 方向是否有 walkable
        for dy in -1isize..=1 {
            for dx in -1isize..=1 {
                if dx == 0 && dy == 0 { continue; }
                let nx = sx.wrapping_add_signed(dx);
                let ny = sy.wrapping_add_signed(dy);
                if nx < MAP_WIDTH && ny < MAP_HEIGHT && self.tiles[ny][nx].walkable() {
                    return; // 已经有出口
                }
            }
        }
        // 被困住了 → 醉汉游走
        let mut rng2 = rand::rngs::SmallRng::seed_from_u64(seed);
        let (mut cx, mut cy) = (sx as isize, sy as isize);
        for _ in 0..100 {
            let dx = rng2.random_range(-1i32..2) as isize;
            let dy = rng2.random_range(-1i32..2) as isize;
            if dx == 0 && dy == 0 { continue; }
            cx = (cx + dx).clamp(0, MAP_WIDTH as isize - 1);
            cy = (cy + dy).clamp(0, MAP_HEIGHT as isize - 1);
            let (ux, uy) = (cx as usize, cy as usize);
            if !self.tiles[uy][ux].walkable() {
                self.tiles[uy][ux] = Tile::Floor;
            }
            // 一旦打通就停止
            if ux.abs_diff(sx) + uy.abs_diff(sy) > 3 {
                // 检查当前点是否已经有通路
                let mut free = false;
                for dy in -1isize..=1 {
                    for dx in -1isize..=1 {
                        let nx = ux.wrapping_add_signed(dx);
                        let ny = uy.wrapping_add_signed(dy);
                        if nx < MAP_WIDTH && ny < MAP_HEIGHT && self.tiles[ny][nx].walkable() && (nx != sx || ny != sy) {
                            free = true;
                        }
                    }
                }
                if free { break; }
            }
        }
    }

    /// 确保 from 到 to 之间有 walkable 路径。
    /// 若无路径，用加权醉汉游走从 from 向 to 方向挖掘。
    /// 每步 70% 概率向目标方向走（signum），30% 随机。
    pub fn ensure_connection_between(&mut self, rng: &mut impl Rng, from: (usize, usize), to: (usize, usize)) {
        use rand::RngExt;
        if self.has_path_between(from, to) { return; }

        let (mut cx, mut cy) = (from.0 as isize, from.1 as isize);
        let (tx, ty) = (to.0 as isize, to.1 as isize);
        for _ in 0..500 {
            if (cx - tx).abs() + (cy - ty).abs() < 3 { break; }
            let dx = if rng.random_range(0..100) < 70 { (tx - cx).signum() } else { rng.random_range(-1i32..2) as isize };
            let dy = if rng.random_range(0..100) < 70 { (ty - cy).signum() } else { rng.random_range(-1i32..2) as isize };
            if dx == 0 && dy == 0 { continue; }
            cx = (cx + dx).clamp(0, MAP_WIDTH as isize - 1);
            cy = (cy + dy).clamp(0, MAP_HEIGHT as isize - 1);
            let (ux, uy) = (cx as usize, cy as usize);
            if !self.tiles[uy][ux].walkable() {
                self.tiles[uy][ux] = Tile::Floor;
            }
        }
    }

    /// BFS 检查 from 到 to 是否有 walkable 路径
    pub fn has_path_between(&self, from: (usize, usize), to: (usize, usize)) -> bool {
        if !self.tiles[from.1][from.0].walkable() || !self.tiles[to.1][to.0].walkable() {
            return false;
        }
        let mut visited = [[false; MAP_WIDTH]; MAP_HEIGHT];
        let mut stack = vec![from];
        while let Some((x, y)) = stack.pop() {
            if (x, y) == to { return true; }
            if visited[y][x] { continue; }
            visited[y][x] = true;
            for (ny, nx) in [(y.wrapping_sub(1), x), (y + 1, x), (y, x.wrapping_sub(1)), (y, x + 1)] {
                if nx < MAP_WIDTH && ny < MAP_HEIGHT && !visited[ny][nx] && self.tiles[ny][nx].walkable() {
                    stack.push((nx, ny));
                }
            }
        }
        false
    }

    // ── 工具函数 ──

    /// 统计地图中某种 tile 的数量
    pub fn count_tile(&self, tile: Tile) -> usize {
        self.tiles.iter().flatten().filter(|&&t| t == tile).count()
    }

    /// 判断 (x,y) 是否远离所有房间中心（保护楼梯/出生点不被水体覆盖）
    pub fn is_away_from_rooms(&self, x: usize, y: usize, min_dist: usize) -> bool {
        self.rooms.iter().all(|r| {
            let (cx, cy) = r.center();
            x.abs_diff(cx) + y.abs_diff(cy) >= min_dist
        })
    }

    /// 统计 (x,y) 的 8 邻域中可行走格数量
    pub fn count_walkable_neighbors(&self, x: usize, y: usize) -> usize {
        let mut n = 0;
        for dy in -1isize..=1 {
            for dx in -1isize..=1 {
                if dx == 0 && dy == 0 { continue; }
                let nx = x.wrapping_add_signed(dx);
                let ny = y.wrapping_add_signed(dy);
                if nx < MAP_WIDTH && ny < MAP_HEIGHT && self.tiles[ny][nx].walkable() {
                    n += 1;
                }
            }
        }
        n
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

    /// BFS 收集所有可行走连通区，按大小降序返回
    pub fn collect_walkable_regions(&self) -> Vec<Vec<(usize, usize)>> {
        let mut visited = [[false; MAP_WIDTH]; MAP_HEIGHT];
        let mut regions = Vec::new();
        for sy in 0..MAP_HEIGHT {
            for sx in 0..MAP_WIDTH {
                if visited[sy][sx] || !self.tiles[sy][sx].walkable() { continue; }
                let mut stack = vec![(sx, sy)];
                let mut region = Vec::new();
                while let Some((x, y)) = stack.pop() {
                    if visited[y][x] { continue; }
                    visited[y][x] = true;
                    region.push((x, y));
                    for (ny, nx) in [(y.wrapping_sub(1), x), (y + 1, x), (y, x.wrapping_sub(1)), (y, x + 1)] {
                        if nx < MAP_WIDTH && ny < MAP_HEIGHT && !visited[ny][nx] && self.tiles[ny][nx].walkable() {
                            stack.push((nx, ny));
                        }
                    }
                }
                if region.len() >= 6 { regions.push(region); }
            }
        }
        regions.sort_by(|a, b| b.len().cmp(&a.len()));
        regions
    }

    /// 从洞穴 walkable 区域中检测连通区域，返回按大小降序排列的房间列表。，返回按大小降序排列的房间列表。
    /// `max_rooms` 限制最大房间数。返回的房间用 Room 近似（bounding box）。
    /// 扩展：后续可加入区域分类（入口区/战斗区/Boss 区）。
    pub fn detect_cave_regions(&self, max_rooms: usize) -> Vec<Room> {
        let mut visited = [[false; MAP_WIDTH]; MAP_HEIGHT];
        let mut regions: Vec<Vec<(usize, usize)>> = Vec::new();

        for sy in 0..MAP_HEIGHT {
            for sx in 0..MAP_WIDTH {
                if visited[sy][sx] || !self.tiles[sy][sx].walkable() { continue; }

                // BFS 找连通区
                let mut stack = vec![(sx, sy)];
                let mut region = Vec::new();
                while let Some((x, y)) = stack.pop() {
                    if visited[y][x] { continue; }
                    visited[y][x] = true;
                    region.push((x, y));

                    for (ny, nx) in [(y.wrapping_sub(1), x), (y + 1, x), (y, x.wrapping_sub(1)), (y, x + 1)] {
                        if nx < MAP_WIDTH && ny < MAP_HEIGHT && !visited[ny][nx] && self.tiles[ny][nx].walkable() {
                            stack.push((nx, ny));
                        }
                    }
                }

                // 过滤过小区域（避免玩家卡在 3×3 隔间里）
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
        (0..MAP_HEIGHT).map(|row| (0..MAP_WIDTH).map(|col| self.tiles[row][col].glyph()).collect()).collect()
    }
}
impl Default for Map { fn default() -> Self { Self::new() } }

#[cfg(test)]
mod tests;
