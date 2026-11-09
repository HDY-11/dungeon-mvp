//! 存档 V5：bincode 完整快照 + **手写投影**（设计见 `ecs_core/DESIGN.md` DsnE19 / DsnE22 阶段 ④ / DsnE23 / DsnE24 / DsnE27）。
//!
//! 只写"重开一局后无法重算"的状态：`map_seed`、楼层、玩家数值、身体容器树，
//! 以及物品的**实例差异**（目前是武器养成等级/经验；耐久已被 DsnE27 删除）。
//! 地图 / 怪物 / 楼梯 / 地面物品 / 格子效果都由 `(map_seed, floor)` 重新生成（DsnE17），
//! 所以磁盘上**没有 `Entity`**，也不存这些派生内容；物品的基础数值永远跟蓝图
//! （`assets/items.json`）走。
//!
//! # 为什么不是 `derive(Serialize)` 整个世界
//!
//! 世界里有 `Entity` 引用与派生资源；直接 derive 会把它们也变成磁盘契约。
//! 投影只写"玩家身上发生了什么"，形状小、能读懂、加字段时只需要管 `SAVE_VERSION`。
//!
//! # 文件格式（V5）
//!
//! ```text
//! 文件头（8 字节）：magic b"DNSV" + 版本 u32（小端，当前 5）
//! 载荷：bincode(SaveV5)
//! SaveV5 { items_schema, map_seed, floor, player: PlayerSave }
//! PlayerSave { x, y, level, experience, health, mana, magic_mastery,
//!              body: [Option<ItemSave>] }        // 身体七部位；health/mana 只存 current
//! ItemSave   { id, count, overrides: ItemOverrides, slots: Option<[Option<ItemSave>]> }
//! ItemOverrides { weapon: Option<WeaponSave> }
//! WeaponSave { level, exp }                     // 只存玩家行为造成的差异
//! ```
//!
//! 版本放在**文件头**而不是 bincode 载荷里：bincode 1.3 不是自描述格式，
//! 结构一变旧档解不出来，如果版本在载荷里就永远读不到它，只能报"编码失败"。
//! 头部先读、版本先判，才能给出"版本 1 不认识（当前 5）"这种可读错误。
//!
//! V4 在 V3 的基础上只加了一件事：武器的等级/经验（DsnE24 阶段 ②）——它们由
//! 玩家行为（造成伤害、升级）改变，重开一局无法重算，所以必须进档；而等级带来
//! 的攻击加成、力量上限等派生值永远从蓝图 `WeaponGrowth` 重算，不写盘。
//!
//! V3 只存 `health.current` / `mana.current`，**上限不存**（DsnE23）：
//! 上限是"等级曲线 + 装备修正"，读档后按当前规则重算——否则改公式后旧档会
//! 带着旧上限继续跑，出现"同一件装备两个上限"。
//!
//! `items_schema` 是物品数据的 schema 版本：与编译期内嵌的 `assets/items.json`
//! 不一致时**拒绝读**（MVP 不保证跨内容版本，DsnE22 已拍板），避免把编号按
//! 新含义解释成另一件物品。
//!
//! 读档 = `new_game(map_seed)` → `rebuild_floor`（若楼层 > 1）→ 按蓝图重新装配
//! 物品实体、再套上实例差异 → 插回等级/经验 → 重算上限 → 还原 current。
//! 丢在地上的东西不入档：读档后地面回到 `(seed, floor)` 的初始内容。
//!
//! 两个 V1 起就有的取舍（写进 DESIGN DsnE19）：
//! - **不存 `GameRng`**：读档后随机流从该层开局重来，与"继续玩"会分叉；
//! - **不存回合计数**：`TurnManager` 本来就没有回合数，只有 game_over/wants_quit。

use std::fmt;
use std::path::{Path, PathBuf};

use bevy_ecs::prelude::*;
use bevy_ecs::system::RunSystemOnce;
use ecs_core::components::{Experience, Health, Level, Mana, MagicMastery, Position};
use ecs_core::item::{
    Amount, BODY, Container, ITEMS_SCHEMA_VERSION, Item, ItemId, ItemTag, WeaponProgress,
    spawn_item_assembled,
};
use ecs_core::resources::{EventLog, EventMessage, FloorNumber, MapMemory, MapSeed, VisibleMemory};
use ecs_core::world::query::player_entity;
use serde::{Deserialize, Serialize};

/// 当前存档格式版本。V5 = 物品部件化（DsnE27）：删掉耐久实例差异、物品 schema 升 8；
/// V4 = 武器养成（等级/经验）作为实例差异进档；V3 = 生命/法力只存 current。
pub const SAVE_VERSION: u32 = 5;

/// 存档文件头的魔数：读档第一件事是确认"这确实是我们写的文件"。
pub const FILE_MAGIC: [u8; 4] = *b"DNSV";

/// 文件头长度：magic(4) + 版本 u32(4)。
pub const HEADER_LEN: usize = 8;

/// 默认存档文件名（当前工作目录下）。
pub const SAVE_FILE: &str = "dungeon-save.bin";

/// 默认存档路径。
pub fn default_path() -> PathBuf {
    PathBuf::from(SAVE_FILE)
}

/// 存档根：物品数据版本、种子、楼层、玩家。文件格式版本在文件头里，不在这里。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SaveV5 {
    /// 写档时 `assets/items.json` 的 schema 版本；读档不一致直接拒绝（MVP 不迁移）。
    pub items_schema: u32,
    /// 本局地图种子（决定所有楼层内容，DsnX17）。
    pub map_seed: u64,
    /// 玩家所在楼层（从 1 开始）。
    pub floor: u32,
    pub player: PlayerSave,
}

/// 玩家：位置、成长数值、身体容器。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlayerSave {
    pub x: u16,
    pub y: u16,
    pub level: u64,
    pub experience: ExpSave,
    /// 当前生命值（上限不入档，读档时按等级曲线 + 装备修正重算，DsnE23）。
    pub health: f64,
    /// 当前法力值（同上）。
    pub mana: f64,
    pub magic_mastery: f64,
    /// 身体七部位的槽位（`None` = 空槽）；行囊在「背包」槽里，递归在 `ItemSave.slots`。
    pub body: Vec<Option<ItemSave>>,
}

/// 经验：当前值与下一级门槛。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ExpSave {
    pub exp: f64,
    pub exp_to_next: f64,
}

/// 一件物品（以及它作为容器时的子树）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ItemSave {
    /// 注册表编号（`ItemId.0`）。
    pub id: u16,
    /// 这一堆的数量（不堆叠的物品为 1）。
    pub count: u32,
    /// 与蓝图不同的实例数据（DsnE22 阶段 ④）：只写玩家行为造成的差异。
    pub overrides: ItemOverrides,
    /// 自带的容器槽位（行囊类）；`None` = 它不是容器。
    pub slots: Option<Vec<Option<ItemSave>>>,
}

/// 一件物品的实例差异。空白（`None`）= 与模板装配完全一致。
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct ItemOverrides {
    /// 武器养成进度；`None` = 1 级 0 经验（模板装配值）。
    pub weapon: Option<WeaponSave>,
}

/// 武器的实例养成数据（DsnE24 阶段 ②）：只剩等级与经验，攻击加成等派生值
/// 永远从蓝图 `WeaponGrowth` 重算。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WeaponSave {
    pub level: u32,
    pub exp: f64,
}

/// 存档失败的原因。玩家要能看懂（`describe` 写进 `EventLog`）。
#[derive(Debug)]
pub enum SaveError {
    Io(std::io::Error),
    Encode(bincode::Error),
    UnsupportedVersion(u32),
    /// 存档里的物品数据版本与当前内嵌数据不一致（MVP 不做迁移）。
    ContentVersion { save: u32, current: u32 },
    NoPlayer,
    GameOver,
    Corrupt(String),
}

impl SaveError {
    /// 给玩家看的一句话。
    pub fn describe(&self) -> String {
        match self {
            SaveError::Io(error) => format!("存档文件读写失败：{error}"),
            SaveError::Encode(error) => format!("存档编码失败：{error}"),
            SaveError::UnsupportedVersion(version) => {
                format!("存档版本 {version} 不认识（当前版本 {SAVE_VERSION}）")
            }
            SaveError::ContentVersion { save, current } => format!(
                "存档的物品数据版本 {save} 与当前内嵌数据版本 {current} 不一致，无法读取"
            ),
            SaveError::NoPlayer => "找不到玩家".to_string(),
            SaveError::GameOver => "你已经死了，不能存档".to_string(),
            SaveError::Corrupt(why) => format!("存档损坏：{why}"),
        }
    }
}

impl fmt::Display for SaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.describe())
    }
}

impl std::error::Error for SaveError {}

/// 把当前世界写成存档文件（文件头 + bincode 载荷）。
pub fn write(world: &World, path: &Path) -> Result<(), SaveError> {
    let save = capture(world)?;
    let payload = bincode::serialize(&save).map_err(SaveError::Encode)?;
    let mut bytes = Vec::with_capacity(HEADER_LEN + payload.len());
    bytes.extend_from_slice(&FILE_MAGIC);
    bytes.extend_from_slice(&SAVE_VERSION.to_le_bytes());
    bytes.extend_from_slice(&payload);
    std::fs::write(path, bytes).map_err(SaveError::Io)
}

/// 读存档文件。先验文件头（magic + 版本），通过后才解 bincode 载荷。
pub fn read(path: &Path) -> Result<SaveV5, SaveError> {
    let bytes = std::fs::read(path).map_err(SaveError::Io)?;
    if bytes.len() < HEADER_LEN || bytes[..4] != FILE_MAGIC {
        return Err(SaveError::Corrupt("不是存档文件（文件头不对）".to_string()));
    }
    let version = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    if version != SAVE_VERSION {
        return Err(SaveError::UnsupportedVersion(version));
    }
    let save: SaveV5 = bincode::deserialize(&bytes[HEADER_LEN..]).map_err(SaveError::Encode)?;
    if save.items_schema != ITEMS_SCHEMA_VERSION {
        return Err(SaveError::ContentVersion {
            save: save.items_schema,
            current: ITEMS_SCHEMA_VERSION,
        });
    }
    Ok(save)
}

/// 从世界抽取存档投影（`write` 的前半段；测试可单独用）。
pub fn capture(world: &World) -> Result<SaveV5, SaveError> {
    let player = player_entity(world).ok_or(SaveError::NoPlayer)?;
    let corrupt = |why: &str| SaveError::Corrupt(why.to_string());

    let position = *world.get::<Position>(player).ok_or_else(|| corrupt("玩家没有位置"))?;
    let health = *world.get::<Health>(player).ok_or_else(|| corrupt("玩家没有生命"))?;
    let magic = *world.get::<Mana>(player).ok_or_else(|| corrupt("玩家没有法力"))?;
    let level = world.get::<Level>(player).ok_or_else(|| corrupt("玩家没有等级"))?.0;
    let experience = *world.get::<Experience>(player).ok_or_else(|| corrupt("玩家没有经验"))?;
    let magic_mastery = world
        .get::<MagicMastery>(player)
        .ok_or_else(|| corrupt("玩家没有法术精通"))?
        .0;
    let body = world
        .get::<Container>(player)
        .ok_or_else(|| corrupt("玩家没有身体容器"))?;

    let x = u16::try_from(position.x).map_err(|_| corrupt("玩家 x 坐标超出存档范围"))?;
    let y = u16::try_from(position.y).map_err(|_| corrupt("玩家 y 坐标超出存档范围"))?;

    Ok(SaveV5 {
        items_schema: ITEMS_SCHEMA_VERSION,
        map_seed: world.resource::<MapSeed>().0,
        floor: world.resource::<FloorNumber>().0,
        player: PlayerSave {
            x,
            y,
            level,
            experience: ExpSave {
                exp: experience.exp,
                exp_to_next: experience.exp_to_next,
            },
            health: health.current,
            mana: magic.current,
            magic_mastery,
            body: container_to_save(world, body)?,
        },
    })
}

/// 把容器摊平成"每个槽位一个 `Option`"（含空槽，长度 = 容器容量）。
fn container_to_save(
    world: &World,
    container: &Container,
) -> Result<Vec<Option<ItemSave>>, SaveError> {
    let mut slots = Vec::with_capacity(container.len());
    for index in 0..container.len() {
        slots.push(match container.slot(index) {
            Some(item) => Some(item_to_save(world, item)?),
            None => None,
        });
    }
    Ok(slots)
}

/// 一件物品 → 存档（含它自带的容器子树）。
fn item_to_save(world: &World, entity: Entity) -> Result<ItemSave, SaveError> {
    let item = *world
        .get::<Item>(entity)
        .ok_or_else(|| SaveError::Corrupt("容器里有一个没有 Item 组件的实体".to_string()))?;
    if !item.id.exists() {
        return Err(SaveError::Corrupt(format!(
            "物品编号 {} 不在注册表里",
            item.id.0
        )));
    }
    let count = world.get::<Amount>(entity).map(|amount| amount.0).unwrap_or(1);
    let weapon = world
        .get::<WeaponProgress>(entity)
        .map(|progress| WeaponSave {
            level: progress.level,
            exp: progress.exp,
        });
    if weapon.is_some() && item.def().weapon().is_none() {
        return Err(SaveError::Corrupt(format!(
            "物品 {} 带着武器养成进度，但蓝图里没有武器部件",
            item.id.0
        )));
    }
    let container_def = item.def().container_def();
    let slots = match (world.get::<Container>(entity), container_def) {
        (Some(container), Some(_)) => Some(container_to_save(world, container)?),
        (Some(_), None) => {
            return Err(SaveError::Corrupt(format!(
                "物品 {} 带着容器组件，但注册表里它不是容器",
                item.id.0
            )));
        }
        (None, Some(def)) => Some(vec![None; def.slots.len()]),
        (None, None) => None,
    };
    Ok(ItemSave {
        id: item.id.0,
        count,
        overrides: ItemOverrides { weapon },
        slots,
    })
}

/// 按存档建一个世界：重新生成该层，再按蓝图装配物品、套实例差异、还原玩家数值。
///
/// 版本校验在 [`read`] 里（文件头 + 物品数据 schema）。这里拿到的 `SaveV5`
/// 已经是当前版本的形状。
pub fn restore_to_world(save: &SaveV5) -> Result<World, SaveError> {
    let mut world = ecs_core::new_game(save.map_seed);

    // 楼层：`new_game` 恒第 1 层。高于 1 层时改资源后重建一次——与 `descend`
    // 走同一个 `rebuild_floor`，所以"重建层内容"只有一条实现。
    if save.floor > 1 {
        world.insert_resource(FloorNumber(save.floor));
        ecs_core::rebuild_floor(&mut world);
    }

    let player = player_entity(&world).ok_or(SaveError::NoPlayer)?;

    // 新开局带一个行囊；整棵树丢掉，按存档重建。
    clear_containers(&mut world, player);

    let mut body = Container::new(&BODY);
    for (slot, entry) in save.player.body.iter().enumerate() {
        let Some(entry) = entry else {
            continue;
        };
        if slot >= body.len() {
            return Err(SaveError::Corrupt(format!(
                "存档有第 {slot} 个身体槽位，但身体只有 {} 个",
                body.len()
            )));
        }
        let entity = spawn_from_save(&mut world, entry)?;
        let tags = ItemId(entry.id).def().tags;
        body.insert(slot, entity, tags).map_err(|error| {
            SaveError::Corrupt(format!("存档第 {slot} 个身体槽位放不进去：{error:?}"))
        })?;
    }
    world.entity_mut(player).insert(body);

    world.entity_mut(player).insert((
        Position::new(usize::from(save.player.x), usize::from(save.player.y)),
        Level(save.player.level),
        Experience::new(
            save.player.experience.exp,
            save.player.experience.exp_to_next,
        ),
        // 上限占位：紧接着按当前规则重算（DsnE23 的唯一上限公式）。
        // 装备已经插回身体（上面 `body`），所以装备修正能算进去。
        Health::new(1.0),
        Mana::new(1.0),
        MagicMastery(save.player.magic_mastery),
    ));

    ecs_core::refresh_player_vitals(&mut world);
    {
        let mut health = world
            .get_mut::<Health>(player)
            .ok_or_else(|| SaveError::Corrupt("读档后玩家没有生命".to_string()))?;
        health.current = save.player.health.clamp(0.0, health.max);
    }
    {
        let mut mana = world
            .get_mut::<Mana>(player)
            .ok_or_else(|| SaveError::Corrupt("读档后玩家没有法力".to_string()))?;
        mana.current = save.player.mana.clamp(0.0, mana.max);
    }

    // 位置变了：记忆与派生数据按"存档里的位置"重算，别把出生点周围当成探索过。
    world.insert_resource(MapMemory::new());
    world.insert_resource(VisibleMemory::default());
    refresh_derived(&mut world);

    Ok(world)
}

/// 把一件存档物品造成实体（含子容器）：先按蓝图**装配**（与系统里的 `spawn_item`
/// 同一条实现），再套上存档记录的实例差异。
fn spawn_from_save(world: &mut World, data: &ItemSave) -> Result<Entity, SaveError> {
    let id = ItemId(data.id);
    if !id.exists() {
        return Err(SaveError::Corrupt(format!(
            "存档里有未知物品编号 {}",
            data.id
        )));
    }
    let def = id.def();
    let count = data.count.max(1);
    let entity = spawn_item_assembled(world, id, count);

    if let Some(weapon) = data.overrides.weapon {
        let growth = def
            .weapon()
            .map(|spec| spec.growth)
            .ok_or_else(|| {
                SaveError::Corrupt(format!(
                    "存档里物品 {} 有武器养成进度，但蓝图没有武器部件",
                    data.id
                ))
            })?;
        let valid_level = (1..=growth.max_level).contains(&weapon.level);
        if !valid_level || !weapon.exp.is_finite() || weapon.exp < 0.0 {
            return Err(SaveError::Corrupt(format!(
                "存档里物品 {} 的养成进度不合法（等级 {}、经验 {}）",
                data.id, weapon.level, weapon.exp
            )));
        }
        world.entity_mut(entity).insert(WeaponProgress {
            level: weapon.level,
            exp: weapon.exp,
        });
    }

    let Some(slots) = &data.slots else {
        if def.container_def().is_some() {
            return Err(SaveError::Corrupt(format!(
                "存档里容器物品 {} 没有子槽位",
                data.id
            )));
        }
        return Ok(entity);
    };

    let container_def = def.container_def().ok_or_else(|| {
        SaveError::Corrupt(format!(
            "存档里物品 {} 有子槽位，但注册表里它不是容器",
            data.id
        ))
    })?;
    let mut container = Container::new(container_def);
    for (index, child) in slots.iter().enumerate() {
        let Some(child) = child else {
            continue;
        };
        if index >= container.len() {
            return Err(SaveError::Corrupt(format!(
                "存档里 {} 有第 {index} 个子槽位，但它只有 {} 格",
                def.name,
                container.len()
            )));
        }
        let child_entity = spawn_from_save(world, child)?;
        let tags: &'static [ItemTag] = ItemId(child.id).def().tags;
        container.insert(index, child_entity, tags).map_err(|error| {
            SaveError::Corrupt(format!(
                "存档里 {} 的第 {index} 个子槽位放不进去：{error:?}",
                def.name
            ))
        })?;
    }
    world.entity_mut(entity).insert(container);
    Ok(entity)
}

/// 摘掉玩家当前身体容器：连同行囊里的东西整棵树销毁（开局世界只有一个行囊）。
fn clear_containers(world: &mut World, player: Entity) {
    let tops: Vec<Entity> = world
        .get::<Container>(player)
        .map(|body| body.iter().map(|(_, item)| item).collect())
        .unwrap_or_default();
    for top in tops {
        despawn_tree(world, top);
    }
    world.entity_mut(player).remove::<Container>();
}

/// 递归销毁一件物品（先销毁它容器里的东西）。
fn despawn_tree(world: &mut World, entity: Entity) {
    let children: Vec<Entity> = world
        .get::<Container>(entity)
        .map(|container| container.iter().map(|(_, item)| item).collect())
        .unwrap_or_default();
    for child in children {
        despawn_tree(world, child);
    }
    if world.get_entity(entity).is_ok() {
        world.despawn(entity);
    }
}

/// 按玩家当前位置重建视野 / 记忆 / 占用图 / 格索引（读档的最后一步）。
fn refresh_derived(world: &mut World) {
    let _ = world.run_system_once(ecs_core::fov_system);
    let _ = world.run_system_once(ecs_core::update_map_memory_system);
    let _ = world.run_system_once(ecs_core::update_visible_memory_system);
    let _ = world.run_system_once(ecs_core::rebuild_occupancy_system);
    let _ = world.run_system_once(ecs_core::effect::rebuild_cell_effects_system);
}

/// 把一条系统消息写进世界日志（应用层存档/读档成功失败都用它）。
pub fn push_log(world: &mut World, text: impl Into<String>) {
    world
        .resource_mut::<EventLog>()
        .push(EventMessage::system(text));
}
