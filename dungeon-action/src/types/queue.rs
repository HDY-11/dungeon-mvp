//! Action queue data structures.

use super::action::{ActionKindV3, GameAction};
use bevy_ecs::prelude::*;
use dungeon_core::OptionLogExt;

#[derive(Debug)]
pub struct ActionEntry {
    pub entity: Entity,
    pub kind: ActionKindV3,
    pub action: Option<Box<dyn GameAction>>,
    pub av_remaining: f32,
}

impl Clone for ActionEntry {
    fn clone(&self) -> Self {
        ActionEntry {
            entity: self.entity,
            kind: self.kind.clone(),
            action: self.action.as_ref().map(|a| a.clone_box()),
            av_remaining: self.av_remaining,
        }
    }
}

#[derive(Resource, Default)]
pub struct ActionQueue {
    pub entries: Vec<ActionEntry>,
}

impl ActionQueue {
    pub fn enqueue(&mut self, entity: Entity, kind: ActionKindV3, av: f32) {
        self.entries.push(ActionEntry {
            entity,
            kind,
            action: None,
            av_remaining: av,
        });
    }

    pub fn advance(&mut self, amount: f32) {
        for entry in &mut self.entries {
            if entry.av_remaining > 0.0 {
                entry.av_remaining = (entry.av_remaining - amount).max(0.0);
            }
        }
    }

    pub fn next_event_distance(&self) -> Option<f32> {
        self.entries
            .iter()
            .filter(|e| e.av_remaining > 0.0)
            .map(|e| e.av_remaining)
            .min_by(|a, b| a.partial_cmp(b).expect_log("AV values should never be NaN"))
    }

    pub fn pop_ready(&mut self) -> Vec<ActionEntry> {
        let mut ready = Vec::new();
        self.entries.retain(|e| {
            if e.av_remaining <= 0.0 {
                ready.push(e.clone());
                false
            } else {
                true
            }
        });
        ready
    }

    pub fn has_entity(&self, entity: Entity) -> bool {
        self.entries.iter().any(|e| e.entity == entity)
    }

    pub fn enqueue_or_replace(&mut self, entity: Entity, kind: ActionKindV3, av: f32) {
        self.entries.retain(|e| e.entity != entity);
        self.entries.push(ActionEntry {
            entity,
            kind,
            action: None,
            av_remaining: av,
        });
    }
}
