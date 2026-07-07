//! 存档/读档

use dungeon_core::{
    components::*, items::*, resources::*,
    Map, Tile, MAP_WIDTH, MAP_HEIGHT,
    ActionQueue, ActionEntry, ActionKindV3, InputBuffer, PlayerPreview,
    ChaseIntents, FleeIntents, WanderIntents,
    Reaction, agility_to_reaction,
    CanMove, CanChase, CanFlee, CanWander, CanWait,
};

use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct SavedStack {
    pub item_id: usize,
    pub count: u32,
}

/// 可序列化的行动种类（不含 Attack — 含 Entity 引用无法跨存档）
#[derive(Serialize, Deserialize, Clone)]
pub enum SavedActionKind {
    Move { dx: isize, dy: isize },
    Chase, Flee, Wander, Wait,
    Skill(usize),
}

/// 可序列化的行动条目（按实体位置 + 行动种类标识）
#[derive(Serialize, Deserialize, Clone)]
pub struct SavedActionEntry {
    pub x: u16, pub y: u16,         // 实体所在位置（用于 restore 时重映射 Entity）
    pub kind: SavedActionKind,
    pub av_remaining: f32,
}

/// 可序列化的意图条目（按实体位置 + 优先级 + AV + 种类）
#[derive(Serialize, Deserialize, Clone)]
pub struct SavedIntentEntry {
    pub x: u16, pub y: u16,
    pub priority: u32,
    pub av: f32,
    pub kind: SavedActionKind,
}

#[derive(Serialize, Deserialize)]
pub struct GameSave {
    pub floor: u32,
    pub map_seed: u64,
    pub px: u16, pub py: u16,
    pub st: SavedStats,
    pub inv: Vec<SavedStack>,
    pub weapon_item_id: Option<usize>, pub weapon_count: Option<u32>,
    pub armor_item_id: Option<usize>, pub armor_count: Option<u32>,
    pub ring_item_id: Option<usize>, pub ring_count: Option<u32>,
    pub buffs: SavedBuffs,
    pub map_tiles: Vec<Tile>,
    pub rooms: Vec<dungeon_core::Room>,
    pub explored: Vec<u8>,
    pub monsters: Vec<SavedMonster>,
    pub items: Vec<SavedGroundItem>,
    pub sx: u16, pub sy: u16,
    pub player_class: Option<PlayerClass>,
    pub action_queue: Vec<SavedActionEntry>,
    #[serde(default)]
    pub chase_intents: Vec<SavedIntentEntry>,
    #[serde(default)]
    pub flee_intents: Vec<SavedIntentEntry>,
    #[serde(default)]
    pub wander_intents: Vec<SavedIntentEntry>,
}

#[derive(Serialize, Deserialize)]
pub struct SavedStats {
    pub level: u32, pub hp: i32, pub max_hp: i32, pub mp: i32, pub max_mp: i32,
    pub exp: u64, pub exp_to_next: u64,
    pub attack: u32, pub defense: u32, pub magic_mastery: u32, pub agility: u32,
    pub crit_rate: f32, pub crit_damage: f32,
}

impl From<Stats> for SavedStats {
    fn from(s: Stats) -> Self { Self {
        level: s.level, hp: s.hp, max_hp: s.max_hp, mp: s.mp, max_mp: s.max_mp,
        exp: s.exp, exp_to_next: s.exp_to_next,
        attack: s.attack, defense: s.defense, magic_mastery: s.magic_mastery, agility: s.agility,
        crit_rate: s.crit_rate, crit_damage: s.crit_damage,
    } }
}

impl SavedStats {
    pub fn into_stats(self) -> Stats { Stats {
        level: self.level, hp: self.hp, max_hp: self.max_hp, mp: self.mp, max_mp: self.max_mp,
        exp: self.exp, exp_to_next: self.exp_to_next,
        attack: self.attack, defense: self.defense, magic_mastery: self.magic_mastery, agility: self.agility,
        crit_rate: self.crit_rate, crit_damage: self.crit_damage,
    } }
}

#[derive(Serialize, Deserialize)]
pub struct SavedBuffs { pub shield_turns: i32, pub shield_def: i32, pub berserk_turns: i32, pub berserk_atk: i32 }

impl From<Buffs> for SavedBuffs {
    fn from(b: Buffs) -> Self { Self { shield_turns: b.shield_turns, shield_def: b.shield_def, berserk_turns: b.berserk_turns, berserk_atk: b.berserk_atk } }
}
impl SavedBuffs {
    pub fn into_buffs(self) -> Buffs { Buffs { shield_turns: self.shield_turns, shield_def: self.shield_def, berserk_turns: self.berserk_turns, berserk_atk: self.berserk_atk } }
}

#[derive(Serialize, Deserialize)]
pub struct SavedMonster {
    pub x: u16, pub y: u16, pub glyph: char, pub r: u8, pub g: u8, pub b: u8,
    pub name: String, pub st: SavedStats,
}

#[derive(Serialize, Deserialize)]
pub struct SavedGroundItem { pub x: u16, pub y: u16, pub item_id: usize, pub count: u32 }

impl GameSave {
    pub fn capture(world: &World) -> Self {
        let w = world;
        let floor = w.resource::<FloorNumber>().0;
        let map_seed = w.resource::<MapSeed>().0;
        let explored = w.resource::<MapMemory>().explored;
        let mut map_tiles = Vec::with_capacity(MAP_WIDTH * MAP_HEIGHT);
        {
            let map = w.resource::<Map>();
            for row in 0..MAP_HEIGHT {
                for col in 0..MAP_WIDTH { map_tiles.push(map.tiles[row][col]); }
            }
        }
        let rooms = { let map = w.resource::<Map>(); map.rooms.clone() };

        let (sx, sy) = {
            let mut sq = w.try_query::<(&Stairs, &Position)>().expect("Stairs+Position registered at init");
            sq.iter(&w).next().map(|(_, p)| (p.x as u16, p.y as u16)).unwrap_or((0, 0))
        };

        let (px, py, st, inv, weapon_item_id, weapon_count, armor_item_id, armor_count, ring_item_id, ring_count, buffs, player_class) = {
            let mut q = w.try_query::<(&Position, &Stats, &Inventory, &Equipment, &Buffs, &PlayerClass)>().expect("Pos+Stats+Inv+Eq+Buffs+Class reg at init");
            let (pos, st, inv, eq, bu, cls) = q.iter(&w).next().unwrap();
            (pos.x as u16, pos.y as u16,
             SavedStats::from(st.clone()),
             inv.stacks.iter().map(|s| SavedStack { item_id: s.item_id, count: s.count }).collect(),
             eq.weapon.as_ref().map(|s| s.item_id), eq.weapon.as_ref().map(|s| s.count),
             eq.armor.as_ref().map(|s| s.item_id), eq.armor.as_ref().map(|s| s.count),
             eq.ring.as_ref().map(|s| s.item_id), eq.ring.as_ref().map(|s| s.count),
             SavedBuffs::from(bu.clone()), Some(cls.clone()))
        };

        let monsters = {
            let mut mq = w.try_query::<(&Monster, &Position, &Stats, &EntityName, &Renderable)>().expect("Mon+Pos+Stats+Name+Rend reg at init");
            mq.iter(&w).map(|(_, pos, st, name, rend)| {
                let (r, g, b) = rend.color;
                SavedMonster {
                    x: pos.x as u16, y: pos.y as u16, glyph: rend.glyph, r, g, b,
                    name: name.0.clone(), st: SavedStats::from(st.clone()),
                }
            }).collect()
        };

        let items = {
            let mut iq = w.try_query::<(&ItemPickup, &Position)>().expect("ItemPickup+Position registered at init");
            iq.iter(&w).map(|(item, pos)| SavedGroundItem {
                x: pos.x as u16, y: pos.y as u16,
                item_id: item.stack.item_id, count: item.stack.count,
            }).collect()
        };

        let action_queue: Vec<SavedActionEntry> = {
            let queue = w.resource::<ActionQueue>();
            queue.entries.iter().filter_map(|entry| {
                // 保存实体位置用于 restore 时重映射
                let pos = w.get::<Position>(entry.entity)?;
                let kind = match &entry.kind {
                    ActionKindV3::Move { dx, dy } => SavedActionKind::Move { dx: *dx, dy: *dy },
                    ActionKindV3::Chase => SavedActionKind::Chase,
                    ActionKindV3::Flee => SavedActionKind::Flee,
                    ActionKindV3::Wander => SavedActionKind::Wander,
                    ActionKindV3::Wait => SavedActionKind::Wait,
                    ActionKindV3::Skill(idx) => SavedActionKind::Skill(*idx),
                    ActionKindV3::Attack { .. } => return None, // 含 Entity 引用，无法保存
                };
                Some(SavedActionEntry {
                    x: pos.x as u16, y: pos.y as u16,
                    kind, av_remaining: entry.av_remaining,
                })
            }).collect()
        };

        let save_intent = |entries: &Vec<(Entity, u32, f32, ActionKindV3)>| {
            entries.iter().filter_map(|(e, pri, av, kind)| {
                let pos = w.get::<Position>(*e)?;
                let sk = match kind {
                    ActionKindV3::Chase => SavedActionKind::Chase,
                    ActionKindV3::Flee => SavedActionKind::Flee,
                    ActionKindV3::Wander => SavedActionKind::Wander,
                    _ => return None,
                };
                Some(SavedIntentEntry { x: pos.x as u16, y: pos.y as u16, priority: *pri, av: *av, kind: sk })
            }).collect()
        };
        let chase_intents = save_intent(&w.resource::<ChaseIntents>().0);
        let flee_intents = save_intent(&w.resource::<FleeIntents>().0);
        let wander_intents = save_intent(&w.resource::<WanderIntents>().0);

        Self {
            floor, map_seed, px, py, st, inv,
            weapon_item_id, weapon_count, armor_item_id, armor_count, ring_item_id, ring_count,
            buffs,
            map_tiles, rooms,
            explored: explored.iter().flat_map(|r| r.iter().map(|&b| b as u8)).collect(),
            monsters, items, sx, sy, player_class,
            action_queue,
            chase_intents, flee_intents, wander_intents,
        }
    }

    pub fn restore(self, world: &mut World) {
        let w = world;
        let dead: Vec<Entity> = { let mut q = w.query::<(Entity,)>();
            q.iter(&mut *w).map(|(e,)| e).collect() };
        for e in dead { let _ = w.despawn(e); }

        w.insert_resource(FloorNumber(self.floor));
        w.insert_resource(MapSeed(self.map_seed));
        let mut tiles = [[Tile::Wall; MAP_WIDTH]; MAP_HEIGHT];
        for (i, &v) in self.map_tiles.iter().enumerate() {
            tiles[i / MAP_WIDTH][i % MAP_WIDTH] = v;
        }
        w.insert_resource(Map { tiles, rooms: self.rooms });
        let mut explored = [[false; MAP_WIDTH]; MAP_HEIGHT];
        for (i, &v) in self.explored.iter().enumerate() { explored[i / MAP_WIDTH][i % MAP_WIDTH] = v != 0; }
        w.insert_resource(MapMemory { explored });
        w.insert_resource(PendingExp::default());
        w.insert_resource(EventLog::new());
        w.insert_resource(TurnManager::new());
        w.insert_resource(OccupancyMap::new());
        w.insert_resource(ActionQueue::default());
        w.insert_resource(InputBuffer::default());
        w.insert_resource(PlayerPreview::default());
        w.insert_resource(ChaseIntents::default());
        w.insert_resource(FleeIntents::default());
        w.insert_resource(WanderIntents::default());
        w.insert_resource(GameRng::new(self.map_seed.wrapping_add(42)));

        let s = self.st.into_stats();
        let pc = self.player_class.unwrap_or(PlayerClass::Warrior);
        let agi = s.agility;
        w.spawn((
            Player, Position { x: self.px as usize, y: self.py as usize },
            Renderable { glyph: '@', color: (255, 255, 0) },
            MovingDir::default(), Viewshed { range: 10, visible_tiles: Vec::new() },
            s, EntityName("冒险者".into()),
            Inventory {
                stacks: self.inv.into_iter()
                    .map(|s| ItemStack { item_id: s.item_id, count: s.count })
                    .collect(),
                capacity: 36,
            },
            Equipment {
                weapon: self.weapon_item_id.map(|id| ItemStack { item_id: id, count: self.weapon_count.unwrap_or(1) }),
                armor: self.armor_item_id.map(|id| ItemStack { item_id: id, count: self.armor_count.unwrap_or(1) }),
                ring: self.ring_item_id.map(|id| ItemStack { item_id: id, count: self.ring_count.unwrap_or(1) }),
            },
            pc.clone(), self.buffs.into_buffs(),
            dungeon_core::Skills { list: pc.skills() },
            Reaction { time: agility_to_reaction(agi) },
            CanMove::new(100), CanWait::new(0),
        ));

        w.spawn((Stairs, Position { x: self.sx as usize, y: self.sy as usize },
            Renderable { glyph: '>', color: (0, 255, 0) }));

        for m in self.monsters {
            let mon_stats = m.st.into_stats();
            let agi = mon_stats.agility;
            let kind = match m.glyph { 's' => dungeon_core::MonsterKindId::Scorpion, 'g' => dungeon_core::MonsterKindId::Goblin, _ => dungeon_core::MonsterKindId::Rat };
            let loot = dungeon_core::monster_def::monster_loot(kind);
            w.spawn((
                Monster, Position { x: m.x as usize, y: m.y as usize },
                Renderable { glyph: m.glyph, color: (m.r, m.g, m.b) },
                Viewshed { range: 10, visible_tiles: Vec::new() },
                mon_stats, EntityName(m.name),
                AttackName(if m.glyph == 'r' { "撕咬" } else if m.glyph == 's' { "螫刺" } else { "重击" }.into()),
                loot,
                Reaction { time: agility_to_reaction(agi) },
                CanChase::new(100), CanFlee::new(200), CanWander::new(50), CanWait::new(0),
            ));
        }

        for gi in self.items {
            let def = ItemRegistry::global().get(gi.item_id).unwrap_or_else(|| {
                panic!("物品 ID {} 在注册表中不存在", gi.item_id)
            });
            w.spawn((
                ItemPickup { stack: ItemStack { item_id: gi.item_id, count: gi.count } },
                Position { x: gi.x as usize, y: gi.y as usize },
                Renderable { glyph: def.glyph, color: def.color },
            ));
        }

        // 恢复 ActionQueue：根据位置重映射 Entity
        let mut entries: Vec<ActionEntry> = Vec::new();
        for saved in &self.action_queue {
            let kind = match &saved.kind {
                SavedActionKind::Move { dx, dy } => ActionKindV3::Move { dx: *dx, dy: *dy },
                SavedActionKind::Chase => ActionKindV3::Chase,
                SavedActionKind::Flee => ActionKindV3::Flee,
                SavedActionKind::Wander => ActionKindV3::Wander,
                SavedActionKind::Wait => ActionKindV3::Wait,
                SavedActionKind::Skill(idx) => ActionKindV3::Skill(*idx),
            };
            // 在当前位置找对应实体（不可变查询，不需要 &mut World）
            let entity = w.query::<(Entity, &Position)>().iter(w).find_map(|(e, p)| {
                if p.x as u16 == saved.x && p.y as u16 == saved.y { Some(e) } else { None }
            });
            if let Some(entity) = entity {
                entries.push(ActionEntry { entity, kind, av_remaining: saved.av_remaining });
            }
        }
        // 一次性写入队列
        {
            let mut queue = w.resource_mut::<ActionQueue>();
            queue.entries.extend(entries);
        }

        // 恢复意图缓冲区：根据位置重映射 Entity
        // 先收集所有 entity→position 映射
        let pos_map: Vec<(Entity, u16, u16)> = w.query::<(Entity, &Position)>().iter(w)
            .map(|(e, p)| (e, p.x as u16, p.y as u16)).collect();
        let remap = |saved: &[SavedIntentEntry]| -> Vec<(Entity, u32, f32, ActionKindV3)> {
            saved.iter().filter_map(|entry| {
                let entity = pos_map.iter().find(|(_, px, py)| *px == entry.x && *py == entry.y)?.0;
                let kind = match &entry.kind {
                    SavedActionKind::Chase => ActionKindV3::Chase,
                    SavedActionKind::Flee => ActionKindV3::Flee,
                    SavedActionKind::Wander => ActionKindV3::Wander,
                    _ => return None,
                };
                Some((entity, entry.priority, entry.av, kind))
            }).collect()
        };
        w.resource_mut::<ChaseIntents>().0 = remap(&self.chase_intents);
        w.resource_mut::<FleeIntents>().0 = remap(&self.flee_intents);
        w.resource_mut::<WanderIntents>().0 = remap(&self.wander_intents);
    }
}
