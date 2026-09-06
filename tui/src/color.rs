//! 颜色转换。

use ratatui::style::Color as RataColor;
use utils::Rgb;

pub fn to_ratatui(rgb: Rgb) -> RataColor {
    RataColor::Rgb(rgb.0, rgb.1, rgb.2)
}

pub fn tuple_to_ratatui((r, g, b): (u8, u8, u8)) -> RataColor {
    RataColor::Rgb(r, g, b)
}

pub fn rgb_to_ratatui(r: u8, g: u8, b: u8) -> RataColor {
    RataColor::Rgb(r, g, b)
}
