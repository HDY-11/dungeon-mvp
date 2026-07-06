//! 存档/读档

use dungeon_core::{
    components::*, items::*, resources::*,
    Map, Tile, MAP_WIDTH, MAP_HEIGHT,
    ActionQueue, InputBuffer, PlayerPreview,
    ChaseIntents, FleeIntents, WanderIntents,
    Reaction, agility_to_reaction,
    CanMove, CanChase, CanFlee, CanWander, CanWait,
};
use crate::loot::{rat_loot, goblin_loot};
use bevy_ecs::prelude::*;
use rand::SeedableRng;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct SavedStack {
    pub item_id: usize,
    pub count: u32,
}

#[derive(Serialize, Deserialize)]
pub struct GameSave {
    pub floor: u32,
    pub px: u16, pub py: u16,
    pub st: SavedStats,
    pub inv: Vec<SavedStack>,
    pub weapon_item_id: Option<usize>, pub weapon_count: Option<u32>,
    pub armor_item_id: Option<usize>, pub armor_count: Option<u32>,
    pub ring_item_id: Option<usize>, pub ring_count: Option<u32>,
    pub buffs: SavedBuffs,
    pub map_tiles: Vec<u8>,
    pub rooms: Vec<dungeon_core::Room>,
    pub explored: Vec<u8>,
    pub monsters: Vec<SavedMonster>,
    pub items: Vec<SavedGroundItem>,
    pub sx: u16, pub sy: u16,
    pub player_class: Option<PlayerClass>,
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
        let explored = w.resource::<MapMemory>().explored;
        let mut map_tiles = Vec::with_capacity(MAP_WIDTH * MAP_HEIGHT);
        {
            let map = w.resource::<Map>();
            for row in 0..MAP_HEIGHT {
                for col in 0..MAP_WIDTH { map_tiles.push(map.tiles[row][col] as u8); }
            }
        }
        let rooms = { let map = w.resource::<Map>(); map.rooms.clone() };

        let (sx, sy) = {
            let mut sq = w.try_query::<(&Stairs, &Position)>().unwrap();
            sq.iter(&w).next().map(|(_, p)| (p.x as u16, p.y as u16)).unwrap_or((0, 0))
        };

        let (px, py, st, inv, weapon_item_id, weapon_count, armor_item_id, armor_count, ring_item_id, ring_count, buffs, player_class) = {
            let mut q = w.try_query::<(&Position, &Stats, &Inventory, &Equipment, &Buffs, &PlayerClass)>().unwrap();
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
            let mut mq = w.try_query::<(&Monster, &Position, &Stats, &EntityName, &Renderable)>().unwrap();
            mq.iter(&w).map(|(_, pos, st, name, rend)| {
                let (r, g, b) = rend.color;
                SavedMonster {
                    x: pos.x as u16, y: pos.y as u16, glyph: rend.glyph, r, g, b,
                    name: name.0.clone(), st: SavedStats::from(st.clone()),
                }
            }).collect()
        };

        let items = {
            let mut iq = w.try_query::<(&ItemPickup, &Position)>().unwrap();
            iq.iter(&w).map(|(item, pos)| SavedGroundItem {
                x: pos.x as u16, y: pos.y as u16,
                item_id: item.stack.item_id, count: item.stack.count,
            }).collect()
        };

        Self {
            floor, px, py, st, inv,
            weapon_item_id, weapon_count, armor_item_id, armor_count, ring_item_id, ring_count,
            buffs,
            map_tiles, rooms,
            explored: explored.iter().flat_map(|r| r.iter().map(|&b| b as u8)).collect(),
            monsters, items, sx, sy, player_class,
        }
    }

    pub fn restore(self, world: &mut World) {
        let w = world;
        let dead: Vec<Entity> = { let mut q = w.query::<(Entity,)>();
            q.iter(&mut *w).map(|(e,)| e).collect() };
        for e in dead { let _ = w.despawn(e); }

        w.insert_resource(FloorNumber(self.floor));
        let mut tiles = [[Tile::Wall; MAP_WIDTH]; MAP_HEIGHT];
        for (i, &v) in self.map_tiles.iter().enumerate() {
            tiles[i / MAP_WIDTH][i % MAP_WIDTH] = if v == 0 { Tile::Wall } else { Tile::Floor };
        }
        w.insert_resource(Map { tiles, rooms: self.rooms });
        let mut explored = [[false; MAP_WIDTH]; MAP_HEIGHT];
        for (i, &v) in self.explored.iter().enumerate() { explored[i / MAP_WIDTH][i % MAP_WIDTH] = v != 0; }
        w.insert_resource(MapMemory { explored });
        w.insert_resource(PendingExp::default());
        w.insert_resource(EventLog::new());
        w.insert_resource(TurnManager::new());
        w.insert_resource(PendingLevelUp::default());
        w.insert_resource(OccupancyMap::new());
        w.insert_resource(ActionQueue::default());
        w.insert_resource(InputBuffer::default());
        w.insert_resource(PlayerPreview::default());
        w.insert_resource(ChaseIntents::default());
        w.insert_resource(FleeIntents::default());
        w.insert_resource(WanderIntents::default());
        w.insert_resource(GameRng { rng: rand::rngs::SmallRng::seed_from_u64(0) });

        let s = self.st.into_stats();
        let pc = self.player_class.unwrap_or(PlayerClass::Warrior);
        let agi = s.agility;
        w.spawn((
            Player, Position { x: self.px as usize, y: self.py as usize },
            Renderable { glyph: '@', color: (255, 255, 0) },
            MovingDir::default(), Viewshed { range: 8, visible_tiles: Vec::new() },
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
            let loot = if m.glyph == 'g' { goblin_loot() } else { rat_loot() };
            w.spawn((
                Monster, Position { x: m.x as usize, y: m.y as usize },
                Renderable { glyph: m.glyph, color: (m.r, m.g, m.b) },
                Viewshed { range: 8, visible_tiles: Vec::new() },
                mon_stats, EntityName(m.name),
                AttackName(if m.glyph == 'r' { "撕咬" } else { "重击" }.into()),
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
    }
}
