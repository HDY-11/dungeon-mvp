//! 行动系统 v3 — 组件化行动 + 全局队列 + 统一输入
//!
//! 每个行动是一个独立 Component，含 cooldown/reaction_time/priority/timer。
//! 系统收集条件满足的行动 → 仲裁 → 入全局队列 → 推进执行。

use crate::world;
use crate::{Stats, Viewshed, Player, Position, EntityName, Monster};
use bevy_ecs::prelude::*;

// ══════════════════════════════════════════════════════
// Action 组件
// ══════════════════════════════════════════════════════

/// 移动行动
#[derive(Component, Clone, Debug)]
pub struct CanMove {
    pub cooldown: f32,
    pub cooldown_remaining: f32,
    pub reaction_time: f32,
    pub reaction_remaining: f32,
    pub priority: u32,
}

impl CanMove {
    pub fn new(priority: u32) -> Self {
        Self {
            cooldown: 250.0,
            cooldown_remaining: 0.0,
            reaction_time: 50.0,
            reaction_remaining: 0.0,
            priority,
        }
    }

    /// 判断是否能执行移动
    pub fn condition(target_is_walkable: bool, target_is_occupied_by_enemy: bool) -> bool {
        target_is_walkable || target_is_occupied_by_enemy
    }
}

/// 追击行动（怪物专用）
#[derive(Component, Clone, Debug)]
pub struct CanChase {
    pub cooldown: f32,
    pub cooldown_remaining: f32,
    pub reaction_time: f32,
    pub reaction_remaining: f32,
    pub priority: u32,
}

impl CanChase {
    pub fn new(priority: u32) -> Self {
        Self {
            cooldown: 250.0,
            cooldown_remaining: 0.0,
            reaction_time: 100.0,
            reaction_remaining: 0.0,
            priority,
        }
    }

    pub fn condition(can_see_player: bool) -> bool {
        can_see_player
    }
}

/// 逃跑行动（怪物专用）
#[derive(Component, Clone, Debug)]
pub struct CanFlee {
    pub cooldown: f32,
    pub cooldown_remaining: f32,
    pub reaction_time: f32,
    pub reaction_remaining: f32,
    pub priority: u32,
}

impl CanFlee {
    pub fn new(priority: u32) -> Self {
        Self {
            cooldown: 250.0,
            cooldown_remaining: 0.0,
            reaction_time: 50.0,
            reaction_remaining: 0.0,
            priority,
        }
    }

    pub fn condition(hp_ratio: f32) -> bool {
        hp_ratio < 0.25
    }
}

/// 游荡行动（怪物专用）
#[derive(Component, Clone, Debug)]
pub struct CanWander {
    pub cooldown: f32,
    pub cooldown_remaining: f32,
    pub reaction_time: f32,
    pub reaction_remaining: f32,
    pub priority: u32,
}

impl CanWander {
    pub fn new(priority: u32) -> Self {
        Self {
            cooldown: 500.0,
            cooldown_remaining: 0.0,
            reaction_time: 50.0,
            reaction_remaining: 0.0,
            priority,
        }
    }

    pub fn condition() -> bool {
        true // 兜底行动，始终可执行
    }
}

/// 等待行动（玩家/怪物通用）
#[derive(Component, Clone, Debug)]
pub struct CanWait {
    pub cooldown: f32,
    pub cooldown_remaining: f32,
    pub reaction_time: f32,
    pub reaction_remaining: f32,
    pub priority: u32,
}

impl CanWait {
    pub fn new(priority: u32) -> Self {
        Self {
            cooldown: 800.0,
            cooldown_remaining: 0.0,
            reaction_time: 10.0,
            reaction_remaining: 0.0,
            priority,
        }
    }

    pub fn condition() -> bool {
        true // 始终可等待
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

#[derive(Clone, Debug)]
pub struct ActionEntry {
    pub entity: Entity,
    pub kind: ActionKindV3,
    pub reaction_remaining: f32,
    pub cooldown_remaining: f32,
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
    pub fn enqueue(&mut self, entity: Entity, kind: ActionKindV3, reaction_time: f32) {
        self.entries.push(ActionEntry {
            entity,
            kind,
            reaction_remaining: reaction_time,
            cooldown_remaining: 0.0,
        });
    }

    pub fn advance(&mut self, amount: f32) {
        for entry in &mut self.entries {
            if entry.reaction_remaining > 0.0 {
                entry.reaction_remaining = (entry.reaction_remaining - amount).max(0.0);
            } else if entry.cooldown_remaining > 0.0 {
                entry.cooldown_remaining = (entry.cooldown_remaining - amount).max(0.0);
            }
        }
    }

    /// 找最小正 reaction_remaining 或 cooldown_remaining（= 下一次事件的距离）
    pub fn next_event_distance(&self) -> Option<f32> {
        self.entries
            .iter()
            .filter(|e| e.reaction_remaining > 0.0 || e.cooldown_remaining > 0.0)
            .map(|e| {
                if e.reaction_remaining > 0.0 {
                    e.reaction_remaining
                } else {
                    e.cooldown_remaining
                }
            })
            .min_by(|a, b| a.partial_cmp(b).unwrap())
    }

    /// 取出所有 reaction_remaining ≤ 0 且 cooldown_remaining ≤ 0 的条目
    pub fn pop_ready(&mut self) -> Vec<ActionEntry> {
        let mut ready = Vec::new();
        self.entries.retain(|e| {
            if e.reaction_remaining <= 0.0 && e.cooldown_remaining <= 0.0 {
                ready.push(e.clone());
                false
            } else {
                true
            }
        });
        ready
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

/// 临时收集待仲裁的行动请求
struct ActionRequest {
    entity: Entity,
    kind: ActionKindV3,
    priority: u32,
    reaction_time: f32,
}

/// 遍历所有怪物，检查各 Action 组件的条件，收集就绪行动，按优先级入队
pub fn run_monster_decision() {
    // 阶段 1：在独立作用域收集数据，所有权释放后再操作
    let mut collected: Vec<(Entity, u32, f32, ActionKindV3)> = Vec::new();
    {
        let mut w = world!(mut);
        let player_pos = w.query::<(&Player, &Position)>().iter(&mut *w).next().map(|(_, p)| (p.x, p.y));

        for (entity, chase, stats, view) in w.query::<(Entity, &CanChase, &Stats, &Viewshed)>().iter(&mut *w)
        {
            if chase.cooldown_remaining > 0.0 { continue; }
            let can_see = player_pos.map_or(false, |pp| view.visible_tiles.contains(&pp));
            if CanChase::condition(can_see) {
                collected.push((entity, chase.priority, chase.reaction_time, ActionKindV3::Chase));
            }
        }

        for (entity, flee, stats) in w.query::<(Entity, &CanFlee, &Stats)>().iter(&mut *w) {
            if flee.cooldown_remaining > 0.0 { continue; }
            let hp_ratio = stats.hp as f32 / stats.max_hp as f32;
            if CanFlee::condition(hp_ratio) {
                collected.push((entity, flee.priority, flee.reaction_time, ActionKindV3::Flee));
            }
        }

        for (entity, wander) in w.query::<(Entity, &CanWander)>().iter(&mut *w) {
            if wander.cooldown_remaining > 0.0 { continue; }
            if !collected.iter().any(|(e, _, _, _)| *e == entity) && CanWander::condition() {
                collected.push((entity, wander.priority, wander.reaction_time, ActionKindV3::Wander));
            }
        }
    }

    // 阶段 2：排序
    collected.sort_by(|(_, pa, _, _), (_, pb, _, _)| {
        pb.cmp(pa).then_with(|| crate::global::rand_u8().cmp(&crate::global::rand_u8()))
    });

    // 阶段 3：入队
    let mut w = world!(mut);
    let mut seen = std::collections::HashSet::new();
    let mut queue = w.resource_mut::<ActionQueue>();
    for (entity, _priority, reaction_time, kind) in &collected {
        if seen.insert(*entity) {
            queue.enqueue(*entity, kind.clone(), *reaction_time);
        }
    }
}

// ══════════════════════════════════════════════════════
// 行动执行引擎：推进队列 + 保活检查 + 执行
// ══════════════════════════════════════════════════════

/// 推进一次事件（到下一个 reaction 结束或 cooldown 结束）
pub fn advance_action_queue() {
    let mut w = world!(mut);
    let dist = {
        let queue = w.resource::<ActionQueue>();
        queue.next_event_distance().unwrap_or(0.0)
    };
    if dist <= 0.0 { return; }

    // 推进
    w.resource_mut::<ActionQueue>().advance(dist);

    // 取出就绪的
    let ready = w.resource_mut::<ActionQueue>().pop_ready();
    for entry in ready {
        execute_entry(&entry);
    }
}

/// 执行一个行动条目（保活检查 + 调用 execute）
fn execute_entry(entry: &ActionEntry) {
    match &entry.kind {
        ActionKindV3::Chase => execute_chase(entry.entity),
        ActionKindV3::Flee => execute_flee(entry.entity),
        ActionKindV3::Wander => execute_wander(entry.entity),
        ActionKindV3::Wait => execute_wait(entry.entity),
        _ => {} // Move/Attack/Skill 由玩家输入驱动
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

    // 设置冷却
    if let Some(mut chase) = w.get_mut::<CanChase>(entity) {
        chase.cooldown_remaining = chase.cooldown;
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
    if let Some(mut flee) = w.get_mut::<CanFlee>(entity) {
        flee.cooldown_remaining = flee.cooldown;
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
    if let Some(mut wander) = w.get_mut::<CanWander>(entity) {
        wander.cooldown_remaining = wander.cooldown;
    }
}

fn execute_wait(entity: Entity) {
    // 等待只是消耗时间，不需要实际行动
    if let Some(mut wait) = world!(mut).get_mut::<CanWait>(entity) {
        wait.cooldown_remaining = wait.cooldown;
    }
}

/// 对所有实体递减冷却计时器（每 tick 调用）
pub fn tick_all_cooldowns(amount: f32) {
    let mut w = world!(mut);
    for mut move_ in w.query::<&mut CanMove>().iter_mut(&mut *w) {
        if move_.cooldown_remaining > 0.0 {
            move_.cooldown_remaining = (move_.cooldown_remaining - amount).max(0.0);
        }
    }
    for mut chase in w.query::<&mut CanChase>().iter_mut(&mut *w) {
        if chase.cooldown_remaining > 0.0 {
            chase.cooldown_remaining = (chase.cooldown_remaining - amount).max(0.0);
        }
    }
    for mut flee in w.query::<&mut CanFlee>().iter_mut(&mut *w) {
        if flee.cooldown_remaining > 0.0 {
            flee.cooldown_remaining = (flee.cooldown_remaining - amount).max(0.0);
        }
    }
    for mut wander in w.query::<&mut CanWander>().iter_mut(&mut *w) {
        if wander.cooldown_remaining > 0.0 {
            wander.cooldown_remaining = (wander.cooldown_remaining - amount).max(0.0);
        }
    }
    for mut wait in w.query::<&mut CanWait>().iter_mut(&mut *w) {
        if wait.cooldown_remaining > 0.0 {
            wait.cooldown_remaining = (wait.cooldown_remaining - amount).max(0.0);
        }
    }
}
