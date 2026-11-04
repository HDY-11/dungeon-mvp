//! `rules::modifier` 的测试。
//!
//! 三类用例，分别对应三件必须钉住的事：
//!
//! 1. **空修正器 = 零行为变化**（H8 的验收线）：把现有数值接上求值侧，
//!    结果必须与没接之前逐值相等，连 NaN/inf 的兜底路径也不例外；
//! 2. **过滤是数据**：无视某一类来源只影响该桶，"无视"本身不进规则分支；
//! 3. **选择是数据**（2026-09 物品轮新增）：一条修正只作用于它声明的 `Stat`，
//!    攻击加成绝不混进速度——这条漏了不会报错，只会悄悄把数值算错。

use super::*;

/// 本模块绝大多数用例折算的是移动速度；单独给个短名字，避免每行都写 `Stat::MoveSpeed`
/// 把"这条用例其实在测什么"淹掉。
const SPEED: Stat = Stat::MoveSpeed;

/// H8 验收：修正器为**空**时 `apply_modifiers` 必须逐值等于 `base`。
///
/// 这条保证"接上求值侧"不改变任何现有行为——在还没有任何效果来源的阶段
/// （H8 只有形状，H9/H11 才有真实的 bucket 内容），这条就是"零行为变化"的全部依据。
#[test]
fn empty_modifier_list_is_a_no_op() {
    for base in [
        0.0, 0.25, 0.8, 1.0, 1.25, 2.0, 4.0, f64::MIN_POSITIVE, 1e300, -3.5,
    ] {
        assert_eq!(
            apply_modifiers(SPEED, base, &[], &[]),
            base,
            "空修正器列表必须原样返回 base={base}"
        );
    }
}

/// 与上一条同样的保证，但覆盖"非有限输入"的兜底路径。
///
/// `clamp_speed` 会把 NaN/inf 夹成 `MIN_SPEED`，因此 `action_av` 的输入
/// 在正常路径上已是有限的；这里单独钉住 `apply_modifiers` 自己**不引入**新的
/// NaN 语义（空列表下 NaN 进、NaN 出——不做任何自作主张的替换）。
#[test]
fn empty_modifier_list_preserves_non_finite_inputs() {
    assert!(apply_modifiers(SPEED, f64::NAN, &[], &[]).is_nan());
    assert_eq!(
        apply_modifiers(SPEED, f64::INFINITY, &[], &[]),
        f64::INFINITY
    );
    assert_eq!(
        apply_modifiers(SPEED, f64::NEG_INFINITY, &[], &[]),
        f64::NEG_INFINITY
    );
}

/// 基础值修正位：加在 base 上。
#[test]
fn base_delta_shifts_the_value() {
    let mods = [
        Modifier::delta(SPEED, EffectSource::Terrain, -0.5),
        Modifier::delta(SPEED, EffectSource::Equipment, 0.25),
    ];
    assert_eq!(apply_modifiers(SPEED, 1.0, &mods, &[]), 0.75);
}

/// 乘区位：作为独立因子相乘，而不是加到基础值上。
#[test]
fn multipliers_compose_as_factors() {
    let mods = [
        Modifier::scaled(SPEED, EffectSource::Status, 2.0),
        Modifier::scaled(SPEED, EffectSource::Terrain, 0.5),
    ];
    // 加算会得到 1.0 + 1.5 = 2.5；乘算是 1.0 × 2.0 × 0.5 = 1.0。
    // 这条用例的作用就是把这个区别钉死，避免"看起来一样"的实现漂移。
    assert_eq!(apply_modifiers(SPEED, 1.0, &mods, &[]), 1.0);
}

/// 折叠顺序：**先基础值、后乘区**（`[暂定]` 口径，见 `apply_modifiers` 的说明）。
///
/// 顺序反过来会得到不同结果，因此它必须是一条被钉住的语义而不是实现细节：
/// `(1.0 + 1.0) × 3.0 = 6.0`，而 `1.0 × 3.0 + 1.0 = 4.0`。
#[test]
fn fold_order_is_delta_then_multiplier() {
    let mods = [
        Modifier::delta(SPEED, EffectSource::Equipment, 1.0),
        Modifier::scaled(SPEED, EffectSource::Status, 3.0),
    ];
    assert_eq!(apply_modifiers(SPEED, 1.0, &mods, &[]), 6.0);
}

/// 一条修正两个位同时生效（`Modifier` 的两个数值字段不是二选一）。
#[test]
fn a_single_modifier_can_use_both_slots() {
    let mods = [Modifier {
        stat: SPEED,
        source: EffectSource::Equipment,
        base_delta: -1.0,
        multiplier: 0.5,
    }];
    // (1.25 - 1.0) × 0.5 = 0.125（取值全为二进制可精确表示的数）
    assert_eq!(apply_modifiers(SPEED, 1.25, &mods, &[]), 0.125);
}

/// **"无视某类" = 丢掉那一桶**（DsnE10 第 ③ 步），只影响被无视的来源。
///
/// 这是本项目要的形状：规则里没有"若带了某装备则……"的分支，
/// 只有"这一桶丢了"这一条数据。
///
/// **取值刻意用二进制可精确表示的数**（0.5 / 0.25 / -0.25）：`0.1` 一类十进制小数
/// 在 `f64` 里本就不精确（`1.0 - 0.5 + 0.1 - 0.2` 得到 `0.39999999999999997`），
/// 拿它做 `assert_eq!` 是在测浮点表示而不是测折叠逻辑。
#[test]
fn ignoring_a_source_drops_only_that_bucket() {
    let mods = [
        Modifier::delta(SPEED, EffectSource::Terrain, -0.5),
        Modifier::delta(SPEED, EffectSource::Equipment, 0.25),
        Modifier::delta(SPEED, EffectSource::Status, -0.25),
    ];

    // 全都要：1.0 - 0.5 + 0.25 - 0.25 = 0.5
    assert_eq!(apply_modifiers(SPEED, 1.0, &mods, &[]), 0.5);

    // 无视地形：1.0 + 0.25 - 0.25 = 1.0（地形那一桶整体消失，其余不受影响）
    assert_eq!(
        apply_modifiers(SPEED, 1.0, &mods, &[EffectSource::Terrain]),
        1.0
    );

    // 无视多个来源（含乘区那条一起丢）
    let mods_with_factor = [
        Modifier::delta(SPEED, EffectSource::Terrain, -0.5),
        Modifier::scaled(SPEED, EffectSource::Terrain, 0.5),
        Modifier::delta(SPEED, EffectSource::Equipment, 0.25),
    ];
    assert_eq!(
        apply_modifiers(SPEED, 1.0, &mods_with_factor, &[EffectSource::Terrain]),
        1.25
    );
}

/// 无视**全部**来源等于"只剩 base"——与空列表同结果。
///
/// 这条用例把"过滤"与"没有修正"两条路径接在一起：若哪天有人给 `ignored`
/// 加了副作用（例如顺手改 base），它会立刻失败。
#[test]
fn ignoring_every_source_equals_no_modifiers() {
    let mods = [
        Modifier::delta(SPEED, EffectSource::Terrain, -0.5),
        Modifier::scaled(SPEED, EffectSource::Status, 3.0),
        Modifier::delta(SPEED, EffectSource::Equipment, 0.1),
    ];
    let all = [
        EffectSource::Intrinsic,
        EffectSource::Equipment,
        EffectSource::Status,
        EffectSource::Terrain,
    ];
    assert_eq!(apply_modifiers(SPEED, 1.25, &mods, &all), 1.25);
    assert_eq!(apply_modifiers(SPEED, 1.25, &[], &[]), 1.25);
}

/// 分解读数：谁参与了、谁被无视了——日志/调试面板要能解释"为什么是这个数"。
#[test]
fn evaluate_reports_what_was_applied_and_what_was_ignored() {
    let mods = [
        Modifier::delta(SPEED, EffectSource::Terrain, -0.5),
        Modifier::delta(SPEED, EffectSource::Terrain, 0.25),
        Modifier::delta(SPEED, EffectSource::Equipment, 0.25),
    ];

    let outcome = evaluate_modifiers(SPEED, 1.0, &mods, &[EffectSource::Terrain]);
    assert_eq!(outcome.value, 1.25);
    assert_eq!(outcome.relevant, 3, "三条都作用于移动速度");
    assert_eq!(outcome.applied, 1, "只有装备那一条参与了折叠");
    assert_eq!(outcome.ignored, 2, "地形那两条被丢掉");

    // 与纯折叠结果一致（两条入口不许分叉）。
    assert_eq!(
        outcome.value,
        apply_modifiers(SPEED, 1.0, &mods, &[EffectSource::Terrain])
    );
}

// ── 第 ④ 步的"选择"：Stat 维度（2026-09 物品轮） ──────

/// 一条修正只作用于它声明的那一项：攻击加成不会漏进速度。
///
/// 这是物品落地时补上的维度。没有它，一把"攻击 +3"的剑会让玩家移动速度也涨 3
/// ——数值串味且**不会报错**，只能靠"手感不对"发现。
#[test]
fn a_modifier_only_affects_the_stat_it_declares() {
    let mods = [
        Modifier::delta(Stat::Attack, EffectSource::Equipment, 3.0),
        Modifier::delta(SPEED, EffectSource::Equipment, -0.25),
    ];

    assert_eq!(
        apply_modifiers(Stat::Attack, 8.0, &mods, &[]),
        11.0,
        "攻击吃攻击那一条"
    );
    assert_eq!(
        apply_modifiers(SPEED, 1.0, &mods, &[]),
        0.75,
        "速度只吃速度那一条，绝不把攻击的 +3 也算进来"
    );
    assert_eq!(
        apply_modifiers(Stat::Defense, 4.0, &mods, &[]),
        4.0,
        "没有任何一条作用于防御 → 逐值等于 base"
    );
}

/// `Stat` 与 `EffectSource` 是**正交**的两维：无视一个来源只丢那个来源，
/// 不会顺手丢掉"同一来源作用于别的项"的修正。
///
/// 反例（这条用例存在的意义）：若有人把"无视地形"实现成"丢掉列表里第一段满足
/// 条件的元素"，或者把两个维度混成一个过滤，这里就会失败。
#[test]
fn source_filtering_and_stat_selection_do_not_interfere() {
    let mods = [
        Modifier::delta(SPEED, EffectSource::Terrain, -0.5),
        Modifier::delta(Stat::Attack, EffectSource::Terrain, 2.0),
        Modifier::delta(SPEED, EffectSource::Equipment, 0.25),
    ];

    // 无视地形：速度丢掉地形那一条，装备那条仍在。
    assert_eq!(
        apply_modifiers(SPEED, 1.0, &mods, &[EffectSource::Terrain]),
        1.25
    );
    // 同一份 `ignored` 作用在攻击上：攻击的地形加成也被丢掉。
    assert_eq!(
        apply_modifiers(Stat::Attack, 8.0, &mods, &[EffectSource::Terrain]),
        8.0
    );
}

/// `for_stat` 是选择的**唯一**实现：拆分读数与折叠必须挑出同一批修正。
#[test]
fn for_stat_and_the_fold_agree_on_what_is_relevant() {
    let mods = [
        Modifier::delta(Stat::Attack, EffectSource::Equipment, 3.0),
        Modifier::scaled(Stat::Attack, EffectSource::Status, 1.5),
        Modifier::delta(SPEED, EffectSource::Terrain, -0.5),
    ];

    let selected: Vec<&Modifier> = for_stat(&mods, Stat::Attack).collect();
    assert_eq!(selected.len(), 2, "两条作用于攻击");
    assert_eq!(
        evaluate_modifiers(Stat::Attack, 10.0, &mods, &[]).relevant,
        selected.len(),
        "分解读数里的 relevant 必须与 for_stat 一致"
    );
}

/// 详情面板/日志用的修正描述（纯显示，不影响折叠）。
///
/// 把"这条修正写了什么"钉在一条用例里：显示层不重新推导数值，只读 `describe()`；
/// 将来若改文案，改的是同一处，日志与详情面板一起变。
#[test]
fn modifiers_describe_themselves() {
    assert_eq!(
        Modifier::delta(Stat::Attack, EffectSource::Equipment, 1.0).describe(),
        "攻击 +1"
    );
    assert_eq!(
        Modifier::scaled(Stat::MoveSpeed, EffectSource::Terrain, 0.8).describe(),
        "移动速度 ×0.8"
    );
    let both = Modifier {
        stat: Stat::AttackSpeed,
        source: EffectSource::Equipment,
        base_delta: -2.0,
        multiplier: 1.25,
    };
    assert_eq!(both.describe(), "攻击速度 -2 ×1.25");
    // 两个位都中性时也要有可见的文字，避免详情里出现只有属性名的空行。
    assert_eq!(
        Modifier::delta(Stat::Defense, EffectSource::Equipment, 0.0).describe(),
        "防御 +0"
    );
}
