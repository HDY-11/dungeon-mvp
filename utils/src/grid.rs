//! 通用二维网格。

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grid2<T> {
    width: usize,
    height: usize,
    cells: Vec<T>,
}

impl<T: Clone> Grid2<T> {
    pub fn new(width: usize, height: usize, fill: T) -> Self {
        Self {
            width,
            height,
            cells: vec![fill; width * height],
        }
    }

    pub fn from_cells(width: usize, height: usize, cells: Vec<T>) -> Self {
        assert_eq!(cells.len(), width * height);
        Self {
            width,
            height,
            cells,
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

    fn index(&self, x: usize, y: usize) -> usize {
        debug_assert!(self.in_bounds(x, y));
        y * self.width + x
    }

    pub fn get(&self, x: usize, y: usize) -> Option<&T> {
        if !self.in_bounds(x, y) {
            return None;
        }
        Some(&self.cells[self.index(x, y)])
    }

    pub fn get_mut(&mut self, x: usize, y: usize) -> Option<&mut T> {
        if !self.in_bounds(x, y) {
            return None;
        }
        let idx = self.index(x, y);
        Some(&mut self.cells[idx])
    }

    pub fn set(&mut self, x: usize, y: usize, value: T) -> bool {
        if !self.in_bounds(x, y) {
            return false;
        }
        let idx = self.index(x, y);
        self.cells[idx] = value;
        true
    }

    pub fn fill(&mut self, value: T) {
        self.cells.fill(value);
    }

    pub fn cells(&self) -> &[T] {
        &self.cells
    }

    pub fn iter(&self) -> impl Iterator<Item = (usize, usize, &T)> {
        self.cells.iter().enumerate().map(|(idx, cell)| {
            let x = idx % self.width;
            let y = idx / self.width;
            (x, y, cell)
        })
    }
}

impl<T> std::ops::Index<(usize, usize)> for Grid2<T> {
    type Output = T;

    fn index(&self, (x, y): (usize, usize)) -> &Self::Output {
        assert!(x < self.width && y < self.height, "grid index out of bounds");
        &self.cells[y * self.width + x]
    }
}

impl<T> std::ops::IndexMut<(usize, usize)> for Grid2<T> {
    fn index_mut(&mut self, (x, y): (usize, usize)) -> &mut Self::Output {
        assert!(x < self.width && y < self.height, "grid index out of bounds");
        &mut self.cells[y * self.width + x]
    }
}
