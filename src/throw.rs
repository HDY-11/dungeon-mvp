//! 投掷模式工具函数：弹道计算 + 副手管理

use bevy_ecs::prelude::*;
use dungeon_core::{
    Equipment, EventLog, Inventory, LookCursor, OptionLogExt, Player, Position, ThrowPreview, ops,
};

pub fn update_throw_path(world: &mut World) {
    let player_pos = match world.try_query::<(&Player, &Position)>() {
        Some(mut q) => q
            .iter(world)
            .next()
            .map(|(_, p)| (p.x, p.y))
            .unwrap_or((0, 0)),
        None => (0, 0),
    };

    // Phase 1: 只读收集（所有 &World 借用在此完成）
    let cursor = world.resource::<ThrowPreview>().cursor;
    let (cx, cy) = cursor;
    // I68: 射程/视线判定收敛到 core（与 execute_throw::validate_throw 同一实现）
    let in_range = ops::chebyshev(player_pos, (cx, cy)) <= dungeon_core::THROW_RANGE;
    let los_clear = {
        let map = world.resource::<dungeon_core::Map>();
        ops::los_clear(map, player_pos, (cx, cy))
    }; // &Map 借用在此结束
    let path = ops::line_bresenham(player_pos.0, player_pos.1, cx, cy);

    // Phase 2: 写入（&mut World）
    let mut tp = world.resource_mut::<ThrowPreview>();
    tp.path = path;
    tp.valid_target = in_range && los_clear;
}

/// 副手是否持有可投掷物（I73 收敛：game/throw_select 共用同一判定）
pub fn has_throwable_offhand(world: &World) -> bool {
    world
        .try_query::<(&Player, &Equipment)>()
        .and_then(|mut q| {
            q.iter(world).next().map(|(_, eq)| {
                eq.off_hand
                    .as_ref()
                    .map(|s| dungeon_core::is_throwable(s.item_id))
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

/// 进入投掷瞄准（I73 收敛：game/throw_select 的重复初始化块）。
/// 副手可投掷则初始化 ThrowPreview/LookCursor 并 push ThrowAim；返回 true。
/// 无投掷物返回 false，调用方决定后续（进选择页/提示）。
pub fn try_enter_throw_aim(world: &mut World) -> bool {
    if !has_throwable_offhand(world) {
        return false;
    }
    let (cx, cy) = {
        let mut q = world
            .try_query::<(&Player, &Position)>()
            .expect_log("Player+Position registered");
        q.iter(world)
            .next()
            .map(|(_, p)| (p.x, p.y))
            .unwrap_or((0, 0))
    };
    world.insert_resource(ThrowPreview {
        active: true,
        cursor: (cx, cy),
        path: Vec::new(),
        valid_target: false,
    });
    world.insert_resource(LookCursor {
        active: true,
        x: cx,
        y: cy,
    });
    update_throw_path(world);
    world
        .resource_mut::<dungeon_action::PageStack>()
        .push(dungeon_action::Page::ThrowAim);
    true
}
/// 从背包里找一个投掷物装到副手
/// 副手已有投掷物 → 跳过；副手有不可投掷物（如木盾）→ 先放回背包再装填（I60）
/// G24: 原子语义（Dsn10）——旧副手放不回背包（背包满）时放弃装填，绝不静默丢失物品
pub fn auto_equip_throwable(world: &mut World) {
    let Some(player) = dungeon_core::ops::player_entity(world) else {
        return;
    };
    // 副手已有可投掷物则跳过
    let has_throwable = world
        .get::<Equipment>(player)
        .and_then(|eq| eq.off_hand.as_ref())
        .map(|s| dungeon_core::is_throwable(s.item_id))
        .unwrap_or(false);
    if has_throwable {
        return;
    }
    // 副手有不可投掷物 → 预检背包空间后再卸下（G24：放不回则回滚，保持副手原状）
    let old = {
        world
            .get_mut::<Equipment>(player)
            .and_then(|mut eq| eq.off_hand.take())
    };
    if let Some(old) = &old {
        let can = world
            .get::<Inventory>(player)
            .map(|inv| inv.can_add(old.item_id, old.count))
            .unwrap_or(false);
        if !can {
            // 背包满：旧装备放回副手，放弃本次装填
            if let Some(mut eq) = world.get_mut::<Equipment>(player) {
                eq.off_hand = Some(old.clone());
            }
            world
                .resource_mut::<EventLog>()
                .push(dungeon_core::EventMessage::item(
                    "背包已满，无法替换副手".to_string(),
                ));
            return;
        }
        if let Some(mut inv) = world.get_mut::<Inventory>(player) {
            inv.add(old.item_id, old.count);
        }
    }
    // 从背包找石子（ITEM_STONE）
    if let Some(mut inv) = world.get_mut::<Inventory>(player) {
        let idx = inv
            .stacks
            .iter()
            .position(|s| s.item_id == dungeon_core::ITEM_STONE);
        if let Some(i) = idx {
            let stack = inv.stacks.remove(i);
            if let Some(mut eq) = world.get_mut::<Equipment>(player) {
                eq.off_hand = Some(stack);
            }
        }
    }
}
