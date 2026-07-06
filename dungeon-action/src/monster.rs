//! 怪物决策：遍历怪物收集就绪行动，按优先级仲裁入队

use dungeon_core::{
    world, action_types::*, components::*,
};
use bevy_ecs::prelude::*;

/// 遍历所有怪物，检查各 Action 组件的条件，收集就绪行动，按优先级入队
pub fn run_monster_decision() {
    let mut collected: Vec<(Entity, u32, f32, ActionKindV3)> = Vec::new();
    {
        let w = world!();
        let player_pos = w.try_query::<(&Player, &Position)>().unwrap().iter(&w).next().map(|(_, p)| (p.x, p.y));

        for (entity, chase, _stats, view, reaction) in
            w.try_query::<(Entity, &CanChase, &Stats, &Viewshed, &Reaction)>().unwrap().iter(&w)
        {
            let can_see = player_pos.map_or(false, |pp| view.visible_tiles.contains(&pp));
            if CanChase::condition(can_see) {
                let av = reaction.time + chase.duration;
                collected.push((entity, chase.priority, av, ActionKindV3::Chase));
            }
        }

        for (entity, flee, stats, reaction) in
            w.try_query::<(Entity, &CanFlee, &Stats, &Reaction)>().unwrap().iter(&w)
        {
            let hp_ratio = stats.hp as f32 / stats.max_hp as f32;
            if CanFlee::condition(hp_ratio) {
                let av = reaction.time + flee.duration;
                collected.push((entity, flee.priority, av, ActionKindV3::Flee));
            }
        }

        for (entity, wander, reaction) in
            w.try_query::<(Entity, &CanWander, &Reaction)>().unwrap().iter(&w)
        {
            if !collected.iter().any(|(e, _, _, _)| *e == entity) && CanWander::condition() {
                let av = reaction.time + wander.duration;
                collected.push((entity, wander.priority, av, ActionKindV3::Wander));
            }
        }
    }

    collected.sort_by(|(_, pa, _, _), (_, pb, _, _)| {
        pb.cmp(pa).then_with(|| dungeon_core::global::rand_u8().cmp(&dungeon_core::global::rand_u8()))
    });

    let mut w = world!(mut);
    let mut queue = w.resource_mut::<ActionQueue>();
    for (entity, _priority, av, kind) in &collected {
        if !queue.has_entity(*entity) {
            queue.enqueue(*entity, kind.clone(), *av);
        }
    }
}
