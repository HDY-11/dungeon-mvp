//! 开发者文件日志：5MB 自动轮转。
//!
//! 玩家可见日志由 core 的 `EventLog` 管理，不在此处。

use log::{LevelFilter, Log, Metadata, Record};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::sync::Mutex;

const MAX_LOG_SIZE: u64 = 5 * 1024 * 1024;

fn log_filename() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("logs/game_{}.log", secs)
}

pub fn init_logging() {
    let _ = fs::create_dir_all("logs");
    let path = log_filename();
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .unwrap_or_else(|e| panic!("无法创建日志文件 {path}: {e}"));

    let logger = FileLogger {
        file: Mutex::new(Some(file)),
        path,
    };

    log::set_boxed_logger(Box::new(logger))
        .map(|()| log::set_max_level(LevelFilter::Debug))
        .expect("init_logging 被调用多次");
}

struct FileLogger {
    file: Mutex<Option<File>>,
    path: String,
}

impl Log for FileLogger {
    fn enabled(&self, _: &Metadata) -> bool {
        true
    }

    fn log(&self, record: &Record) {
        let Ok(mut guard) = self.file.lock() else {
            return;
        };
        let Some(file) = guard.as_mut() else {
            return;
        };

        let _ = writeln!(
            file,
            "[{} {}:{}] {}",
            record.level(),
            record.file().unwrap_or("?"),
            record.line().unwrap_or(0),
            record.args()
        );

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

pub fn shutdown_logging() {
    log::logger().flush();
}
