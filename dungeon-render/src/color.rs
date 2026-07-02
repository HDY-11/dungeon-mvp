use ratatui::style::Color as RataColor;

/// 将 dungeon_core 的 (u8, u8, u8) 颜色转为 ratatui::Color。
pub fn to_ratatui(r: u8, g: u8, b: u8) -> RataColor {
    RataColor::Rgb(r, g, b)
}

/// 解析 dungeon_core 的 Renderable 颜色为 ratatui::Color。
pub fn renderable_color(glyph_color: (u8, u8, u8)) -> RataColor {
    RataColor::Rgb(glyph_color.0, glyph_color.1, glyph_color.2)
}
