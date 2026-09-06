//! 世界生命周期：初始化、查询与主循环装配。

pub mod init;
pub mod query;

pub use init::*;
pub use query::*;

/// `loop` 是 Rust 关键字，模块使用 `loop_`。
pub mod loop_;

pub use loop_::*;
