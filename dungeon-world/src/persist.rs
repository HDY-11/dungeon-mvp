//! 存档/读档：GameSave 序列化、V0 旧档兼容、capture/restore 与楼层/实体重建。

use dungeon_action::{CanChase, CanFlee, CanMove, CanWait, CanWander, Idle, PlayerPreview};
use dungeon_core::{MAP_HEIGHT, MAP_WIDTH, Map, Tile, components::*, items::*, resources::*};

use bevy_ecs::prelude::*;
use dungeon_core::OptionLogExt;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone)]
pub struct SavedStack {
    pub item_id: usize,
    pub count: u32,
}

/// 可序列化的行动种类（A35: Attack 按目标坐标重映射，不再丢失）
#[derive(Serialize, Deserialize, Clone)]
pub struct SavedActiveBuff {
    pub kind: u8,
    pub remaining_av: f32,
    pub magnitude: i32,
}

/// 可序列化的技能条目
#[derive(Serialize, Deserialize, Clone)]
pub struct SavedSkill {
    pub name: String,
    pub key: char,
    pub cost_mp: i32,
    pub description: String,
    /// 0=Heal, 1=Shield, 2=Berserk
    pub kind: u8,
    /// Heal.amount 或 Shield.def_boost 或 Berserk.atk_boost
    pub extra: i32,
    /// Shield.duration 或 Berserk.duration（Heal=0）
    pub duration: u32,
    pub proficiency: u32,
}

/// 从 SavedSkill 列表重建 Skills（I64：直接使用 String，无需 Box::leak）
fn restore_skills(saved: &[SavedSkill]) -> Vec<dungeon_core::Skill> {
    use dungeon_core::SkillKind;
    saved
        .iter()
        .map(|sk| {
            let kind = match sk.kind {
                0 => SkillKind::Heal { amount: sk.extra },
                1 => SkillKind::Shield {
                    def_boost: sk.extra,
                    duration: sk.duration,
                },
                2 => SkillKind::Berserk {
                    atk_boost: sk.extra,
                    duration: sk.duration,
                },
                _ => SkillKind::Heal { amount: sk.extra },
            };
            dungeon_core::Skill {
                name: sk.name.clone(),
                key: sk.key,
                cost_mp: sk.cost_mp,
                description: sk.description.clone(),
                kind,
                proficiency: sk.proficiency,
            }
        })
        .collect()
}

#[derive(Serialize, Deserialize)]
pub struct GameSave {
    pub floor: u32,
    pub map_seed: u64,
    pub px: u16,
    pub py: u16,
    pub st: dungeon_core::Stats,
    pub inv: Vec<SavedStack>,
    pub weapon_item_id: Option<usize>,
    pub weapon_count: Option<u32>,
    pub armor_item_id: Option<usize>,
    pub armor_count: Option<u32>,
    pub ring_item_id: Option<usize>,
    pub ring_count: Option<u32>,
    #[serde(default)]
    pub off_hand_item_id: Option<usize>,
    #[serde(default)]
    pub off_hand_count: Option<u32>,
    pub map_tiles: Vec<Tile>,
    pub rooms: Vec<dungeon_core::Room>,
    pub explored: Vec<u8>,
    pub monsters: Vec<SavedMonster>,
    pub items: Vec<SavedGroundItem>,
    pub sx: u16,
    pub sy: u16,
    pub player_class: Option<PlayerClass>,
    #[serde(default)]
    pub active_buffs: Vec<SavedActiveBuff>,
    /// Skills 序列化，#[serde(default)] 兼容旧存档
    #[serde(default)]
    pub skills: Vec<SavedSkill>,
    // ── 以下为新格式（DSV1）字段 ──
    /// 背包容量（A40: 不再硬编码 36）
    #[serde(default)]
    pub inv_capacity: usize,
    /// GameRng 状态（G32: SL 刷掉落修复——读档精确恢复随机序列）
    #[serde(default)]
    pub rng_state: u64,
    #[serde(default)]
    pub rng_steps: u64,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct SavedMonster {
    pub x: u16,
    pub y: u16,
    pub glyph: char,
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub name: String,
    pub st: dungeon_core::Stats,
    /// 怪物种类（A24：旧存档为 None 时按 glyph 推断兜底）
    #[serde(default)]
    pub kind: Option<dungeon_core::MonsterKindId>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct SavedGroundItem {
    pub x: u16,
    pub y: u16,
    pub item_id: usize,
    pub count: u32,
}

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
                for col in 0..MAP_WIDTH {
                    map_tiles.push(map.tiles[row][col]);
                }
            }
        }
        let rooms = {
            let map = w.resource::<Map>();
            map.rooms.clone()
        };

        let (sx, sy) = {
            let mut sq = w
                .try_query::<(&Stairs, &Position)>()
                .expect_log("Stairs+Position registered at init");
            sq.iter(w)
                .next()
                .map(|(_, p)| (p.x as u16, p.y as u16))
                .unwrap_or((0, 0))
        };

        let (
            px,
            py,
            st,
            inv,
            weapon_item_id,
            weapon_count,
            armor_item_id,
            armor_count,
            ring_item_id,
            ring_count,
            off_hand_item_id,
            off_hand_count,
            active_buffs,
            player_class,
            saved_skills,
        ) = {
            // A25: 查询显式包含 &Player 组件（L31 最具体约束）
            let mut q = w
                .try_query::<(
                    &Player,
                    &Position,
                    &Stats,
                    &Inventory,
                    &Equipment,
                    &ActiveBuffs,
                    &PlayerClass,
                    &dungeon_core::Skills,
                )>()
                .expect_log("Player components registered at init");
            let (_, pos, st, inv, eq, ab, cls, sk) =
                q.iter(w).next().expect_log("Player entity exists for save");
            let saved_ab: Vec<SavedActiveBuff> =
                ab.0.iter()
                    .map(|b| SavedActiveBuff {
                        kind: match b.kind {
                            BuffKind::Shield => 0,
                            BuffKind::Berserk => 1,
                        },
                        remaining_av: b.remaining_av,
                        magnitude: b.magnitude,
                    })
                    .collect();
            use dungeon_core::SkillKind;
            let saved_skills: Vec<SavedSkill> = sk
                .list
                .iter()
                .map(|s| {
                    let (kind, extra, duration) = match &s.kind {
                        SkillKind::Heal { amount } => (0u8, *amount, 0u32),
                        SkillKind::Shield {
                            def_boost,
                            duration,
                        } => (1u8, *def_boost, *duration),
                        SkillKind::Berserk {
                            atk_boost,
                            duration,
                        } => (2u8, *atk_boost, *duration),
                    };
                    SavedSkill {
                        name: s.name.to_string(),
                        key: s.key,
                        cost_mp: s.cost_mp,
                        description: s.description.to_string(),
                        kind,
                        extra,
                        duration,
                        proficiency: s.proficiency,
                    }
                })
                .collect();
            (
                pos.x as u16,
                pos.y as u16,
                st.clone(),
                inv.stacks
                    .iter()
                    .map(|s| SavedStack {
                        item_id: s.item_id,
                        count: s.count,
                    })
                    .collect(),
                eq.main_hand.as_ref().map(|s| s.item_id),
                eq.main_hand.as_ref().map(|s| s.count),
                eq.armor.as_ref().map(|s| s.item_id),
                eq.armor.as_ref().map(|s| s.count),
                eq.ring.as_ref().map(|s| s.item_id),
                eq.ring.as_ref().map(|s| s.count),
                eq.off_hand.as_ref().map(|s| s.item_id),
                eq.off_hand.as_ref().map(|s| s.count),
                saved_ab,
                Some(cls.clone()),
                saved_skills,
            )
        };

        let monsters = {
            let mut mq = w
                .try_query::<(
                    &Monster,
                    &Position,
                    &Stats,
                    &EntityName,
                    &Renderable,
                    &dungeon_core::MonsterKindId,
                )>()
                .expect_log("Mon+Pos+Stats+Name+Rend+Kind reg at init");
            mq.iter(w)
                .map(|(_, pos, st, name, rend, kind)| {
                    let (r, g, b) = rend.color;
                    SavedMonster {
                        x: pos.x as u16,
                        y: pos.y as u16,
                        glyph: rend.glyph,
                        r,
                        g,
                        b,
                        name: name.0.clone(),
                        st: st.clone(),
                        kind: Some(*kind),
                    }
                })
                .collect()
        };

        let items = {
            let mut iq = w
                .try_query::<(&ItemPickup, &Position)>()
                .expect_log("ItemPickup+Position registered at init");
            iq.iter(w)
                .map(|(item, pos)| SavedGroundItem {
                    x: pos.x as u16,
                    y: pos.y as u16,
                    item_id: item.stack.item_id,
                    count: item.stack.count,
                })
                .collect()
        };

        // A40: 保存背包容量（不再依赖 restore 硬编码 36）
        let inv_capacity = {
            let mut q = w
                .try_query::<(&Player, &Inventory)>()
                .expect_log("Player+Inventory registered at init");
            q.iter(w).next().map(|(_, inv)| inv.capacity).unwrap_or(36)
        };
        // G32: 保存 RNG 精确状态（读档不再重放/重掷随机序列）
        let (rng_state, rng_steps) = {
            let rng = w.resource::<dungeon_core::GameRng>();
            (rng.state, rng.steps)
        };

        Self {
            floor,
            map_seed,
            px,
            py,
            st,
            inv,
            weapon_item_id,
            weapon_count,
            armor_item_id,
            armor_count,
            ring_item_id,
            ring_count,
            off_hand_item_id,
            off_hand_count,
            active_buffs,
            skills: saved_skills,
            map_tiles,
            rooms,
            explored: explored
                .iter()
                .flat_map(|r| r.iter().map(|&b| b as u8))
                .collect(),
            monsters,
            items,
            sx,
            sy,
            player_class,
            inv_capacity,
            rng_state,
            rng_steps,
        }
    }

    pub fn restore(self, world: &mut World) {
        let w = world;
        let dead: Vec<Entity> = {
            let mut q = w.query::<(Entity,)>();
            q.iter(&*w).map(|(e,)| e).collect()
        };
        for e in dead {
            let _ = w.despawn(e);
        }

        w.insert_resource(FloorNumber(self.floor));
        w.insert_resource(MapSeed(self.map_seed));
        let mut tiles = [[Tile::Wall; MAP_WIDTH]; MAP_HEIGHT];
        // I85: 长度校验——损坏/篡改存档（超长 Vec）不再越界 panic；多余截断、缺失保持 Wall
        if self.map_tiles.len() != MAP_WIDTH * MAP_HEIGHT {
            log::warn!(
                "存档 map_tiles 长度 {} ≠ {}，已截断/补齐",
                self.map_tiles.len(),
                MAP_WIDTH * MAP_HEIGHT
            );
        }
        for (i, &v) in self
            .map_tiles
            .iter()
            .take(MAP_WIDTH * MAP_HEIGHT)
            .enumerate()
        {
            tiles[i / MAP_WIDTH][i % MAP_WIDTH] = v;
        }
        w.insert_resource(Map {
            tiles,
            rooms: self.rooms,
        });
        let mut explored = [[false; MAP_WIDTH]; MAP_HEIGHT];
        if self.explored.len() != MAP_WIDTH * MAP_HEIGHT {
            log::warn!(
                "存档 explored 长度 {} ≠ {}，已截断/补齐",
                self.explored.len(),
                MAP_WIDTH * MAP_HEIGHT
            );
        }
        for (i, &v) in self
            .explored
            .iter()
            .take(MAP_WIDTH * MAP_HEIGHT)
            .enumerate()
        {
            explored[i / MAP_WIDTH][i % MAP_WIDTH] = v != 0;
        }
        w.insert_resource(MapMemory { explored });
        w.insert_resource(PendingExp::default());
        w.insert_resource(EventLog::new());
        w.insert_resource(TurnManager::new());
        w.insert_resource(OccupancyMap::new());
        w.insert_resource(PlayerPreview::default());
        // G32: 精确恢复 RNG 状态（rng_state==0 表示旧档 → 保持旧派生种子行为）
        if self.rng_state != 0 {
            w.insert_resource(GameRng::from_state(self.rng_state, self.rng_steps));
        } else {
            w.insert_resource(GameRng::new(self.map_seed.wrapping_add(42)));
        }

        let s = self.st.clone();
        let pc = self.player_class.unwrap_or(PlayerClass::Warrior);
        // A40: 背包容量从存档恢复（旧档默认 36）
        let inv_capacity = if self.inv_capacity == 0 {
            36
        } else {
            self.inv_capacity
        };
        let mut player_entity = w.spawn((
            Player,
            Position {
                x: self.px as usize,
                y: self.py as usize,
            },
            Renderable {
                glyph: '@',
                color: (255, 255, 0),
            },
            Viewshed {
                range: 10,
                visible_tiles: Vec::new(),
            },
            s,
            EntityName("冒险者".into()),
            Inventory {
                stacks: self
                    .inv
                    .into_iter()
                    .map(|s| ItemStack {
                        item_id: s.item_id,
                        count: s.count,
                        meta: None,
                    })
                    .collect(),
                capacity: inv_capacity,
            },
            Equipment {
                main_hand: self.weapon_item_id.map(|id| ItemStack {
                    item_id: id,
                    count: self.weapon_count.unwrap_or(1),
                    meta: None,
                }),
                off_hand: self.off_hand_item_id.map(|id| ItemStack {
                    item_id: id,
                    count: self.off_hand_count.unwrap_or(1),
                    meta: None,
                }),
                armor: self.armor_item_id.map(|id| ItemStack {
                    item_id: id,
                    count: self.armor_count.unwrap_or(1),
                    meta: None,
                }),
                ring: self.ring_item_id.map(|id| ItemStack {
                    item_id: id,
                    count: self.ring_count.unwrap_or(1),
                    meta: None,
                }),
            },
            pc.clone(),
            dungeon_core::Skills {
                list: restore_skills(&self.skills),
            },
            // I79: 与 setup_world/descend 三路径一致（L44）——读档玩家也必须持有 AttackName
            AttackName("斩击".into()),
            CanMove::new(100),
            CanWait::new(0),
            Idle,
        ));
        let restored_buffs: Vec<Buff> = self
            .active_buffs
            .iter()
            .map(|sab| {
                let kind = match sab.kind {
                    0 => BuffKind::Shield,
                    1 => BuffKind::Berserk,
                    _ => BuffKind::Shield,
                };
                Buff {
                    kind,
                    remaining_av: sab.remaining_av,
                    magnitude: sab.magnitude,
                    stack_type: BuffStackType::None,
                }
            })
            .collect();
        player_entity.insert(ActiveBuffs(restored_buffs));

        // G22: 旧存档楼梯可能在被覆盖的不可走格上，读档时兜底到最近可行走格
        let (sx, sy) = {
            let map = w.resource::<Map>();
            map.nearest_walkable(self.sx as usize, self.sy as usize)
        };
        w.spawn((
            Stairs,
            Position { x: sx, y: sy },
            Renderable {
                glyph: '>',
                color: (0, 255, 0),
            },
        ));

        for m in self.monsters {
            let mon_stats = m.st.clone();
            // A24: 优先用存档的 kind，旧存档（None）按 glyph 推断
            let kind = m.kind.unwrap_or(match m.glyph {
                's' => dungeon_core::MonsterKindId::Scorpion,
                'g' => dungeon_core::MonsterKindId::Goblin,
                _ => dungeon_core::MonsterKindId::Rat,
            });
            let loot = dungeon_core::monster_def::monster_loot(kind);
            w.spawn((
                Monster,
                Position {
                    x: m.x as usize,
                    y: m.y as usize,
                },
                Renderable {
                    glyph: m.glyph,
                    color: (m.r, m.g, m.b),
                },
                Viewshed {
                    range: 10,
                    visible_tiles: Vec::new(),
                },
                mon_stats,
                EntityName(m.name),
                // I80: 攻击名按已存档的 kind 查表（A24 同根因残留——旧实现按 glyph 分支，新怪全部错误显示"重击"）
                AttackName(dungeon_core::monster_def::monster_attack_name(kind).into()),
                loot,
                kind,
                CanChase::new(100),
                CanFlee::new(200),
                CanWander::new(50),
                CanWait::new(0),
                Idle,
                // A31: 与 setup_world/descend 三路径一致（L44 第五次）——缺失则 chase 决策查询过滤，追击 AI 失效
                LastKnownPlayerPos::default(),
            ));
        }

        for gi in self.items {
            // I71: 损坏存档（未知物品 ID）降级跳过而非 panic——读档永不崩溃
            let Some(def) = ItemRegistry::global().get(gi.item_id) else {
                log::warn!("存档包含未知物品 ID {}，已跳过", gi.item_id);
                continue;
            };
            w.spawn((
                ItemPickup {
                    stack: ItemStack {
                        item_id: gi.item_id,
                        count: gi.count,
                        meta: None,
                    },
                },
                Position {
                    x: gi.x as usize,
                    y: gi.y as usize,
                },
                Renderable {
                    glyph: def.glyph,
                    color: def.color,
                },
            ));
        }

    }
}

// ── 存档 I/O 单入口（A37：main.rs/game.rs 两处调用方收敛至此） ──

/// 新格式 magic 前缀：`DSV1` + bincode(GameSave)。
pub const SAVE_MAGIC: &[u8; 4] = b"DSV1";

/// 保存游戏到 path（总是写新格式）
pub fn save_game(world: &World, path: &str) -> Result<(), String> {
    let save = GameSave::capture(world);
    let payload = bincode::serialize(&save).map_err(|e| format!("序列化失败: {e}"))?;
    let mut data = Vec::with_capacity(SAVE_MAGIC.len() + payload.len());
    data.extend_from_slice(SAVE_MAGIC);
    data.extend_from_slice(&payload);
    std::fs::write(path, data).map_err(|e| format!("写入失败: {e}"))
}

/// 从 path 读档并 restore 到 world（含 post_load_refresh）。
/// 从 path 读取并 restore 到 world（含 post_load_refresh）。
pub fn load_game(world: &mut World, path: &str) -> Result<(), String> {
    let data = std::fs::read(path).map_err(|e| format!("读取失败: {e}"))?;
    if !data.starts_with(SAVE_MAGIC) {
        return Err("不支持的存档格式".to_string());
    }
    let save: GameSave = bincode::deserialize::<GameSave>(&data[SAVE_MAGIC.len()..])
        .map_err(|e| format!("存档解析失败: {e}"))?;
    save.restore(world);
    dungeon_core::ops::post_load_refresh(world);
    Ok(())
}
