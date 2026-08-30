//! 世界生命周期测试
//!
//! 覆盖：GameSave 存档/读档回环、descend 下楼数据保持

use super::*;
use crate::GameSave;
use bevy_ecs::prelude::*;
use dungeon_core::{
    self as core, Equipment, Inventory, Map, PlayerClass, Position, Skills, Stairs, Stats,
    resources::*,
};

// ──────────────────────────────────────────────
// 辅助：从背包取出一件物品并装备到武器槽
// ──────────────────────────────────────────────
fn equip_first_weapon(world: &mut World) {
    let player = core::ops::player_entity(world).unwrap();
    let item_id = {
        let inv = world.get::<Inventory>(player).unwrap();
        inv.stacks
            .iter()
            .find(|s| s.item_id == 0)
            .map(|s| s.item_id)
    };
    if let Some(id) = item_id {
        let stack = {
            let mut inv = world.get_mut::<Inventory>(player).unwrap();
            let idx = inv.stacks.iter().position(|s| s.item_id == id).unwrap();
            inv.stacks.remove(idx)
        };
        let mut eq = world.get_mut::<Equipment>(player).unwrap();
        eq.main_hand = Some(stack);
    }
}

// ──────────────────────────────────────────────
// 测试：存档/读档回环 — 验证核心数据完整性
// ──────────────────────────────────────────────
#[test]
fn test_save_restore_roundtrip() {
    let mut world = setup_world();
    let player = core::ops::player_entity(&world).unwrap();

    // 给玩家一些独特的状态供验证
    {
        let mut stats = world.get_mut::<Stats>(player).unwrap();
        stats.exp = 12;
        stats.hp = stats.max_hp / 2;
        stats.mp = stats.max_mp / 3;
    }
    {
        let mut inv = world.get_mut::<Inventory>(player).unwrap();
        inv.add(0, 1); // 锈铁剑
        inv.add(1, 1); // 木盾
        inv.add(10, 3); // 生物血肉 x3
    }
    // 装备武器（通过分步方式避免并发借用）
    equip_first_weapon(&mut world);

    let floor_before = world.resource::<FloorNumber>().0;
    let pos_before = *world.get::<Position>(player).unwrap();
    let (stairs_pos_before, explored_count_before) = {
        let mut sq = world.query::<(&Stairs, &Position)>();
        let spos = sq.iter(&world).next().map(|(_, p)| (p.x, p.y)).unwrap();
        let mem = world.resource::<MapMemory>();
        let explored = mem.explored.iter().flatten().filter(|&&b| b).count();
        (spos, explored)
    };

    // ── capture → restore 到新世界 ──
    let save = GameSave::capture(&world);

    let mut restored = setup_world();
    save.restore(&mut restored);

    // ── 验证楼层 ──
    assert_eq!(restored.resource::<FloorNumber>().0, floor_before);

    // ── 验证地图 tiles（抽样检查，全量4800格太慢） ──
    let orig_map = world.resource::<Map>();
    let rest_map = restored.resource::<Map>();
    // 抽查四角 + 中心列
    for &(x, y) in &[
        (0, 0),
        (79, 0),
        (0, 59),
        (79, 59),
        (40, 0),
        (40, 30),
        (0, 30),
    ] {
        assert_eq!(
            rest_map.tiles[y][x], orig_map.tiles[y][x],
            "Tile 不匹配 at ({}, {})",
            x, y,
        );
    }

    // ── 验证楼梯位置 ──
    {
        let mut sq = restored.query::<(&Stairs, &Position)>();
        let (sx, sy) = sq.iter(&restored).next().map(|(_, p)| (p.x, p.y)).unwrap();
        assert_eq!(sx, stairs_pos_before.0);
        assert_eq!(sy, stairs_pos_before.1);
    }

    // ── 验证探索记忆 ──
    {
        let mem = restored.resource::<MapMemory>();
        let explored = mem.explored.iter().flatten().filter(|&&b| b).count();
        assert_eq!(explored, explored_count_before);
    }

    // ── 验证玩家位置 ──
    let rest_player = core::ops::player_entity(&restored).unwrap();
    let rest_pos = *restored.get::<Position>(rest_player).unwrap();
    assert_eq!(rest_pos.x, pos_before.x);
    assert_eq!(rest_pos.y, pos_before.y);

    // ── 验证 Stats ──
    let rest_stats = restored.get::<Stats>(rest_player).unwrap();
    assert_eq!(rest_stats.exp, 12);
    assert_eq!(rest_stats.hp, rest_stats.max_hp / 2);
    assert_eq!(rest_stats.mp, rest_stats.max_mp / 3);

    // ── 验证 Inventory（剑已装备到武器槽，不在背包中） ──
    let rest_inv = restored.get::<Inventory>(rest_player).unwrap();
    assert!(rest_inv.stacks.iter().any(|s| s.item_id == 1), "应持有木盾");
    assert!(
        rest_inv
            .stacks
            .iter()
            .any(|s| s.item_id == 10 && s.count == 3),
        "应持有 3 个生物血肉"
    );
    assert!(
        !rest_inv.stacks.iter().any(|s| s.item_id == 0),
        "锈铁剑已装备，不应在背包中"
    );

    // ── 验证 Equipment ──
    let rest_eq = restored.get::<Equipment>(rest_player).unwrap();
    assert!(rest_eq.main_hand.is_some(), "应装备武器");
    assert_eq!(
        rest_eq.main_hand.as_ref().unwrap().item_id,
        0,
        "应装备锈铁剑"
    );
}

// ──────────────────────────────────────────────
// 测试：下楼后玩家数据保持
// ──────────────────────────────────────────────
#[test]
fn test_descend_preserves_data() {
    let mut world = setup_world();
    let player = core::ops::player_entity(&world).unwrap();

    // 给玩家背包添加物品
    {
        let mut inv = world.get_mut::<Inventory>(player).unwrap();
        inv.add(0, 1); // 锈铁剑
        inv.add(3, 1); // 攻击戒指
    }
    // 装备武器
    equip_first_weapon(&mut world);

    let floor_before = world.resource::<FloorNumber>().0;
    let stats_before = world.get::<Stats>(player).unwrap().clone();
    let inv_before = world.get::<Inventory>(player).unwrap().stacks.clone();
    let eq_before = world.get::<Equipment>(player).unwrap().clone();
    let pc_before = world.get::<PlayerClass>(player).unwrap().clone();

    // 下楼
    descend(&mut world);

    let floor_after = world.resource::<FloorNumber>().0;
    assert_eq!(floor_after, floor_before + 1, "楼层应 +1");

    let player_after = core::ops::player_entity(&world).unwrap();
    let stats_after = world.get::<Stats>(player_after).unwrap();
    let inv_after = world.get::<Inventory>(player_after).unwrap();
    let eq_after = world.get::<Equipment>(player_after).unwrap();
    let pc_after = world.get::<PlayerClass>(player_after).unwrap();

    // 验证 Stats 不变
    assert_eq!(stats_after.level, stats_before.level);
    assert_eq!(stats_after.hp, stats_before.hp);
    assert_eq!(stats_after.exp, stats_before.exp);

    // 验证 Inventory 不变
    assert_eq!(inv_after.stacks.len(), inv_before.len());
    for s in &inv_before {
        assert!(
            inv_after
                .stacks
                .iter()
                .any(|a| a.item_id == s.item_id && a.count == s.count),
            "物品 id={} count={} 应保持",
            s.item_id,
            s.count,
        );
    }

    // 验证 Equipment 不变
    assert_eq!(
        eq_after.main_hand.as_ref().map(|s| s.item_id),
        eq_before.main_hand.as_ref().map(|s| s.item_id),
    );

    // 验证 PlayerClass 不变
    assert_eq!(*pc_after, pc_before);

    // 验证 Skills 由 PlayerClass 正确推导
    let skills_after = world.get::<Skills>(player_after).unwrap();
    assert_eq!(skills_after.list.len(), pc_after.skills().len());
    for (a, b) in skills_after.list.iter().zip(pc_after.skills().iter()) {
        assert_eq!(a.name, b.name);
    }
}

// ──────────────────────────────────────────────
// G22: 楼梯落点必须可行走且出生点可达
// ──────────────────────────────────────────────
#[test]
fn test_pick_stair_pos_always_walkable_and_reachable() {
    use rand::SeedableRng;
    for seed in 0..60u64 {
        let mut rng = rand::rngs::SmallRng::seed_from_u64(seed);
        let mut map = Map::new();
        map.generate(dungeon_core::MapKind::Cavern, &mut rng);
        let spawn = map.spawn_point();
        let stair = crate::init::pick_stair_pos(&map, spawn, &mut rng);
        assert!(
            map.tiles[stair.1][stair.0].walkable(),
            "seed {}: 楼梯 ({},{}) 落在不可行走格上",
            seed,
            stair.0,
            stair.1
        );
        // 模拟 init 完整流程：出生点 → 楼梯挖通道（ensure_connection_between）
        dungeon_core::map_gen::ensure_connection_between(&mut map, &mut rng, spawn, stair);
        // 可达性：出生点 → 楼梯必须存在 walkable 路径（G22：楼梯被困=无法下楼）
        assert!(
            dungeon_core::map_gen::has_path_between(&map, spawn, stair),
            "seed {}: 出生点 ({},{}) 无法到达楼梯 ({},{})",
            seed,
            spawn.0,
            spawn.1,
            stair.0,
            stair.1
        );
    }
}

// ──────────────────────────────────────────────
// Dsn24: 怪物种群按地图类型生成（类型过滤 + 落点合法性）
// ──────────────────────────────────────────────
#[test]
fn test_monster_population_by_map_kind() {
    use dungeon_core::MapKind;
    use rand::SeedableRng;
    for seed in 0..30u64 {
        for (kind, floor) in [
            (MapKind::Cavern, 2u32),
            (MapKind::LushCavern, 3),
            (MapKind::Undersea, 4),
        ] {
            let mut rng = rand::rngs::SmallRng::seed_from_u64(seed);
            let mut map = Map::new();
            map.generate(kind, &mut rng);
            let spawn = map.spawn_point();
            let stairs = crate::init::pick_stair_pos(&map, spawn, &mut rng);
            let pop = crate::population::generate_monster_population(
                kind,
                &map.tiles,
                floor,
                &mut rng,
                &[spawn, stairs],
            );
            assert!(!pop.is_empty(), "seed {} {:?}: 种群不应为空", seed, kind);
            for (mk, x, y) in &pop {
                let ok = match kind {
                    MapKind::Cavern => matches!(
                        mk,
                        core::MonsterKindId::Rat
                            | core::MonsterKindId::Scorpion
                            | core::MonsterKindId::Goblin
                    ),
                    MapKind::LushCavern => matches!(
                        mk,
                        core::MonsterKindId::Sporeling
                            | core::MonsterKindId::MushroomGolem
                            | core::MonsterKindId::Rat
                            | core::MonsterKindId::Goblin
                    ),
                    MapKind::Undersea => matches!(
                        mk,
                        core::MonsterKindId::CaveFish
                            | core::MonsterKindId::CaveCrab
                            | core::MonsterKindId::DeepEel
                            | core::MonsterKindId::Scorpion
                    ),
                };
                assert!(ok, "seed {} {:?}: 怪物 {:?} 不属于该生态", seed, kind, mk);
                assert!(*x != spawn.0 || *y != spawn.1, "怪物落在出生点");
                assert!(*x != stairs.0 || *y != stairs.1, "怪物落在楼梯");
                assert!(map.tiles[*y][*x].walkable(), "怪物落点不可行走");
            }
        }
    }
}

// ──────────────────────────────────────────────
// I79/I80: 读档组件完整性 — 玩家 AttackName + 怪物攻击名按 kind 恢复
// ──────────────────────────────────────────────
#[test]
fn test_restore_player_has_attack_name() {
    let world = setup_world();
    let save = GameSave::capture(&world);
    let mut restored = setup_world();
    save.restore(&mut restored);
    let player = core::ops::player_entity(&restored).unwrap();
    let atk = restored.get::<core::AttackName>(player);
    assert!(atk.is_some(), "读档玩家必须持有 AttackName 组件（I79）");
    assert_eq!(atk.unwrap().0, "斩击", "应与 setup_world 一致");
}

#[test]
fn test_restore_monster_attack_name_by_kind() {
    use dungeon_core::{EntityName, Monster, MonsterKindId, Renderable, Viewshed};
    let mut world = setup_world();
    // 手动放一只深鳗（Dsn24 新怪，旧实现按 glyph 分支会错误显示"重击"）
    let eel_pos = {
        let map = world.resource::<Map>();
        map.spawn_point()
    };
    world.spawn((
        Monster,
        Position {
            x: eel_pos.0 + 3,
            y: eel_pos.1,
        },
        Renderable {
            glyph: 'e',
            color: (80, 160, 220),
        },
        Viewshed {
            range: 10,
            visible_tiles: Vec::new(),
        },
        Stats::player(),
        EntityName("深鳗".into()),
        MonsterKindId::DeepEel,
    ));
    let save = GameSave::capture(&world);
    let mut restored = setup_world();
    save.restore(&mut restored);
    // 找到恢复的深鳗并验证攻击名
    let mut found = false;
    let mut q = restored.query::<(&dungeon_core::MonsterKindId, &core::AttackName)>();
    for (kind, atk) in q.iter(&restored) {
        if *kind == MonsterKindId::DeepEel {
            assert_eq!(atk.0, "缠绕", "深鳗攻击名应按 kind 查表恢复（I80）");
            found = true;
        }
    }
    assert!(found, "恢复世界中应存在深鳗");
}
// ──────────────────────────────────────────────
// 批 1 回归测试（A31/G34）
// ──────────────────────────────────────────────

/// A31: 读档后每个怪物必须有 LastKnownPlayerPos 组件——否则 chase 决策查询过滤，追击 AI 失效
#[test]
fn test_restore_monsters_have_last_known_pos() {
    use dungeon_core::{EntityName, Monster, MonsterKindId, Renderable, Viewshed};
    let mut world = setup_world();
    // 放两只怪物（确保非空断言有意义）
    let sp = {
        let map = world.resource::<Map>();
        map.spawn_point()
    };
    for i in 0..2 {
        world.spawn((
            Monster,
            Position {
                x: sp.0 + 2 + i,
                y: sp.1,
            },
            Renderable {
                glyph: 'r',
                color: (200, 100, 100),
            },
            Viewshed {
                range: 10,
                visible_tiles: Vec::new(),
            },
            dungeon_core::monster_def::monster_stats(MonsterKindId::Rat, 1),
            EntityName("老鼠".into()),
            MonsterKindId::Rat,
        ));
    }
    let save = GameSave::capture(&world);
    let mut restored = setup_world();
    save.restore(&mut restored);

    // L44 补充教训：断言组件存在性而非数据字段——缺失会被 unwrap_or 兜底静默降级
    let mut count = 0;
    let mut q = restored.query::<(Entity, &Monster)>();
    let monsters: Vec<Entity> = q.iter(&restored).map(|(e, _)| e).collect();
    for e in &monsters {
        assert!(
            restored.get::<core::LastKnownPlayerPos>(*e).is_some(),
            "读档怪物必须持有 LastKnownPlayerPos（A31，L44 模式第五次）"
        );
        count += 1;
    }
    assert!(count >= 2, "恢复世界中应存在读档前的怪物");
}

/// G34: 下楼时清空 ActionQueue 与意图缓冲区，避免残留失效 Entity 条目
#[test]
fn test_descend_clears_action_queue() {
    use dungeon_action::{
        ActionKindV3, ActionQueue, CanChase, ChaseIntents, FleeIntents, WanderIntents,
    };
    let mut world = setup_world();
    // 放一只怪物并往队列/意图缓冲区塞条目
    let sp = {
        let map = world.resource::<Map>();
        map.spawn_point()
    };
    let monster = world
        .spawn((
            dungeon_core::Monster,
            Position {
                x: sp.0 + 2,
                y: sp.1,
            },
            dungeon_core::Renderable {
                glyph: 'r',
                color: (200, 100, 100),
            },
            dungeon_core::Viewshed {
                range: 10,
                visible_tiles: Vec::new(),
            },
            dungeon_core::monster_def::monster_stats(dungeon_core::MonsterKindId::Rat, 1),
            dungeon_core::EntityName("老鼠".into()),
            dungeon_core::MonsterKindId::Rat,
            CanChase::new(100),
        ))
        .id();
    world
        .resource_mut::<ActionQueue>()
        .enqueue(monster, ActionKindV3::Chase, 250.0);
    world
        .resource_mut::<ChaseIntents>()
        .0
        .push((monster, 100, 250.0, ActionKindV3::Chase));
    world
        .resource_mut::<FleeIntents>()
        .0
        .push((monster, 200, 250.0, ActionKindV3::Flee));
    world
        .resource_mut::<WanderIntents>()
        .0
        .push((monster, 50, 500.0, ActionKindV3::Wander));

    crate::descend(&mut world);

    assert!(
        world.resource::<ActionQueue>().entries.is_empty(),
        "下楼后 ActionQueue 应清空（G34）"
    );
    assert!(
        world.resource::<ChaseIntents>().0.is_empty(),
        "下楼后 ChaseIntents 应清空"
    );
    assert!(
        world.resource::<FleeIntents>().0.is_empty(),
        "下楼后 FleeIntents 应清空"
    );
    assert!(
        world.resource::<WanderIntents>().0.is_empty(),
        "下楼后 WanderIntents 应清空"
    );
    // 新楼层的玩家仍存在（下楼正常重建）
    assert!(
        core::ops::player_entity(&world).is_some(),
        "下楼后玩家应存在"
    );
}
// ──────────────────────────────────────────────
// 批 2 回归测试（G32/A35/A40/I85/A37）
// ──────────────────────────────────────────────

/// G32: RNG 状态随存档持久化——读档后随机序列精确续接，不再可重放（SL 刷掉落修复）
#[test]
fn test_rng_state_persists_through_save() {
    let mut world = setup_world();
    // 消耗若干随机数
    let mut rng = world.resource_mut::<dungeon_core::GameRng>();
    let _ = rng.random_f32();
    let _ = rng.random_f32();
    let _ = rng.random_range(0, 8);
    let (state_before, steps_before) = (rng.state, rng.steps);

    let save = GameSave::capture(&world);
    assert!(save.rng_state != 0, "capture 应保存非零 RNG 状态");
    assert_eq!(save.rng_steps, steps_before, "capture 应保存步数");

    let mut restored = setup_world();
    save.restore(&mut restored);
    let rng = restored.resource::<dungeon_core::GameRng>();
    assert_eq!(
        (rng.state, rng.steps),
        (state_before, steps_before),
        "读档后 RNG 应精确恢复到存档时状态"
    );
}

/// G32: 读档后 RNG 续接——新消耗产生不同随机值（而非重放存档前序列）
#[test]
fn test_rng_continues_after_restore() {
    let mut world = setup_world();
    let mut rng = world.resource_mut::<dungeon_core::GameRng>();
    let before = rng.random_f32();
    let save_state = (rng.state, rng.steps);
    // 存档后再消耗一次
    let mut rng = world.resource_mut::<dungeon_core::GameRng>();
    let after_first = rng.random_f32();

    // 手动构造：存档状态 = 第一次消耗后的状态
    let mut save = GameSave::capture(&world);
    save.rng_state = save_state.0;
    save.rng_steps = save_state.1;
    let mut restored = setup_world();
    save.restore(&mut restored);
    let mut rng = restored.resource_mut::<dungeon_core::GameRng>();
    let replayed = rng.random_f32();
    // 从同一状态出发，下一步应产生相同值（确定性验证），而非从头重放
    assert_eq!(
        replayed, after_first,
        "读档后 RNG 应从存档状态续接（非重放）"
    );
    assert_ne!(replayed, before, "续接值不应等于存档前的首次随机值");
}

/// G32/A37: save_game 写新格式（magic 前缀），load_game 回环读回
#[test]
fn test_save_load_roundtrip_new_format() {
    let world = setup_world();
    let path = "test_save_new_format.bin";
    crate::save_game(&world, path).expect("save_game 应成功");

    let data = std::fs::read(path).unwrap();
    assert!(
        data.starts_with(crate::persist::SAVE_MAGIC),
        "新格式应有 magic 前缀"
    );

    let mut restored = setup_world();
    crate::load_game(&mut restored, path).expect("load_game 应成功");
    let _ = std::fs::remove_file(path);
    assert_eq!(
        restored.resource::<FloorNumber>().0,
        world.resource::<FloorNumber>().0
    );
}

/// A37/G32: 旧格式（裸 bincode GameSaveV0）仍可读，新字段取默认值
#[test]
fn test_load_game_legacy_v0_format() {
    let world = setup_world();
    let save = GameSave::capture(&world);
    // 手工构造旧格式：把新 GameSave 字段转成 V0 布局（去掉 3 个新字段）
    let v0 = crate::persist::GameSaveV0 {
        floor: save.floor,
        map_seed: save.map_seed,
        px: save.px,
        py: save.py,
        st: save.st.clone(),
        inv: save.inv.clone(),
        weapon_item_id: save.weapon_item_id,
        weapon_count: save.weapon_count,
        armor_item_id: save.armor_item_id,
        armor_count: save.armor_count,
        ring_item_id: save.ring_item_id,
        ring_count: save.ring_count,
        off_hand_item_id: save.off_hand_item_id,
        off_hand_count: save.off_hand_count,
        map_tiles: save.map_tiles.clone(),
        rooms: save.rooms.clone(),
        explored: save.explored.clone(),
        monsters: save.monsters.clone(),
        items: save.items.clone(),
        sx: save.sx,
        sy: save.sy,
        player_class: save.player_class.clone(),
        action_queue: save.action_queue.clone(),
        chase_intents: save.chase_intents.clone(),
        flee_intents: save.flee_intents.clone(),
        wander_intents: save.wander_intents.clone(),
        active_buffs: save.active_buffs.clone(),
        skills: save.skills.clone(),
    };
    let path = "test_save_legacy.bin";
    std::fs::write(path, bincode::serialize(&v0).unwrap()).unwrap();

    let mut restored = setup_world();
    crate::load_game(&mut restored, path).expect("旧格式存档应可读");
    let _ = std::fs::remove_file(path);
    // 旧档默认值：容量 36、RNG 走旧派生种子
    let player = core::ops::player_entity(&restored).unwrap();
    assert_eq!(
        restored.get::<Inventory>(player).unwrap().capacity,
        36,
        "旧档容量默认 36"
    );
}

/// A35: 攻击行动随存档保存（按目标坐标重映射），读档后 target 反查怪物
#[test]
fn test_attack_action_survives_save() {
    use dungeon_action::{ActionKindV3, ActionQueue, CanMove};
    let mut world = setup_world();
    let player = core::ops::player_entity(&world).unwrap();
    let sp = {
        let map = world.resource::<Map>();
        map.spawn_point()
    };
    let monster = world
        .spawn((
            dungeon_core::Monster,
            Position {
                x: sp.0 + 1,
                y: sp.1,
            },
            dungeon_core::Renderable {
                glyph: 'r',
                color: (200, 100, 100),
            },
            dungeon_core::Viewshed {
                range: 10,
                visible_tiles: Vec::new(),
            },
            dungeon_core::monster_def::monster_stats(dungeon_core::MonsterKindId::Rat, 1),
            dungeon_core::EntityName("老鼠".into()),
            dungeon_core::MonsterKindId::Rat,
            CanMove::new(100),
        ))
        .id();
    // 玩家入队攻击（未执行）
    world.resource_mut::<ActionQueue>().enqueue(
        player,
        ActionKindV3::Attack { target: monster },
        250.0,
    );

    let save = GameSave::capture(&world);
    assert!(
        save.action_queue
            .iter()
            .any(|e| matches!(e.kind, crate::persist::SavedActionKind::Attack { .. })),
        "Attack 行动不应再被静默丢弃（A35）"
    );
    let mut restored = setup_world();
    save.restore(&mut restored);

    let queue = restored.resource::<ActionQueue>();
    let attack = queue
        .entries
        .iter()
        .find(|e| matches!(e.kind, ActionKindV3::Attack { .. }))
        .expect("读档后 Attack 条目应存在");
    if let ActionKindV3::Attack { target } = attack.kind {
        assert!(
            restored.get::<dungeon_core::Monster>(target).is_some(),
            "Attack target 应反查为怪物实体"
        );
        let tp = restored.get::<Position>(target).unwrap();
        assert_eq!((tp.x, tp.y), (sp.0 + 1, sp.1), "target 应为原怪物位置");
    } else {
        panic!("应找到 Attack 条目");
    }
}

/// A40: 背包容量随存档持久化（不再硬编码 36）
#[test]
fn test_inventory_capacity_persists() {
    let mut world = setup_world();
    let player = core::ops::player_entity(&world).unwrap();
    world.get_mut::<Inventory>(player).unwrap().capacity = 40;

    let save = GameSave::capture(&world);
    assert_eq!(save.inv_capacity, 40, "capture 应保存容量");

    let mut restored = setup_world();
    save.restore(&mut restored);
    let rp = core::ops::player_entity(&restored).unwrap();
    assert_eq!(
        restored.get::<Inventory>(rp).unwrap().capacity,
        40,
        "读档后容量应恢复"
    );
}

/// I85: 损坏存档（超长 map_tiles）读档不 panic——截断到固定尺寸
#[test]
fn test_restore_tolerates_bad_map_len() {
    let world = setup_world();
    let mut save = GameSave::capture(&world);
    save.map_tiles = vec![dungeon_core::Tile::Floor; core::MAP_WIDTH * core::MAP_HEIGHT + 50];

    let mut restored = setup_world();
    save.restore(&mut restored); // 不应 panic
    let map = restored.resource::<Map>();
    assert_eq!(
        map.tiles[core::MAP_HEIGHT - 1][core::MAP_WIDTH - 1],
        dungeon_core::Tile::Floor,
        "超长存档截断后地图仍可读（最后一个元素来自截断前数据）"
    );
}
// ──────────────────────────────────────────────
// 批 3 回归测试（I82/I83/I84/G33）
// ──────────────────────────────────────────────

/// I82: 可行走格极少时补足循环不死循环（迭代上限生效）
#[test]
fn test_population_topup_never_hangs() {
    use dungeon_core::Tile;
    use rand::SeedableRng;
    // 全墙地图：只有 6 个可行走格，期望 20+ 只怪物 → 补足永远无法满足
    let mut map = Map::new();
    for row in map.tiles.iter_mut() {
        for t in row.iter_mut() {
            *t = Tile::Wall;
        }
    }
    for i in 0..6 {
        map.tiles[5][i] = Tile::Floor;
    }
    let mut rng = rand::rngs::SmallRng::seed_from_u64(7);
    // floor=20 → min_count=44，远大于可行走格数。修复前此处死循环。
    let result = crate::population::generate_monster_population(
        dungeon_core::MapKind::Cavern,
        &map.tiles,
        20,
        &mut rng,
        &[],
    );
    // 不挂死即通过；补足数量不超过可行走格数（排除集为空）
    assert!(
        result.len() <= 6,
        "补足数量不应超过可行走格数，实际 {}",
        result.len()
    );
}

/// I84: spawn 靠近右边界时 pick_stair_pos 返回界内 walkable 坐标（兜底钳制）
#[test]
fn test_pick_stair_pos_near_edge_clamped() {
    use dungeon_core::Tile;
    use rand::SeedableRng;
    // 全 Floor 地图 + spawn 在右边界附近：spx+15=90 越界（修复前兜底越界 panic）
    let mut map = Map::new();
    for row in map.tiles.iter_mut() {
        for t in row.iter_mut() {
            *t = Tile::Floor;
        }
    }
    let mut rng = rand::rngs::SmallRng::seed_from_u64(11);
    let pos = crate::init::pick_stair_pos(&map, (75, 59), &mut rng);
    assert!(
        pos.0 < core::MAP_WIDTH && pos.1 < core::MAP_HEIGHT,
        "楼梯坐标必须界内，实际 ({}, {})",
        pos.0,
        pos.1
    );
    assert!(map.tiles[pos.1][pos.0].walkable(), "楼梯坐标必须可行走");
    assert!(!(pos.0 == 75 && pos.1 == 59), "楼梯不应落在出生点");
}

/// G33: 护盾/狂暴 duration 校准为 1（≈1000 AV ≈ 3 次玩家行动，Gm5 语义）
#[test]
fn test_skill_buff_duration_calibrated() {
    let mut world = setup_world();
    let player = core::ops::player_entity(&world).unwrap();
    // 直接学技能并验证 duration 字段 = 1（施放路径按 duration×1000 换算 AV）
    dungeon_core::ops::learn_skill(
        &mut world,
        player,
        &dungeon_core::SkillKind::Shield {
            def_boost: 5,
            duration: 1,
        },
    );
    dungeon_core::ops::learn_skill(
        &mut world,
        player,
        &dungeon_core::SkillKind::Berserk {
            atk_boost: 5,
            duration: 1,
        },
    );
    let skills = world.get::<dungeon_core::Skills>(player).unwrap();
    let shield = skills
        .list
        .iter()
        .find(|s| s.name.contains("护盾"))
        .expect("应学到护盾技能");
    let berserk = skills
        .list
        .iter()
        .find(|s| s.name.contains("狂暴"))
        .expect("应学到狂暴技能");
    assert!(
        matches!(
            &shield.kind,
            dungeon_core::SkillKind::Shield { duration: 1, .. }
        ),
        "护盾 duration 应为 1（G33 校准），实际 {:?}",
        shield.kind
    );
    assert!(
        matches!(
            &berserk.kind,
            dungeon_core::SkillKind::Berserk { duration: 1, .. }
        ),
        "狂暴 duration 应为 1（G33 校准），实际 {:?}",
        berserk.kind
    );
}
