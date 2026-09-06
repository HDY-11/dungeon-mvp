//! 纯颜色数学：RGB 类型、HSV 生成、亮度调整。

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self(r, g, b)
    }

    pub const fn as_tuple(self) -> (u8, u8, u8) {
        (self.0, self.1, self.2)
    }

    /// 按 `factor` 向灰色（96, 96, 96）靠近。`factor=0` 不变，`factor=1` 全灰。
    pub fn dim(self, factor: f64) -> Self {
        let dim_channel = |c: u8| -> u8 {
            let v = c as f64 * (1.0 - factor) + 96.0 * factor;
            v.round().clamp(0.0, 255.0) as u8
        };
        Self(dim_channel(self.0), dim_channel(self.1), dim_channel(self.2))
    }

    /// 在 `self` 与 `other` 之间线性插值。
    pub fn lerp(self, other: Self, t: f64) -> Self {
        let t = t.clamp(0.0, 1.0);
        let mix = |a: u8, b: u8| -> u8 {
            (a as f64 + (b as f64 - a as f64) * t).round().clamp(0.0, 255.0) as u8
        };
        Self(mix(self.0, other.0), mix(self.1, other.1), mix(self.2, other.2))
    }
}

fn hsv_to_rgb(h: f64, s: f64, v: f64) -> (u8, u8, u8) {
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match (h as u32 / 60) % 6 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    (
        ((r + m) * 255.0).round() as u8,
        ((g + m) * 255.0).round() as u8,
        ((b + m) * 255.0).round() as u8,
    )
}

/// 基于实体 ID 生成鲜艳且稳定的颜色。
pub fn entity_color(id_bits: u64, seed: u64) -> Rgb {
    use std::hash::{Hash, Hasher};

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    id_bits.hash(&mut hasher);
    seed.hash(&mut hasher);
    let hash = hasher.finish();

    let h = ((hash >> 40) as f64) / 255.0 * 360.0;
    let s = 0.7 + ((hash >> 20) as u8 as f64) / 255.0 * 0.3;
    let v = 0.7 + (hash as u8 as f64) / 255.0 * 0.3;
    let (r, g, b) = hsv_to_rgb(h, s, v);
    Rgb(r, g, b)
}
