use bevy_ecs::prelude::World;
use rand::rngs::SmallRng;
use rand::SeedableRng;
use std::cell::RefCell;
use std::sync::{OnceLock, RwLock};

static WORLD: OnceLock<RwLock<World>> = OnceLock::new();

/// 设置/替换全局 World。
/// - 首次调用 = 初始化
/// - 后续调用 = 替换旧的，返回旧的（测试用）
pub fn set_world(world: World) -> Option<World> {
    match WORLD.get() {
        Some(rwlock) => {
            let mut guard = rwlock.write().unwrap();
            Some(std::mem::replace(&mut *guard, world))
        }
        None => {
            WORLD
                .set(RwLock::new(world))
                .expect("concurrent initialization of global World");
            None
        }
    }
}

/// 获取全局 World 的读锁。
pub fn read_world() -> std::sync::RwLockReadGuard<'static, World> {
    WORLD
        .get()
        .expect("World not initialized — call set_world first")
        .read()
        .unwrap()
}

/// 获取全局 World 的写锁。
pub fn write_world() -> std::sync::RwLockWriteGuard<'static, World> {
    WORLD
        .get()
        .expect("World not initialized — call set_world first")
        .write()
        .unwrap()
}

/// 线程局部的随机数生成器（用于仲裁时随机选择相同优先级的行动）
std::thread_local! {
    static RNG: RefCell<SmallRng> = RefCell::new(SmallRng::seed_from_u64(0));
}

pub fn rand_u8() -> u8 {
    use rand::RngExt;
    RNG.with(|r| {
        let mut guard = r.borrow_mut();
        (&mut *guard).random_range(0u8..=255u8)
    })
}

/// 获取全局 World 的读锁（不可变借用）。
/// 用法：`world!()` 读，`world!(mut)` 写。
#[macro_export]
macro_rules! world {
    () => {{
        $crate::global::read_world()
    }};
    (mut) => {{
        $crate::global::write_world()
    }};
}
