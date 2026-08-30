//! 渲染管线：从 ECS World 提取渲染快照，并转换为 ratatui 可绘制的网格/UI 数据。

use crate::color::renderable_color;
use bevy_ecs::prelude::World;
use dungeon_core::OptionLogExt;
use dungeon_core::{
    EntityRenderable, FloorNumber, LookCursor, MAP_HEIGHT, MAP_WIDTH, Map, MapMemory, Player,
    Position, RenderableView, ThrowPreview, Tile, TurnManager, VIEWPORT_HEIGHT, VIEWPORT_WIDTH,
    Viewshed, VisibleMemory, collect_renderables,
};
use ratatui::style::Color;
use std::collections::HashSet;

/// 地图格栅单元（glyph, 前景色, 背景色）
pub type GridCell = (char, Color, Color);

/// 一帧渲染的快照数据（管道过滤器间传递）
pub struct RenderScene {
    pub game_over: bool,
    pub player_visible: HashSet<(usize, usize)>,
    pub tiles: [[Tile; MAP_WIDTH]; MAP_HEIGHT],
    pub explored: [[bool; MAP_WIDTH]; MAP_HEIGHT],
    pub px: usize,
    pub py: usize,
    pub visible_mem: Vec<RenderableView>,
    pub renderables: Vec<EntityRenderable>,
    // I76: 状态面板快照（渲染不再直接查询 ECS 的玩家核心数据）
    pub stats: Option<dungeon_core::Stats>,
    pub equip: Option<dungeon_core::Equipment>,
    pub buffs: Vec<dungeon_core::Buff>,
    pub skills: Option<dungeon_core::Skills>,
    pub floor: u32,
}

/// 管道 0：从 ECS World 提取帧数据
pub fn extract_scene(world: &World) -> RenderScene {
    let game_over = world.resource::<TurnManager>().game_over;
    let player_visible: HashSet<(usize, usize)> = {
        let mut q = world
            .try_query::<(&Player, &Viewshed)>()
            .expect_log("Player+Viewshed registered at init");
        q.iter(world)
            .next()
            .map(|(_, v)| v.visible_tiles.iter().copied().collect())
            .unwrap_or_default()
    };
    let tiles = world.resource::<Map>().tiles;
    let explored = world.resource::<MapMemory>().explored;
    let (px, py) = world
        .try_query::<(&Player, &Position)>()
        .expect_log("Player+Position registered at init")
        .iter(world)
        .next()
        .map(|(_, p)| (p.x, p.y))
        .unwrap_or((0, 0));
    let visible_mem: Vec<_> = world
        .resource::<VisibleMemory>()
        .entries
        .values()
        .copied()
        .collect();
    let renderables = collect_renderables(world);
    // I76: 状态面板快照（玩家核心数据一次收集，渲染层不再逐帧查询）
    let stats = world
        .try_query::<(&Player, &dungeon_core::Stats)>()
        .and_then(|mut q| q.iter(world).next().map(|(_, s)| s.clone()));
    let (equip, buffs, skills) = {
        let mut q = world
            .try_query::<(
                &Player,
                &dungeon_core::Equipment,
                Option<&dungeon_core::ActiveBuffs>,
                &dungeon_core::Skills,
            )>()
            .expect_log("Player+Equipment+ActiveBuffs+Skills registered at init");
        match q.iter(world).next() {
            Some((_, eq, ab, sk)) => (
                Some(eq.clone()),
                ab.map(|b| b.0.clone()).unwrap_or_default(),
                Some(sk.clone()),
            ),
            None => (None, Vec::new(), None),
        }
    };
    let floor = world.resource::<FloorNumber>().0;
    RenderScene {
        game_over,
        player_visible,
        tiles,
        explored,
        px,
        py,
        visible_mem,
        renderables,
        stats,
        equip,
        buffs,
        skills,
        floor,
    }
}

/// 管道 1：建立地图格栅 + 实体叠加
pub fn render_map_grid(scene: &RenderScene, world: &World) -> (Vec<Vec<GridCell>>, usize, usize) {
    let vw = VIEWPORT_WIDTH;
    let vh = VIEWPORT_HEIGHT;
    let cam_x = (scene.px.saturating_sub(vw / 2)).min(MAP_WIDTH.saturating_sub(vw));
    let cam_y = (scene.py.saturating_sub(vh / 2)).min(MAP_HEIGHT.saturating_sub(vh));

    let dim = |c: u8, factor: f32| -> u8 { (c as f32 * (1.0 - factor) + 96.0 * factor) as u8 };
    let dim_tile = |tile: Tile| -> (Color, Color) {
        let (r, g, b) = tile.fg_color();
        let fg = Color::Rgb(dim(r, 0.55), dim(g, 0.55), dim(b, 0.55));
        let bg = tile
            .bg_color()
            .map(|(r, g, b)| Color::Rgb(dim(r, 0.7), dim(g, 0.7), dim(b, 0.7)))
            .unwrap_or(Color::Reset);
        (fg, bg)
    };

    let mut lines: Vec<Vec<GridCell>> = Vec::with_capacity(vh);
    for vy in 0..vh {
        let my = cam_y + vy;
        let mut row = Vec::with_capacity(vw);
        for vx in 0..vw {
            let mx = cam_x + vx;
            let pos = (mx, my);
            let tile = scene.tiles[my][mx];
            if scene.player_visible.contains(&pos) {
                let (r, g, b) = tile.fg_color();
                let bg = tile
                    .bg_color()
                    .map(|(r, g, b)| Color::Rgb(r, g, b))
                    .unwrap_or(Color::Reset);
                row.push((tile.glyph(), Color::Rgb(r, g, b), bg));
            } else if scene.explored[my][mx] {
                let (fg, bg) = dim_tile(tile);
                row.push((tile.glyph(), fg, bg));
            } else {
                row.push((' ', Color::DarkGray, Color::Reset));
            }
        }
        lines.push(row);
    }
    // 实体叠加
    for &(_entity, ex, ey, glyph, (r, g, b)) in &scene.renderables {
        if ey >= cam_y
            && ey < cam_y + vh
            && ex >= cam_x
            && ex < cam_x + vw
            && scene.player_visible.contains(&(ex, ey))
        {
            let (idx, jdx) = (ey - cam_y, ex - cam_x);
            let bg = lines[idx][jdx].2;
            lines[idx][jdx] = (glyph, renderable_color((r, g, b)), bg);
        }
    }
    // 视野记忆（灰色）
    for &(mx, my, glyph, _) in &scene.visible_mem {
        if !scene.player_visible.contains(&(mx, my))
            && scene.explored[my][mx]
            && my >= cam_y
            && my < cam_y + vh
            && mx >= cam_x
            && mx < cam_x + vw
        {
            let (idx, jdx) = (my - cam_y, mx - cam_x);
            lines[idx][jdx] = (
                glyph,
                Color::Rgb(dim(160, 0.5), dim(160, 0.5), dim(160, 0.5)),
                lines[idx][jdx].2,
            );
        }
    }
    // 光标高亮
    if let Some(cursor) = world.get_resource::<LookCursor>().filter(|c| c.active)
        && cursor.y >= cam_y
        && cursor.y < cam_y + vh
        && cursor.x >= cam_x
        && cursor.x < cam_x + vw
    {
        let (idx, jdx) = (cursor.y - cam_y, cursor.x - cam_x);
        let (g, fg, _) = lines[idx][jdx];
        lines[idx][jdx] = (g, fg, Color::Rgb(80, 80, 40));
    }
    // 投掷轨迹
    if let Some(tp) = world.get_resource::<ThrowPreview>().filter(|t| t.active) {
        let trajectory_color = if tp.valid_target {
            Color::Rgb(80, 160, 255)
        } else {
            Color::Rgb(220, 60, 60)
        };
        for &(tx, ty) in &tp.path {
            if ty >= cam_y && ty < cam_y + vh && tx >= cam_x && tx < cam_x + vw {
                let (idx, jdx) = (ty - cam_y, tx - cam_x);
                lines[idx][jdx] = ('*', trajectory_color, lines[idx][jdx].2);
            }
        }
    }
    (lines, cam_x, cam_y)
}
