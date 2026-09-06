//! 开发者文件日志：文件持久化 + 可选内存捕获。
//!
//! `init_logging` 返回一个接收端，应用层可将其送入 TUI Debug 面板。

use log::{Level, LevelFilter, Log, Metadata, Record};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, Sender, channel};

const MAX_LOG_SIZE: u64 = 5 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct LogRecord {
    pub level: Level,
    pub target: String,
    pub message: String,
    pub file: Option<String>,
    pub line: Option<u32>,
}

fn log_filename() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("logs/game_{}.log", secs)
}

pub fn init_logging() -> Receiver<LogRecord> {
    let _ = fs::create_dir_all("logs");
    let path = log_filename();
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .unwrap_or_else(|e| panic!("无法创建日志文件 {path}: {e}"));

    let (sender, receiver) = channel::<LogRecord>();
    let logger = FileLogger {
        file: Mutex::new(Some(file)),
        path,
        sender: Mutex::new(Some(sender)),
    };

    log::set_boxed_logger(Box::new(logger))
        .map(|()| log::set_max_level(LevelFilter::Trace))
        .expect("init_logging 被调用多次");

    receiver
}

struct FileLogger {
    file: Mutex<Option<File>>,
    path: String,
    sender: Mutex<Option<Sender<LogRecord>>>,
}

impl Log for FileLogger {
    fn enabled(&self, _: &Metadata) -> bool {
        true
    }

    fn log(&self, record: &Record) {
        let record_for_ui = LogRecord {
            level: record.level(),
            target: record.target().to_string(),
            message: record.args().to_string(),
            file: record.file().map(str::to_string),
            line: record.line(),
        };

        if let Ok(sender) = self.sender.lock() {
            if let Some(sender) = sender.as_ref() {
                let _ = sender.send(record_for_ui.clone());
            }
        }

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