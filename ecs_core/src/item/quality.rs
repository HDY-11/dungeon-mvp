//! 物品品质（DESIGN DsnE23）。
//!
//! 品质是**蓝图上的分类**，不是实例状态：一件「稀有匕首」掉出来就是稀有，
//! 以后升品是独立机制，不在本类型里表达。品质的作用有三处（都由数据/界面消费，
//! 规则层不直接读它）：
//!
//! - **预算**：内容作者用品质粗分掉落强度，品质越高数值预算越大；
//! - **门槛**：附魔学派、配方、未来词条可以要求最低品质（DsnE24/DsnE25）；
//! - **EXP 倍率**：武器养成的经验曲线按品质反向缩放（DsnE24）。
//!
//! # 为什么是有序枚举
//!
//! 品质天然有大小（粗劣 < 普通 < … < 传说）：排序、筛选、颜色表都依赖这个顺序。
//! 内部用 `rank()` 的 0–5 作为稳定排序键；数据文件里写名字（`"rare"`），
//! 反序列化未知名字直接报错，不会静默落成某个档。
//!
//! 面向玩家的名字是中文档名（`label()`），界面按品质上色（DsnE23 的 UI 规则：
//! 颜色只做辅助，详情面板永远写出品质名，色觉障碍/黑白终端下仍可读）。

use serde::{Deserialize, Serialize};

/// 物品品质六档（DsnE23）。
///
/// `Default` 是 [`Quality::Common`]：数据里漏写时不会意外变成"传说"；
/// 加载器仍要求显式写 `quality` 字段（schema 5 起必填），默认值只服务
/// 反序列化的 `#[serde(default)]` 场景与测试。
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
#[serde(rename_all = "snake_case")]
pub enum Quality {
    /// 粗劣：几乎只有材料价值的东西。
    Coarse,
    /// 普通：日用品与早期装备。
    #[default]
    Common,
    /// 精良：有明确用途的装备。
    Fine,
    /// 稀有：值得为它绕路的装备。
    Rare,
    /// 史诗：一个流派的核心件。
    Epic,
    /// 传说：一层/一局的招牌。
    Legendary,
}

impl Quality {
    /// 全部档位（从低到高）。
    pub const ALL: [Self; 6] = [
        Self::Coarse,
        Self::Common,
        Self::Fine,
        Self::Rare,
        Self::Epic,
        Self::Legendary,
    ];

    /// 稳定排序键（0 = 粗劣，5 = 传说）。
    pub const fn rank(self) -> u8 {
        match self {
            Self::Coarse => 0,
            Self::Common => 1,
            Self::Fine => 2,
            Self::Rare => 3,
            Self::Epic => 4,
            Self::Legendary => 5,
        }
    }

    /// 界面/日志用的中文档名。
    pub const fn label(self) -> &'static str {
        match self {
            Self::Coarse => "粗劣",
            Self::Common => "普通",
            Self::Fine => "精良",
            Self::Rare => "稀有",
            Self::Epic => "史诗",
            Self::Legendary => "传说",
        }
    }

    /// 由排序键反查档位（越界返回 `None`）。
    pub const fn from_rank(rank: u8) -> Option<Self> {
        match rank {
            0 => Some(Self::Coarse),
            1 => Some(Self::Common),
            2 => Some(Self::Fine),
            3 => Some(Self::Rare),
            4 => Some(Self::Epic),
            5 => Some(Self::Legendary),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Quality;

    #[test]
    fn ranks_are_dense_ordered_and_round_trip() {
        for (index, quality) in Quality::ALL.into_iter().enumerate() {
            assert_eq!(usize::from(quality.rank()), index);
            assert_eq!(Quality::from_rank(quality.rank()), Some(quality));
        }
        assert_eq!(Quality::from_rank(6), None);
    }

    #[test]
    fn labels_are_non_empty_and_unique() {
        let mut labels: Vec<&str> = Quality::ALL.iter().map(|quality| quality.label()).collect();
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(labels.len(), Quality::ALL.len());
        assert!(labels.iter().all(|label| !label.is_empty()));
    }

    #[test]
    fn quality_orders_from_coarse_to_legendary() {
        assert!(Quality::Coarse < Quality::Common);
        assert!(Quality::Rare < Quality::Epic);
        assert_eq!(Quality::default(), Quality::Common);
    }

    #[test]
    fn data_names_are_snake_case() {
        for quality in Quality::ALL {
            let json = serde_json::to_string(&quality).expect("品质必须可序列化");
            assert_eq!(json, format!("\"{}\"", name(quality)));
        }
    }

    fn name(quality: Quality) -> &'static str {
        match quality {
            Quality::Coarse => "coarse",
            Quality::Common => "common",
            Quality::Fine => "fine",
            Quality::Rare => "rare",
            Quality::Epic => "epic",
            Quality::Legendary => "legendary",
        }
    }
}
