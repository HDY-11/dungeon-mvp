//! 后台键盘输入线程。
//!
//! 线程负责终端轮询与 33ms 同键去重，主线程通过 `try_recv` 非阻塞消费。

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

/// 启动输入线程，返回按键接收端。
///
/// 线程会持续运行直到接收端被丢弃。
pub fn spawn_key_source() -> Receiver<KeyCode> {
    let (tx, rx) = std::sync::mpsc::channel::<KeyCode>();

    std::thread::spawn(move || {
        let mut last_code = KeyCode::Null;
        let mut last_time = Instant::now();

        loop {
            if crossterm::event::poll(Duration::from_millis(33)).unwrap_or(false)
                && let Ok(Event::Key(key)) = event::read()
            {
                if key.kind != KeyEventKind::Press {
                    continue;
                }

                let now = Instant::now();
                if key.code == last_code && now - last_time < Duration::from_millis(33) {
                    continue;
                }
                last_code = key.code;
                last_time = now;

                if tx.send(key.code).is_err() {
                    break;
                }
            }
        }
    });

    rx
}

pub fn try_recv_key(rx: &Receiver<KeyCode>) -> Option<KeyCode> {
    match rx.try_recv() {
        Ok(key) => Some(key),
        Err(TryRecvError::Empty) => None,
        Err(TryRecvError::Disconnected) => None,
    }
}

/// 阻塞读取一个 Press 按键。用于标题画面等没有主循环的简单场景。
pub fn read_key_blocking() -> std::io::Result<KeyCode> {
    loop {
        if let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            return Ok(key.code);
        }
    }
}

/// 消费本帧所有按键。
pub fn drain_keys(rx: &Receiver<KeyCode>, mut f: impl FnMut(KeyCode)) {
    while let Some(key) = try_recv_key(rx) {
        f(key);
    }
}
