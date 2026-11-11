//! 层—区块地图的数据骨架（DsnE30）。
//!
//! 这里只定义「区块坐标 + 区块格 + 当前区块 + 访问集」，不负责地图实例化与切图；
//! 那些属于后续 S1/S2 切片（`Map` 动态尺寸、区块生成、连接口切换）。
//!
//! 设计要点：
//! - 层内区块是规则的二维格（`width × height`），每个格要么是空（实心岩壁），
//!   要么是一个 `BlockInfo`；
//! - `current` 是玩家所在区块，`visited` 是已访问集合；二者进存档（V6）；
//! - 区块内实体不在这里，离开即由区块初始化重建（DsnE30 决策 2/5）。

use std::collections::BTreeSet;

use crate::map::MapKind;

/// 层内区块坐标（`x` 向右、`y` 向下）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BlockCoord {
    pub x: i32,
    pub y: i32,
}

impl BlockCoord {
    pub const ORIGIN: Self = Self { x: 0, y: 0 };

    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }

    pub const fn offset(self, dx: i32, dy: i32) -> Self {
        Self {
            x: self.x + dx,
            y: self.y + dy,
        }
    }

    /// 偏移后仍在 `width × height` 的区块网内时返回坐标。
    pub fn offset_in(self, dx: i32, dy: i32, width: u32, height: u32) -> Option<Self> {
        let next = self.offset(dx, dy);
        if next.x < 0
            || next.y < 0
            || next.x as u32 >= width
            || next.y as u32 >= height
        {
            None
        } else {
            Some(next)
        }
    }
}

/// 一个区块的描述。目前只需要地图类型；群系/结构/连接口在 S2 扩。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockInfo {
    pub kind: MapKind,
}

impl BlockInfo {
    pub const fn new(kind: MapKind) -> Self {
        Self { kind }
    }
}

/// 一层的区块网与玩家位置。
#[derive(Debug, Clone)]
pub struct Layer {
    floor: u32,
    width: u32,
    height: u32,
    blocks: Vec<Option<BlockInfo>>,
    current: BlockCoord,
    entry: BlockCoord,
    visited: BTreeSet<BlockCoord>,
}

impl Layer {
    /// 空层：所有区块都未放置。`width`/`height` 为 0 时按 1 处理。
    pub fn new(floor: u32, width: u32, height: u32) -> Self {
        let width = width.max(1);
        let height = height.max(1);
        Self {
            floor,
            width,
            height,
            blocks: vec![None; (width * height) as usize],
            current: BlockCoord::ORIGIN,
            entry: BlockCoord::ORIGIN,
            visited: BTreeSet::new(),
        }
    }

    /// 单块层：只有 0 号区块，等于当前「一层=一张图」的形态。
    pub fn single(floor: u32, kind: MapKind) -> Self {
        let mut layer = Self::new(floor, 1, 1);
        layer.set_block(BlockCoord::ORIGIN, Some(BlockInfo::new(kind)));
        layer.enter(BlockCoord::ORIGIN);
        layer
    }

    pub const fn floor(&self) -> u32 {
        self.floor
    }

    pub const fn width(&self) -> u32 {
        self.width
    }

    pub const fn height(&self) -> u32 {
        self.height
    }

    pub const fn current(&self) -> BlockCoord {
        self.current
    }

    pub const fn entry(&self) -> BlockCoord {
        self.entry
    }

    pub fn visited(&self) -> &BTreeSet<BlockCoord> {
        &self.visited
    }

    pub fn has_visited(&self, coord: BlockCoord) -> bool {
        self.visited.contains(&coord)
    }

    fn index(&self, coord: BlockCoord) -> Option<usize> {
        if coord.x < 0
            || coord.y < 0
            || coord.x as u32 >= self.width
            || coord.y as u32 >= self.height
        {
            return None;
        }
        Some((coord.y as u32 * self.width + coord.x as u32) as usize)
    }

    pub fn block(&self, coord: BlockCoord) -> Option<&BlockInfo> {
        self.index(coord).and_then(|i| self.blocks[i].as_ref())
    }

    pub fn set_block(&mut self, coord: BlockCoord, info: Option<BlockInfo>) -> Option<BlockInfo> {
        let index = self.index(coord)?;
        let old = self.blocks[index];
        self.blocks[index] = info;
        if info.is_none() {
            self.visited.remove(&coord);
        }
        old
    }

    /// 进入一个已存在的区块；成功时更新 `current` 与 `visited`。
    pub fn enter(&mut self, coord: BlockCoord) -> bool {
        if self.block(coord).is_none() {
            return false;
        }
        self.current = coord;
        self.visited.insert(coord);
        true
    }

    /// 与目标区块相邻、且已放置的区块（四方向，不含对角线）。
    pub fn neighbors(&self, coord: BlockCoord) -> Vec<BlockCoord> {
        const DIRS: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];
        DIRS.iter()
            .filter_map(|(dx, dy)| coord.offset_in(*dx, *dy, self.width, self.height))
            .filter(|next| self.block(*next).is_some())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_layer_has_one_origin_block() {
        let layer = Layer::single(1, MapKind::Cavern);
        assert_eq!(layer.width(), 1);
        assert_eq!(layer.height(), 1);
        assert_eq!(layer.current(), BlockCoord::ORIGIN);
        assert!(layer.block(BlockCoord::ORIGIN).is_some());
        assert!(layer.has_visited(BlockCoord::ORIGIN));
        assert!(layer.neighbors(BlockCoord::ORIGIN).is_empty());
    }

    #[test]
    fn empty_layer_has_no_blocks_and_cannot_be_entered() {
        let mut layer = Layer::new(1, 4, 3);
        assert_eq!(layer.width(), 4);
        assert_eq!(layer.height(), 3);
        assert!(layer.block(BlockCoord::new(1, 1)).is_none());
        assert!(!layer.enter(BlockCoord::new(1, 1)));
        assert!(!layer.has_visited(BlockCoord::new(1, 1)));
    }

    #[test]
    fn block_get_set_and_out_of_bounds() {
        let mut layer = Layer::new(1, 2, 2);
        let a = BlockCoord::new(1, 1);
        assert_eq!(layer.set_block(a, Some(BlockInfo::new(MapKind::Undersea))), None);
        assert_eq!(
            layer.block(a).map(|b| b.kind),
            Some(MapKind::Undersea)
        );
        assert_eq!(
            layer.set_block(a, Some(BlockInfo::new(MapKind::Cavern))),
            Some(BlockInfo::new(MapKind::Undersea))
        );
        // 越界坐标无法写入，返回 None。
        assert_eq!(
            layer.set_block(BlockCoord::new(2, 0), Some(BlockInfo::new(MapKind::Cavern))),
            None
        );
        assert_eq!(layer.block(BlockCoord::new(2, 0)), None);
    }

    #[test]
    fn entering_marks_visited_and_moves_current() {
        let mut layer = Layer::new(1, 2, 1);
        layer.set_block(BlockCoord::ORIGIN, Some(BlockInfo::new(MapKind::Cavern)));
        layer.set_block(BlockCoord::new(1, 0), Some(BlockInfo::new(MapKind::LushCavern)));
        assert!(layer.enter(BlockCoord::new(1, 0)));
        assert_eq!(layer.current(), BlockCoord::new(1, 0));
        assert!(layer.has_visited(BlockCoord::new(1, 0)));
        // 点回原块，访问集保持两格。
        assert!(layer.enter(BlockCoord::ORIGIN));
        assert_eq!(layer.visited().len(), 2);
    }

    #[test]
    fn neighbors_only_lists_existing_orthogonal_blocks() {
        let mut layer = Layer::new(1, 3, 3);
        layer.set_block(BlockCoord::new(1, 1), Some(BlockInfo::new(MapKind::Cavern)));
        layer.set_block(BlockCoord::new(2, 1), Some(BlockInfo::new(MapKind::Undersea)));
        layer.set_block(BlockCoord::new(1, 0), Some(BlockInfo::new(MapKind::Cavern)));
        // 对角块不算相邻。
        layer.set_block(BlockCoord::new(2, 2), Some(BlockInfo::new(MapKind::Cavern)));
        let neighbors = layer.neighbors(BlockCoord::new(1, 1));
        assert_eq!(neighbors.len(), 2);
        assert!(neighbors.contains(&BlockCoord::new(2, 1)));
        assert!(neighbors.contains(&BlockCoord::new(1, 0)));
        assert!(!neighbors.contains(&BlockCoord::new(2, 2)));
    }

    #[test]
    fn offset_in_respects_layer_bounds() {
        let c = BlockCoord::new(0, 0).offset(1, 2);
        assert_eq!(c, BlockCoord::new(1, 2));
        assert_eq!(BlockCoord::ORIGIN.offset_in(1, 0, 2, 2), Some(BlockCoord::new(1, 0)));
        assert_eq!(BlockCoord::ORIGIN.offset_in(-1, 0, 2, 2), None);
        assert_eq!(BlockCoord::ORIGIN.offset_in(2, 0, 2, 2), None);
    }
}
