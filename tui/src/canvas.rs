//! 持久 Canvas：为后续 dirty tracking / 半块字符叠加做准备。
//!
//! 当前先提供数据结构，不接渲染管线。

use std::collections::HashSet;
use utils::Rgb;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CanvasCell {
    pub glyph: char,
    pub fg: Rgb,
    pub bg: Option<Rgb>,
}

impl Default for CanvasCell {
    fn default() -> Self {
        Self {
            glyph: ' ',
            fg: Rgb::new(255, 255, 255),
            bg: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Canvas {
    width: usize,
    height: usize,
    cells: Vec<CanvasCell>,
    dirty: HashSet<(usize, usize)>,
}

impl Canvas {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            cells: vec![CanvasCell::default(); width * height],
            dirty: (0..height)
                .flat_map(|y| (0..width).map(move |x| (x, y)))
                .collect(),
        }
    }

    pub const fn width(&self) -> usize {
        self.width
    }

    pub const fn height(&self) -> usize {
        self.height
    }

    pub fn in_bounds(&self, x: usize, y: usize) -> bool {
        x < self.width && y < self.height
    }

    pub fn get(&self, x: usize, y: usize) -> Option<&CanvasCell> {
        self.in_bounds(x, y).then(|| &self.cells[y * self.width + x])
    }

    pub fn set(&mut self, x: usize, y: usize, cell: CanvasCell) -> bool {
        if !self.in_bounds(x, y) {
            return false;
        }
        let idx = y * self.width + x;
        if self.cells[idx] != cell {
            self.cells[idx] = cell;
            self.dirty.insert((x, y));
        }
        true
    }

    pub fn clear(&mut self, glyph: char, fg: Rgb, bg: Option<Rgb>) {
        let cell = CanvasCell { glyph, fg, bg };
        self.cells.fill(cell);
        self.dirty.clear();
    }

    pub fn dirty_cells(&self) -> impl Iterator<Item = (usize, usize, CanvasCell)> + '_ {
        self.dirty.iter().filter_map(|&(x, y)| {
            self.get(x, y).map(|cell| (x, y, *cell))
        })
    }

    pub fn clear_dirty(&mut self) {
        self.dirty.clear();
    }
}
