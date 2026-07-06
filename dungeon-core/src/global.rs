use rand::rngs::SmallRng;
use rand::SeedableRng;
use std::cell::RefCell;

// 全局 World 不再使用 OnceLock<RwLock<World>>。
// 所有函数改为显式接收 &World / &mut World 参数。
// 以下仅保留线程局部的随机数生成器。

std::thread_local! {
    static RNG: RefCell<SmallRng> = RefCell::new(SmallRng::seed_from_u64(0));
}

/// 线程局部的随机数，用于仲裁时随机选择相同优先级的行动
pub fn rand_u8() -> u8 {
    use rand::RngExt;
    RNG.with(|r| {
        let mut guard = r.borrow_mut();
        (&mut *guard).random_range(0u8..=255u8)
    })
}
