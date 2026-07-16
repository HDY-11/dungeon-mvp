//! 键盘事件获取 + 分派。
//!
//! 职责边界：
//!   - InputDriver：获取。独立线程阻塞 poll(crossterm) → 去重 → push channel
//!   - EventBus：分派。主循环非阻塞 poll channel → 优先级订阅者链
//!   - 不知道 World 存在？分派层知道（传 &mut World 给 handler）。获取层不知道。
//!   - 不知道渲染？不知道。

use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};
use crossterm::event::{self, Event, KeyCode};
use bevy_ecs::prelude::World;

// ══════════════════════════════════════════════════════
// 事件类型（纯数据）
// ══════════════════════════════════════════════════════

#[derive(Clone, Copy, Debug)]
pub struct KeyEvent {
    pub code: KeyCode,
}

// ══════════════════════════════════════════════════════
// 消费结果
// ══════════════════════════════════════════════════════

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DispatchResult {
    /// 消费了，停止传播给更低优先级的订阅者
    Consumed,
    /// 不消费，传给下一个
    Pass,
    /// 消费了，并且本订阅者请求退订（模态结束）
    Unsubscribe,
}

// ══════════════════════════════════════════════════════
// 获取层（独立线程）
// ══════════════════════════════════════════════════════

pub struct InputDriver {
    rx: mpsc::Receiver<KeyEvent>,
}

impl InputDriver {
    /// 启动输入线程并返回驱动句柄
    pub fn spawn() -> Self {
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let mut last_code = KeyCode::Null;
            let mut last_time = Instant::now();
            loop {
                if event::poll(Duration::from_millis(16)).unwrap_or(false)
                    && let Ok(Event::Key(key)) = event::read() {
                        let now = Instant::now();
                        // 50ms 同键去重
                        if key.code == last_code
                            && now - last_time < Duration::from_millis(50) { continue; }
                        last_code = key.code;
                        last_time = now;
                        if tx.send(KeyEvent { code: key.code }).is_err() { break; }
                    }
            }
        });
        Self { rx }
    }

    /// 非阻塞拉取一个事件
    pub fn poll(&mut self) -> Option<KeyEvent> {
        self.rx.try_recv().ok()
    }
}

// ══════════════════════════════════════════════════════
// 分派层（优先级订阅者链）
// ══════════════════════════════════════════════════════

pub struct EventBus {
    subscribers: Vec<Subscriber>,
    next_id: usize,
}

struct Subscriber {
    id: usize,
    priority: i8,
    handler: Box<dyn FnMut(KeyEvent, &mut World) -> DispatchResult>,
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

impl EventBus {
    pub fn new() -> Self {
        Self { subscribers: Vec::new(), next_id: 0 }
    }

    /// 注册一个订阅者。priority 越高越优先收到事件。
    pub fn subscribe<F>(&mut self, priority: i8, handler: F) -> usize
    where
        F: FnMut(KeyEvent, &mut World) -> DispatchResult + 'static,
    {
        let id = self.next_id;
        self.next_id += 1;
        self.subscribers.push(Subscriber {
            id,
            priority,
            handler: Box::new(handler),
        });
        self.subscribers.sort_by(|a, b| b.priority.cmp(&a.priority));
        id
    }

    /// 按 id 退订
    pub fn unsubscribe(&mut self, id: usize) {
        self.subscribers.retain(|s| s.id != id);
    }

    /// 分派一个事件到订阅者链。返回 true = 被某个订阅者消费。
    pub fn dispatch(&mut self, event: KeyEvent, world: &mut World) -> bool {
        let mut i = 0;
        while i < self.subscribers.len() {
            let (consumed, should_remove) = {
                let sub = &mut self.subscribers[i];
                match (sub.handler)(event, world) {
                    DispatchResult::Consumed => (true, false),
                    DispatchResult::Pass => (false, false),
                    DispatchResult::Unsubscribe => (true, true),
                }
            };
            if should_remove {
                self.subscribers.swap_remove(i);
            } else {
                i += 1;
            }
            if consumed {
                return true;
            }
        }
        false
    }

    /// 是否有高于指定优先级的活跃订阅者
    pub fn has_priority_above(&self, threshold: i8) -> bool {
        self.subscribers.iter().any(|s| s.priority > threshold)
    }
}
