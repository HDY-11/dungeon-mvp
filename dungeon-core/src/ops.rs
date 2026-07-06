//! World 工具函数（纯读写，无执行逻辑）
//!
//! 按关注点归入 dungeon-core，因为它们是"对游戏数据的简单查询/操作"。
//! 不包含"何时/如何行动"的判断逻辑，也不包含"世界如何创建/演化"的生命周期逻辑。

use crate::{
    components::*, items::*, resources::*,
};
use bevy_ecs::prelude::*;
use crate::world;

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
pub fn player_entity() -> Option<Entity> {
    let w = world!();
    let mut q = w.try_query::<(Entity, &Player)>().unwrap();
    q.iter(&w).next().map(|(e, _)| e)
}

/// 判断玩家是否站在楼梯上
pub fn on_stairs() -> bool {
    let w = world!();
    let pp = *w.try_query::<&Position>().unwrap().iter(&w).next().unwrap_or(&Position { x: 0, y: 0 });
    let mut q2 = w.try_query::<(&Stairs, &Position)>().unwrap();
    q2.iter(&w).any(|(_, sp)| sp.x == pp.x && sp.y == pp.y)
}

/// 拾取玩家所在格的全部地面物品
pub fn pickup_ground() {
    let (ppx, ppy) = {
        let w = world!();
        let mut q = w.try_query::<(&Player, &Position)>().unwrap();
        q.iter(&w).next().map(|(_, p)| (p.x, p.y)).unwrap_or((0, 0))
    };
    let ground: Vec<(Entity, ItemStack)> = {
        let w = world!();
        let mut q = w.try_query::<(Entity, &ItemPickup, &Position)>().unwrap();
        q.iter(&w)
            .filter(|(_, _, pos)| pos.x == ppx && pos.y == ppy)
            .map(|(e, p, _)| (e, p.stack.clone()))
            .collect()
    };
    if ground.is_empty() { return; }
    let mut logs = Vec::new();
    let mut despawn = Vec::new();
    for (entity, stack) in &ground {
        let mut w = world!(mut);
        let mut q = w.query::<(&mut Inventory,)>();
        if let Some((mut inv,)) = q.iter_mut(&mut *w).next() {
            let leftover = inv.add(stack.item_id, stack.count);
            let picked = stack.count - leftover;
            if picked > 0 { logs.push(format!("拾取了{}x{}", stack.name(), picked)); }
            despawn.push(*entity);
        }
    }
    for e in despawn { let mut w = world!(mut); w.entity_mut(e).despawn(); }
    for msg in logs { let mut w = world!(mut); w.resource_mut::<EventLog>().push(msg); }
}

// ── 地图/视野记忆操作 ──────────────────────────────

pub fn update_map_memory() {
    let visible: Vec<(usize, usize)> = {
        let mut w = world!(mut);
        let mut q = w.query::<(&Player, &Viewshed)>();
        q.iter(&mut *w).next().map(|(_, v)| v.visible_tiles.clone()).unwrap_or_default()
    };
    let mut w = world!(mut);
    let mut memory = w.resource_mut::<MapMemory>();
    for &(x, y) in &visible { memory.explored[y][x] = true; }
}

/// 更新可见实体记忆（记录视野内的实体，用于视野外灰色显示）
pub fn update_visible_memory() {
    let player_visible: std::collections::HashSet<(usize, usize)>;
    let entities: Vec<(Entity, usize, usize, char, (u8, u8, u8))>;
    {
        let mut w = world!(mut);
        player_visible = {
            let mut q = w.query::<(&Player, &Viewshed)>();
            q.iter(&mut *w).next()
                .map(|(_, v)| v.visible_tiles.iter().copied().collect())
                .unwrap_or_default()
        };
        entities = {
            let mut q = w.query::<(Entity, &Position, &Renderable)>();
            q.iter(&mut *w)
                .filter(|(_, pos, _)| player_visible.contains(&(pos.x, pos.y)))
                .map(|(e, pos, rend)| (e, pos.x, pos.y, rend.glyph, rend.color))
                .collect()
        };
    }
    let mut w = world!(mut);
    // 移除已不存在的实体（死亡/消失）
    let alive: std::collections::HashSet<Entity> = {
        let mut q = w.query::<(Entity,)>();
        q.iter(&mut *w).map(|(e,)| e).collect()
    };
    let mut memory = w.resource_mut::<VisibleMemory>();
    for (entity, x, y, glyph, color) in entities {
        memory.entries.insert(entity, (x, y, glyph, color));
    }
    memory.entries.retain(|&e, _| alive.contains(&e));
}

// ── 碰撞图 ─────────────────────────────────────────

pub fn rebuild_occupancy() {
    let mut w = world!(mut);
    // 收集不可通行的实体（排除 ItemPickup 和 Stairs）
    let positions: Vec<(Entity, usize, usize)> = {
        let mut q = w.query::<(Entity, &Position, Option<&ItemPickup>, Option<&Stairs>)>();
        q.iter(&mut *w)
            .filter(|(_, _, pickup, stairs)| pickup.is_none() && stairs.is_none())
            .map(|(e, p, _, _)| (e, p.x, p.y)).collect()
    };
    let mut occupancy = w.resource_mut::<OccupancyMap>();
    occupancy.clear();
    for (entity, x, y) in positions { occupancy.set(x, y, entity); }
}

// ── 渲染数据收集 ───────────────────────────────────

pub fn collect_renderables() -> Vec<(usize, usize, char, crate::RgbColor)> {
    let w = world!();
    let mut query = w.try_query::<(&Position, &Renderable)>().unwrap();
    let mut v: Vec<(usize, usize, char, crate::RgbColor)> = query.iter(&w)
        .map(|(pos, rend)| (pos.x, pos.y, rend.glyph, rend.color)).collect();
    // 玩家 (@) 最后渲染，确保显示在最上层
    v.sort_by_key(|(_, _, glyph, _)| if *glyph == '@' { 1 } else { 0 });
    v
}

pub fn set_player_dir(dx: isize, dy: isize) {
    let mut w = world!(mut);
    let mut query = w.query::<&mut MovingDir>();
    for mut dir in query.iter_mut(&mut *w) { dir.dx = dx; dir.dy = dy; }
}
