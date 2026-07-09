//! World 工具函数（纯读写，无执行逻辑）
//!
//! 按关注点归入 dungeon-core，因为它们是"对游戏数据的简单查询/操作"。
//! 不包含"何时/如何行动"的判断逻辑，也不包含"世界如何创建/演化"的生命周期逻辑。

use crate::{
    components::*, items::*, resources::*,
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

pub fn effective_attack(stats: &Stats, inv: &Inventory, equip: &Equipment, active_buffs: Option<&ActiveBuffs>) -> u32 {
    let bonus = crate::items::equipment_bonus(inv, equip);
    let mut atk = (stats.attack as i32) + bonus.attack;
    // 新 AV Buff 系统（旧 Buffs 已废弃，不再参与计算）
    if let Some(ab) = active_buffs {
        for b in &ab.0 {
            if b.kind == BuffKind::Berserk { atk += b.magnitude; }
        }
    }
    atk.max(1) as u32
}

pub fn effective_defense(stats: &Stats, inv: &Inventory, equip: &Equipment, active_buffs: Option<&ActiveBuffs>) -> u32 {
    let bonus = crate::items::equipment_bonus(inv, equip);
    let mut def = (stats.defense as i32) + bonus.defense;
    // 新 AV Buff 系统（旧 Buffs 已废弃，不再参与计算）
    if let Some(ab) = active_buffs {
        for b in &ab.0 {
            if b.kind == BuffKind::Shield { def += b.magnitude; }
        }
    }
    def.max(0) as u32
}

// ── 实体查询 ────────────────────────────────────────

/// 获取玩家实体
pub fn player_entity(world: &World) -> Option<Entity> {
    let mut q = world.try_query::<(Entity, &Player)>().expect("Entity+Player registered at init");
    q.iter(world).next().map(|(e, _)| e)
}

/// 判断玩家是否站在楼梯上
pub fn on_stairs(world: &World) -> bool {
    let pp = world.try_query::<(&Player, &Position)>().expect("Player+Position registered at init")
        .iter(world).next().map(|(_, p)| *p);
    let Some(pp) = pp else { return false };
    let mut q2 = world.try_query::<(&Stairs, &Position)>().expect("Stairs+Position registered at init");
    q2.iter(world).any(|(_, sp)| sp.x == pp.x && sp.y == pp.y)
}

/// 拾取玩家所在格的全部地面物品
pub fn pickup_ground(world: &mut World) {
    let (ppx, ppy) = {
        let mut q = world.try_query::<(&Player, &Position)>().expect("Player+Position registered at init");
        q.iter(world).next().map(|(_, p)| (p.x, p.y)).unwrap_or((0, 0))
    };
    let ground: Vec<(Entity, ItemStack)> = {
        let mut q = world.try_query::<(Entity, &ItemPickup, &Position)>().expect("Entity+ItemPickup+Position registered at init");
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

// ── Bresenham 画线 ─────────────────────────────────

/// Bresenham 直线算法，返回从 (x0,y0) 到 (x1,y1) 的**中间格**（不含起点）。
/// 返回顺序从起点旁第一个格到目标格（含目标）。
pub fn line_bresenham(x0: usize, y0: usize, x1: usize, y1: usize) -> Vec<(usize, usize)> {
    let mut points = Vec::new();
    let dx = (x1 as isize - x0 as isize).abs();
    let dy = -(y1 as isize - y0 as isize).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;
    let mut x = x0 as isize;
    let mut y = y0 as isize;
    loop {
        let e2 = 2 * err;
        if e2 >= dy { err += dy; x += sx; }
        if e2 <= dx { err += dx; y += sy; }
        if x == x1 as isize && y == y1 as isize {
            points.push((x as usize, y as usize));
            break;
        }
        points.push((x as usize, y as usize));
    }
    points
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

pub fn collect_renderables(world: &World) -> Vec<(Entity, usize, usize, char, RgbColor)> {
    let mut query = world.try_query::<(Entity, &Position, &Renderable)>().expect("Entity+Position+Renderable registered at init");
    let mut items: Vec<(Entity, usize, usize, char, RgbColor)> = Vec::new();
    for (entity, pos, rend) in query.iter(world) {
        items.push((entity, pos.x, pos.y, rend.glyph, rend.color));
    }
    // 图层优先级：玩家 (2) > 怪物 (1) > 物品/楼梯/其他 (0)
    items.sort_by_key(|(e, _, _, _, _)| {
        if world.get::<Player>(*e).is_some() { 2u8 }
        else if world.get::<Monster>(*e).is_some() { 1 }
        else { 0 }
    });
    items.into_iter().collect()
}

pub fn set_player_dir(world: &mut World, dx: isize, dy: isize) {
    let mut query = world.query::<&mut MovingDir>();
    for mut dir in query.iter_mut(world) { dir.dx = dx; dir.dy = dy; }
}

// ── 技能学习与熟练度 ─────────────────────────────────

/// 从 SkillKind 构造 Skill 实例（技能卷轴专用）
pub fn skill_from_kind(kind: &SkillKind) -> crate::components::Skill {
    use crate::components::SkillKind as SK;
    match kind {
        SK::Heal { amount: _ } => crate::components::Skill {
            name: "治愈", key: '1', cost_mp: 6,
            description: "HP恢复", kind: kind.clone(),
            proficiency: 1,
        },
        SK::Shield { def_boost: _, duration: _ } => crate::components::Skill {
            name: "护盾", key: '2', cost_mp: 5,
            description: "防御+5", kind: kind.clone(),
            proficiency: 1,
        },
        SK::Berserk { atk_boost: _, duration: _ } => crate::components::Skill {
            name: "狂暴", key: '3', cost_mp: 5,
            description: "攻击+5", kind: kind.clone(),
            proficiency: 1,
        },
    }
}

/// 学习技能：未学则添加，已学则提高熟练度
pub fn learn_skill(world: &mut World, entity: bevy_ecs::prelude::Entity, kind: &SkillKind) {
    use crate::components::SkillKind as SK;
    let skill_name = match kind {
        SK::Heal { .. } => "治愈",
        SK::Shield { .. } => "护盾",
        SK::Berserk { .. } => "狂暴",
    };

    // 先检查是否已学（不可变借）→ 决定操作
    let already_learned = {
        let skills = world.get::<crate::components::Skills>(entity)
            .expect("Player has Skills component");
        skills.list.iter().any(|s| s.name == skill_name)
    };

    if already_learned {
        // 先取 proficiency（不可变），再推日志
        let new_prof = {
            let mut skills = world.get_mut::<crate::components::Skills>(entity)
                .expect("Player has Skills component");
            if let Some(existing) = skills.list.iter_mut().find(|s| s.name == skill_name) {
                existing.proficiency += 1;
                existing.proficiency
            } else {
                return; // 不应发生
            }
        };
        world.resource_mut::<crate::resources::EventLog>()
            .push(format!("熟练度提升！{} 熟练度 {}", skill_name, new_prof));
    } else {
        let new_skill = skill_from_kind(kind);
        world.get_mut::<crate::components::Skills>(entity)
            .expect("Player has Skills component")
            .list.push(new_skill);
        world.resource_mut::<crate::resources::EventLog>()
            .push(format!("学会了{}！", skill_name));
    }
}

