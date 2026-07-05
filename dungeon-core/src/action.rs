//! 行动系统 v3 — 组件化行动 + 全局队列 + 统一输入
//!
//! 每个行动是一个独立 Component，含 cooldown/reaction_time/priority/timer。
//! 系统收集条件满足的行动 → 仲裁 → 入全局队列 → 推进执行。

use crate::world;
use crate::{Stats, Viewshed, Player, Position, EntityName, Monster, EventLog};
use bevy_ecs::prelude::*;
use bevy_ecs::system::RunSystemOnce;

// ══════════════════════════════════════════════════════
// 实体属性
// ══════════════════════════════════════════════════════

/// 反应时：从决策锁定到行动执行的延迟。
/// 由敏捷派生，敏捷越高反应越快（反应时越短）。
#[derive(Component, Clone, Debug)]
pub struct Reaction {
    pub time: f32,
}

/// 从敏捷推算反应时
pub fn agility_to_reaction(agility: u32) -> f32 {
    (100.0 - agility as f32 * 3.0).max(20.0)
}

// ══════════════════════════════════════════════════════
// Action 组件
// ══════════════════════════════════════════════════════
//
// 每个 Action 组件包含：
//   - duration: 该行动的耗时
//   - priority: 仲裁优先级
//
// AV = reaction_time + duration，作为单一值入队倒计时。
// 反应时（reaction_time）不在此处——它是实体的属性（见 Reaction 组件）。

/// 移动行动
#[derive(Component, Clone, Debug)]
pub struct CanMove {
    pub duration: f32,
    pub priority: u32,
}

impl CanMove {
    pub fn new(priority: u32) -> Self {
        Self { duration: 300.0, priority }
    }

    pub fn condition(target_is_walkable: bool, target_is_occupied_by_enemy: bool) -> bool {
        target_is_walkable || target_is_occupied_by_enemy
    }
}

/// 追击行动（怪物专用）
#[derive(Component, Clone, Debug)]
pub struct CanChase {
    pub duration: f32,
    pub priority: u32,
}

impl CanChase {
    pub fn new(priority: u32) -> Self {
        Self { duration: 250.0, priority }
    }

    pub fn condition(can_see_player: bool) -> bool {
        can_see_player
    }
}

/// 逃跑行动（怪物专用）
#[derive(Component, Clone, Debug)]
pub struct CanFlee {
    pub duration: f32,
    pub priority: u32,
}

impl CanFlee {
    pub fn new(priority: u32) -> Self {
        Self { duration: 250.0, priority }
    }

    pub fn condition(hp_ratio: f32) -> bool {
        hp_ratio < 0.25
    }
}

/// 游荡行动（怪物专用）
#[derive(Component, Clone, Debug)]
pub struct CanWander {
    pub duration: f32,
    pub priority: u32,
}

impl CanWander {
    pub fn new(priority: u32) -> Self {
        Self { duration: 500.0, priority }
    }

    pub fn condition() -> bool {
        true
    }
}

/// 等待行动（玩家/怪物通用）
#[derive(Component, Clone, Debug)]
pub struct CanWait {
    pub duration: f32,
    pub priority: u32,
}

impl CanWait {
    pub fn new(priority: u32) -> Self {
        Self { duration: 800.0, priority }
    }

    pub fn condition() -> bool {
        true
    }
}

// ══════════════════════════════════════════════════════
// 行动队列
// ══════════════════════════════════════════════════════

#[derive(Clone, Debug)]
pub enum ActionKindV3 {
    Move { dx: isize, dy: isize },
    Chase,
    Flee,
    Wander,
    Wait,
    Attack { target: Entity },
    Skill(usize),
}

/// 行动队列条目
#[derive(Clone, Debug)]
pub struct ActionEntry {
    pub entity: Entity,
    pub kind: ActionKindV3,
    /// AV 剩余 = reaction_time + duration，单一倒计时
    pub av_remaining: f32,
}

/// 全局行动队列（FIFO）
#[derive(Resource)]
pub struct ActionQueue {
    pub entries: Vec<ActionEntry>,
}

impl Default for ActionQueue {
    fn default() -> Self { Self { entries: Vec::new() } }
}

impl ActionQueue {
    /// 入队：av = reaction_time + duration
    pub fn enqueue(&mut self, entity: Entity, kind: ActionKindV3, av: f32) {
        self.entries.push(ActionEntry {
            entity,
            kind,
            av_remaining: av,
        });
    }

    pub fn advance(&mut self, amount: f32) {
        for entry in &mut self.entries {
            if entry.av_remaining > 0.0 {
                entry.av_remaining = (entry.av_remaining - amount).max(0.0);
            }
        }
    }

    /// 找最小正 av_remaining（= 下一次事件的距离）
    pub fn next_event_distance(&self) -> Option<f32> {
        self.entries
            .iter()
            .filter(|e| e.av_remaining > 0.0)
            .map(|e| e.av_remaining)
            .min_by(|a, b| a.partial_cmp(b).unwrap())
    }

    /// 取出所有 av_remaining ≤ 0 的条目
    pub fn pop_ready(&mut self) -> Vec<ActionEntry> {
        let mut ready = Vec::new();
        self.entries.retain(|e| {
            if e.av_remaining <= 0.0 {
                ready.push(e.clone());
                false
            } else {
                true
            }
        });
        ready
    }

    /// 检查实体是否已在队列中
    pub fn has_entity(&self, entity: Entity) -> bool {
        self.entries.iter().any(|e| e.entity == entity)
    }

    /// 入队或跳过：如果实体已在队列中，忽略（保留已有行动的 av）
    pub fn enqueue_if_absent(&mut self, entity: Entity, kind: ActionKindV3, av: f32) {
        if !self.entries.iter().any(|e| e.entity == entity) {
            self.entries.push(ActionEntry { entity, kind, av_remaining: av });
        }
    }
}

// ══════════════════════════════════════════════════════
// 输入管线
// ══════════════════════════════════════════════════════

/// 已识别但未消费的玩家输入
#[derive(Clone, Debug)]
pub enum RecognizedInput {
    Direction(isize, isize),
    Skill(usize),
    Wait,
    OpenBag,
    Quit,
    Confirm,
}

/// 缓冲区
#[derive(Resource, Default)]
pub struct InputBuffer {
    /// 已识别待消费的输入
    pub buffer: Vec<RecognizedInput>,
}

impl InputBuffer {
    pub fn push(&mut self, input: RecognizedInput) {
        if self.buffer.len() >= 2 {
            self.buffer.remove(0);
        }
        // 去重：连续相同方向只保留一个
        if let Some(last) = self.buffer.last() {
            match (last, &input) {
                (RecognizedInput::Direction(ax, ay), RecognizedInput::Direction(bx, by))
                    if ax == bx && ay == by => return,
                _ => {}
            }
        }
        self.buffer.push(input);
    }

    pub fn pop(&mut self) -> Option<RecognizedInput> {
        if self.buffer.is_empty() {
            None
        } else {
            Some(self.buffer.remove(0))
        }
    }
}

/// 玩家预览态
#[derive(Resource)]
pub struct PlayerPreview {
    pub kind: Option<ActionKindV3>,
}

impl Default for PlayerPreview {
    fn default() -> Self { Self { kind: None } }
}

// ══════════════════════════════════════════════════════
// 仲裁系统：从所有就绪行动中选优先级最高的入队
// ══════════════════════════════════════════════════════

/// 遍历所有怪物，检查各 Action 组件的条件，收集就绪行动，按优先级入队
pub fn run_monster_decision() {
    // 阶段 1：收集 (entity, priority, av, kind)
    let mut collected: Vec<(Entity, u32, f32, ActionKindV3)> = Vec::new();
    {
        let mut w = world!(mut);
        let player_pos = w.query::<(&Player, &Position)>().iter(&mut *w).next().map(|(_, p)| (p.x, p.y));

        // 追击（读取 Reaction 获取反应时）
        for (entity, chase, stats, view, reaction) in
            w.query::<(Entity, &CanChase, &Stats, &Viewshed, &Reaction)>().iter(&mut *w)
        {
            let can_see = player_pos.map_or(false, |pp| view.visible_tiles.contains(&pp));
            if CanChase::condition(can_see) {
                let av = reaction.time + chase.duration;
                collected.push((entity, chase.priority, av, ActionKindV3::Chase));
            }
        }

        // 逃跑
        for (entity, flee, stats, reaction) in
            w.query::<(Entity, &CanFlee, &Stats, &Reaction)>().iter(&mut *w)
        {
            let hp_ratio = stats.hp as f32 / stats.max_hp as f32;
            if CanFlee::condition(hp_ratio) {
                let av = reaction.time + flee.duration;
                collected.push((entity, flee.priority, av, ActionKindV3::Flee));
            }
        }

        // 游荡
        for (entity, wander, reaction) in
            w.query::<(Entity, &CanWander, &Reaction)>().iter(&mut *w)
        {
            if !collected.iter().any(|(e, _, _, _)| *e == entity) && CanWander::condition() {
                let av = reaction.time + wander.duration;
                collected.push((entity, wander.priority, av, ActionKindV3::Wander));
            }
        }
    }

    // 阶段 2：按 priority 排序，相同时随机
    collected.sort_by(|(_, pa, _, _), (_, pb, _, _)| {
        pb.cmp(pa).then_with(|| crate::global::rand_u8().cmp(&crate::global::rand_u8()))
    });

    // 阶段 3：入队（已在队列中的实体不再重复入队）
    let mut w = world!(mut);
    let mut queue = w.resource_mut::<ActionQueue>();
    for (entity, _priority, av, kind) in &collected {
        if !queue.has_entity(*entity) {
            queue.enqueue(*entity, kind.clone(), *av);
        }
    }
}

// ══════════════════════════════════════════════════════
// 行动执行引擎：推进队列 + 保活检查 + 执行
// ══════════════════════════════════════════════════════

/// 推进行动队列，返回实际推进量
pub fn advance_action_queue() -> f32 {
    // 阶段 1：推进队列（持有写锁）
    let dist;
    let ready;
    {
        let mut w = world!(mut);
        dist = {
            let queue = w.resource::<ActionQueue>();
            queue.next_event_distance().unwrap_or(0.0)
        };
        if dist <= 0.0 { return 0.0; }
        w.resource_mut::<ActionQueue>().advance(dist);
        ready = w.resource_mut::<ActionQueue>().pop_ready();
    }

    // 阶段 2：保活检查 + 执行就绪条目
    for entry in &ready {
        if check_condition(entry) {
            execute_entry(entry);
            let _ = world!(mut).run_system_once(crate::systems::apply_exp_system);
        } else {
            // 条件不再满足，丢弃行动（实体已损失 AV）
            world!(mut).resource_mut::<EventLog>().push(format!("行动被取消"));
        }
    }
    dist
}

/// 保活检查：执行前回调组件验证条件是否仍满足
fn check_condition(entry: &ActionEntry) -> bool {
    let mut w = world!(mut);
    match &entry.kind {
        ActionKindV3::Chase => {
            // 玩家是否仍在视野内
            let player_pos = w.query::<(&Player, &Position)>().iter(&mut *w).next().map(|(_, p)| (p.x, p.y));
            let Some((px, py)) = player_pos else { return false };
            w.get::<Viewshed>(entry.entity)
                .map(|v| v.visible_tiles.contains(&(px, py)))
                .unwrap_or(false)
        }
        ActionKindV3::Flee => {
            // HP 比率是否仍低于阈值
            w.get::<Stats>(entry.entity)
                .map(|s| (s.hp as f32 / s.max_hp as f32) < 0.25)
                .unwrap_or(false)
        }
        ActionKindV3::Wander | ActionKindV3::Wait => true,
        ActionKindV3::Move { .. } => true, // 玩家行动不检查
        ActionKindV3::Attack { target } => {
            // 目标是否仍存在且是怪物
            w.get::<Monster>(*target).is_some()
        }
        ActionKindV3::Skill(_) => true, // MP 检查在 execute 中
    }
}

/// 执行一个行动条目（保活检查通过后调用）
fn execute_entry(entry: &ActionEntry) {
    match &entry.kind {
        ActionKindV3::Chase => execute_chase(entry.entity),
        ActionKindV3::Flee => execute_flee(entry.entity),
        ActionKindV3::Wander => execute_wander(entry.entity),
        ActionKindV3::Wait => execute_wait(entry.entity),
        ActionKindV3::Move { dx, dy } => execute_player_move(entry.entity, *dx, *dy),
        ActionKindV3::Attack { target } => execute_attack(entry.entity, *target),
        ActionKindV3::Skill(idx) => execute_skill(entry.entity, *idx),
    }
}

fn execute_chase(entity: Entity) {
    use crate::{Map, MAP_WIDTH, MAP_HEIGHT, Tile, OccupancyMap, FloorNumber};
    use crate::pathfinding::find_path;
    use crate::components::AttackName;

    let mut w = world!(mut);
    let Some(player_entity) = w.query::<(Entity, &Player)>().iter(&mut *w).next().map(|(e, _)| e) else { return };
    let player_pos = w.get::<Position>(player_entity).map(|p| (p.x, p.y));
    let Some((px, py)) = player_pos else { return };
    let pos = match w.get::<Position>(entity) {
        Some(p) => (p.x, p.y),
        None => return,
    };
    let dist = pos.0.abs_diff(px) + pos.1.abs_diff(py);
    let map = w.resource::<Map>();
    let occupancy = w.resource::<OccupancyMap>();

    if dist <= 1 {
        // 近战攻击
        let dmg = w.get::<Stats>(entity).map(|s| s.attack as i32).unwrap_or(1);
        let name = w.get::<EntityName>(entity).map(|n| n.0.clone()).unwrap_or("怪物".into());
        if let Some(mut ps) = w.get_mut::<Stats>(player_entity) {
            ps.hp -= dmg.max(1);
        }
        w.resource_mut::<crate::EventLog>().push(format!("{} 攻击了你，{}伤", name, dmg));
    } else {
        // 向玩家移动一格
        let dx = if px > pos.0 { 1 } else if px < pos.0 { -1 } else { 0 };
        let dy = if py > pos.1 { 1 } else if py < pos.1 { -1 } else { 0 };
        let attempts = if px.abs_diff(pos.0) >= py.abs_diff(pos.1) {
            vec![(dx, 0), (0, dy)]
        } else {
            vec![(0, dy), (dx, 0)]
        };
        drop(map); drop(occupancy);
        for (ndx, ndy) in attempts {
            let nx = pos.0.wrapping_add_signed(ndx);
            let ny = pos.1.wrapping_add_signed(ndy);
            if nx < MAP_WIDTH && ny < MAP_HEIGHT
                && w.resource::<Map>().tiles[ny][nx] == Tile::Floor
                && !w.resource::<OccupancyMap>().is_occupied(nx, ny)
            {
                if let Some(mut p) = w.get_mut::<Position>(entity) {
                    p.x = nx; p.y = ny;
                }
                break;
            }
        }
    }
}

fn execute_flee(entity: Entity) {
    use crate::{Map, MAP_WIDTH, MAP_HEIGHT, Tile, OccupancyMap};
    let mut w = world!(mut);
    let player_pos = w.query::<(&Player, &Position)>().iter(&mut *w).next().map(|(_, p)| (p.x, p.y));
    let Some((px, py)) = player_pos else { return };
    let pos = match w.get::<Position>(entity) {
        Some(p) => (p.x, p.y),
        None => return,
    };
    let dirs: [(isize, isize); 4] = [(0, -1), (0, 1), (-1, 0), (1, 0)];
    let mut best: Option<(usize, usize)> = None;
    let mut best_dist = 0usize;
    for &(dx, dy) in &dirs {
        let nx = pos.0.wrapping_add_signed(dx);
        let ny = pos.1.wrapping_add_signed(dy);
        if nx < MAP_WIDTH && ny < MAP_HEIGHT
            && w.resource::<Map>().tiles[ny][nx] == Tile::Floor
            && !w.resource::<OccupancyMap>().is_occupied(nx, ny)
        {
            let d = nx.abs_diff(px) + ny.abs_diff(py);
            if d > best_dist { best_dist = d; best = Some((nx, ny)); }
        }
    }
    if let Some((nx, ny)) = best {
        if let Some(mut p) = w.get_mut::<Position>(entity) {
            p.x = nx; p.y = ny;
        }
    }
}

fn execute_wander(entity: Entity) {
    use crate::{Map, MAP_WIDTH, MAP_HEIGHT, Tile, OccupancyMap, FloorNumber};
    let mut w = world!(mut);
    let dirs: [(isize, isize); 4] = [(0, -1), (0, 1), (-1, 0), (1, 0)];
    let r = (w.resource::<FloorNumber>().0 as usize + w.query::<(Entity, &Monster)>().iter(&mut *w).count()) % 4;
    let (dx, dy) = dirs[r];
    if let Some(pos) = w.get::<Position>(entity) {
        let nx = pos.x.wrapping_add_signed(dx);
        let ny = pos.y.wrapping_add_signed(dy);
        if nx < MAP_WIDTH && ny < MAP_HEIGHT
            && w.resource::<Map>().tiles[ny][nx] == Tile::Floor
            && !w.resource::<OccupancyMap>().is_occupied(nx, ny)
        {
            if let Some(mut p) = w.get_mut::<Position>(entity) {
                p.x = nx; p.y = ny;
            }
        }
    }
}

fn execute_wait(entity: Entity) {
    // 纯等待，无副作用
    let _ = entity;
}

fn execute_player_move(entity: Entity, dx: isize, dy: isize) {
    use crate::{Map, Tile, OccupancyMap, MAP_WIDTH, MAP_HEIGHT, movement_system};
    // 先读数据
    let pos = {
        let w = world!();
        let p = match w.get::<Position>(entity) {
            Some(p) => (p.x, p.y),
            None => return,
        };
        let nx = p.0.wrapping_add_signed(dx);
        let ny = p.1.wrapping_add_signed(dy);
        if nx >= MAP_WIDTH || ny >= MAP_HEIGHT { return; }
        let tile = w.resource::<Map>().tiles[ny][nx];
        let occupied = w.resource::<OccupancyMap>().is_occupied(nx, ny);
        if tile != Tile::Floor { return; }
        (nx, ny, occupied)
    };
    if pos.2 {
        // 有怪物 → bump 攻击
        crate::set_player_dir(dx, dy);
        crate::rebuild_occupancy();
        world!(mut).run_system_once(movement_system);
        crate::rebuild_occupancy();
    } else {
        let mut w = world!(mut);
        if let Some(mut p) = w.get_mut::<Position>(entity) {
            p.x = pos.0;
            p.y = pos.1;
        }
    }
}

fn execute_attack(attacker: Entity, target: Entity) {
    use crate::{Stats, EntityName, EventLog, AttackName, Buffs, Inventory, Equipment, PendingExp, LootTable, ItemPickup, Renderable, Position};
    // 先读取需要的数据
    let (exp, name, atk_name, base_atk, crit_rate, crit_dmg, target_def, target_pos);
    {
        let mut w = world!(mut);
        let Some(target_stats) = w.get::<Stats>(target).cloned() else { return };
        let Some(attacker_stats) = w.get::<Stats>(attacker).cloned() else { return };
        name = w.get::<EntityName>(target).map(|n| n.0.clone()).unwrap_or("怪物".into());
        atk_name = w.get::<AttackName>(attacker).map(|a| a.0.clone()).unwrap_or("攻击".into());
        target_pos = w.get::<Position>(target).map(|p| (p.x, p.y));
        let inventory = w.get::<Inventory>(attacker);
        let equipment = w.get::<Equipment>(attacker);
        base_atk = if let (Some(inv), Some(eq)) = (inventory, equipment) {
            crate::equipment_bonus(inv, eq).attack + attacker_stats.attack as i32
        } else {
            attacker_stats.attack as i32
        };
        crit_rate = attacker_stats.crit_rate;
        crit_dmg = attacker_stats.crit_damage;
        target_def = target_stats.defense as i32;
        exp = target_stats.exp;
    }

    // 计算伤害
    let raw_dmg = (base_atk - target_def).max(1);
    let crit = crit_rate > rand::random::<f32>();
    let dmg = if crit { (raw_dmg as f32 * (1.0 + crit_dmg)).round() as i32 } else { raw_dmg };

    // 应用伤害 + 掉落
    {
        let mut w = world!(mut);
        let Some(mut target_stats) = w.get_mut::<Stats>(target) else { return };
        target_stats.hp -= dmg;
        if target_stats.hp <= 0 {
            w.resource_mut::<PendingExp>().amount += exp;
            w.resource_mut::<EventLog>().push(format!("你{}击杀了{}！获得{}经验", atk_name, name, exp));

            // 掉落
            let loot_stacks = w.get::<LootTable>(target)
                .map(|lt| lt.roll())
                .unwrap_or_default();
            if let Some((px, py)) = target_pos {
                for stack in &loot_stacks {
                    let sname = stack.name();
                    w.resource_mut::<EventLog>().push(format!("{}掉落{}x{}", name, sname, stack.count));
                    w.spawn((
                        ItemPickup { stack: stack.clone() },
                        Position { x: px, y: py },
                        Renderable { glyph: stack.glyph(), color: stack.color() },
                    ));
                }
            }

            w.entity_mut(target).despawn();
        } else {
            w.resource_mut::<EventLog>().push(format!("你{}了{}{}，造成{}点伤害", atk_name, name, if crit { "！暴击" } else { "" }, dmg));
        }
    };
}

fn execute_skill(entity: Entity, skill_idx: usize) {
    use crate::{Stats, Skills};
    // 读取技能数据和玩家属性
    let (skill_kind, cost_mp, skill_name);
    {
        let w = world!();
        let Some(skills) = w.get::<Skills>(entity) else { return };
        let Some(skill) = skills.list.get(skill_idx) else { return };
        let Some(stats) = w.get::<Stats>(entity) else { return };
        if stats.mp < skill.cost_mp {
            let msg = format!("MP不足，无法施放{}", skill.name);
            drop(w);
            world!(mut).resource_mut::<EventLog>().push(msg);
            return;
        }
        skill_kind = skill.kind.clone();
        cost_mp = skill.cost_mp;
        skill_name = skill.name.clone();
    }

    // 扣 MP
    {
        let mut w = world!(mut);
        if let Some(mut stats) = w.get_mut::<Stats>(entity) {
            stats.mp -= cost_mp;
        }
    }

    // 执行技能效果
    match skill_kind {
        crate::SkillKind::Firebolt { damage } => {
            // 先读取需要的数据
            let (pp, magic_bonus, crit_rate, crit_dmg);
            {
                let mut w = world!(mut);
                pp = w.query::<(&Player, &Position)>().iter(&mut *w).next().map(|(_, p)| (p.x, p.y));
                let stats = w.get::<Stats>(entity);
                magic_bonus = stats.map(|s| (s.magic_mastery as f32 * 0.5) as i32).unwrap_or(0);
                crit_rate = stats.map(|s| s.crit_rate).unwrap_or(0.0);
                crit_dmg = stats.map(|s| s.crit_damage).unwrap_or(0.0);
            }
            // 计算伤害并收集目标
            let mut hits: Vec<(Entity, i32)> = Vec::new();
            let mut hit_any = false;
            {
                let mut w = world!(mut);
                for (me, mut ms, mp, mn) in w.query::<(Entity, &mut Stats, &Position, &EntityName)>().iter_mut(&mut *w) {
                    if let Some((px, py)) = pp {
                        if mp.x.abs_diff(px) + mp.y.abs_diff(py) <= 1 {
                            let is_crit = crit_rate > rand::random::<f32>();
                            let mut dmg = (damage + magic_bonus - ms.defense as i32).max(1);
                            if is_crit { dmg = (dmg as f32 * (1.0 + crit_dmg)).round() as i32; }
                            ms.hp -= dmg;
                            hits.push((me, dmg));
                            hit_any = true;
                        }
                    }
                }
            }
            // 日志和清理
            {
                let mut w = world!(mut);
                for (me, dmg) in &hits {
                    let name = w.get::<EntityName>(*me).map(|n| n.0.clone()).unwrap_or("怪物".into());
                    let hp = w.get::<Stats>(*me).map(|s| s.hp).unwrap_or(0);
                    w.resource_mut::<EventLog>().push(format!("火球击中 {}！{}伤", name, dmg));
                    if hp <= 0 {
                        w.entity_mut(*me).despawn();
                    }
                }
                if !hit_any { w.resource_mut::<EventLog>().push(String::from("附近没有敌人")); }
            }
        }
        crate::SkillKind::Heal { amount } => {
            let mut w = world!(mut);
            if let Some(mut stats) = w.get_mut::<Stats>(entity) {
                stats.hp = (stats.hp + amount).min(stats.max_hp);
            }
            w.resource_mut::<EventLog>().push(format!("{}恢复了{}HP", skill_name, amount));
        }
        crate::SkillKind::Shield { def_boost, duration } => {
            let mut w = world!(mut);
            if let Some(mut buffs) = w.get_mut::<crate::Buffs>(entity) {
                buffs.shield_turns = duration as i32;
                buffs.shield_def = def_boost;
            }
            w.resource_mut::<EventLog>().push(format!("{}施放了护盾，防御+{}持续{}回合", skill_name, def_boost, duration));
        }
        crate::SkillKind::Berserk { atk_boost, duration } => {
            let mut w = world!(mut);
            if let Some(mut buffs) = w.get_mut::<crate::Buffs>(entity) {
                buffs.berserk_turns = duration as i32;
                buffs.berserk_atk = atk_boost;
            }
            w.resource_mut::<EventLog>().push(format!("{}进入狂暴，攻击+{}持续{}回合", skill_name, atk_boost, duration));
        }
    }
}
