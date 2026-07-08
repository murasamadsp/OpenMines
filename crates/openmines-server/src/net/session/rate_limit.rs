//! Per-player rate limiting (GCRA через `governor`).
//!
//! Лимитеры хранятся в `GameState::rate_limiters` — `DashMap<PlayerId, PlayerLimiters>`.
//! Создаются лениво при первом пакете игрока, удаляются при дисконнекте.
//! Все параметры берутся из `config.gameplay.rate_limits` — не хардкод.
use governor::clock::DefaultClock;
use governor::state::{InMemoryState, NotKeyed};
use governor::{Quota, RateLimiter};
use std::num::NonZeroU32;

pub type Limiter = RateLimiter<NotKeyed, InMemoryState, DefaultClock>;

/// Набор лимитеров для одного игрока.
pub struct PlayerLimiters {
    pub chat: Limiter,
    pub gui: Limiter,
}

impl PlayerLimiters {
    /// Создаёт лимитеры по параметрам конфига.
    /// `chat_per_sec`/`gui_per_sec` — скорость пополнения (tokens/s).
    /// `chat_burst`/`gui_burst` — размер корзины (максимальный всплеск).
    pub fn new(chat_per_sec: u32, chat_burst: u32, gui_per_sec: u32, gui_burst: u32) -> Self {
        let chat_quota = Quota::per_second(nz(chat_per_sec)).allow_burst(nz(chat_burst));
        let gui_quota = Quota::per_second(nz(gui_per_sec)).allow_burst(nz(gui_burst));
        Self {
            chat: RateLimiter::direct(chat_quota),
            gui: RateLimiter::direct(gui_quota),
        }
    }
}

/// Паника при 0 — конфиг уже провалидирован при старте (>0 обязательно).
const fn nz(v: u32) -> NonZeroU32 {
    NonZeroU32::new(v).expect("rate limit config value must be > 0")
}
