//! 颜色工具函数（纯数学，无 TUI 依赖）
//!
//! entity_color 为每个实体生成独特色彩，用于怪物个体区分。
//! 纯 (u64,u64)→(u8,u8,u8) 哈希+HSV 变换，无渲染层依赖。

/// HSV → RGB 转换（s, v ∈ [0,1]）
fn hsv_to_rgb(h: f64, s: f64, v: f64) -> (u8, u8, u8) {
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match (h as u32 / 60) % 6 {
        0 => (c, x, 0.0), 1 => (x, c, 0.0), 2 => (0.0, c, x),
        3 => (0.0, x, c), 4 => (x, 0.0, c), _ => (c, 0.0, x),
    };
    (
        ((r + m) * 255.0).round() as u8,
        ((g + m) * 255.0).round() as u8,
        ((b + m) * 255.0).round() as u8,
    )
}

/// 基于实体 ID 生成亮色，确保高饱和度 + 高亮度，避免灰色/黑色/棕色。
/// HSV 空间：H 自由（0-360°），S/V 固定在 [0.7, 1.0]，保证鲜艳。
pub fn entity_color(id_bits: u64, seed: u64) -> (u8, u8, u8) {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    id_bits.hash(&mut hasher);
    seed.hash(&mut hasher);
    let hash = hasher.finish();
    let h = ((hash >> 40) as f64) / 255.0 * 360.0;          // 0-360°
    let s = 0.7 + ((hash >> 20) as u8 as f64) / 255.0 * 0.3; // 0.7-1.0
    let v = 0.7 + (hash as u8 as f64) / 255.0 * 0.3;         // 0.7-1.0
    hsv_to_rgb(h, s, v)
}
