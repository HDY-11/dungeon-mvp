//! World 工具函数（纯读写，无执行逻辑）
//!
//! 按关注点归入 dungeon-core，因为它们是"对游戏数据的简单查询/操作"。
//! 不包含"何时/如何行动"的判断逻辑，也不包含"世界如何创建/演化"的生命周期逻辑。

use crate::ext::OptionLogExt;
use crate::{Map, RgbColor, components::*, items::*, resources::*};
use bevy_ecs::prelude::*;
use bevy_ecs::system::RunSystemOnce;

// ── 经验公式 ────────────────────────────────────────

pub fn exp_to_next_level(level: u32) -> u64 {
    (25.0 * (level as f64).powf(1.5) + 10.0 * level as f64) as u64
}

pub fn max_hp_for(level: u32, defense: u32) -> i32 {
    20 + level as i32 * 5 + defense as i32 * 2
}

pub fn max_mp_for(level: u32, mastery: u32) -> i32 {
    5 + level as i32 * 3 + mastery as i32
}

// ── 有效属性计算 ────────────────────────────────────

pub fn effective_attack(
    stats: &Stats,
    equip: &Equipment,
    active_buffs: Option<&ActiveBuffs>,
) -> u32 {
    let bonus = crate::items::equipment_bonus(equip);
    let mut atk = (stats.attack as i32) + bonus.attack;
    // 新 AV Buff 系统（旧 Buffs 已废弃，不再参与计算）
    if let Some(ab) = active_buffs {
        for b in &ab.0 {
            if b.kind == BuffKind::Berserk {
                atk += b.magnitude;
            }
        }
    }
    atk.max(1) as u32
}

pub fn effective_defense(
    stats: &Stats,
    equip: &Equipment,
    active_buffs: Option<&ActiveBuffs>,
) -> u32 {
    let bonus = crate::items::equipment_bonus(equip);
    let mut def = (stats.defense as i32) + bonus.defense;
    // 新 AV Buff 系统（旧 Buffs 已废弃，不再参与计算）
    if let Some(ab) = active_buffs {
        for b in &ab.0 {
            if b.kind == BuffKind::Shield {
                def += b.magnitude;
            }
        }
    }
    def.max(0) as u32
}

// ── 实体查询 ────────────────────────────────────────

/// 获取玩家实体
pub fn player_entity(world: &World) -> Option<Entity> {
    let mut q = world
        .try_query::<(Entity, &Player)>()
        .expect_log("Entity+Player registered at init");
    q.iter(world).next().map(|(e, _)| e)
}

/// 判断玩家是否站在楼梯上
/// 玩家位置（I73 收敛：取代各处 try_query(&Player,&Position) 模式）
pub fn player_pos(world: &World) -> Option<(usize, usize)> {
    world
        .try_query::<(&Player, &Position)>()
        .and_then(|mut q| q.iter(world).next().map(|(_, p)| (p.x, p.y)))
}

pub fn on_stairs(world: &World) -> bool {
    let pp = world
        .try_query::<(&Player, &Position)>()
        .expect_log("Player+Position registered at init")
        .iter(world)
        .next()
        .map(|(_, p)| *p);
    let Some(pp) = pp else { return false };
    let mut q2 = world
        .try_query::<(&Stairs, &Position)>()
        .expect_log("Stairs+Position registered at init");
    q2.iter(world).any(|(_, sp)| sp.x == pp.x && sp.y == pp.y)
}

/// 拾取玩家所在格的全部地面物品
pub fn pickup_ground(world: &mut World) {
    let (ppx, ppy) = {
        let mut q = world
            .try_query::<(&Player, &Position)>()
            .expect_log("Player+Position registered at init");
        q.iter(world)
            .next()
            .map(|(_, p)| (p.x, p.y))
            .unwrap_or((0, 0))
    };
    let ground: Vec<(Entity, ItemStack)> = {
        let mut q = world
            .try_query::<(Entity, &ItemPickup, &Position)>()
            .expect_log("Entity+ItemPickup+Position registered at init");
        q.iter(world)
            .filter(|(_, _, pos)| pos.x == ppx && pos.y == ppy)
            .map(|(e, p, _)| (e, p.stack.clone()))
            .collect()
    };
    if ground.is_empty() {
        return;
    }
    let mut logs = Vec::new();
    let mut despawn = Vec::new();
    for (entity, stack) in &ground {
        // A25: 查询显式包含 &Player 组件（L31 最具体约束）
        let (picked, leftover) = {
            let mut q = world.query::<(&Player, &mut Inventory)>();
            if let Some((_, mut inv)) = q.iter_mut(world).next() {
                let leftover = inv.add(stack.item_id, stack.count);
                (stack.count - leftover, leftover)
            } else {
                (0, stack.count)
            }
        };
        if picked > 0 {
            logs.push(format!("拾取了{}x{}", stack.name(), picked));
        }
        if leftover == 0 && picked > 0 {
            // G28: 只有全部装下才销毁实体
            despawn.push(*entity);
        } else if leftover > 0 {
            // G28: 装不下的部分保留在地面，写回剩余数量——不再静默销毁
            if let Some(mut ip) = world.get_mut::<ItemPickup>(*entity) {
                ip.stack.count = leftover;
            }
            logs.push(format!("背包已满，{}{}留在地上", stack.name(), leftover));
        }
    }
    for e in despawn {
        world.entity_mut(e).despawn();
    }
    for msg in logs {
        world
            .resource_mut::<EventLog>()
            .push(crate::EventMessage::item(msg));
    }
}

/// 拾取指定地面物品实体到玩家背包（背包详情页 g 键，I66）。
/// 预检背包空间，放不下返回 false 且不消耗实体。
pub fn pickup_ground_item(world: &mut World, pickup_entity: Entity) -> bool {
    let Some(player) = player_entity(world) else {
        return false;
    };
    let Some(stack) = world
        .get::<ItemPickup>(pickup_entity)
        .map(|ip| ip.stack.clone())
    else {
        return false;
    };
    let can = world
        .get::<Inventory>(player)
        .map(|inv| inv.can_add(stack.item_id, stack.count))
        .unwrap_or(false);
    if !can {
        return false;
    }
    world
        .get_mut::<Inventory>(player)
        .expect_log("Inventory exists for pickup")
        .add(stack.item_id, stack.count);
    world.entity_mut(pickup_entity).despawn();
    true
}

// ── 投掷/装备共享工具函数 ─────────────────────────

/// 消耗副手 1 个物品。栈空时清除槽位。返回是否还有剩余。
pub fn consume_off_hand(world: &mut World, entity: Entity) -> bool {
    if let Some(mut eq) = world.get_mut::<Equipment>(entity)
        && let Some(ref mut stack) = eq.off_hand
    {
        stack.count = stack.count.saturating_sub(1);
        if stack.count == 0 {
            eq.off_hand = None;
            return false;
        }
        return true;
    }
    false
}

// ── 地图/视野记忆操作 ──────────────────────────────

pub fn update_map_memory(world: &mut World) {
    let visible: Vec<(usize, usize)> = {
        let mut q = world.query::<(&Player, &Viewshed)>();
        q.iter(world)
            .next()
            .map(|(_, v)| v.visible_tiles.clone())
            .unwrap_or_default()
    };
    let mut memory = world.resource_mut::<MapMemory>();
    for &(x, y) in &visible {
        memory.explored[y][x] = true;
    }
}

/// 更新可见实体记忆。
/// 记录视野内所有非 Player 实体（怪物、物品、楼梯等）的最后已知位置。
/// 实体离开视野后永久保留记忆（灰色显示），直到再次被看到或实体被销毁。
pub fn update_visible_memory(world: &mut World) {
    let player_visible: std::collections::HashSet<(usize, usize)>;
    let entities: Vec<EntityRenderable>;
    {
        player_visible = {
            let mut q = world.query::<(&Player, &Viewshed)>();
            q.iter(world)
                .next()
                .map(|(_, v)| v.visible_tiles.iter().copied().collect())
                .unwrap_or_default()
        };
        // 记录所有非 Player 可见实体（怪物/物品/楼梯……）
        entities = {
            let mut q = world.query::<(Entity, Option<&Player>, &Position, &Renderable)>();
            q.iter(world)
                .filter(|(_, is_player, pos, _)| {
                    is_player.is_none() && player_visible.contains(&(pos.x, pos.y))
                })
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

// ── 平衡常量（I71: 数值单源——渲染/行动/保活共用，防止阈值分裂） ──

/// 投掷最大射程（切比雪夫距离，Gm9）
pub const THROW_RANGE: usize = 5;
/// 投掷行动耗时（ms，与 CanThrow.duration 同源）
pub const THROW_DURATION: f32 = 190.0;
/// 低血量显示阈值（HP 比率 ≤ 此值标红）
pub const LOW_HP_RATIO: f32 = 0.3;
/// 逃跑触发/保活阈值（HP 比率 < 此值逃跑；README Gm8）
pub const FLEE_HP_RATIO: f32 = 0.25;
/// 逃跑退出阈值（G30 滞回：HP 恢复到该值以上退出逃跑——决策层用进入阈值，保活检查用退出阈值）
pub const FLEE_HP_RATIO_EXIT: f32 = 0.30;

/// 切比雪夫距离（8 方向移动的格距，I68 收敛：投掷射程等共用）
pub fn chebyshev(a: (usize, usize), b: (usize, usize)) -> usize {
    (a.0 as isize - b.0 as isize)
        .unsigned_abs()
        .max((a.1 as isize - b.1 as isize).unsigned_abs())
}

/// 视线是否畅通（I68 收敛）：from→to 的 Bresenham 路径上，
/// 除目标格外任一格阻挡视线即不可见。投掷/远程判定的唯一实现。
pub fn los_clear(map: &Map, from: (usize, usize), to: (usize, usize)) -> bool {
    line_bresenham(from.0, from.1, to.0, to.1)
        .iter()
        .all(|&(px, py)| (px == to.0 && py == to.1) || !map.tiles[py][px].blocks_vision())
}

// ── Bresenham 画线 ─────────────────────────────────

/// Bresenham 直线算法，返回从 (x0,y0) 到 (x1,y1) 的**中间格**（不含起点）。
/// 返回顺序从起点旁第一个格到目标格（含目标）。
pub fn line_bresenham(x0: usize, y0: usize, x1: usize, y1: usize) -> Vec<(usize, usize)> {
    if x0 == x1 && y0 == y1 {
        return Vec::new();
    }
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
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
        if x == x1 as isize && y == y1 as isize {
            points.push((x as usize, y as usize));
            break;
        }
        points.push((x as usize, y as usize));
    }
    points
}

/// 读档/下楼后的世界刷新序列（I73 收敛：fov + 视野记忆 + 占用图）。
/// 调用方需保证 Map/MapMemory/VisibleMemory 已就位。
pub fn post_load_refresh(world: &mut World) {
    let _ = world.run_system_once(crate::systems::fov_system);
    update_map_memory(world);
    update_visible_memory(world);
    rebuild_occupancy(world);
}

// ── 碰撞图 ─────────────────────────────────────────

pub fn rebuild_occupancy(world: &mut World) {
    // 收集不可通行的实体（排除 ItemPickup 和 Stairs）
    let positions: Vec<(Entity, usize, usize)> = {
        let mut q = world.query::<(Entity, &Position, Option<&ItemPickup>, Option<&Stairs>)>();
        q.iter(world)
            .filter(|(_, _, pickup, stairs)| pickup.is_none() && stairs.is_none())
            .map(|(e, p, _, _)| (e, p.x, p.y))
            .collect()
    };
    let mut occupancy = world.resource_mut::<OccupancyMap>();
    occupancy.clear();
    for (entity, x, y) in positions {
        occupancy.set(x, y, entity);
    }
}

// ── 渲染数据收集 ───────────────────────────────────

pub fn collect_renderables(world: &World) -> Vec<(Entity, usize, usize, char, RgbColor)> {
    let mut query = world
        .try_query::<(Entity, &Position, &Renderable)>()
        .expect_log("Entity+Position+Renderable registered at init");
    let mut items: Vec<(Entity, usize, usize, char, RgbColor)> = Vec::new();
    for (entity, pos, rend) in query.iter(world) {
        items.push((entity, pos.x, pos.y, rend.glyph, rend.color));
    }
    // 图层优先级：玩家 (2) > 怪物 (1) > 物品/楼梯/其他 (0)
    items.sort_by_key(|(e, _, _, _, _)| {
        if world.get::<Player>(*e).is_some() {
            2u8
        } else if world.get::<Monster>(*e).is_some() {
            1
        } else {
            0
        }
    });
    items.into_iter().collect()
}

// ── 技能学习与熟练度 ─────────────────────────────────

/// 从 SkillKind 构造 Skill 实例（技能卷轴专用）
pub fn skill_from_kind(kind: &SkillKind) -> crate::components::Skill {
    use crate::components::SkillKind as SK;
    match kind {
        SK::Heal { amount: _ } => crate::components::Skill {
            name: "治愈".to_string(),
            key: '1',
            cost_mp: 6,
            description: "HP恢复".to_string(),
            kind: kind.clone(),
            proficiency: 1,
        },
        SK::Shield {
            def_boost: _,
            duration: _,
        } => crate::components::Skill {
            name: "护盾".to_string(),
            key: '2',
            cost_mp: 5,
            description: "防御+5".to_string(),
            kind: kind.clone(),
            proficiency: 1,
        },
        SK::Berserk {
            atk_boost: _,
            duration: _,
        } => crate::components::Skill {
            name: "狂暴".to_string(),
            key: '3',
            cost_mp: 5,
            description: "攻击+5".to_string(),
            kind: kind.clone(),
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
        let skills = world
            .get::<crate::components::Skills>(entity)
            .expect_log("Player has Skills component");
        skills.list.iter().any(|s| s.name == skill_name)
    };

    if already_learned {
        // 先取 proficiency（不可变），再推日志
        let new_prof = {
            let mut skills = world
                .get_mut::<crate::components::Skills>(entity)
                .expect_log("Player has Skills component");
            if let Some(existing) = skills.list.iter_mut().find(|s| s.name == skill_name) {
                existing.proficiency += 1;
                existing.proficiency
            } else {
                return; // 不应发生
            }
        };
        world
            .resource_mut::<crate::resources::EventLog>()
            .push(crate::EventMessage::skill(format!(
                "熟练度提升！{} 熟练度 {}",
                skill_name, new_prof
            )));
    } else {
        let new_skill = skill_from_kind(kind);
        world
            .get_mut::<crate::components::Skills>(entity)
            .expect_log("Player has Skills component")
            .list
            .push(new_skill);
        world
            .resource_mut::<crate::resources::EventLog>()
            .push(crate::EventMessage::skill(format!(
                "学会了{}！",
                skill_name
            )));
    }
}
