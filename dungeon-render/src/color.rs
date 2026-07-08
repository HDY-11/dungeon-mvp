use ratatui::style::Color as RataColor;

/// 将 dungeon_core 的 (u8, u8, u8) 颜色转为 ratatui::Color。
pub fn to_ratatui(r: u8, g: u8, b: u8) -> RataColor {
    RataColor::Rgb(r, g, b)
}

/// 解析 dungeon_core 的 Renderable 颜色为 ratatui::Color。
pub fn renderable_color(glyph_color: (u8, u8, u8)) -> RataColor {
    RataColor::Rgb(glyph_color.0, glyph_color.1, glyph_color.2)
}

/// 基于实体 ID 生成颜色，无基准色，直接用 SipHash 扩散后映射 RGB。
/// 微小 ID 变化产生大幅颜色跳跃，低碰撞率。
pub fn entity_color(id_bits: u64, seed: u64) -> (u8, u8, u8) {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    id_bits.hash(&mut hasher);
    seed.hash(&mut hasher);
    let hash = hasher.finish();
    ((hash >> 40) as u8, (hash >> 20) as u8, hash as u8)
}
