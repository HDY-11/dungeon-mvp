//! 游戏日志系统。
//!
//! 两层设计：
//! - 开发者日志 → `log::info!()` → 文件（持久化，按大小轮转）
//! - 玩家日志   → `EventLog::push()` → 终端（游戏内显示，自动转发到文件）
//!
//! 在 `main()` 开头调用 `init_logging()` 注册。

use crate::ext::ResultLogExt;
use log::{LevelFilter, Log, Metadata, Record};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::sync::Mutex;

/// 日志文件最大字节数，超过时轮转
const MAX_LOG_SIZE: u64 = 5 * 1024 * 1024;

/// 生成日志文件名
fn log_filename() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("logs/game_{}.log", secs)
}

/// 启动日志系统。
///
/// 应在 `main()` 开头调用，在 panic hook 之前。
/// 会创建 `logs/` 目录和日志文件，注册 `FileLogger` 为全局 logger。
pub fn init_logging() {
    let _ = fs::create_dir_all("logs");
    let path = log_filename();
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .unwrap_or_else(|e| panic!("无法创建日志文件 {}: {}", path, e));

    let logger = FileLogger {
        file: Mutex::new(Some(file)),
        path,
    };

    log::set_boxed_logger(Box::new(logger))
        .map(|()| log::set_max_level(LevelFilter::Debug))
        .expect_log("init_logging 被调用多次");
}

/// 文件日志器。线程安全（内部使用 Mutex），单线程游戏无竞争开销。
struct FileLogger {
    file: Mutex<Option<File>>,
    path: String,
}

impl Log for FileLogger {
    fn enabled(&self, _: &Metadata) -> bool {
        true
    }

    fn log(&self, record: &Record) {
        let mut guard = self.file.lock().expect_log("FileLogger mutex poisoned");
        let Some(file) = guard.as_mut() else { return };
        let _ = writeln!(
            file,
            "[{} {}:{}] {}",
            record.level(),
            record.file().unwrap_or("?"),
            record.line().unwrap_or(0),
            record.args()
        );
        // 超过大小 → 轮转
        if file.metadata().map(|m| m.len()).unwrap_or(0) > MAX_LOG_SIZE {
            let _ = file.flush();
            let old_path = format!("{}.old", self.path);
            let _ = fs::rename(&self.path, &old_path);
            if let Ok(f) = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)
            {
                *guard = Some(f);
            }
        }
    }

    fn flush(&self) {
        if let Ok(mut guard) = self.file.lock()
            && let Some(ref mut file) = *guard
        {
            let _ = file.flush();
        }
    }
}

/// 关闭日志系统。游戏退出前调用，确保缓冲数据写入磁盘。
pub fn shutdown_logging() {
    log::logger().flush();
}
