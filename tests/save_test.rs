//! 存档 V5 的集成测试（DsnE19 / DsnE22 阶段 ④ / DsnE23 / DsnE24 阶段 ② / DsnE27）。
//!
//! 覆盖：投影 roundtrip（数值 + 身体槽 + 行囊嵌套物品 + 武器养成进度）、
//! 楼层重建、版本/文件/损坏/物品数据版本四类错误、模板装配与存档装配的一致性，以及
//! F5/F9 经 `App::handle` 的端到端接线。V3 起只存 `health/mana` 的 current，上限读档时
//! 按等级曲线重算（DsnE23）；V4 起武器的等级/经验也作为实例差异进档；
//! V5 删除耐久实例差异（DsnE27），物品 schema 升到 8。

use std::path::PathBuf;

use bevy_ecs::prelude::*;
use dungeon_app::App;
use dungeon_app::save::{
    self, ExpSave, ItemOverrides, ItemSave, PlayerSave, SaveError, SaveV5, WeaponSave,
};
use ecs_core::{
    Amount, BODY, Container, Experience, FloorNumber, Health, ITEMS_SCHEMA_VERSION, Item, ItemId,
    ItemTag, Level, MagicMastery, Mana, Map, Monster, Pool, Position, Stairs, WeaponProgress,
    hp_curve, mp_curve, spawn_item_assembled,
};
use render_api::{InputEvent, Key, KeyEvent};

/// 每局测试一个唯一临时文件（进程 id + 名字），开始前先删掉旧文件。
fn temp_path(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("dungeon-save-{}-{name}.bin", std::process::id()));
    let _ = std::fs::remove_file(&path);
    path
}

/// 能接受某个标签的身体槽位下标（由 `BODY` 的过滤表推出，不写常数）。
fn body_slot_for(tag: ItemTag) -> usize {
    BODY.slots
        .iter()
        .position(|slot| slot.accept.accepts(&[tag]))
        .expect("身体必须有能接受该标签的槽位")
}

/// 直接造一个物品实体（测试里绕过 `Commands`）：走蓝图装配，实例组件自动带上。
fn spawn_item(world: &mut World, id: u16, count: u32) -> Entity {
    spawn_item_assembled(world, ItemId(id), count)
}

fn count_monsters(world: &mut World) -> usize {
    let mut query = world.query_filtered::<Entity, With<Monster>>();
    query.iter(world).count()
}

#[test]
fn a_save_round_trip_restores_the_player_and_the_container_tree() {
    let mut app = App::new(2026, (80, 30));
    let player = ecs_core::player_entity(app.world()).expect("新开局有玩家");

    // 改出与初始状态都不相同的数值 + 一件装备 + 行囊里一堆蘑菇。
    {
        let world = app.world_mut();
        world.entity_mut(player).insert((
            Position::new(3, 4),
            Level(3),
            Experience::new(40.0, 100.0),
            Health::full(12.0, 38.0),
            Mana::from(Pool::full(3.0, 9.0)),
            MagicMastery(1.5),
        ));

        let sword = spawn_item(world, 0, 1);
        let main_hand = body_slot_for(ItemTag::Weapon);
        world
            .get_mut::<Container>(player)
            .expect("玩家有身体容器")
            .insert(main_hand, sword, ItemId(0).def().tags)
            .expect("主手槽接受武器");
        // 实例数据：剑养成到 3 级 12.5 经验（模板装配是 1 级 0 经验），存档要记住。
        {
            let mut progress = world
                .get_mut::<WeaponProgress>(sword)
                .expect("蓝图带成长曲线的武器装配时应有 WeaponProgress");
            progress.level = 3;
            progress.exp = 12.5;
        }

        let mushroom = spawn_item(world, 25, 3);
        let pack = world
            .get::<Container>(player)
            .and_then(|body| body.slot(ecs_core::pack_slot_index()))
            .expect("背包槽里有行囊");
        world
            .get_mut::<Container>(pack)
            .expect("行囊是容器")
            .insert(0, mushroom, &[])
            .expect("行囊第 0 格是空的");
    }

    let path = temp_path("roundtrip");
    save::write(app.world(), &path).expect("写存档");
    let loaded = App::load(&path, (80, 30)).expect("读存档");
    let _ = std::fs::remove_file(&path);

    let world = loaded.world();
    let player = ecs_core::player_entity(world).expect("读档后仍有玩家");
    assert_eq!(world.resource::<FloorNumber>().0, 1);
    assert_eq!(world.get::<Position>(player).unwrap().to_tuple(), (3, 4));
    assert_eq!(world.get::<Level>(player).unwrap().0, 3);
    assert_eq!(world.get::<Experience>(player).unwrap().exp, 40.0);
    let health = world.get::<Health>(player).unwrap();
    // current 按存档还原；max 由等级曲线与装备修正重算（Lv3 → 56）。
    assert_eq!(health.current, 12.0);
    assert_eq!(health.max, hp_curve(3));
    let mana = world.get::<Mana>(player).unwrap();
    assert_eq!(mana.current, 3.0);
    assert_eq!(mana.max, mp_curve(3));
    assert_eq!(world.get::<MagicMastery>(player).unwrap().0, 1.5);

    let body = world.get::<Container>(player).unwrap();
    let sword = body
        .slot(body_slot_for(ItemTag::Weapon))
        .expect("主手槽里还有剑");
    assert_eq!(world.get::<Item>(sword).unwrap().id, ItemId(0));
    let progress = world
        .get::<WeaponProgress>(sword)
        .expect("读档后剑仍有养成进度");
    assert_eq!((progress.level, progress.exp), (3, 12.5));

    let pack = body
        .slot(ecs_core::pack_slot_index())
        .and_then(|pack| world.get::<Container>(pack))
        .expect("行囊还是容器");
    let mushroom = pack.slot(0).expect("行囊第 0 格里有蘑菇");
    assert_eq!(world.get::<Item>(mushroom).unwrap().id, ItemId(25));
    assert_eq!(world.get::<Amount>(mushroom).unwrap().0, 3);
}

#[test]
fn a_save_on_a_lower_floor_rebuilds_the_same_floor() {
    let mut app = App::new(7, (80, 30));

    // 把玩家搬到楼梯上再下楼（`descend` 只从楼梯生效）。
    {
        let world = app.world_mut();
        let stairs = {
            let mut query = world.query_filtered::<&Position, With<Stairs>>();
            *query.iter(world).next().expect("每层都有楼梯")
        };
        let player = ecs_core::player_entity(world).expect("有玩家");
        *world.get_mut::<Position>(player).expect("玩家有位置") = stairs;
        assert!(ecs_core::descend(world), "站在楼梯上应该能下楼");
        assert_eq!(world.resource::<FloorNumber>().0, 2);
    }

    let path = temp_path("floor2");
    save::write(app.world(), &path).expect("写存档");
    let saved_spawn = app.world().resource::<Map>().spawn_point();
    let saved_monsters = count_monsters(app.world_mut());
    assert!(saved_monsters > 0, "第 2 层应该有怪物");

    let mut loaded = App::load(&path, (80, 30)).expect("读存档");
    let _ = std::fs::remove_file(&path);

    // 同 (seed, floor) 的生成是确定的：出生点与怪物数量都应一致。
    assert_eq!(loaded.world().resource::<FloorNumber>().0, 2);
    assert_eq!(loaded.world().resource::<Map>().spawn_point(), saved_spawn);
    assert_eq!(count_monsters(loaded.world_mut()), saved_monsters);
}

#[test]
fn an_unknown_version_in_the_file_header_is_rejected() {
    let save = SaveV5 {
        items_schema: ITEMS_SCHEMA_VERSION,
        map_seed: 1,
        floor: 1,
        player: PlayerSave {
            x: 1,
            y: 1,
            level: 1,
            experience: ExpSave {
                exp: 0.0,
                exp_to_next: 100.0,
            },
            health: 38.0,
            mana: 9.0,
            magic_mastery: 0.0,
            body: vec![None; BODY.len()],
        },
    };
    // 版本在文件头里，载荷仍是合法的 bincode——证明是"先判版本、后解载荷"。
    let mut bytes = save::FILE_MAGIC.to_vec();
    bytes.extend_from_slice(&999u32.to_le_bytes());
    bytes.extend_from_slice(&bincode::serialize(&save).expect("编码载荷"));
    let path = temp_path("version");
    std::fs::write(&path, bytes).expect("写文件");
    let result = save::read(&path);
    let _ = std::fs::remove_file(&path);

    match result {
        Err(SaveError::UnsupportedVersion(999)) => {}
        other => panic!("期望版本被拒绝，实际：{other:?}"),
    }
}

#[test]
fn a_file_without_the_magic_bytes_is_rejected_as_corrupt() {
    let path = temp_path("magic");
    std::fs::write(&path, b"not a save file at all").expect("写文件");
    let result = save::read(&path);
    let _ = std::fs::remove_file(&path);

    match result {
        Err(SaveError::Corrupt(why)) => assert!(why.contains("文件头"), "错误应指向文件头：{why}"),
        other => panic!("期望文件头被拒绝，实际：{other:?}"),
    }
}

#[test]
fn a_missing_file_is_reported_as_io() {
    let path = std::env::temp_dir().join("dungeon-save-does-not-exist.bin");
    let _ = std::fs::remove_file(&path);
    match save::read(&path) {
        Err(SaveError::Io(_)) => {}
        other => panic!("期望文件不存在，实际：{other:?}"),
    }
}

#[test]
fn an_unknown_item_id_in_the_save_is_reported_as_corrupt() {
    let mut body = vec![None; BODY.len()];
    body[0] = Some(ItemSave {
        id: 9999,
        count: 1,
        overrides: ItemOverrides::default(),
        slots: None,
    });
    let save = SaveV5 {
        items_schema: ITEMS_SCHEMA_VERSION,
        map_seed: 1,
        floor: 1,
        player: PlayerSave {
            x: 1,
            y: 1,
            level: 1,
            experience: ExpSave {
                exp: 0.0,
                exp_to_next: 100.0,
            },
            health: 38.0,
            mana: 9.0,
            magic_mastery: 0.0,
            body,
        },
    };
    match save::restore_to_world(&save) {
        Err(SaveError::Corrupt(why)) => {
            assert!(why.contains("9999"), "错误里应该带编号：{why}");
        }
        other => panic!("期望未知物品被拒绝，实际：{other:?}"),
    }
}

#[test]
fn f5_and_f9_reload_the_game_through_the_keyboard() {
    let mut app = App::new(11, (80, 30));
    let player = ecs_core::player_entity(app.world()).expect("有玩家");
    app.world_mut().entity_mut(player).insert(Level(5));

    assert!(app.handle(InputEvent::Key(KeyEvent::pressed(Key::F(5)))));
    let path = std::env::current_dir()
        .expect("有当前目录")
        .join(save::SAVE_FILE);
    assert!(path.exists(), "F5 应该写出 {}", save::SAVE_FILE);

    // 存盘后改状态：F9 应该回到存盘时的那一局。
    app.world_mut().entity_mut(player).insert(Level(9));
    assert!(app.handle(InputEvent::Key(KeyEvent::pressed(Key::F(9)))));

    let reloaded = ecs_core::player_entity(app.world()).expect("读档后仍有玩家");
    assert_eq!(app.world().get::<Level>(reloaded).unwrap().0, 5);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn an_old_v1_file_is_rejected_by_the_header_version() {
    let path = temp_path("v1");
    let mut bytes = save::FILE_MAGIC.to_vec();
    bytes.extend_from_slice(&1u32.to_le_bytes());
    std::fs::write(&path, bytes).expect("写文件");
    let result = save::read(&path);
    let _ = std::fs::remove_file(&path);

    match result {
        Err(SaveError::UnsupportedVersion(1)) => {}
        other => panic!("期望 V1 被版本头拒绝，实际：{other:?}"),
    }
}

#[test]
fn a_save_with_a_different_items_schema_is_rejected() {
    let save = SaveV5 {
        items_schema: ITEMS_SCHEMA_VERSION + 1,
        map_seed: 1,
        floor: 1,
        player: PlayerSave {
            x: 1,
            y: 1,
            level: 1,
            experience: ExpSave { exp: 0.0, exp_to_next: 100.0 },
            health: 38.0,
            mana: 9.0,
            magic_mastery: 0.0,
            body: vec![None; BODY.len()],
        },
    };
    let mut bytes = save::FILE_MAGIC.to_vec();
    bytes.extend_from_slice(&save::SAVE_VERSION.to_le_bytes());
    bytes.extend_from_slice(&bincode::serialize(&save).expect("编码载荷"));
    let path = temp_path("items-schema");
    std::fs::write(&path, bytes).expect("写文件");
    let result = save::read(&path);
    let _ = std::fs::remove_file(&path);

    match result {
        Err(SaveError::ContentVersion { save, current }) => {
            assert_eq!(save, ITEMS_SCHEMA_VERSION + 1);
            assert_eq!(current, ITEMS_SCHEMA_VERSION);
        }
        other => panic!("期望物品数据版本被拒绝，实际：{other:?}"),
    }
}

#[test]
fn template_assembly_matches_save_assembly_when_there_are_no_overrides() {
    let weapon_slot = body_slot_for(ItemTag::Weapon);
    let mut body = vec![None; BODY.len()];
    body[weapon_slot] = Some(ItemSave {
        id: 0,
        count: 1,
        overrides: ItemOverrides::default(),
        slots: None,
    });

    let save = SaveV5 {
        items_schema: ITEMS_SCHEMA_VERSION,
        map_seed: 1,
        floor: 1,
        player: PlayerSave {
            x: 1,
            y: 1,
            level: 1,
            experience: ExpSave { exp: 0.0, exp_to_next: 100.0 },
            health: 38.0,
            mana: 9.0,
            magic_mastery: 0.0,
            body,
        },
    };
    let mut world = save::restore_to_world(&save).expect("还原世界");
    let player = ecs_core::player_entity(&world).expect("有玩家");
    let restored = world
        .get::<Container>(player)
        .expect("身体容器")
        .slot(weapon_slot)
        .expect("主手有剑");
    let fresh = spawn_item_assembled(&mut world, ItemId(0), 1);

    let restored_progress = world
        .get::<WeaponProgress>(restored)
        .expect("存档装配带养成进度");
    let fresh_progress = world
        .get::<WeaponProgress>(fresh)
        .expect("模板装配带养成进度");
    assert_eq!(
        restored_progress, fresh_progress,
        "没有实例差异时，武器等级/经验也必须与模板装配一致"
    );
    assert_eq!((restored_progress.level, restored_progress.exp), (1, 0.0));
}

#[test]
fn an_illegal_weapon_progress_in_the_save_is_rejected() {
    let weapon_slot = body_slot_for(ItemTag::Weapon);
    let mut body = vec![None; BODY.len()];
    body[weapon_slot] = Some(ItemSave {
        id: 0,
        count: 1,
        overrides: ItemOverrides {
            // 锈铁剑的成长曲线最多 5 级；9 级是损坏的存档。
            weapon: Some(WeaponSave {
                level: 9,
                exp: 0.0,
            }),
        },
        slots: None,
    });
    let save = SaveV5 {
        items_schema: ITEMS_SCHEMA_VERSION,
        map_seed: 1,
        floor: 1,
        player: PlayerSave {
            x: 1,
            y: 1,
            level: 1,
            experience: ExpSave {
                exp: 0.0,
                exp_to_next: 100.0,
            },
            health: 38.0,
            mana: 9.0,
            magic_mastery: 0.0,
            body,
        },
    };
    match save::restore_to_world(&save) {
        Err(SaveError::Corrupt(why)) => {
            assert!(why.contains("养成进度"), "错误应指向养成进度：{why}");
        }
        other => panic!("期望非法养成进度被拒绝，实际：{other:?}"),
    }
}
