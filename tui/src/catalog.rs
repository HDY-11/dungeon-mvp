//! `TuiCatalog`：把 [`VisualKey`] 映射成终端的 glyph 与颜色。
//!
//! 这是契约层「只说什么、不说长什么样」的另一半：`presentation` 产出语义 key，
//! 这里决定画成什么字符。未来的 `GpuCatalog` 做同样的事，只是映射到纹理/材质。
//!
//! # 为什么用查表函数而不是 `match` 散在各处
//!
//! 每个 key 的外观只有**一处**定义，golden 测试与"新增地形忘了配色"这类问题
//! 都只需要看这一个文件。后端渲染代码只调 [`TuiCatalog::glyph`] / [`TuiCatalog::fg`]。

use render_api::{UiIcon, VisualKey};
use ratatui::style::Color;
use utils::Rgb;

/// 终端外观目录。
///
/// 无状态（全是关联函数），因为外观不随游戏状态变化——需要状态的外观
/// （例如"同种怪物不同个体颜色不同"）应当在 `presentation` 里变成不同的
/// `VisualKey` payload，而不是让后端持有状态。
#[derive(Debug, Default, Clone, Copy)]
pub struct TuiCatalog;

impl TuiCatalog {
    /// key → 前景色。
    pub fn fg(key: VisualKey) -> Color {
        let rgb = Self::fg_rgb(key);
        Color::Rgb(rgb.0, rgb.1, rgb.2)
    }

    /// key → 前景色（RGB 形式，便于测试比较）。
    pub fn fg_rgb(key: VisualKey) -> Rgb {
        match key {
            VisualKey::Player => Rgb::new(255, 255, 0),
            VisualKey::Monster(id) => Self::monster_fg(id),
            VisualKey::Stairs => Rgb::new(0, 255, 0),
            VisualKey::Item(_) => Rgb::new(220, 200, 120),
            VisualKey::Tile(id) => Self::tile_fg(id),
            VisualKey::Effect(_) => Rgb::new(255, 140, 0),
            VisualKey::UiIcon(icon) => Self::icon_fg(icon),
            VisualKey::Unknown(_) => Rgb::new(255, 255, 255),
        }
    }

    /// key → glyph。
    ///
    /// 未知 key 一律画 `?`：**可见的异常**比"画成地板"更容易被发现——
    /// 后者会让"某个新 key 忘了配色"看起来像地形正常。
    pub fn glyph(key: VisualKey) -> char {
        match key {
            VisualKey::Player => '@',
            VisualKey::Monster(id) => Self::monster_glyph(id),
            VisualKey::Stairs => '>',
            VisualKey::Item(_) => '!',
            VisualKey::Tile(id) => Self::tile_glyph(id),
            VisualKey::Effect(_) => '*',
            VisualKey::UiIcon(icon) => Self::icon_glyph(icon),
            VisualKey::Unknown(_) => '?',
        }
    }

    /// key → 背景色。`None` 表示沿用终端默认底色。
    pub fn bg(key: VisualKey) -> Option<Color> {
        match key {
            VisualKey::Tile(id) => Self::tile_bg(id).map(|rgb| Color::Rgb(rgb.0, rgb.1, rgb.2)),
            _ => None,
        }
    }

    /// 地形背景色（比前景暗，保证前景字符可读）。
    pub fn tile_bg(id: u16) -> Option<Rgb> {
        Some(match id {
            0 => Rgb::new(50, 50, 60),   // Wall
            1 => Rgb::new(20, 22, 25),   // Floor
            2 => Rgb::new(120, 190, 250), // ShallowWater
            3 => Rgb::new(20, 60, 140),  // DeepWater
            4 => Rgb::new(60, 55, 20),   // Stalactite
            5 => Rgb::new(25, 45, 25),   // Mycelium
            6 => Rgb::new(20, 55, 25),   // FungalPatch
            7 => Rgb::new(15, 40, 25),   // HangingVine
            8 => Rgb::new(60, 55, 35),   // Sand
            9 => Rgb::new(25, 55, 35),   // Seagrass
            10 => Rgb::new(70, 35, 25),  // CoralReef
            _ => return None,
        })
    }

    /// 地形前景色。
    ///
    /// 编号与 `presentation::tile_id` 的契约编号一致（也就是 `Tile` 的 serde
    /// 判别值）。编号对不上时落到 `_` 分支画 `?`，不会静默画错。
    pub fn tile_fg(id: u16) -> Rgb {
        match id {
            0 | 4 => Rgb::new(180, 180, 180), // Wall / Stalactite
            1 => Rgb::new(200, 200, 200),     // Floor
            2 => Rgb::new(220, 240, 255),     // ShallowWater
            3 => Rgb::new(80, 150, 220),      // DeepWater
            5 => Rgb::new(140, 190, 120),     // Mycelium
            6 => Rgb::new(90, 220, 110),      // FungalPatch
            7 => Rgb::new(40, 130, 70),       // HangingVine
            8 => Rgb::new(230, 215, 160),     // Sand
            9 => Rgb::new(70, 170, 110),      // Seagrass
            10 => Rgb::new(240, 150, 90),     // CoralReef
            _ => Rgb::new(255, 0, 255),       // 未登记的地形：洋红提示
        }
    }

    /// 地形 glyph。
    pub fn tile_glyph(id: u16) -> char {
        match id {
            0 => '#',       // Wall
            1 => '.',       // Floor
            2 => '~',       // ShallowWater
            3 => '≈',       // DeepWater
            4 => '#',       // Stalactite
            5 => ';',       // Mycelium
            6 => '♣',       // FungalPatch
            7 => '░',       // HangingVine
            8 => ':',       // Sand
            9 => ',',       // Seagrass
            10 => '%',      // CoralReef
            _ => '?',
        }
    }

    /// 怪物 glyph，按 `MonsterKindId` 的契约编号。
    pub fn monster_glyph(id: u16) -> char {
        match id {
            0 => 'r', // Rat
            1 => 's', // Scorpion
            2 => 'g', // Goblin
            3 => 'm', // Sporeling
            4 => 'M', // MushroomGolem
            5 => 'f', // CaveFish
            6 => 'c', // CaveCrab
            7 => 'e', // DeepEel
            _ => '?',
        }
    }

    /// 怪物前景色。
    pub fn monster_fg(id: u16) -> Rgb {
        match id {
            0 => Rgb::new(255, 0, 0),     // Rat
            1 => Rgb::new(180, 180, 0),   // Scorpion
            2 => Rgb::new(0, 255, 0),     // Goblin
            3 => Rgb::new(140, 255, 140), // Sporeling
            4 => Rgb::new(200, 120, 220), // MushroomGolem
            5 => Rgb::new(120, 200, 255), // CaveFish
            6 => Rgb::new(255, 140, 80),  // CaveCrab
            7 => Rgb::new(80, 160, 220),  // DeepEel
            _ => Rgb::new(255, 0, 255),
        }
    }

    fn icon_glyph(icon: UiIcon) -> char {
        match icon {
            UiIcon::Heart => '♥',
            UiIcon::Mana => '◆',
            UiIcon::Experience => '★',
            UiIcon::Floor => '≡',
            UiIcon::Map => '▦',
            UiIcon::Cursor => '⌖',
            UiIcon::Arrow => '→',
            UiIcon::Key => '⚿',
            UiIcon::Gold => '¤',
            UiIcon::Unknown => '·',
        }
    }

    fn icon_fg(icon: UiIcon) -> Rgb {
        match icon {
            UiIcon::Heart => Rgb::new(255, 80, 80),
            UiIcon::Mana => Rgb::new(80, 140, 255),
            UiIcon::Experience => Rgb::new(255, 220, 80),
            _ => Rgb::new(200, 200, 200),
        }
    }

    /// 记忆中的地形（当前不可见、但探索过）的暗化前景色。
    ///
    /// 暗化比例由后端决定（契约层只给"可见/已探索"两个布尔），这样 GPU 后端
    /// 可以用自己的调色方式，不必跟 TUI 一样按固定系数压暗。
    pub fn dim_fg(key: VisualKey) -> Color {
        let rgb = Self::fg_rgb(key).dim(0.55);
        Color::Rgb(rgb.0, rgb.1, rgb.2)
    }

    /// 记忆中的地形背景色（更暗）。
    pub fn dim_bg(key: VisualKey) -> Option<Color> {
        Self::bg(key).map(|color| match color {
            Color::Rgb(r, g, b) => {
                let dim = Rgb::new(r, g, b).dim(0.7);
                Color::Rgb(dim.0, dim.1, dim.2)
            }
            other => other,
        })
    }

    /// 视野外的记忆实体前景色（比地形更暗，避免抢注意力）。
    pub fn remembered_fg(key: VisualKey) -> Color {
        let rgb = Self::fg_rgb(key).dim(0.45);
        Color::Rgb(rgb.0, rgb.1, rgb.2)
    }
}

/// 契约层的 [`render_api::LogLevel`] → 终端颜色。
///
/// 与 [`TuiCatalog`] 同样属于"外观"，所以放在同一个文件里：新增日志等级时
/// 只改这里，不必去渲染代码里找 match。
pub fn log_color(level: render_api::LogLevel) -> Color {
    use render_api::LogLevel as L;
    match level {
        L::Combat => Color::Red,
        L::Item => Color::Yellow,
        L::Skill => Color::Cyan,
        L::System => Color::Gray,
        L::Danger => Color::LightRed,
        L::Debug => Color::DarkGray,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use render_api::VisualKey;

    /// 契约编号 0..=10 的地形都必须有**非提示色**的外观。
    ///
    /// 这条测试的防的是"新增地形只改了 `presentation::tile_id`，忘了在 catalog
    /// 里登记"——那会让新地形画成洋红 `?`，在真机上容易被误认为渲染 bug。
    #[test]
    fn every_contract_tile_id_has_a_registered_appearance() {
        let placeholder = Rgb::new(255, 0, 255);
        let unknown_glyph = '?';
        for id in 0..=10u16 {
            assert_ne!(
                TuiCatalog::tile_fg(id),
                placeholder,
                "地形 {id} 未在 catalog 登记颜色"
            );
            assert_ne!(
                TuiCatalog::tile_glyph(id),
                unknown_glyph,
                "地形 {id} 未在 catalog 登记 glyph"
            );
            assert!(TuiCatalog::tile_bg(id).is_some(), "地形 {id} 缺少背景色");
        }
        // 范围外仍然落到提示外观（说明检测手段本身有效）。
        assert_eq!(TuiCatalog::tile_fg(11), placeholder);
        assert_eq!(TuiCatalog::tile_glyph(11), unknown_glyph);
    }

    /// 怪物编号 0..=7 同理。
    #[test]
    fn every_contract_monster_id_has_a_registered_appearance() {
        let placeholder = Rgb::new(255, 0, 255);
        for id in 0..=7u16 {
            assert_ne!(
                TuiCatalog::monster_fg(id),
                placeholder,
                "怪物 {id} 未登记颜色"
            );
            assert_ne!(TuiCatalog::monster_glyph(id), '?', "怪物 {id} 未登记 glyph");
        }
        assert_eq!(TuiCatalog::monster_fg(8), placeholder);
        assert_eq!(TuiCatalog::monster_glyph(8), '?');
    }

    /// 未知 key 必须画成可见的 `?`，而不是伪装成地形。
    #[test]
    fn unknown_keys_are_visibly_marked() {
        assert_eq!(TuiCatalog::glyph(VisualKey::Unknown(0)), '?');
        assert_eq!(TuiCatalog::glyph(VisualKey::Unknown(999)), '?');
        assert_eq!(TuiCatalog::glyph(VisualKey::Tile(999)), '?');
        assert_eq!(TuiCatalog::glyph(VisualKey::Monster(999)), '?');
    }

    /// 玩家与楼梯的 glyph 是终端的约定俗成符号。
    #[test]
    fn player_and_stairs_use_conventional_glyphs() {
        assert_eq!(TuiCatalog::glyph(VisualKey::Player), '@');
        assert_eq!(TuiCatalog::glyph(VisualKey::Stairs), '>');
    }

    /// 只有地形有背景色（实体不该刷底，否则地图会被盖住）。
    #[test]
    fn only_terrain_has_a_background() {
        assert!(TuiCatalog::bg(VisualKey::Tile(1)).is_some());
        assert!(TuiCatalog::bg(VisualKey::Player).is_none());
        assert!(TuiCatalog::bg(VisualKey::Monster(0)).is_none());
        assert!(TuiCatalog::bg(VisualKey::Stairs).is_none());
    }

    /// 暗化必须让外观**可区分**于原始颜色。
    ///
    /// 注意 `utils::Rgb::dim` 的语义是**向灰色靠拢**（提高低通道、压低高通道），
    /// 不是简单变暗——所以这里断言"通道值有变化"，不断言"亮度和更小"：
    /// 对 `(200,200,200)` 这种中间灰，向灰靠拢反而会略微提亮。
    #[test]
    fn dimmed_colors_differ_from_the_original() {
        for key in [
            VisualKey::Tile(1),
            VisualKey::Tile(3),
            VisualKey::Tile(10),
            VisualKey::Player,
        ] {
            let bright = TuiCatalog::fg_rgb(key);
            let dimmed = TuiCatalog::fg_rgb(key).dim(0.55);
            assert_ne!(bright, dimmed, "{key:?} 暗化后必须与原色不同");
        }
    }

    /// 暗化比例越大越接近灰色 96。
    #[test]
    fn dim_moves_towards_the_neutral_gray() {
        let bright = Rgb::new(255, 0, 0);
        let once = bright.dim(0.5);
        let twice = bright.dim(0.9);
        let distance = |rgb: Rgb| -> i32 {
            let to_i32 = |v: u8| i32::from(v);
            [rgb.0, rgb.1, rgb.2]
                .into_iter()
                .map(|channel| (to_i32(channel) - 96).abs())
                .sum()
        };
        assert!(
            distance(twice) < distance(once),
            "暗化越多应当越接近中性灰：{twice:?} vs {once:?}"
        );
    }

    /// 每个日志等级都有颜色（新增等级时不会漏配）。
    #[test]
    fn every_log_level_has_a_color() {
        use render_api::LogLevel as L;
        for level in [
            L::Combat,
            L::Item,
            L::Skill,
            L::System,
            L::Danger,
            L::Debug,
        ] {
            let color = log_color(level);
            // 至少要能构造出颜色，且不是"隐形"的 Reset。
            assert_ne!(color, Color::Reset, "{level:?} 不得使用 Reset");
        }
    }
}
