//! 存档 V1 的集成测试（DsnE19）。
//!
//! 覆盖：投影 roundtrip（数值 + 身体槽 + 行囊嵌套物品）、楼层重建、版本/文件/损坏
//! 三类错误，以及 F5/F9 经 `App::handle` 的端到端接线。

use std::path::PathBuf;

use bevy_ecs::prelude::*;
use dungeon_app::App;
use dungeon_app::save::{self, ExpSave, ItemSave, PlayerSave, SaveError, SaveV1, VitSave};
use ecs_core::{
    Amount, BODY, Container, Experience, FloorNumber, Health, Item, ItemId, ItemTag, Level, Magic,
    MagicMastery, Map, Monster, Position, Stairs,
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

/// 直接造一个物品实体（测试里绕过 `Commands`）。
fn spawn_item(world: &mut World, id: u16, count: u32) -> Entity {
    world
        .spawn((Item::new(ItemId(id)), Amount::new(count)))
        .id()
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
            Magic {
                current: 3.0,
                max: 9.0,
            },
            MagicMastery(1.5),
        ));

        let sword = spawn_item(world, 0, 1);
        let main_hand = body_slot_for(ItemTag::Weapon);
        world
            .get_mut::<Container>(player)
            .expect("玩家有身体容器")
            .insert(main_hand, sword, ItemId(0).def().tags)
            .expect("主手槽接受武器");

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
    assert_eq!((health.current, health.max), (12.0, 38.0));
    let magic = world.get::<Magic>(player).unwrap();
    assert_eq!((magic.current, magic.max), (3.0, 9.0));
    assert_eq!(world.get::<MagicMastery>(player).unwrap().0, 1.5);

    let body = world.get::<Container>(player).unwrap();
    let sword = body
        .slot(body_slot_for(ItemTag::Weapon))
        .expect("主手槽里还有剑");
    assert_eq!(world.get::<Item>(sword).unwrap().id, ItemId(0));

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
    let save = SaveV1 {
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
            health: VitSave {
                current: 38.0,
                max: 38.0,
            },
            magic: VitSave {
                current: 9.0,
                max: 9.0,
            },
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
        slots: None,
    });
    let save = SaveV1 {
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
            health: VitSave {
                current: 38.0,
                max: 38.0,
            },
            magic: VitSave {
                current: 9.0,
                max: 9.0,
            },
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
