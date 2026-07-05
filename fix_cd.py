import re

with open('dungeon-core/src/action.rs', 'r') as f:
    content = f.read()

# 1. Simplify ActionEntry - remove cooldown_remaining
content = content.replace(
    '''/// 行动队列条目
#[derive(Clone, Debug)]
pub struct ActionEntry {
    pub entity: Entity,
    pub kind: ActionKindV3,
    /// 反应时剩余（来自实体的 Reaction.time，入队时填入）
    pub reaction_remaining: f32,
    /// 冷却剩余（来自动作的 duration，执行后填入）
    pub cooldown_remaining: f32,
}''',
    '''/// 行动队列条目
#[derive(Clone, Debug)]
pub struct ActionEntry {
    pub entity: Entity,
    pub kind: ActionKindV3,
    /// 反应时剩余（来自实体的 Reaction.time，入队时填入）
    pub reaction_remaining: f32,
}'''
)

# 2. Simplify enqueue - remove duration parameter
content = content.replace(
    '''    pub fn enqueue(&mut self, entity: Entity, kind: ActionKindV3, reaction_time: f32, duration: f32) {
        self.entries.push(ActionEntry {
            entity,
            kind,
            reaction_remaining: reaction_time,
            cooldown_remaining: 0.0, // 执行后才填入 duration
        });
    }''',
    '''    pub fn enqueue(&mut self, entity: Entity, kind: ActionKindV3, reaction_time: f32) {
        self.entries.push(ActionEntry {
            entity,
            kind,
            reaction_remaining: reaction_time,
        });
    }'''
)

# 3. Simplify advance - only reaction_remaining matters
content = content.replace(
    '''    pub fn advance(&mut self, amount: f32) {
        for entry in &mut self.entries {
            if entry.reaction_remaining > 0.0 {
                entry.reaction_remaining = (entry.reaction_remaining - amount).max(0.0);
            } else if entry.cooldown_remaining > 0.0 {
                entry.cooldown_remaining = (entry.cooldown_remaining - amount).max(0.0);
            }
        }
    }''',
    '''    pub fn advance(&mut self, amount: f32) {
        for entry in &mut self.entries {
            if entry.reaction_remaining > 0.0 {
                entry.reaction_remaining = (entry.reaction_remaining - amount).max(0.0);
            }
        }
    }'''
)

# 4. Simplify next_event_distance
content = content.replace(
    '''    pub fn next_event_distance(&self) -> Option<f32> {
        self.entries
            .iter()
            .filter(|e| e.reaction_remaining > 0.0 || e.cooldown_remaining > 0.0)
            .map(|e| {
                if e.reaction_remaining > 0.0 {
                    e.reaction_remaining
                } else {
                    e.cooldown_remaining
                }
            })
            .min_by(|a, b| a.partial_cmp(b).unwrap())
    }''',
    '''    pub fn next_event_distance(&self) -> Option<f32> {
        self.entries
            .iter()
            .filter(|e| e.reaction_remaining > 0.0)
            .map(|e| e.reaction_remaining)
            .min_by(|a, b| a.partial_cmp(b).unwrap())
    }'''
)

# 5. Simplify pop_ready
content = content.replace(
    '''    pub fn pop_ready(&mut self) -> Vec<ActionEntry> {
        let mut ready = Vec::new();
        self.entries.retain(|e| {
            if e.reaction_remaining <= 0.0 && e.cooldown_remaining <= 0.0 {
                ready.push(e.clone());
                false
            } else {
                true
            }
        });
        ready
    }''',
    '''    pub fn pop_ready(&mut self) -> Vec<ActionEntry> {
        let mut ready = Vec::new();
        self.entries.retain(|e| {
            if e.reaction_remaining <= 0.0 {
                ready.push(e.clone());
                false
            } else {
                true
            }
        });
        ready
    }'''
)

# 6. Update enqueue call in run_monster_decision (remove duration)
content = content.replace(
    'queue.enqueue(*entity, kind.clone(), *reaction_time, *duration);',
    'queue.enqueue(*entity, kind.clone(), *reaction_time);',
)

with open('dungeon-core/src/action.rs', 'w') as f:
    f.write(content)
print('OK')
