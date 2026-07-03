pub mod action;
pub mod api;
pub mod components;
pub mod global;
pub mod items;
pub mod pathfinding;
pub mod resources;
pub mod save;
pub mod systems;


pub use api::*;
pub use components::*;
pub use items::*;
pub use pathfinding::*;
pub use resources::*;
pub use systems::*;

use rand::Rng;

pub use components::EntityName;

// ── 行动成本常量 ─────────────────────────────────────

pub mod action_cost {
    // 基础 AV 成本（受 speed 缩放：effective = base × 50/speed）
    pub const MOVE: f32 = 300.0;        // 走一格
    pub const ATTACK: f32 = 200.0;      // 普通攻击
    pub const SKILL_CAST: f32 = 600.0;  // 施法
    pub const SHIELD_BLOCK: f32 = 40.0; // 格挡（极短）
    pub const USE_POTION: f32 = 80.0;   // 使用药水
    pub const MONSTER_CHASE: f32 = 250.0;
    pub const MONSTER_WANDER: f32 = 500.0;
    pub const MONSTER_FLEE: f32 = 250.0;
    pub const PICKUP: f32 = 200.0;
    pub const WAIT: f32 = 800.0;
}

// ── 常量 ──────────────────────────────────────────────

pub const MAP_WIDTH: usize = 40;
pub const MAP_HEIGHT: usize = 20;

// ── Tile ──────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tile { Wall, Floor }

impl Tile {
    pub fn char(self) -> char {
        match self { Tile::Wall => '#', Tile::Floor => '.' }
    }
}

// ── Room ──────────────────────────────────────────────

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Room {
    pub x: usize, pub y: usize, pub w: usize, pub h: usize,
}
impl Room {
    pub fn center(&self) -> (usize, usize) { (self.x + self.w / 2, self.y + self.h / 2) }
}

// ── Map（ECS Resource）────────────────────────────────

#[derive(bevy_ecs::prelude::Resource)]
pub struct Map {
    pub tiles: [[Tile; MAP_WIDTH]; MAP_HEIGHT],
    pub rooms: Vec<Room>,
}

impl Map {
    pub fn new() -> Self { Self { tiles: [[Tile::Wall; MAP_WIDTH]; MAP_HEIGHT], rooms: Vec::new() } }
    pub fn generate(&mut self, rng: &mut impl Rng) {
        use rand::RngExt;
        self.tiles = [[Tile::Wall; MAP_WIDTH]; MAP_HEIGHT]; self.rooms.clear();
        let target = rng.random_range(4..=7);
        for _ in 0..target * 3 {
            if self.rooms.len() >= target { break; }
            let w = rng.random_range(4..=9); let h = rng.random_range(3..=6);
            let x = rng.random_range(1..(MAP_WIDTH - w - 1)); let y = rng.random_range(1..(MAP_HEIGHT - h - 1));
            let room = Room { x, y, w, h };
            if !self.overlaps_any(&room) { self.carve_room(&room); self.rooms.push(room); }
        }
        for i in 1..self.rooms.len() {
            let prev = &self.rooms[i - 1]; let curr = &self.rooms[i];
            self.carve_corridor(prev.center(), curr.center());
        }
    }
    fn overlaps_any(&self, room: &Room) -> bool {
        for r in &self.rooms { if room.x < r.x + r.w + 1 && room.x + room.w + 1 > r.x && room.y < r.y + r.h + 1 && room.y + room.h + 1 > r.y { return true; } }
        false
    }
    fn carve_room(&mut self, room: &Room) {
        for row in room.y..room.y + room.h { for col in room.x..room.x + room.w { self.tiles[row][col] = Tile::Floor; } }
    }
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
#[cfg(test)]
mod tests;
