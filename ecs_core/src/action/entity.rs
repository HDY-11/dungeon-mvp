//! 行动实体链路（REFACTOR.md §3.6 / §11.3 Phase C；DESIGN DsnE8）。
//!
//! **一个行动 = 一个 actor 的瞬态子实体**，取代中央分派的 `ActionKind`。
//!
//! 本模块保留链路中**必须共享**的部分：
//!
//! - actor 行动状态机（`Idle`/`Active`/`Failure` 的唯一写入口 `begin_action` / `finish_action`）；
//! - action 实体组件（`ActionOf` 另有 ownership 模块）与优先级表；
//! - 速度规则 `SpeedRule` / `SpeedModifiers` 与共享查询别名；
//! - 终态出口 `ActionMessages`（执行器只能经它结束行动）。
//!
//! 链路其余部件按职责拆到兄弟模块：
//!
//! - [`crate::action::generation`]：AI/玩家生成（只 spawn 候选或 active action）；
//! - [`crate::action::arbitration`]：仲裁与 tick；
//! - [`crate::action::completion`]：终态回收与调度装配；
//! - [`crate::action::validation`] / [`crate::action::settle`]：判定与落地。
//!
//! 完整六阶段图见 `docs/architecture-map.html` 与 `action/mod.rs`。
//!
//! # 不存档
//!
//! action 实体是瞬态子实体；存档只存 actor 状态，读档后重新生成行动
//! （见 REFACTOR.md §10.7）。

use crate::action::ownership::ActionOf;
use crate::balance::action_av;
use crate::components::{Active, Failure, Idle, Position, Ready, Stats, Viewshed};
use crate::entity_cls::{Monster, Player};
use crate::events::{ActionFailedEvent, ActionSucceededEvent};

use crate::map::Map;
use crate::resources::OccupancyMap;
use crate::rules::{EffectSource, Modifier, StatId, apply_modifiers};
use bevy_ecs::prelude::*;
use bevy_ecs::query::Or;
use bevy_ecs::system::SystemParam;

// ── 查询与参数别名 ───────────────────────────────────
//
// 行动链路里同一段「actor 过滤条件」被五个生成系统重复，同一段
// 「到期行动过滤条件」被六个执行器重复，同一批「行动事件写入器」被三个执行器
// 重复。写成别名/`SystemParam` 之后，口径只有一份：改一次过滤条件不会漏改某个
// 生成系统或执行器。
//
// 顺带解决 `clippy::type_complexity` 与 `clippy::too_many_arguments`：
// 别名本身也是给读代码的人减少噪音，不只是为了让 lint 闭嘴。

/// AI 生成系统共同的 actor 过滤：是怪物、有对应能力、当前空闲或上次失败。
///
/// `Without<Active>` 挡的是「自己已经有行动」，注意它**挡不住**
/// 「actor 名下有 `ActiveAction` 子实体」——仲裁系统另有一道显式检查（§3.4）。
pub(crate) type AiActor<C> = (
    With<Monster>,
    With<C>,
    Without<Active>,
    Or<(With<Idle>, With<Failure>)>,
);

/// 玩家路径的 actor 数据：实体 id + 位置 + 基础属性（速度在 `Stats` 里）。
pub(crate) type PlayerActorData = (Entity, &'static Position, Option<&'static Stats>);

/// 玩家生成器要读的"世界现状"：自己、挡路的怪物、法术书/法力/视野、怪物位置。
///
/// 打成一个 [`SystemParam`] 而不是继续加参数：玩家生成系统原本已经有 9 个参数，
/// 每加一个能力（施法、投掷……）都会再撞一次 `clippy::too_many_arguments`。
/// 这里的字段回答的是同一个问题——"玩家此刻下令时，世界里有什么可参考"。
#[derive(SystemParam)]
pub struct PlayerWorld<'w, 's> {
    pub(crate) players: Query<'w, 's, PlayerActorData, (With<Player>, Without<Active>)>,
    pub(crate) monsters: Query<'w, 's, (), With<Monster>>,
    pub(crate) books: Query<
        'w,
        's,
        (
            &'static crate::spell::SpellBook,
            &'static crate::components::Mana,
            &'static Viewshed,
        ),
    >,
    pub(crate) monster_positions: Query<'w, 's, (Entity, &'static Position), With<Monster>>,
    /// 玩家身体容器（找副手槽里的可投掷物）。
    pub(crate) bodies: Query<'w, 's, &'static crate::item::Container>,
    /// 带 `Throw` 组件的物品实体（`Throw` 只由蓝图 throw 部件装配出来）。
    pub(crate) throwables: Query<'w, 's, &'static crate::item::Throw>,
}

/// 一个 AI actor 的数据：实体 id + 基础属性 + 所在格。
///
/// 用 `Option<&Stats>` 而不是必需组件——理由见 [`actor_speeds`]：
/// 必需组件会让漏挂属性的 actor 直接从查询里消失（LESSONS.md LECS21）。
///
/// `&Position` 是 `&'static`（必需）：收桶要读**脚下这一格的**修正（`EffectContext::collect`），
/// 没有位置就没有格子可言。缺位置的 actor 本来也无法执行移动类行动，
/// 因此这里用必需组件是"提前失败"，不是新的静默风险。
///
/// `With<C>` / `Without<Active>` / `Or<..>` 是 **filter**，不能写进数据元组，
/// 所以必须与 [`AiActor`] 一起作为 `Query` 的两个泛型参数给出去。
pub(crate) type AiActorData = (Entity, Option<&'static Stats>, &'static Position);


/// 校验阶段的范围：**该 actor 名下某种行动**，到点与否都要看（ISSUES ECS43）。
///
/// 这里**刻意不带** `With<Ready>`：保活检查必须覆盖"排队中"的行动，否则失效的行动会一直
/// 占住 actor 唯一的 `ActiveAction` 槽位、让它空转。要不要为它**定案**由校验器自己用
/// [`ActionEntry`] 里的 `Has<Ready>` 决定——两件事的范围不同，所以过滤器只能管前一件。
pub(crate) type PendingAction<A> = (With<ActiveAction>, With<A>);

/// 校验器读的行动数据：实体 id + 归属 + **是否已到点**。
///
/// `Has<Ready>` 是校验器唯一需要区分的两件事：
///
/// - **保活**（"还该做吗"）对**所有**待执行行动生效——到点与否都查；
/// - **定案**（"落在哪 / 打谁" + **预留格子**）只对**到点**的行动做。
///   提前定案没有意义：落点在被兑现之前可能已经过期，而预留还会白占一格。
///
/// 分开之后，`GameRng` 的消耗时机也不变：只有游荡的**定案**取随机方向，而它仍只在
/// 到点那一轮发生（"提前取随机数"会让同一颗种子的整局结果漂移）。
pub(crate) type ActionEntry = (Entity, &'static ActionOf, Has<Ready>);

/// 带 payload 的同上（`Move` / `BasicAttack`）。
pub(crate) type ActionEntryPayload<A> = (Entity, &'static ActionOf, &'static A, Has<Ready>);

/// 行动终态的**唯一出口**：执行器只能经它结束一个行动。
///
/// # 为什么把两个 `MessageWriter` 藏起来（Phase H1 / ECS35 / ECS36）
///
/// H1 之前，每个执行器手写三件事：`remove::<Ready>()`（只有 `Move` 写了）、
/// `write(ActionSucceededEvent)`、`write(ActionFailedEvent)`。这带来一个**编译器
/// 不会报错**的失败模式（ECS35）：新增执行器漏写终态事件 → actor 永久停在
/// `Active`（生成系统被 `Without<Active>` 挡住，不再为它产出候选）→ action 子实体
/// 永久泄漏，且无任何报错。
///
/// 现在**字段是私有的**：执行器拿不到 `MessageWriter`，只能调 [`Self::succeed`] 或
/// [`Self::fail`]，两者都**同时**做「清 `Ready`」+「恰好写一个事件」。于是
/// 「恰好一个终态事件 + `Ready` 被消费」从**约定**变成了**类型层面的保证**。
///
/// 清理放在这里（而不是留给 completion 的 `despawn` 顺带做）还有一层意义：
/// 它去掉了「`Ready` 清理由谁负责」这一处只有注释兜着的不变式（ECS36）。
#[derive(SystemParam)]
pub struct ActionMessages<'w, 's> {
    succeeded: MessageWriter<'w, ActionSucceededEvent>,
    failed: MessageWriter<'w, ActionFailedEvent>,
    /// `Commands<'w, 's>` 需要两个生命周期：`'w` 借 `Entities`，`'s` 借延迟命令队列
    /// （bevy 0.16 的 `Commands` 定义即如此，所以本 `SystemParam` 也必须带两个参数）。
    commands: Commands<'w, 's>,
}

impl ActionMessages<'_, '_> {
    /// 行动成功结束：清 `Ready` + 发 [`ActionSucceededEvent`]（恰好一个）。
    pub fn succeed(&mut self, action: Entity, actor: Entity) {
        self.commands.entity(action).remove::<Ready>();
        self.succeeded.write(ActionSucceededEvent { entity: actor });
    }

    /// 行动失败结束：清 `Ready` + 发 [`ActionFailedEvent`]（恰好一个）。
    ///
    /// 失败**不回收实体**——回收仍是 completion 的职责（见模块头分层说明）。
    pub fn fail(&mut self, action: Entity, actor: Entity) {
        self.commands.entity(action).remove::<Ready>();
        self.failed.write(ActionFailedEvent { entity: actor });
    }
}

/// 移动类执行器共同的只读世界信息。
#[derive(SystemParam)]
pub struct MovementContext<'w> {
    pub map: Res<'w, Map>,
    pub occupancy: Res<'w, OccupancyMap>,
}

// ── actor 行动状态的唯一实现 ──────────────────────────
//
// `Active` / `Idle` / `Failure` 构成 actor 的行动状态机。**只有下面两个函数允许改它们**，
// 三处调用点（玩家挂载、AI 仲裁、completion）共享同一份实现——
// 以前"挂载"和"仲裁"各自手写同一组三行，改一处漏一处就是状态机不一致
// （`ActionPhase::Arbitrate` 的注释还曾写着"唯一写 actor 行动状态的系统"，与事实不符）。

/// actor **开始**一个行动：清 `Idle`/`Failure`、置 `Active`。
///
/// 调用点：[`player_action_generation_system`]（玩家路径不参与 AI 仲裁）与
/// [`action_arbitration_system`]（AI 候选转正）。两处语义必须完全一致。
pub(crate) fn begin_action(actor_cmd: &mut EntityCommands) {
    actor_cmd.remove::<Idle>();
    actor_cmd.remove::<Failure>();
    actor_cmd.insert(Active);
}

/// actor **结束**一个行动：清 `Active`，按结果置 `Idle` / `Failure`。
///
/// 唯一调用点：completion（终态出口）。
pub(crate) fn finish_action(actor_cmd: &mut EntityCommands, succeeded: bool) {
    actor_cmd.remove::<Active>();
    if succeeded {
        actor_cmd.remove::<Failure>();
        actor_cmd.insert(Idle);
    } else {
        actor_cmd.remove::<Idle>();
        actor_cmd.insert(Failure);
    }
}

// ── 优先级表（REFACTOR.md §3.5） ──────────────────────
//
// 仲裁取**最小** `(ActionPriority, index_u32())`，所以优先级越高 = 数值越小。
// 与 §3.5 一一对应：Flee 200 > Chase 100 > Wander 50 > Wait 0。

pub const PRIORITY_FLEE: i32 = -200;
pub const PRIORITY_CHASE: i32 = -100;
pub const PRIORITY_WANDER: i32 = -50;
pub const PRIORITY_WAIT: i32 = 0;

/// 玩家行动不进入 AI 仲裁（§3.5「玩家行动不进入此表」），因此这里给一个
/// 明确凌驾于 AI 之上的值：即使未来玩家行动被写进仲裁路径，也不会被 AI 覆盖。
pub const PRIORITY_PLAYER: i32 = -1000;

// ── action 实体组件 ──────────────────────────────────

/// 仲裁排序键。**越小越优先**；同值由 `action_entity.index_u32()` 升序打破平局。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ActionPriority(pub i32);

/// 行动来源。玩家行动不参与 AI 仲裁（Phase C 才做），这里先只用于调试/日志。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionSource {
    Ai,
    Player,
}

/// 生成系统产出、等待仲裁的候选。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Candidate;

/// 仲裁选中、正在计时/执行的行动。一个 actor 至多一个。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ActiveAction;

/// 人类可读的行动名，只用于日志（§3.6.9 调试建议）。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionName(pub &'static str);

impl ActionName {
    /// 进行中事件的一句话（DsnE21）："哥布林举起了武器" 这类**已起手**的说法。
    ///
    /// 文案与动作标识放在一起：事件栏不会显示 `BasicAttack` 这种内部标识。
    /// 新增行动若忘了登记文案，第一次进入事件栏时会 panic——这里刻意不用
    /// `_ => "行动"` 的兜底，因为静默的兜底会让漏登记永远不被发现。已落定的事实
    /// 文案仍由规则层的日志给。
    pub fn phrase(self) -> &'static str {
        match self.0 {
            "Wander" => "游荡起来",
            "Flee" => "转身逃跑",
            "Wait" => "原地待命",
            "Chase" => "追了上去",
            "Move" => "迈开脚步",
            "BasicAttack" => "举起了武器",
            "Cast" => "开始施法",
            "Throw" => "投掷出去",
            other => panic!("未登记的行动名: {other}"),
        }
    }
}

// ── 行动类别 → 速度组件（Phase D / REFACTOR.md §2.6） ──
//
// 「哪一类行动受哪个速度影响」是**生成期的口径**，所以映射放在这里，
// 而不是散在各生成系统里。执行器完全不关心速度：AV 在挂载时就固化进
// `ActionTimer`，之后 tick 与执行只看剩余值。

/// 行动的速度规则：哪些行动吃哪个速度，以及哪些行动固定耗时。
///
/// 规则**由生成系统在编译期写死**，actor 侧只提供两个速度数值——这样生成系统
/// 不必再判断行动种类，也不会出现两个生成系统对同一行动给出不同口径。
///
/// 枚举而不是常量集合，是为了让非法组合无法表达：`Fixed` 没有速度字段，
/// 也就不存在「固定耗时却读了某个速度」的状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpeedRule {
    /// 读 `Stats` 里的 `MOVE_SPEED`：`Move` / `Chase` / `Flee` / `Wander`。
    Move,
    /// 读 `Stats` 里的 `ATTACK_SPEED`：`BasicAttack`。
    Attack,
    /// 固定耗时、不受任何速度影响：`Wait`（REFACTOR.md §11.6 第 4 项）。
    Fixed,
}

/// actor 的两个速度（缺失组件时已经是回退值）。
///
/// 与 [`SpeedRule`] 一起构成 AV 计算的**纯输入**：`action_av` 不需要
/// `World`/查询，因此口径可以在无 ECS 的单测里逐条钉住。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActorSpeeds {
    move_speed: f64,
    attack_speed: f64,
}

impl SpeedRule {
    /// 计算该规则下行动的 AV。
    ///
    /// 速度先经 [`SpeedModifiers::effective_speed`] 折算（求值侧第 ③④⑤ 步），
    /// 再交给 [`crate::balance::action_av`] 做 clamp 与除法。
    /// **`modifiers` 为空时逐值等于旧口径**——这就是接上求值侧的零行为变化保证。
    pub fn action_av(
        self,
        base_duration: f64,
        speeds: ActorSpeeds,
        modifiers: SpeedModifiers<'_>,
    ) -> f64 {
        self.action_av_ignoring(base_duration, speeds, modifiers, &[])
    }

    /// 同上，但显式给出"无视哪些来源"（`EffectSource` 列表）。
    ///
    /// 单独开一个方法而不是给 [`Self::action_av`] 再加参数：**绝大多数调用点没有
    /// 无视类效果**，让它们继续读作"算 AV"而不是"算 AV 并声明无视了什么"。
    pub fn action_av_ignoring(
        self,
        base_duration: f64,
        speeds: ActorSpeeds,
        modifiers: SpeedModifiers<'_>,
        ignored: &[EffectSource],
    ) -> f64 {
        match self {
            // `Move`/`Attack` 是"倍率速度"路径：修正器作用在**速度**上。
            SpeedRule::Move => action_av(
                base_duration,
                modifiers.effective(StatId::MOVE_SPEED, speeds.move_speed, ignored),
            ),
            SpeedRule::Attack => action_av(
                base_duration,
                modifiers.effective(StatId::ATTACK_SPEED, speeds.attack_speed, ignored),
            ),
            // `Fixed` 不吃任何速度（REFACTOR.md §11.6 第 4 项），因此也**不读修正器**：
            // 让 `Wait` 受地形减速影响是另一条设计决策，不该由这里顺手决定。
            SpeedRule::Fixed => base_duration,
        }
    }
}

/// 速度修正的来源：**actor 自带**（装备/状态效果）与**当前格**（地形）。
///
/// 这是求值五步里第 ② 步"收桶"的**输入形状**（DESIGN DsnE10 / Phase H8）：
/// 两个来源是两份列表，规则层拿到的仍是**一份** `&[Modifier]`——
/// "两个数据来源，一个答案"（DsnE13 第 3 条）。
///
/// 两个桶现在都有生产者：cell 来自格索引（H9），ctor 来自装备与状态效果（2026-09）。
/// 系统层才会往这两个桶里放东西。空桶下 [`effective_speed`] 逐值等于 base，
/// 这就是"接上求值侧"零行为变化的依据。
#[derive(Debug, Clone, Copy, Default)]
pub struct SpeedModifiers<'a> {
    /// actor 自带的修正（`EffectSource::Equipment` / `Status`），按 DsnE11 生成时收集。
    pub actor: &'a [Modifier],
    /// 当前格带来的修正（`EffectSource::Terrain`），来自格索引。
    pub cell: &'a [Modifier],
}

impl<'a> SpeedModifiers<'a> {
    /// 两个桶都为空——**当前所有生成/挂载路径都用这个**。
    pub const EMPTY: Self = Self {
        actor: &[],
        cell: &[],
    };

    /// 依次判断两个桶是否都为空。
    pub fn is_empty(&self) -> bool {
        self.actor.is_empty() && self.cell.is_empty()
    }

    /// 求值五步的第 ③④⑤ 步：过滤 → 折叠 → 有效速度。
    ///
    /// `ignored` 是"无视某类来源"的落点（例：无视地形减速的靴子 = `ignored` 含
    /// `EffectSource::Terrain`）。规则层**不解释**它，只丢掉对应桶。
    ///
    /// **不在算法层做 clamp**：`clamp_speed` 仍由 `balance::action_av` 负责，
    /// 这里的职责只是把效果折算成"有效速度"这一个数。两处都夹会让
    /// "夹取发生了几次"变得不可读，也会掩盖 `MIN/MAX_SPEED` 的真实作用点。
    pub fn effective(&self, stat: StatId, base: f64, ignored: &[EffectSource]) -> f64 {
        // 两个桶拼成一份列表再折叠："两个数据来源，一个答案"。
        // 用固定容量的小数组避免为每次 AV 计算分配 Vec（生成系统每轮都会调用）。
        match (self.actor.len(), self.cell.len()) {
            (0, 0) => base,
            _ => {
                let mut all: Vec<Modifier> = Vec::with_capacity(self.actor.len() + self.cell.len());
                all.extend_from_slice(self.actor);
                all.extend_from_slice(self.cell);
                apply_modifiers(stat, base, &all, ignored)
            }
        }
    }
}

/// 速度属性缺失时的回退值（= 基准 1.0）。
///
/// 生成系统用 `Option<&Stats>` 查询而不是必需组件：**Bevy 的查询遇到
/// 不匹配的 archetype 会静默返回空**（LESSONS.md LECS21）。若写成必需组件，
/// 一个漏挂 `Stats` 的 actor 会直接从 AI 里消失、既不行动也不报错；
/// 回退到基准 + 告警则让这种漏挂可见且不至于卡死行为。
const FALLBACK_SPEED: f64 = 1.0;

/// 从 `Stats` 解析 actor 的两个速度；缺失时告警并回退到基准 [`FALLBACK_SPEED`]。
///
/// 缺失**不应**被静默忽略（LESSONS.md LECS21 的同类问题：查询不匹配就是静静地
/// 什么都不做），所以回退前先告警，让「漏挂 Stats」在日志里可见。
pub(crate) fn actor_speeds(actor: Entity, stats: Option<&Stats>) -> ActorSpeeds {
    let speed_or_fallback = |stat: StatId| {
        stats.map_or_else(
            || {
                log::warn!(
                    "actor={actor:?} 缺少 Stats，按基准 {FALLBACK_SPEED} 计算 AV（缺 {}）",
                    stat.label()
                );
                FALLBACK_SPEED
            },
            |stats| stats.get(stat),
        )
    };
    ActorSpeeds {
        move_speed: speed_or_fallback(StatId::MOVE_SPEED),
        attack_speed: speed_or_fallback(StatId::ATTACK_SPEED),
    }
}

// ── 测试 ─────────────────────────────────────────────

#[cfg(test)]
mod tests {
    //! 行动实体链路的测试。两种放法：
    //!
    //! - `entity_tests.rs`：链路级用例（生成/仲裁/tick/执行/completion/parity）；
    //! - 本文件内联：**纯函数**（AV 公式、速度规则）的用例，不需要 World。
    //!
    //! 两者都在同一个 `mod tests` 下；链路级用例的历史路径是
    //! `action::entity::entity_tests::<name>`（D1 引入嵌套模块前是
    //! `action::entity::tests::<name>`，只影响 `--exact` 过滤）。

    use super::*;
    use crate::balance::{UNARMED_ATTACK_DURATION, WAIT_DURATION, WANDER_DURATION};
    use crate::balance::{MAX_SPEED, MIN_SPEED, clamp_speed};

    /// 浮点比较容差。
    ///
    /// 纯函数用例里尽量用 `assert_eq!`（这些比值的浮点结果恰好精确）；
    /// 需要跨运算顺序比较时才用它——`duration / speed` 与 `duration * factor`
    /// 的结合律不同，末几位会有 ~1e-13 的相对差异，那是浮点顺序而非口径差异。
    const EPS: f64 = 1e-9;

    /// 相对误差比较（`expected` 为 0 时退化成绝对误差）。
    fn approx_eq(actual: f64, expected: f64, what: &str) {
        let tolerance = EPS * expected.abs().max(1.0);
        assert!(
            (actual - expected).abs() <= tolerance,
            "{what}: actual={actual} expected={expected} diff={}",
            (actual - expected).abs()
        );
    }

    // ── Phase D：AV 公式不变式（旧 `Agility` 口径已在 D3 删除） ──

    /// AV 与速度严格成反比、与基础耗时严格成正比。
    ///
    /// 旧口径 `AV = 反应时 + duration × 敏捷系数` 两项都破坏了「AV 与 duration
    /// 成正比」：反应时是常数项，行动越短被它拖累得越狠。D3 之后这条不变式
    /// 成立，也是 GAME.md 能用「耗时 ÷ 速度」一句话写清数值的原因。
    #[test]
    fn av_is_inversely_proportional_to_speed_and_linear_in_duration() {
        // 与速度成反比：AV(speed) × speed 恒定。
        for speed in [0.5, 1.0, 1.25, 2.0, 3.0] {
            approx_eq(
                action_av(WANDER_DURATION, speed) * speed,
                WANDER_DURATION,
                &format!("speed={speed}: AV × speed 必须等于 base_duration"),
            );
        }

        // 单调：速度越高 AV 越短。
        let mut previous = f64::INFINITY;
        for speed in [0.25, 0.5, 1.0, 1.5, 2.0, 4.0] {
            let av = action_av(WANDER_DURATION, speed);
            assert!(av < previous, "speed={speed} 的 AV 必须严格更短");
            previous = av;
        }

        // 与基础耗时成正比：AV 之比只由 base_duration 决定，与速度无关。
        for speed in [0.25, 1.0, 4.0] {
            approx_eq(
                action_av(WANDER_DURATION, speed) / action_av(UNARMED_ATTACK_DURATION, speed),
                WANDER_DURATION / UNARMED_ATTACK_DURATION,
                &format!("speed={speed}: AV 之比必须等于 base_duration 之比"),
            );
        }
    }

    /// `clamp_speed` 必须夹住两端，并把非有限值兜成下限。
    ///
    /// 最后一条防的是「AV 变 NaN → `ActionTimer` 的 `partial_cmp().expect()` panic」
    /// 这条实测过的崩溃路径（`active_action_timer_delta` 里那个 expect）。
    #[test]
    fn clamp_speed_bounds_both_ends_and_non_finite_inputs() {
        assert_eq!(clamp_speed(1.0), 1.0, "区间内原样返回");
        assert_eq!(clamp_speed(MIN_SPEED), MIN_SPEED);
        assert_eq!(clamp_speed(MAX_SPEED), MAX_SPEED);

        assert_eq!(clamp_speed(0.0), MIN_SPEED, "0 速度必须夹到下限");
        assert_eq!(clamp_speed(-3.0), MIN_SPEED, "负速度必须夹到下限");
        assert_eq!(clamp_speed(1e9), MAX_SPEED, "过大速度必须夹到上限");

        assert_eq!(clamp_speed(f64::NAN), MIN_SPEED, "NaN 必须兜成下限");
        assert_eq!(clamp_speed(f64::INFINITY), MIN_SPEED, "inf 必须兜成下限");
        assert_eq!(
            clamp_speed(f64::NEG_INFINITY),
            MIN_SPEED,
            "-inf 必须兜成下限"
        );

        // 兜底的结果本身必须能算出有限 AV（否则只是把 NaN 推后一步）。
        for raw in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 0.0, -1.0] {
            assert!(
                action_av(WANDER_DURATION, raw).is_finite(),
                "raw={raw} 的 AV 必须有限"
            );
        }
    }

    /// 玩家初始速度是 1.25 —— 迁移映射的产物，不是设计值。
    ///
    /// 写成断言而不是注释，是因为「迁移来的数被当成设计基准」正是最容易悄悄
    /// 漂移的一类错误。GAME.md 里做 `[试调]` 重新校准（把玩家基准调回 1.0 并
    /// 重新配平怪物）时，这条断言必须一起改——那就是一次有意的决策。
    #[test]
    fn player_initial_speed_is_the_migration_value() {
        assert_eq!(crate::balance::PLAYER_MOVE_SPEED, 1.25);
        assert_eq!(crate::balance::PLAYER_ATTACK_SPEED, 1.25);
        assert_eq!(
            crate::balance::PLAYER_MOVE_SPEED,
            crate::balance::PLAYER_ATTACK_SPEED,
            "玩家移动与攻击同速（与怪物模板的 uniform() 一致）"
        );
        assert_eq!(
            action_av(UNARMED_ATTACK_DURATION, crate::balance::PLAYER_MOVE_SPEED),
            240.0,
            "玩家一次移动/攻击 = 300/1.25 = 240 AV"
        );
        // `action_av` 是通用公式，它**不**知道行动类别；「Wait 不吃速度」是
        // `SpeedRule::Fixed` 的语义，必须经由规则求值才成立。
        assert_eq!(
            SpeedRule::Fixed.action_av(
                WAIT_DURATION,
                ActorSpeeds {
                    move_speed: crate::balance::PLAYER_MOVE_SPEED,
                    attack_speed: crate::balance::PLAYER_ATTACK_SPEED,
                },
                SpeedModifiers::EMPTY,
            ),
            WAIT_DURATION,
            "Wait 必须固定耗时，即使玩家速度是 1.25"
        );
    }
    /// 速度规则 → 速度组件的映射（`Move` 用移动速度、`Attack` 用攻击速度）。
    ///
    /// 玩家移动耗时沿用 `UNARMED_ATTACK_DURATION`（旧口径就是这样，共享同一个
    /// 基础耗时），所以这里用 `WANDER_DURATION` 区分两类速度更直白。
    #[test]
    fn speed_rule_selects_the_matching_component() {
        let speeds = ActorSpeeds {
            move_speed: 0.5,
            attack_speed: 2.0,
        };

        assert_eq!(
            SpeedRule::Move.action_av(WANDER_DURATION, speeds, SpeedModifiers::EMPTY),
            WANDER_DURATION / speeds.move_speed,
            "移动类行动必须读 MoveSpeed"
        );
        assert_eq!(
            SpeedRule::Attack.action_av(
                UNARMED_ATTACK_DURATION,
                speeds,
                SpeedModifiers::EMPTY
            ),
            UNARMED_ATTACK_DURATION / speeds.attack_speed,
            "攻击类行动必须读 AttackSpeed"
        );
        assert_eq!(
            SpeedRule::Fixed.action_av(WAIT_DURATION, speeds, SpeedModifiers::EMPTY),
            WAIT_DURATION,
            "Wait 固定耗时，不受任何速度影响"
        );
    }

    // ── Phase H8：求值侧接线的形状（空桶零行为变化 / 无视是数据） ──

    /// H8 的接线保证：**两个桶都为空时，AV 逐值等于接上求值侧之前**。
    ///
    /// 这是"接上求值侧本身不改变任何现有行为"的直接证据，也是本阶段唯一
    /// 被允许的产出形式（Phase H8 只碰形状）。
    #[test]
    fn empty_speed_modifiers_keep_the_previous_av_exactly() {
        let speeds = ActorSpeeds {
            move_speed: 1.25,
            attack_speed: 0.8,
        };
        assert!(SpeedModifiers::EMPTY.is_empty());
        assert_eq!(
            SpeedRule::Move.action_av(WANDER_DURATION, speeds, SpeedModifiers::EMPTY),
            action_av(WANDER_DURATION, speeds.move_speed),
            "空桶必须等价于旧口径（移动）"
        );
        assert_eq!(
            SpeedRule::Attack.action_av(
                UNARMED_ATTACK_DURATION,
                speeds,
                SpeedModifiers::EMPTY
            ),
            action_av(UNARMED_ATTACK_DURATION, speeds.attack_speed),
            "空桶必须等价于旧口径（攻击）"
        );
    }

    /// 修正器作用在**速度**上，而不是作用在 AV 上：`AV = base / clamp(有效速度)`。
    ///
    /// 用一对互逆的修正（-0.25 与 ×0.5）把这层关系钉死：`(1.0 - 0.25) × 0.5 = 0.375`，
    /// 于是 AV = `WANDER_DURATION / 0.375`。
    #[test]
    fn speed_modifiers_fold_into_the_speed_not_the_av() {
        let mods = [
            Modifier::delta(StatId::MOVE_SPEED, EffectSource::Equipment, -0.25),
            Modifier::scaled(StatId::MOVE_SPEED, EffectSource::Status, 0.5),
        ];
        let bucket = SpeedModifiers {
            actor: &mods,
            cell: &[],
        };
        let speeds = ActorSpeeds {
            move_speed: 1.0,
            attack_speed: 1.0,
        };

        assert_eq!(bucket.effective(StatId::MOVE_SPEED, 1.0, &[]), 0.375);
        assert_eq!(
            SpeedRule::Move.action_av(WANDER_DURATION, speeds, bucket),
            WANDER_DURATION / 0.375,
            "AV 必须由折算后的有效速度算出"
        );
    }

    /// **"无视某类减速"是数据**：同一个地形修正，只因为 `ignored` 不同就得到不同 AV。
    ///
    /// 这条用例就是 DsnE10 第 ③ 步的可执行定义——规则里没有任何"若带了某装备则……"，
    /// 只有一个被丢掉的来源桶。
    #[test]
    fn ignoring_the_terrain_bucket_changes_the_av_without_any_special_case() {
        let terrain = [Modifier::delta(StatId::MOVE_SPEED, EffectSource::Terrain, -0.5)];
        let boots = [Modifier::delta(StatId::MOVE_SPEED, EffectSource::Equipment, 0.0)];
        let cell = SpeedModifiers {
            actor: &boots,
            cell: &terrain,
        };
        let speeds = ActorSpeeds {
            move_speed: 1.0,
            attack_speed: 1.0,
        };

        // 不无视：1.0 - 0.5 + 0.0 = 0.5 → AV 翻倍
        assert_eq!(
            SpeedRule::Move.action_av_ignoring(WANDER_DURATION, speeds, cell, &[]),
            WANDER_DURATION / 0.5
        );
        // 无视地形：只剩装备那条（+0.0）→ 速度 1.0 → AV 回到基准
        assert_eq!(
            SpeedRule::Move.action_av_ignoring(
                WANDER_DURATION,
                speeds,
                cell,
                &[EffectSource::Terrain]
            ),
            WANDER_DURATION,
            "无视地形之后不该再有地形减速"
        );
    }
}

// 链路级用例：独立的 `#[cfg(test)]` 兄弟模块（历史路径 `entity::tests::entity_tests::*`）。
#[cfg(test)]
#[path = "entity_tests.rs"]
mod entity_tests;
