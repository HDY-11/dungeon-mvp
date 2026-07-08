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
/// 使用黄金比例哈希将相邻 ID 扩散到全 64 位空间，每通道偏移 -64..63。
pub fn unique_color(base: (u8, u8, u8), id_bits: u64) -> (u8, u8, u8) {
    let hash = id_bits.wrapping_mul(0x9E3779B97F4A7C15);
    let dr = ((hash >> 40) & 0x7F) as i16 - 64;
    let dg = ((hash >> 20) & 0x7F) as i16 - 64;
    let db = (hash & 0x7F) as i16 - 64;
    (
        (base.0 as i16 + dr).clamp(0, 255) as u8,
        (base.1 as i16 + dg).clamp(0, 255) as u8,
        (base.2 as i16 + db).clamp(0, 255) as u8,
    )
}
