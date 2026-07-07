// 全局 World 不再使用 OnceLock<RwLock<World>>。
// 线程局部 RNG 已移除 — 统一使用 ECS Resource GameRng。
