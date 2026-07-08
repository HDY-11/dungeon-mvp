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
/// 每通道偏移量 -32..+31，由 id_bits 的不同字节段决定。
pub fn unique_color(base: (u8, u8, u8), id_bits: u64) -> (u8, u8, u8) {
    let shift = |v: u8, x: u64| -> u8 {
        let s = ((x & 0x3F) as i16) - 32;
        (v as i16 + s).clamp(0, 255) as u8
    };
    (shift(base.0, id_bits), shift(base.1, id_bits >> 8), shift(base.2, id_bits >> 16))
}
