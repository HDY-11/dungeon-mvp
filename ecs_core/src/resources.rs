//! 领域资源：与实体无关的全局状态。

use crate::map::{MAP_HEIGHT, MAP_WIDTH};
use bevy_ecs::prelude::*;
use std::collections::HashMap;

// ── 随机数 ───────────────────────────────────────────

/// 可序列化的 xorshift64* 随机源。
///
/// 所有游戏规则随机数都应走这个状态机，便于未来存档回放。
#[derive(Resource, Debug, Clone, PartialEq, Eq)]
pub struct GameRng {
    pub state: u64,
    pub steps: u64,
}

impl GameRng {
    pub fn new(seed: u64) -> Self {
        // splitmix64 混洗，避免简单种子退化。
        let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        Self {
            state: (z ^ (z >> 31)) | 1,
            steps: 0,
        }
    }

    pub const fn from_state(state: u64, steps: u64) -> Self {
        Self { state, steps }
    }

    /// 生成 `[0, 1)` 的 f64（消耗 1 步，53 位精度）。
    pub fn random_f64(&mut self) -> f64 {
        (self.xorshift_next() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// 生成 `[0, 1)` 的 f32（消耗 1 步，24 位精度）。
    pub fn random_f32(&mut self) -> f32 {
        (self.xorshift_next() >> 40) as f32 / (1u64 << 24) as f32
    }

    /// 生成 `[lo, hi)` 的随机整数（消耗 1 步；要求 `lo < hi`）。
    pub fn random_range(&mut self, lo: u64, hi: u64) -> u64 {
        debug_assert!(lo < hi);
        if hi <= lo {
            return lo;
        }
        lo + (self.xorshift_next() % (hi - lo))
    }

    fn xorshift_next(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        self.steps = self.steps.wrapping_add(1);
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
}

impl rand::rand_core::TryRng for GameRng {
    type Error = std::convert::Infallible;

    fn try_next_u32(&mut self) -> Result<u32, Self::Error> {
        Ok(self.xorshift_next() as u32)
    }

    fn try_next_u64(&mut self) -> Result<u64, Self::Error> {
        Ok(self.xorshift_next())
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), Self::Error> {
        let mut chunks = dest.chunks_exact_mut(8);
        for chunk in &mut chunks {
            chunk.copy_from_slice(&self.xorshift_next().to_le_bytes());
        }
        let rem = chunks.into_remainder();
        if !rem.is_empty() {
            let v = self.xorshift_next().to_le_bytes();
            rem.copy_from_slice(&v[..rem.len()]);
        }
        Ok(())
    }
}

// ── 地图/视野记忆 ───────────────────────────────────

#[derive(Resource, Clone, Copy)]
pub struct MapMemory {
    pub explored: [[bool; MAP_WIDTH]; MAP_HEIGHT],
}

impl MapMemory {
    pub fn new() -> Self {
        Self {
            explored: [[false; MAP_WIDTH]; MAP_HEIGHT],
        }
    }
}

impl Default for MapMemory {
    fn default() -> Self {
        Self::new()
    }
}

/// 派生 RNG 的**用途**：世界构造期每种用途拿一条独立的确定性随机流。
///
/// # 为什么世界构造期用派生 RNG，而不是 `GameRng`
///
/// `GameRng` 的 `state`/`steps` 是**回放与存档**的基础（DESIGN DsnX17 ③），它是
/// "游戏进行中"的随机源。而世界构造（地图 / 楼梯 / 怪物 / 泥潭 / 物品）发生在
/// **开局那一刻**，由 `(MapSeed, 楼层)` 完全决定——把它塞进 `GameRng` 只会让
/// "世界长什么样"与"玩家怎么打"共用一条流，改动任一方都会平移另一方。
///
/// 所以 DsnE5 的"唯一随机源"是**两种合法形态**：运行期 `GameRng`，构造期派生 RNG。
/// 代价是派生种子不出现在 `GameRng.steps` 里（ISSUES ECS34），因此这里做了两件事：
///
/// 1. **唯一派生入口**：[`derived_rng`]，偏移集中在本枚举（以前是四处的
///    `wrapping_add(777/999/1234/4321)`，散在各系统里，改一处无人知道）；
/// 2. **偏移只能追加**：改值或重排会让同种子世界整体变化，`world/init_tests.rs`
///    的 golden 断言会立刻失败。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DerivePurpose {
    /// 楼梯落点（含连接通道的挖掘）。
    Stairs,
    /// 怪物种群（密度层 + 元胞扩散 + 逐只选种）。
    Monsters,
    /// 格子效果（泥潭）。
    CellEffects,
    /// 地面物品。
    GroundItems,
}

impl DerivePurpose {
    /// 该用途的种子偏移。**只能末尾追加；改值 = 改同种子世界。**
    const fn offset(self) -> u64 {
        match self {
            DerivePurpose::Stairs => 777,
            DerivePurpose::Monsters => 999,
            DerivePurpose::CellEffects => 1234,
            DerivePurpose::GroundItems => 4321,
        }
    }
}

/// **构造期的唯一随机源入口**：`(MapSeed, 楼层)` + 用途 → 确定性随机流。
///
/// 与 [`GameRng`] 的分工见 [`DerivePurpose`]。除本函数外，`ecs_core` 里不应再有
/// `SmallRng::seed_from_u64`（`docs_check.py` 会拦），唯一的例外是地图生成内部
/// 为了"子阶段互不干扰"而用同一 seed 重建的局部流。
pub fn derived_rng(map_seed: u64, floor: u32, purpose: DerivePurpose) -> rand::rngs::SmallRng {
    use rand::SeedableRng;
    let base = crate::world::init::map_seed_for_floor(map_seed, floor);
    rand::rngs::SmallRng::seed_from_u64(base.wrapping_add(purpose.offset()))
}

/// 世界时钟：**AV 时间轴**。
///
/// 状态效果（中毒等）按它计时，而不是按"结算了多少轮"（ISSUES ECS46）——
/// 玩家的一次行动会跨越若干个世界轮，用轮次计时会让"毒多久"随场上怪物数量漂移。
///
/// 只在 `tick_action_timers_system` 里推进：那一轮推进了多少 AV，就是这一轮的时间流逝。
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct WorldClock {
    /// 累计推进的 AV（单调递增，可用于"效果从何时开始"之类的查询）。
    pub total_av: f64,
    /// 上一轮推进了多少 AV（`0` = 本轮没有行动到点，时间没走）。
    pub last_step_av: f64,
}

/// 最后看到的实体位置。实体离开视野后保留记忆，直到再次看见或实体销毁。
#[derive(Resource, Default)]
pub struct VisibleMemory {
    pub entries: HashMap<Entity, (usize, usize)>,
}

/// 碰撞占用图：每格被哪个实体占据。
#[derive(Resource)]
pub struct OccupancyMap {
    pub cells: [[Option<Entity>; MAP_WIDTH]; MAP_HEIGHT],
}

impl OccupancyMap {
    pub fn new() -> Self {
        Self {
            cells: [[None; MAP_WIDTH]; MAP_HEIGHT],
        }
    }

    pub fn is_occupied(&self, x: usize, y: usize) -> bool {
        if x >= MAP_WIDTH || y >= MAP_HEIGHT {
            return true;
        }
        self.cells[y][x].is_some()
    }

    pub fn entity_at(&self, x: usize, y: usize) -> Option<Entity> {
        if x >= MAP_WIDTH || y >= MAP_HEIGHT {
            return None;
        }
        self.cells[y][x]
    }

    pub fn set(&mut self, x: usize, y: usize, entity: Entity) {
        if x < MAP_WIDTH && y < MAP_HEIGHT {
            self.cells[y][x] = Some(entity);
        }
    }

    /// 清空某一格（移动写穿时腾出旧位置用）。
    pub fn clear_at(&mut self, x: usize, y: usize) {
        if x < MAP_WIDTH && y < MAP_HEIGHT {
            self.cells[y][x] = None;
        }
    }

    pub fn clear(&mut self) {
        self.cells = [[None; MAP_WIDTH]; MAP_HEIGHT];
    }
}

impl Default for OccupancyMap {
    fn default() -> Self {
        Self::new()
    }
}

// ── 日志 ─────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventLevel {
    Combat,
    Item,
    Skill,
    System,
    Danger,
}

#[derive(Debug, Clone)]
pub struct EventMessage {
    pub level: EventLevel,
    pub text: String,
    /// 同一条消息连续出现了几次（>1 时后端显示 `(×N)`）。
    ///
    /// 为什么要合并：中毒每个发作点推一条，一次玩家行动可能推 2–3 条，
    /// 直接把"变异蝎子倒下了""你捡起了踏水靴"这类一次性信息挤出日志区（ISSUES SYN8）。
    pub repeat: u32,
}

impl EventMessage {
    pub fn combat(text: impl Into<String>) -> Self {
        Self { level: EventLevel::Combat, text: text.into(), repeat: 1 }
    }
    pub fn item(text: impl Into<String>) -> Self {
        Self { level: EventLevel::Item, text: text.into(), repeat: 1 }
    }
    pub fn skill(text: impl Into<String>) -> Self {
        Self { level: EventLevel::Skill, text: text.into(), repeat: 1 }
    }
    pub fn system(text: impl Into<String>) -> Self {
        Self { level: EventLevel::System, text: text.into(), repeat: 1 }
    }
    pub fn danger(text: impl Into<String>) -> Self {
        Self { level: EventLevel::Danger, text: text.into(), repeat: 1 }
    }
}

#[derive(Resource)]
pub struct EventLog {
    pub messages: Vec<EventMessage>,
    max: usize,
}

impl EventLog {
    pub const fn new() -> Self {
        Self { messages: Vec::new(), max: 50 }
    }

    pub fn push(&mut self, msg: EventMessage) {
        // 连续相同的消息合并成一条并计数，而不是一条条堆进日志（ISSUES SYN8）。
        if let Some(last) = self.messages.last_mut()
            && last.level == msg.level
            && last.text == msg.text
        {
            last.repeat = last.repeat.saturating_add(1);
            return;
        }
        self.messages.push(msg);
        if self.messages.len() > self.max {
            self.messages.remove(0);
        }
    }
}

impl Default for EventLog {
    fn default() -> Self {
        Self::new()
    }
}

// ── 世界状态 ─────────────────────────────────────────

#[derive(Resource)]
pub struct TurnManager {
    pub game_over: bool,
    pub wants_quit: bool,
}

impl TurnManager {
    pub const fn new() -> Self {
        Self { game_over: false, wants_quit: false }
    }
}

impl Default for TurnManager {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Resource, Clone, Copy)]
pub struct FloorNumber(pub u32);

#[derive(Resource, Clone, Copy)]
pub struct MapSeed(pub u64);
