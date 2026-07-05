use crate::*;
use crate::world;
use crate::action::{Reaction, agility_to_reaction, CanMove, CanChase, CanFlee, CanWander, CanWait, ActionQueue, InputBuffer, PlayerPreview};
use bevy_ecs::prelude::*;
use rand::SeedableRng;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct RawItem {
    pub name: String, pub glyph: char, pub r: u8, pub g: u8, pub b: u8,
    pub slot: EquipmentSlot,
    pub bonus_atk: i32, pub bonus_def: i32, pub bonus_mag: i32, pub bonus_agi: i32,
    pub bonus_hp: i32, pub bonus_crit_rate: f32, pub bonus_crit_dmg: f32,
    pub desc: String,
}

impl RawItem {
    fn from_item(item: &ItemInstance) -> Self {
        let (r, g, b) = item.color;
        Self {
            name: item.name.clone(), glyph: item.glyph, r, g, b, slot: item.slot,
            bonus_atk: item.bonus.attack, bonus_def: item.bonus.defense,
            bonus_mag: item.bonus.magic_mastery, bonus_agi: item.bonus.agility,
            bonus_hp: item.bonus.hp, bonus_crit_rate: item.bonus.crit_rate, bonus_crit_dmg: item.bonus.crit_damage,
            desc: item.description.clone(),
        }
    }
    fn into_item(self) -> ItemInstance {
        ItemInstance {
            name: self.name, glyph: self.glyph, color: (self.r, self.g, self.b),
            slot: self.slot, description: self.desc,
            bonus: StatBonus {
                attack: self.bonus_atk, defense: self.bonus_def,
                magic_mastery: self.bonus_mag, agility: self.bonus_agi,
                hp: self.bonus_hp, crit_rate: self.bonus_crit_rate, crit_damage: self.bonus_crit_dmg,
            },
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct GameSave {
    pub floor: u32,
    pub px: u16, pub py: u16,
    pub st: SavedStats,
    pub inv: Vec<RawItem>,
    pub weapon: Option<u16>, pub armor: Option<u16>, pub ring: Option<u16>,
    pub buffs: SavedBuffs,
    pub map_tiles: Vec<u8>,
    pub rooms: Vec<Room>,
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
    fn into_stats(self) -> Stats { Stats {
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
    fn into_buffs(self) -> Buffs { Buffs { shield_turns: self.shield_turns, shield_def: self.shield_def, berserk_turns: self.berserk_turns, berserk_atk: self.berserk_atk } }
}

#[derive(Serialize, Deserialize)]
pub struct SavedMonster {
    pub x: u16, pub y: u16, pub glyph: char, pub r: u8, pub g: u8, pub b: u8,
    pub name: String, pub st: SavedStats,
}

#[derive(Serialize, Deserialize)]
pub struct SavedGroundItem { pub x: u16, pub y: u16, pub item: RawItem }

impl GameSave {
    pub fn capture() -> Self {
        let mut w = world!(mut);
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
            let mut sq = w.query::<(&Stairs, &Position)>();
            sq.iter(&mut *w).next().map(|(_, p)| (p.x as u16, p.y as u16)).unwrap_or((0, 0))
        };

        let (px, py, st, inv, weapon, armor, ring, buffs, player_class) = {
            let mut q = w.query::<(&Position, &Stats, &Inventory, &Equipment, &Buffs, &PlayerClass)>();
            let (pos, st, inv, eq, bu, cls) = q.iter(&mut *w).next().unwrap();
            (pos.x as u16, pos.y as u16,
             SavedStats::from(st.clone()),
             inv.items.iter().map(RawItem::from_item).collect(),
             eq.weapon.map(|i| i as u16), eq.armor.map(|i| i as u16), eq.ring.map(|i| i as u16),
             SavedBuffs::from(bu.clone()), Some(cls.clone()))
        };

        let monsters = {
            let mut mq = w.query::<(&Monster, &Position, &Stats, &EntityName, &Renderable)>();
            mq.iter(&mut *w).map(|(_, pos, st, name, rend)| {
                let (r, g, b) = rend.color;
                SavedMonster {
                    x: pos.x as u16, y: pos.y as u16, glyph: rend.glyph, r, g, b,
                    name: name.0.clone(), st: SavedStats::from(st.clone()),
                }
            }).collect()
        };

        let items = {
            let mut iq = w.query::<(&ItemPickup, &Position)>();
            iq.iter(&mut *w).map(|(item, pos)| SavedGroundItem {
                x: pos.x as u16, y: pos.y as u16, item: RawItem::from_item(&item.item),
            }).collect()
        };

        Self {
            floor, px, py, st, inv, weapon, armor, ring, buffs,
            map_tiles, rooms,
            explored: explored.iter().flat_map(|r| r.iter().map(|&b| b as u8)).collect(),
            monsters, items, sx, sy, player_class,
        }
    }

    pub fn restore(self) {
        let mut w = world!(mut);
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
        w.insert_resource(PendingPickup::default());
        w.insert_resource(PendingSkill::default());
        w.insert_resource(EventLog::new());
        w.insert_resource(TurnManager::new());
        w.insert_resource(PendingLevelUp::default());
        w.insert_resource(OccupancyMap::new());
        w.insert_resource(ActionQueue::default());
        w.insert_resource(InputBuffer::default());
        w.insert_resource(PlayerPreview::default());
        w.insert_resource(GameRng { rng: rand::rngs::SmallRng::seed_from_u64(0) });

        let s = self.st.into_stats();
        let pc = self.player_class.unwrap_or(PlayerClass::Warrior);
        let agi = s.agility;
        w.spawn((
            Player, Position { x: self.px as usize, y: self.py as usize },
            Renderable { glyph: '@', color: (255, 255, 0) },
            MovingDir::default(), Viewshed { range: 8, visible_tiles: Vec::new() },
            s, EntityName("冒险者".into()),
            Inventory { items: self.inv.into_iter().map(RawItem::into_item).collect(), capacity: 36 },
            Equipment {
                weapon: self.weapon.map(|i| i as usize),
                armor: self.armor.map(|i| i as usize),
                ring: self.ring.map(|i| i as usize),
            },
            pc.clone(), self.buffs.into_buffs(),
            Skills { list: pc.skills() },
            Reaction { time: agility_to_reaction(agi) },
            CanMove::new(100), CanWait::new(0),
        ));

        w.spawn((Stairs, Position { x: self.sx as usize, y: self.sy as usize },
            Renderable { glyph: '>', color: (0, 255, 0) }));

        for m in self.monsters {
            let mon_stats = m.st.into_stats();
            let agi = mon_stats.agility;
            w.spawn((
                Monster, Position { x: m.x as usize, y: m.y as usize },
                Renderable { glyph: m.glyph, color: (m.r, m.g, m.b) },
                Viewshed { range: 8, visible_tiles: Vec::new() },
                mon_stats, EntityName(m.name),
                AttackName(if m.glyph == 'r' { "撕咬" } else { "重击" }.into()),
                Reaction { time: agility_to_reaction(agi) },
                CanChase::new(100), CanFlee::new(200), CanWander::new(50), CanWait::new(0),
            ));
        }

        for gi in self.items {
            let (glyph, r, g, b) = (gi.item.glyph, gi.item.r, gi.item.g, gi.item.b);
            let item = gi.item.into_item();
            w.spawn((
                ItemPickup { item },
                Position { x: gi.x as usize, y: gi.y as usize },
                Renderable { glyph, color: (r, g, b) },
            ));
        }
    }
}
