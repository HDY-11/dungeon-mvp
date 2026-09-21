//! Skill execution.

use bevy_ecs::prelude::*;
use dungeon_core::{components::*, resources::*};

pub(crate) fn execute_skill(world: &mut World, entity: Entity, skill_idx: usize) {
    let (skill_kind, cost_mp, skill_name, skill_proficiency, magic_mastery);
    {
        let has_skill = world
            .get::<Skills>(entity)
            .map(|s| s.list.get(skill_idx).is_some())
            .unwrap_or(false);
        if !has_skill {
            world
                .resource_mut::<EventLog>()
                .push(dungeon_core::EventMessage::skill("技能未学习".to_string()));
            return;
        }
        let Some(skills) = world.get::<Skills>(entity) else {
            return;
        };
        let Some(skill) = skills.list.get(skill_idx) else {
            return;
        };
        let Some(stats) = world.get::<Stats>(entity) else {
            return;
        };
        if !stats.can_afford_mp(skill.cost_mp) {
            let msg = format!("MP不足，无法施放{}", skill.name);
            world
                .resource_mut::<EventLog>()
                .push(dungeon_core::EventMessage::skill(msg));
            return;
        }
        skill_kind = skill.kind.clone();
        cost_mp = skill.cost_mp;
        skill_name = skill.name.to_string();
        skill_proficiency = skill.proficiency;
        magic_mastery = stats.magic_mastery;
    }
    {
        if let Some(mut stats) = world.get_mut::<Stats>(entity) {
            stats.spend_mp(cost_mp);
        }
    }
    match skill_kind {
        dungeon_core::SkillKind::Heal { amount } => {
            let effective = amount + magic_mastery as i32 + skill_proficiency as i32 * 3;
            if let Some(mut stats) = world.get_mut::<Stats>(entity) {
                stats.heal(effective);
            }
            world
                .resource_mut::<EventLog>()
                .push(dungeon_core::EventMessage::skill(format!(
                    "{}恢复了{}HP（熟练度+{}）",
                    skill_name, effective, skill_proficiency
                )));
        }
        dungeon_core::SkillKind::Shield {
            def_boost,
            duration,
        } => {
            let effective_def = def_boost + skill_proficiency as i32 * 2;
            if let Some(mut ab) = world.get_mut::<ActiveBuffs>(entity) {
                let av = duration as f32 * 1000.0;
                ab.set(BuffKind::Shield, av, effective_def);
            }
            world
                .resource_mut::<EventLog>()
                .push(dungeon_core::EventMessage::skill(format!(
                    "{}施放了护盾，防御+{}持续{}秒（熟练度{}）",
                    skill_name, effective_def, duration, skill_proficiency
                )));
        }
        dungeon_core::SkillKind::Berserk {
            atk_boost,
            duration,
        } => {
            let effective_atk = atk_boost + skill_proficiency as i32 * 2;
            if let Some(mut ab) = world.get_mut::<ActiveBuffs>(entity) {
                let av = duration as f32 * 1000.0;
                ab.set(BuffKind::Berserk, av, effective_atk);
            }
            world
                .resource_mut::<EventLog>()
                .push(dungeon_core::EventMessage::skill(format!(
                    "{}进入狂暴，攻击{}持续{}秒（熟练度{}）",
                    skill_name, effective_atk, duration, skill_proficiency
                )));
        }
    }
}
