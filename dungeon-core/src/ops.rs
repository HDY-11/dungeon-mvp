//! World 工具函数（纯读写，无执行逻辑）
//!
//! 按关注点归入 dungeon-core，因为它们是"对游戏数据的简单查询/操作"。
//! 不包含"何时/如何行动"的判断逻辑，也不包含"世界如何创建/演化"的生命周期逻辑。

use crate::{
    components::*, items::*, resources::*,
    Tile, MAP_HEIGHT, MAP_WIDTH,
};
use bevy_ecs::prelude::*;
use crate::RgbColor;

// ── 经验公式 ────────────────────────────────────────

pub fn exp_to_next_level(level: u32) -> u64 {
    (25.0 * (level as f64).powf(1.5) + 10.0 * level as f64) as u64
}

pub fn max_hp_for(level: u32, defense: u32) -> i32 { 20 + level as i32 * 5 + defense as i32 * 2 }

pub fn max_mp_for(level: u32, mastery: u32) -> i32 { 5 + level as i32 * 3 + mastery as i32 }

// ── 有效属性计算 ────────────────────────────────────

pub fn effective_attack(stats: &Stats, inv: &Inventory, equip: &Equipment, buffs: Option<&Buffs>) -> u32 {
    let bonus = crate::items::equipment_bonus(inv, equip);
    let mut atk = (stats.attack as i32) + bonus.attack;
    if let Some(b) = buffs { atk += b.berserk_atk; }
    atk.max(1) as u32
}

pub fn effective_defense(stats: &Stats, inv: &Inventory, equip: &Equipment, buffs: Option<&Buffs>) -> u32 {
    let bonus = crate::items::equipment_bonus(inv, equip);
    let mut def = (stats.defense as i32) + bonus.defense;
    if let Some(b) = buffs { def += b.shield_def; }
    def.max(0) as u32
}

// ── 实体查询 ────────────────────────────────────────

/// 获取玩家实体
pub fn player_entity(world: &World) -> Option<Entity> {
    let mut q = world.try_query::<(Entity, &Player)>().unwrap();
    q.iter(world).next().map(|(e, _)| e)
}

/// 判断玩家是否站在楼梯上
pub fn on_stairs(world: &World) -> bool {
    let pp = *world.try_query::<&Position>().unwrap().iter(world).next().unwrap_or(&Position { x: 0, y: 0 });
    let mut q2 = world.try_query::<(&Stairs, &Position)>().unwrap();
    q2.iter(world).any(|(_, sp)| sp.x == pp.x && sp.y == pp.y)
}

/// 拾取玩家所在格的全部地面物品
pub fn pickup_ground(world: &mut World) {
    let (ppx, ppy) = {
        let mut q = world.try_query::<(&Player, &Position)>().unwrap();
        q.iter(world).next().map(|(_, p)| (p.x, p.y)).unwrap_or((0, 0))
    };
    let ground: Vec<(Entity, ItemStack)> = {
        let mut q = world.try_query::<(Entity, &ItemPickup, &Position)>().unwrap();
        q.iter(world)
            .filter(|(_, _, pos)| pos.x == ppx && pos.y == ppy)
            .map(|(e, p, _)| (e, p.stack.clone()))
            .collect()
    };
    if ground.is_empty() { return; }
    let mut logs = Vec::new();
    let mut despawn = Vec::new();
    for (entity, stack) in &ground {
        let mut q = world.query::<(&mut Inventory,)>();
        if let Some((mut inv,)) = q.iter_mut(world).next() {
            let leftover = inv.add(stack.item_id, stack.count);
            let picked = stack.count - leftover;
            if picked > 0 { logs.push(format!("拾取了{}x{}", stack.name(), picked)); }
            despawn.push(*entity);
        }
    }
    for e in despawn { world.entity_mut(e).despawn(); }
    for msg in logs { world.resource_mut::<EventLog>().push(msg); }
}

// ── 地图/视野记忆操作 ──────────────────────────────

pub fn update_map_memory(world: &mut World) {
    let visible: Vec<(usize, usize)> = {
        let mut q = world.query::<(&Player, &Viewshed)>();
        q.iter(world).next().map(|(_, v)| v.visible_tiles.clone()).unwrap_or_default()
    };
    let mut memory = world.resource_mut::<MapMemory>();
    for &(x, y) in &visible { memory.explored[y][x] = true; }
}

/// 更新可见实体记忆。
/// 记录视野内所有非 Player 实体（怪物、物品、楼梯等）的最后已知位置。
/// 实体离开视野后永久保留记忆（灰色显示），直到再次被看到或实体被销毁。
pub fn update_visible_memory(world: &mut World) {
    let player_visible: std::collections::HashSet<(usize, usize)>;
    let entities: Vec<(Entity, usize, usize, char, (u8, u8, u8))>;
    {
        player_visible = {
            let mut q = world.query::<(&Player, &Viewshed)>();
            q.iter(world).next()
                .map(|(_, v)| v.visible_tiles.iter().copied().collect())
                .unwrap_or_default()
        };
        // 记录所有非 Player 可见实体（怪物/物品/楼梯……）
        entities = {
            let mut q = world.query::<(Entity, Option<&Player>, &Position, &Renderable)>();
            q.iter(world)
                .filter(|(_, is_player, pos, _)| is_player.is_none() && player_visible.contains(&(pos.x, pos.y)))
                .map(|(e, _, pos, rend)| (e, pos.x, pos.y, rend.glyph, rend.color))
                .collect()
        };
    }
    // 当前仍存活的实体（用于剔除已销毁的）
    let alive: std::collections::HashSet<Entity> = {
        let mut q = world.query::<(Entity,)>();
        q.iter(world).map(|(e,)| e).collect()
    };
    let mut memory = world.resource_mut::<VisibleMemory>();

    // 更新当前帧可见的实体位置/外观
    for &(entity, x, y, glyph, color) in &entities {
        memory.entries.insert(entity, (x, y, glyph, color));
    }

    // 移除已销毁的实体（死亡/拾取/下楼被清空）
    memory.entries.retain(|&e, _| alive.contains(&e));
}

// ── 碰撞图 ─────────────────────────────────────────

pub fn rebuild_occupancy(world: &mut World) {
    // 收集不可通行的实体（排除 ItemPickup 和 Stairs）
    let positions: Vec<(Entity, usize, usize)> = {
        let mut q = world.query::<(Entity, &Position, Option<&ItemPickup>, Option<&Stairs>)>();
        q.iter(world)
            .filter(|(_, _, pickup, stairs)| pickup.is_none() && stairs.is_none())
            .map(|(e, p, _, _)| (e, p.x, p.y)).collect()
    };
    let mut occupancy = world.resource_mut::<OccupancyMap>();
    occupancy.clear();
    for (entity, x, y) in positions { occupancy.set(x, y, entity); }
}

// ── 渲染数据收集 ───────────────────────────────────

pub fn collect_renderables(world: &World) -> Vec<(usize, usize, char, RgbColor)> {
    let mut query = world.try_query::<(&Position, &Renderable)>().unwrap();
    let mut v: Vec<(usize, usize, char, RgbColor)> = query.iter(world)
        .map(|(pos, rend)| (pos.x, pos.y, rend.glyph, rend.color)).collect();
    // 玩家 (@) 最后渲染，确保显示在最上层
    v.sort_by_key(|(_, _, glyph, _)| if *glyph == '@' { 1 } else { 0 });
    v
}

pub fn set_player_dir(world: &mut World, dx: isize, dy: isize) {
    let mut query = world.query::<&mut MovingDir>();
    for mut dir in query.iter_mut(world) { dir.dx = dx; dir.dy = dy; }
}

// ── A* 寻路（8 方向） ─────────────────────────────

use std::collections::BinaryHeap;
use std::cmp::Ordering;

#[derive(Clone, Copy, Eq, PartialEq)]
struct AStarNode {
    cost: u32,
    heuristic: u32,
    x: usize,
    y: usize,
}

impl Ord for AStarNode {
    fn cmp(&self, other: &Self) -> Ordering {
        (other.cost + other.heuristic).cmp(&(self.cost + self.heuristic))
    }
}

impl PartialOrd for AStarNode {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// A* 寻路。从起点到终点找一条最短路径，返回路径点列表（不含起点，含终点）。
/// `map_tiles` 用于 walkable 检测。`occupied` 可选，传旧 OccupancyMap 避免走入已占格。
/// 支持 8 方向移动。
pub fn astar(
    start: (usize, usize),
    goal: (usize, usize),
    map_tiles: &[[Tile; MAP_WIDTH]; MAP_HEIGHT],
    occupied: Option<&crate::resources::OccupancyMap>,
) -> Option<Vec<(usize, usize)>> {
    if !map_tiles[goal.1][goal.0].walkable() { return None; }

    let dirs: [(isize, isize); 8] = [
        (0, -1), (0, 1), (-1, 0), (1, 0),
        (-1, -1), (1, -1), (-1, 1), (1, 1),
    ];
    let h = |x: usize, y: usize| -> u32 {
        x.abs_diff(goal.0).max(y.abs_diff(goal.1)) as u32 // Chebyshev
    };

    let size = MAP_WIDTH * MAP_HEIGHT;
    let mut heap = BinaryHeap::new();
    let mut costs = vec![u32::MAX; size];
    let mut came_from = vec![None as Option<(usize, usize)>; size];

    let idx = |x: usize, y: usize| y * MAP_WIDTH + x;

    heap.push(AStarNode { cost: 0, heuristic: h(start.0, start.1), x: start.0, y: start.1 });
    costs[idx(start.0, start.1)] = 0;

    while let Some(node) = heap.pop() {
        if (node.x, node.y) == goal {
            let mut path = Vec::new();
            let mut cur = (node.x, node.y);
            while let Some(prev) = came_from[idx(cur.0, cur.1)] {
                path.push(cur);
                cur = prev;
            }
            path.reverse();
            return Some(path);
        }

        let next_cost = node.cost + 1;
        for &(dx, dy) in &dirs {
            let nx = node.x.wrapping_add_signed(dx);
            let ny = node.y.wrapping_add_signed(dy);
            if nx >= MAP_WIDTH || ny >= MAP_HEIGHT { continue; }
            if !map_tiles[ny][nx].walkable() { continue; }
            // 对角穿墙角检查
            if dx != 0 && dy != 0 {
                let cx1 = node.x.wrapping_add_signed(dx);
                let cx2 = node.y.wrapping_add_signed(dy);
                let clear1 = cx1 < MAP_WIDTH && map_tiles[node.y][cx1].walkable();
                let clear2 = cx2 < MAP_HEIGHT && map_tiles[cx2][node.x].walkable();
                if !clear1 && !clear2 { continue; }
            }
            // 检查是否被占据（终点不算阻挡）——已经是不可占用的了
            if let Some(occ) = occupied {
                if (nx, ny) != goal && occ.is_occupied(nx, ny) { continue; }
            }
            let ni = idx(nx, ny);
            if next_cost < costs[ni] {
                costs[ni] = next_cost;
                came_from[ni] = Some((node.x, node.y));
                heap.push(AStarNode { cost: next_cost, heuristic: h(nx, ny), x: nx, y: ny });
            }
        }
    }
    None // 无路径
}
