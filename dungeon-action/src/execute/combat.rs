//! Combat execution: melee attack, monster attack, critical hit, death handling.

use bevy_ecs::prelude::*;
use dungeon_core::OptionLogExt;
use dungeon_core::{components::*, items::*, ops, resources::*};

pub(crate) fn adjacent_8(world: &World, a: Entity, b: Entity) -> bool {
    world
        .get::<Position>(a)
        .zip(world.get::<Position>(b))
        .map(|(pa, pb)| pa.x.abs_diff(pb.x) <= 1 && pa.y.abs_diff(pb.y) <= 1)
        .unwrap_or(false)
}

pub(crate) fn monster_attack_player(world: &mut World, entity: Entity, player_entity: Entity) {
    let (monster_atk, mon_stats) = world
        .get::<Stats>(entity)
        .map(|s| (s.attack as i32, s.clone()))
        .unwrap_or((1, dungeon_core::Stats::player()));
    let player_def = world
        .query::<(&Player, &Stats, &Equipment, Option<&ActiveBuffs>)>()
        .iter(world)
        .next()
        .map(|(_, ps, eq, ab)| ops::effective_defense(ps, eq, ab) as i32)
        .unwrap_or(0);
    let crit_roll = world.resource_mut::<GameRng>().random_f32();
    let (is_crit, crit_mult) =
        calc_crit(&mon_stats, &dungeon_core::StatBonus::default(), crit_roll);
    let base_dmg = (monster_atk - player_def).max(1);
    let dmg = if is_crit {
        (base_dmg as f32 * crit_mult).round() as i32
    } else {
        base_dmg
    };
    let name = world
        .get::<EntityName>(entity)
        .map(|n| n.0.clone())
        .unwrap_or("怪物".into());
    if let Some(mut ps) = world.get_mut::<Stats>(player_entity) {
        ps.take_damage(dmg);
    }
    let crit_suffix = if is_crit { "（暴击）" } else { "" };
    world
        .resource_mut::<EventLog>()
        .push(dungeon_core::EventMessage::danger(format!(
            "{}{} 攻击了你，{}伤害",
            name, crit_suffix, dmg
        )));
}

pub(crate) fn calc_crit(stats: &Stats, bonus: &dungeon_core::StatBonus, crit_roll: f32) -> (bool, f32) {
    let total_crit_rate = (stats.crit_rate + bonus.crit_rate).min(1.0);
    let is_crit = total_crit_rate > crit_roll;
    let crit_mult = if is_crit {
        1.0 + stats.crit_damage
    } else {
        1.0
    };
    (is_crit, crit_mult)
}

pub(crate) fn execute_attack(world: &mut World, attacker: Entity, target: Entity) {
    if !adjacent_8(world, attacker, target) {
        return;
    }
    let (name, atk_name, dmg, crit);
    {
        let Some(target_stats) = world.get::<Stats>(target).cloned() else {
            return;
        };
        let Some(attacker_stats) = world.get::<Stats>(attacker).cloned() else {
            return;
        };
        name = world
            .get::<EntityName>(target)
            .map(|n| n.0.clone())
            .unwrap_or("怪物".into());
        atk_name = world
            .get::<AttackName>(attacker)
            .map(|a| a.0.clone())
            .unwrap_or("攻击".into());
        let equipment = world
            .get::<Equipment>(attacker)
            .expect_log("Attacker has Equipment");

        let ab = world.get::<ActiveBuffs>(attacker);
        let effective_atk = ops::effective_attack(&attacker_stats, equipment, ab) as i32;
        let target_def = {
            let eq = world.get::<Equipment>(target);
            ops::effective_defense(&target_stats, &eq.cloned().unwrap_or_default(), None) as i32
        };
        let raw_dmg = (effective_atk - target_def).max(1);
        let equip = world.get::<Equipment>(attacker);
        let bonus = equip.map(dungeon_core::equipment_bonus).unwrap_or_default();
        let crit_roll = world.resource_mut::<GameRng>().random_f32();
        (dmg, crit) = {
            let (is_crit, crit_mult) = calc_crit(&attacker_stats, &bonus, crit_roll);
            let d = if is_crit {
                (raw_dmg as f32 * crit_mult).round() as i32
            } else {
                raw_dmg
            };
            (d, is_crit)
        };
    }
    {
        let Some(mut target_stats) = world.get_mut::<Stats>(target) else {
            return;
        };
        target_stats.take_damage(dmg);
        if target_stats.is_dead() {
            handle_kill(world, target, &name);
        } else {
            world
                .resource_mut::<EventLog>()
                .push(dungeon_core::EventMessage::combat(format!(
                    "你{}了{}{}，造成{}点伤害",
                    atk_name,
                    name,
                    if crit { "！暴击" } else { "" },
                    dmg
                )));
        }
    }
}

pub(crate) fn calc_player_crit(world: &World, crit_roll: f32) -> (bool, f32) {
    let p = ops::player_entity(world);
    match p {
        Some(p) => {
            let p_stats = world.get::<Stats>(p);
            let equip = world.get::<Equipment>(p);
            let bonus = equip.map(dungeon_core::equipment_bonus).unwrap_or_default();
            match p_stats {
                Some(stats) => calc_crit(stats, &bonus, crit_roll),
                None => (false, 1.0),
            }
        }
        None => (false, 1.0),
    }
}

pub(crate) fn handle_kill(world: &mut World, entity: Entity, name: &str) {
    let exp = world.get::<Stats>(entity).map(|s| s.exp).unwrap_or(0);
    world.resource_mut::<PendingExp>().amount += exp;
    world
        .resource_mut::<EventLog>()
        .push(dungeon_core::EventMessage::combat(format!(
            "击杀了{}！获得{}经验",
            name, exp
        )));
    let pos = world.get::<Position>(entity).map(|p| (p.x, p.y));
    let loot_stacks = {
        let lt = world.get::<LootTable>(entity).cloned();
        lt.map(|l| {
            let mut rng = world.resource_mut::<GameRng>();
            l.roll(&mut rng)
        })
        .unwrap_or_default()
    };
    if let Some((px, py)) = pos {
        for stack in &loot_stacks {
            let sname = stack.name();
            world
                .resource_mut::<EventLog>()
                .push(dungeon_core::EventMessage::item(format!(
                    "{}掉落{}x{}",
                    name, sname, stack.count
                )));
            world.spawn((
                ItemPickup {
                    stack: stack.clone(),
                },
                Position { x: px, y: py },
                Renderable {
                    glyph: stack.glyph(),
                    color: stack.color(),
                },
            ));
        }
    }
    world.entity_mut(entity).despawn();
}
