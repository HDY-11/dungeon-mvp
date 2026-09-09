//! 语义化视觉键与渲染层级。
//!
//! [`VisualKey`] 是渲染后端与游戏逻辑之间的“外观句柄”：
//!
//! - `presentation` 把 `core` 的 `Tile` / `MonsterKindId` / 实体身份映射成稳定的 key；
//! - `tui` 用 `TuiCatalog` 把 key 映射成 glyph + 颜色；
//! - 未来的 `gpu` 用 `GpuCatalog` 把 key 映射成纹理 / 材质。
//!
//! 本模块不包含 glyph、颜色、纹理句柄，因此 TUI 和 GPU 可以共享同一份场景数据。

use std::fmt;

/// 语义化视觉类别。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[repr(u8)]
pub enum VisualCategory {
    Player = 0,
    Monster = 1,
    Stairs = 2,
    Item = 3,
    Tile = 4,
    Effect = 5,
    Ui = 6,
    Unknown = 7,
}

/// 渲染层级。
///
/// 后端按此排序：TUI 的绘制顺序 / GPU 的 z 值。
/// 判别值参与稳定编码，新增层级只能追加。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
#[repr(u8)]
pub enum VisualLayer {
    #[default]
    Terrain = 0,
    Item = 1,
    Actor = 2,
    Effect = 3,
    Ui = 4,
    Debug = 5,
}

/// UI 图标语义键。
///
/// 后端各自决定具体图标；枚举判别值参与 [`VisualKey::as_u64`] 的稳定编码，
/// 新增图标只能追加，不得重排已有判别值。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
#[repr(u8)]
pub enum UiIcon {
    #[default]
    Unknown = 0,
    Heart = 1,
    Mana = 2,
    Experience = 3,
    Floor = 4,
    Map = 5,
    Cursor = 6,
    Arrow = 7,
    Key = 8,
    Gold = 9,
}

impl UiIcon {
    /// 返回稳定的判别值。
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    /// 从判别值还原；未知值归一化为 [`UiIcon::Unknown`]。
    pub const fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::Heart,
            2 => Self::Mana,
            3 => Self::Experience,
            4 => Self::Floor,
            5 => Self::Map,
            6 => Self::Cursor,
            7 => Self::Arrow,
            8 => Self::Key,
            9 => Self::Gold,
            _ => Self::Unknown,
        }
    }
}

/// 语义化视觉键。
///
/// `Monster(u16)` / `Item(u32)` / `Tile(u16)` / `Effect(u16)` 的 payload 是
/// `core` 侧 ID 的不透明映射，由 `presentation` 负责转换；后端只负责查表。
///
/// # 稳定性契约
///
/// - 变体与 payload 的编码不得随意重排；新增变体只能追加。
/// - [`VisualKey::as_u64`] 仅用于单次运行内的批处理 / 哈希，
///   不要用于存档或跨版本持久化。
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum VisualKey {
    Player,
    Monster(u16),
    Stairs,
    Item(u32),
    Tile(u16),
    Effect(u16),
    UiIcon(UiIcon),
    Unknown(u32),
}

impl Default for VisualKey {
    fn default() -> Self {
        Self::Unknown(0)
    }
}

impl VisualKey {
    /// 返回语义类别。
    pub const fn category(self) -> VisualCategory {
        match self {
            Self::Player => VisualCategory::Player,
            Self::Monster(_) => VisualCategory::Monster,
            Self::Stairs => VisualCategory::Stairs,
            Self::Item(_) => VisualCategory::Item,
            Self::Tile(_) => VisualCategory::Tile,
            Self::Effect(_) => VisualCategory::Effect,
            Self::UiIcon(_) => VisualCategory::Ui,
            Self::Unknown(_) => VisualCategory::Unknown,
        }
    }

    /// 是否为未知 key（后端应走 fallback 外观）。
    pub const fn is_unknown(self) -> bool {
        matches!(self, Self::Unknown(_))
    }

    /// 稳定批处理编码：高 32 位为类别 tag，低 32 位为 payload。
    ///
    /// 该编码只保证在同一个 [`crate::CONTRACT_VERSION`] 内稳定。
    pub const fn as_u64(self) -> u64 {
        const TAG_SHIFT: u32 = 32;
        match self {
            Self::Player => 0,
            Self::Monster(payload) => (1_u64 << TAG_SHIFT) | payload as u64,
            Self::Stairs => 2_u64 << TAG_SHIFT,
            Self::Item(payload) => (3_u64 << TAG_SHIFT) | payload as u64,
            Self::Tile(payload) => (4_u64 << TAG_SHIFT) | payload as u64,
            Self::Effect(payload) => (5_u64 << TAG_SHIFT) | payload as u64,
            Self::UiIcon(icon) => (6_u64 << TAG_SHIFT) | icon.as_u8() as u64,
            Self::Unknown(payload) => (7_u64 << TAG_SHIFT) | payload as u64,
        }
    }

    /// 从 [`VisualKey::as_u64`] 解码。
    ///
    /// 未知 tag 返回 `Unknown(payload)`；未知 `UiIcon` 判别值归一化为
    /// [`UiIcon::Unknown`]。
    pub const fn from_u64(raw: u64) -> Self {
        const TAG_MASK: u64 = 0xFFFF_FFFF;
        let tag = raw >> 32;
        let payload = raw & TAG_MASK;
        match tag {
            0 => Self::Player,
            1 => Self::Monster(payload as u16),
            2 => Self::Stairs,
            3 => Self::Item(payload as u32),
            4 => Self::Tile(payload as u16),
            5 => Self::Effect(payload as u16),
            6 => Self::UiIcon(UiIcon::from_u8(payload as u8)),
            _ => Self::Unknown(payload as u32),
        }
    }
}

impl fmt::Debug for VisualKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Player => f.write_str("Player"),
            Self::Monster(id) => write!(f, "Monster({id})"),
            Self::Stairs => f.write_str("Stairs"),
            Self::Item(id) => write!(f, "Item({id})"),
            Self::Tile(id) => write!(f, "Tile({id})"),
            Self::Effect(id) => write!(f, "Effect({id})"),
            Self::UiIcon(icon) => write!(f, "UiIcon({icon:?})"),
            Self::Unknown(id) => write!(f, "Unknown({id})"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TAG_SHIFT: u32 = 32;

    #[test]
    fn as_u64_encoding_is_stable() {
        assert_eq!(VisualKey::Player.as_u64(), 0);
        assert_eq!(VisualKey::Monster(7).as_u64(), (1_u64 << TAG_SHIFT) | 7);
        assert_eq!(VisualKey::Stairs.as_u64(), 2_u64 << TAG_SHIFT);
        assert_eq!(VisualKey::Item(42).as_u64(), (3_u64 << TAG_SHIFT) | 42);
        assert_eq!(VisualKey::Tile(3).as_u64(), (4_u64 << TAG_SHIFT) | 3);
        assert_eq!(VisualKey::Effect(9).as_u64(), (5_u64 << TAG_SHIFT) | 9);
        assert_eq!(
            VisualKey::UiIcon(UiIcon::Heart).as_u64(),
            (6_u64 << TAG_SHIFT) | 1
        );
        assert_eq!(VisualKey::Unknown(11).as_u64(), (7_u64 << TAG_SHIFT) | 11);
    }

    #[test]
    fn from_u64_round_trips_known_keys() {
        let keys = [
            VisualKey::Player,
            VisualKey::Monster(7),
            VisualKey::Stairs,
            VisualKey::Item(42),
            VisualKey::Tile(3),
            VisualKey::Effect(9),
            VisualKey::UiIcon(UiIcon::Heart),
            VisualKey::UiIcon(UiIcon::Gold),
            VisualKey::Unknown(11),
        ];
        for key in keys {
            assert_eq!(VisualKey::from_u64(key.as_u64()), key, "{key:?}");
        }
    }

    #[test]
    fn category_matches_variant() {
        assert_eq!(VisualKey::Player.category(), VisualCategory::Player);
        assert_eq!(VisualKey::Monster(1).category(), VisualCategory::Monster);
        assert_eq!(VisualKey::Stairs.category(), VisualCategory::Stairs);
        assert_eq!(VisualKey::Item(1).category(), VisualCategory::Item);
        assert_eq!(VisualKey::Tile(1).category(), VisualCategory::Tile);
        assert_eq!(VisualKey::Effect(1).category(), VisualCategory::Effect);
        assert_eq!(
            VisualKey::UiIcon(UiIcon::Heart).category(),
            VisualCategory::Ui
        );
        assert_eq!(VisualKey::Unknown(1).category(), VisualCategory::Unknown);
    }

    #[test]
    fn unknown_key_is_detected() {
        assert!(VisualKey::Unknown(0).is_unknown());
        assert!(!VisualKey::Player.is_unknown());
    }

    #[test]
    fn visual_layer_ordering_is_draw_order() {
        assert!(VisualLayer::Terrain < VisualLayer::Item);
        assert!(VisualLayer::Item < VisualLayer::Actor);
        assert!(VisualLayer::Actor < VisualLayer::Effect);
        assert!(VisualLayer::Effect < VisualLayer::Ui);
        assert!(VisualLayer::Ui < VisualLayer::Debug);
    }

    #[test]
    fn ui_icon_encoding_is_stable() {
        assert_eq!(UiIcon::Unknown.as_u8(), 0);
        assert_eq!(UiIcon::Heart.as_u8(), 1);
        assert_eq!(UiIcon::Gold.as_u8(), 9);
        assert_eq!(UiIcon::from_u8(1), UiIcon::Heart);
        assert_eq!(UiIcon::from_u8(255), UiIcon::Unknown);
    }
}
