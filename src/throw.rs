//! 投掷模式：石子投掷瞄准 + 弹道可视化

use std::io;
use std::time::Instant;

use bevy_ecs::prelude::*;
use crossterm::event::{self, Event, KeyCode};
use dungeon_core::{
    ops, EventLog, Monster, Position, Stats,
    ThrowPreview, GameRng, Inventory, Equipment, EntityName,
    Player, FloorNumber, PendingExp, ItemPickup, LootTable,
    MAP_HEIGHT, MAP_WIDTH,
};
use dungeon_render::render_ui;
use ratatui::Terminal;

/// 打开投掷瞄准模式。
/// 返回 true 表示实际执行了投掷（消耗了石子）。
pub fn open_throw_mode(
    terminal: &mut Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
    world: &mut World,
    game_start: Instant,
) -> io::Result<bool> {
    // 以玩家位置为光标起点
    let (cx, cy) = {
        let mut q = world.try_query::<(&Player, &Position)>().expect("Player+Position registered");
        q.iter(&*world).next().map(|(_, p)| (p.x, p.y)).unwrap_or((MAP_WIDTH / 2, MAP_HEIGHT / 2))
    };

    // 初始化 ThrowPreview
    {
        let mut tp = world.resource_mut::<ThrowPreview>();
        tp.active = true;
        tp.cursor = (cx, cy);
        update_throw_path(world);
    }

    loop {
        let _ = terminal.draw(|frame| render_ui(frame, game_start, &*world));
        if let Ok(Event::Key(k)) = event::read() {
            match k.code {
                KeyCode::Up | KeyCode::Down | KeyCode::Left | KeyCode::Right
                | KeyCode::Home | KeyCode::End | KeyCode::PageUp | KeyCode::PageDown => {
                    let mut tp = world.resource_mut::<ThrowPreview>();
                    match k.code {
                        KeyCode::Up => tp.cursor.1 = tp.cursor.1.saturating_sub(1),
                        KeyCode::Down => tp.cursor.1 = (tp.cursor.1 + 1).min(MAP_HEIGHT - 1),
                        KeyCode::Left => tp.cursor.0 = tp.cursor.0.saturating_sub(1),
                        KeyCode::Right => tp.cursor.0 = (tp.cursor.0 + 1).min(MAP_WIDTH - 1),
                        KeyCode::Home => { tp.cursor.0 = 0; tp.cursor.1 = 0; }
                        KeyCode::End => { tp.cursor.0 = MAP_WIDTH - 1; tp.cursor.1 = MAP_HEIGHT - 1; }
                        KeyCode::PageUp => { tp.cursor.1 = tp.cursor.1.saturating_sub(5); }
                        KeyCode::PageDown => { tp.cursor.1 = (tp.cursor.1 + 5).min(MAP_HEIGHT - 1); }
                        _ => {}
                    }
                    update_throw_path(world);
                }
                KeyCode::Enter => {
                    let consumed = execute_throw(world);
                    world.resource_mut::<ThrowPreview>().active = false;
                    return Ok(consumed);
                }
                KeyCode::Char('x') | KeyCode::Char('X') | KeyCode::Esc => {
                    world.resource_mut::<ThrowPreview>().active = false;
                    return Ok(false);
                }
                _ => {}
            }
        }
    }
}

/// 根据光标位置更新 ThrowPreview 的 path 和 valid_target。
fn update_throw_path(world: &mut World) {
    let player_pos = world.try_query::<(&Player, &Position)>()
        .expect("Player+Position registered")
        .iter(world).next().map(|(_, p)| (p.x, p.y))
        .unwrap_or((0, 0));

    // 先读取 map（不可变），再获取 ThrowPreview（可变）
    let map;
    let path;
    let in_range;
    let los_clear;
    let cursor;
    {
        let tp = world.resource::<ThrowPreview>();
        cursor = tp.cursor;
    }
    let (cx, cy) = cursor;

    // 计算切比雪夫距离
    let dist_cheb = (cx as isize - player_pos.0 as isize).unsigned_abs()
        .max((cy as isize - player_pos.1 as isize).unsigned_abs());

    // 计算 Bresenham 路径（不含玩家起点，含目标格）
    path = ops::line_bresenham(player_pos.0, player_pos.1, cx, cy);

    // 射程检查 + 视线检查
    map = world.resource::<dungeon_core::Map>();
    in_range = dist_cheb <= 5;
    los_clear = path.iter().all(|&(px, py)| {
        (px == cx && py == cy) || !map.tiles[py][px].blocks_vision()
    });

    let mut tp = world.resource_mut::<ThrowPreview>();
    tp.path = path;
    tp.valid_target = in_range && los_clear;
}

/// 执行投掷：命中光标格上的怪物则扣血，移除 1 颗石子。
/// 返回 true 表示消耗了石子。
fn execute_throw(world: &mut World) -> bool {
    let tp = world.resource::<ThrowPreview>();
    if !tp.valid_target {
        world.resource_mut::<EventLog>().push("目标无效".to_string());
        return false;
    }
    let (tx, ty) = tp.cursor;

    // 查找光标格上的怪物
    let target = {
        let mut q = world.try_query::<(Entity, &Position, &Monster)>()
            .expect("Entity+Position+Monster registered");
        q.iter(world)
            .find(|(_, pos, _)| pos.x == tx && pos.y == ty)
            .map(|(e, _, _)| e)
    };

    if let Some(target_entity) = target {
        // 计算基础伤害
        let floor = world.resource::<FloorNumber>().0;
        let base_dmg = 3 + floor / 2;
        let extra = world.resource_mut::<GameRng>().random_range(0, 2) as u32;
        let target_def = world.get::<Stats>(target_entity)
            .map(|s| s.defense as i32)
            .unwrap_or(0);
        let raw_dmg = ((base_dmg as i32 + extra as i32 - target_def).max(1)) as u32;

        // 暴击检查（复用玩家装备的暴击率）
        let player_entity = ops::player_entity(world);
        let crit_mult = if let Some(p) = player_entity {
            let p_stats = world.get::<Stats>(p).cloned();
            let inv = world.get::<Inventory>(p).cloned();
            let eq = world.get::<Equipment>(p).cloned();
            let total_crit_rate = p_stats.as_ref().map(|s| {
                let bonus = inv.as_ref().zip(eq.as_ref())
                    .map(|(inv, eq)| dungeon_core::equipment_bonus(inv, eq))
                    .unwrap_or_default();
                (s.crit_rate + bonus.crit_rate).min(1.0)
            }).unwrap_or(0.05);

            let roll = world.resource_mut::<GameRng>().random_f32();
            if roll < total_crit_rate {
                p_stats.map(|s| 1.0 + s.crit_damage).unwrap_or(1.5)
            } else {
                1.0
            }
        } else {
            1.0
        };

        let final_dmg = (raw_dmg as f32 * crit_mult).round() as i32;

        // 扣血
        let target_name = world.get::<EntityName>(target_entity)
            .map(|n| n.0.clone())
            .unwrap_or("怪物".into());
        {
            if let Some(mut s) = world.get_mut::<Stats>(target_entity) {
                s.hp -= final_dmg;
            }
        }

        // 击杀处理
        if world.get::<Stats>(target_entity).map(|s| s.hp <= 0).unwrap_or(false) {
            let exp = world.get::<Stats>(target_entity).map(|s| s.exp).unwrap_or(0);
            world.resource_mut::<PendingExp>().amount += exp;
            world.resource_mut::<EventLog>()
                .push(format!("石子击杀了{}！获得{}经验", target_name, exp));

            // 掉落
            let loot = world.get::<LootTable>(target_entity).cloned();
            if let Some(lt) = loot {
                let mut rng2 = world.resource_mut::<GameRng>();
                let stacks = lt.roll(&mut rng2.rng);
                let pos = world.get::<Position>(target_entity).map(|p| (p.x, p.y));
                if let Some((px, py)) = pos {
                    for stack in &stacks {
                        let sname = stack.name();
                        world.resource_mut::<EventLog>()
                            .push(format!("{}掉落{}x{}", target_name, sname, stack.count));
                        world.spawn((
                            ItemPickup { stack: stack.clone() },
                            Position { x: px, y: py },
                            dungeon_core::Renderable { glyph: stack.glyph(), color: stack.color() },
                        ));
                    }
                }
            }
            world.entity_mut(target_entity).despawn();
        } else {
            let crit_text = if crit_mult > 1.0 { "暴击" } else { "" };
            world.resource_mut::<EventLog>()
                .push(format!("石子命中{}！造成{}伤害{}", target_name, final_dmg, crit_text));
        }

        true
    } else {
        world.resource_mut::<EventLog>().push("石子落在地上".to_string());
        true  // still consumes the stone
    }
}
