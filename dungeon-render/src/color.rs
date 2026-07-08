use ratatui::style::Color as RataColor;

/// 将 dungeon_core 的 (u8, u8, u8) 颜色转为 ratatui::Color。
pub fn to_ratatui(r: u8, g: u8, b: u8) -> RataColor {
    RataColor::Rgb(r, g, b)
}

/// 解析 dungeon_core 的 Renderable 颜色为 ratatui::Color。
pub fn renderable_color(glyph_color: (u8, u8, u8)) -> RataColor {
    RataColor::Rgb(glyph_color.0, glyph_color.1, glyph_color.2)
}

/// 基于实体 ID 对基础颜色做微调，使同类怪物有细微差异但保留色系辨识度。
/// 以基础色为中心，在 ±40 范围内均匀采样 —— 即使通道值为 0 或 255 也能产生视觉差异。
pub fn unique_color(base: (u8, u8, u8), id_bits: u64) -> (u8, u8, u8) {
    let hash = id_bits.wrapping_mul(0x9E3779B97F4A7C15);
    let sample = |v: u8, h: u64| -> u8 {
        let spread = 80u16;
        let lo = (v as u16).saturating_sub(spread / 2);
        let hi = (v as u16 + spread / 2).min(255);
        let range = hi - lo + 1;
        let offset = (h & 0xFF) as u16 % range;
        (lo + offset) as u8
    };
    (sample(base.0, hash), sample(base.1, hash >> 16), sample(base.2, hash >> 32))
}
