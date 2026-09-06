//! 与操作系统的交互：终端、输入、文件、日志。
//!
//! 本 crate 不包含业务规则；输入只提供按键流，文件只提供字节读写。

pub mod files;
pub mod input;
pub mod logger;
pub mod terminal;

pub use files::*;
pub use input::*;
pub use logger::*;
pub use terminal::*;
