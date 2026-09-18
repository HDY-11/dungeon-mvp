//! 从新 core 提取一帧渲染快照。

use bevy_ecs::prelude::*;
use ecs_core::{
    EventLog, FloorNumber, Health, Level, Map, MapKind, MapMemory, MonsterKindId, Player,
    Position, Stairs, Tile, TurnManager, Viewshed, monster_template,
};
use std::collections::HashSet;
use utils::Rgb;

pub const VIEW_WIDTH: usize = 40;
pub const VIEW_HEIGHT: usize = 20;

#[derive(Debug, Clone)]
pub struct EntityView {
    pub x: usize,
    pub y: usize,
    pub glyph: char,
    pub color: Rgb,
    pub name: String,
    pub hp: f64,
    pub max_hp: f64,
}

#[derive(Debug, Clone)]
pub struct Scene {
    pub floor: u32,
    pub game_over: bool,
    pub player: Option<EntityView>,
    pub visible: HashSet<(usize, usize)>,
    pub tiles: [[Tile; ecs_core::MAP_WIDTH]; ecs_core::MAP_HEIGHT],
    pub explored: [[bool; ecs_core::MAP_WIDTH]; ecs_core::MAP_HEIGHT],
    pub entities: Vec<EntityView>,
    pub log: Vec<String>,
    pub map_kind: MapKind,
}

pub fn extract_scene(world: &mut World) -> Scene {
    let floor = world.resource::<FloorNumber>().0;
    let game_over = world.resource::<TurnManager>().game_over;
    let map = world.resource::<Map>();
    let explored = world.resource::<MapMemory>().explored;
    let tiles = map.tiles;

    let map_kind = {
        let seed = world.resource::<ecs_core::MapSeed>().0;
        ecs_core::map_kind_for(seed, floor)
    };

    let visible: HashSet<(usize, usize)> = {
        let mut q = world.query::<(&Player, &Viewshed)>();
        q.iter(world)
            .next()
            .map(|(_, v)| v.visible_tiles.iter().copied().collect())
            .unwrap_or_default()
    };

    let player = {
        let mut q = world.query::<(&Player, &Position, &Health, &Level, &ecs_core::Experience)>();
        q.iter(world).next().map(|(_, p, hp, level, _exp)| EntityView {
            x: p.x,
            y: p.y,
            glyph: '@',
            color: Rgb::new(255, 255, 0),
            name: format!("冒险者 Lv.{}", level.0),
            hp: hp.current,
            max_hp: hp.max,
        })
    };

    let mut entities = Vec::new();
    {
        let mut q = world.query::<(
            Entity,
            &Position,
            Option<&Player>,
            Option<&Stairs>,
            Option<&MonsterKindId>,
            Option<&Health>,
        )>();
        for (_, pos, is_player, stairs, kind, health) in q.iter(world) {
            if is_player.is_some() {
                continue;
            }
            let (glyph, color, name) = if stairs.is_some() {
                ('>', Rgb::new(0, 255, 0), "楼梯".to_string())
            } else if let Some(kind) = kind {
                let template = monster_template(*kind);
                (
                    template.glyph,
                    Rgb::new(template.color.0, template.color.1, template.color.2),
                    template.name.to_string(),
                )
            } else {
                ('?', Rgb::new(255, 255, 255), "未知".to_string())
            };
            let (hp, max_hp) = health.map(|h| (h.current, h.max)).unwrap_or((0.0, 0.0));
            entities.push(EntityView {
                x: pos.x,
                y: pos.y,
                glyph,
                color,
                name,
                hp,
                max_hp,
            });
        }
    }

    let log = world
        .resource::<EventLog>()
        .messages
        .iter()
        .rev()
        .take(12)
        .map(|m| m.text.clone())
        .collect();

    Scene {
        floor,
        game_over,
        player,
        visible,
        tiles,
        explored,
        entities,
        log,
        map_kind,
    }
}

pub fn tile_glyph(tile: Tile) -> char {
    tile.glyph()
}

pub fn tile_color(tile: Tile) -> Rgb {
    match tile {
        Tile::Wall | Tile::Stalactite => Rgb::new(180, 180, 180),
        Tile::Floor => Rgb::new(200, 200, 200),
        Tile::ShallowWater => Rgb::new(220, 240, 255),
        Tile::DeepWater => Rgb::new(80, 150, 220),
        Tile::Mycelium => Rgb::new(140, 190, 120),
        Tile::FungalPatch => Rgb::new(90, 220, 110),
        Tile::HangingVine => Rgb::new(40, 130, 70),
        Tile::Sand => Rgb::new(230, 215, 160),
        Tile::Seagrass => Rgb::new(70, 170, 110),
        Tile::CoralReef => Rgb::new(240, 150, 90),
    }
}

pub fn tile_bg(tile: Tile) -> Option<Rgb> {
    match tile {
        Tile::Wall => Some(Rgb::new(50, 50, 60)),
        Tile::Floor => Some(Rgb::new(20, 22, 25)),
        Tile::ShallowWater => Some(Rgb::new(120, 190, 250)),
        Tile::DeepWater => Some(Rgb::new(20, 60, 140)),
        Tile::Stalactite => Some(Rgb::new(60, 55, 20)),
        Tile::Mycelium => Some(Rgb::new(25, 45, 25)),
        Tile::FungalPatch => Some(Rgb::new(20, 55, 25)),
        Tile::HangingVine => Some(Rgb::new(15, 40, 25)),
        Tile::Sand => Some(Rgb::new(60, 55, 35)),
        Tile::Seagrass => Some(Rgb::new(25, 55, 35)),
        Tile::CoralReef => Some(Rgb::new(70, 35, 25)),
    }
}
