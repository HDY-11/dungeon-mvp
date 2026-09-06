//! TUI 渲染层。
//!
//! 只做渲染与 UI 状态，不读取操作系统输入，不包含业务规则。

pub mod canvas;
pub mod color;
pub mod layout;
pub mod render;
pub mod scene;
pub mod state;
pub mod title;

pub use canvas::*;
pub use color::*;
pub use layout::*;
pub use render::*;
pub use scene::*;
pub use state::*;
pub use title::*;
