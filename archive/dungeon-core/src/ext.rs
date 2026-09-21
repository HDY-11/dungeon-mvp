//! Result/Option 日志扩展。
//!
//! 提供 `expect_log` 方法，在 panic 前通过 `log::error!` 写入日志文件，
//! 配合 `#[track_caller]` 精确记录调用点位置。
//!
//! # 示例
//!
//! ```ignore
//! let mut q = world.try_query::<(&Player, &Position)>()
//!     .expect_log("Player+Position registered at init");
//! ```

use std::panic::Location;

/// `Result<T, E>` 的日志扩展
pub trait ResultLogExt<T, E> {
    /// 错误时记录 `error!`，然后 panic（替换标准库 `expect`）
    #[track_caller]
    fn expect_log(self, context: impl Into<String>) -> T;
}

/// `Option<T>` 的日志扩展
pub trait OptionLogExt<T> {
    /// `None` 时记录 `error!`，然后 panic（替换标准库 `expect`）
    #[track_caller]
    fn expect_log(self, context: impl Into<String>) -> T;
}

impl<T, E: std::fmt::Display> ResultLogExt<T, E> for Result<T, E> {
    fn expect_log(self, context: impl Into<String>) -> T {
        let loc = Location::caller();
        match self {
            Ok(v) => v,
            Err(e) => {
                let ctx = context.into();
                log::error!("[{}:{}] {}: {}", loc.file(), loc.line(), ctx, e);
                panic!("[{}:{}] {}: {}", loc.file(), loc.line(), ctx, e);
            }
        }
    }
}

impl<T> OptionLogExt<T> for Option<T> {
    fn expect_log(self, context: impl Into<String>) -> T {
        let loc = Location::caller();
        match self {
            Some(v) => v,
            None => {
                let ctx = context.into();
                log::error!("[{}:{}] {}", loc.file(), loc.line(), ctx);
                panic!("[{}:{}] {}", loc.file(), loc.line(), ctx);
            }
        }
    }
}
