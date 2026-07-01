use bevy_ecs::prelude::*;

// ── AI 行为 ─────────────────────────────────────────

#[derive(Clone, Debug)]
pub enum AiBehavior {
    FleeWhenHurt { hp_threshold: f32 },
    ChasePlayer,
    Wander,
}

#[derive(Component, Debug)]
pub struct MonsterBrain { pub behaviors: Vec<AiBehavior> }

impl MonsterBrain {
    pub fn creature() -> Self {
        Self {
            behaviors: vec![
                AiBehavior::FleeWhenHurt { hp_threshold: 0.25 },
                AiBehavior::ChasePlayer,
                AiBehavior::Wander,
            ],
        }
    }
}
