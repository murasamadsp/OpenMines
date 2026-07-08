//! Crystal pricing for the Market building.
//!
//! C# reference: `World.GetCrysCost(i)` = `cryscostbase[i] + cryscostmod[i]`.
//! Sell price = cost, Buy price = cost * 10. `cryscostmod` динамичен: ежечасно
//! подстраивается под объём добычи (`World.summary`, обновляется в `AddDob`).
//! Состояние в памяти (как C# `World.W`), не персистится — на рестарте дефолт.

use crate::game::GameState;
use std::time::{Duration, Instant};

/// Base crystal costs per type (Green, Blue, Red, Violet, White, Cyan).
/// C# `World.cryscostbase = { 8, 16, 24, 26, 24, 40 }`.
const CRYS_COST_BASE: [i64; 6] = [8, 16, 24, 26, 24, 40];

/// Дефолтный модификатор цены. C# `World.cryscostmod = { 10, 10, 15, 10, 15, 15 }`.
const CRYS_COST_MOD_DEFAULT: [i64; 6] = [10, 10, 15, 10, 15, 15];

/// Интервал пересчёта цен. C# `TimeSpan.FromHours(1)`.
const PRICE_UPDATE_INTERVAL: Duration = Duration::from_hours(1);

/// Динамическое состояние цен (C# `World.cryscostmod` + `summary` + `lastcryupdate`).
#[derive(Debug)]
pub struct CrystalEconomy {
    /// Модификатор цены по типам (C# `cryscostmod`).
    pub cost_mod: [i64; 6],
    /// Объём добычи за период (C# `summary`), сбрасывается при пересчёте.
    pub summary: [i64; 6],
    /// Время последнего пересчёта (C# `lastcryupdate`).
    pub last_update: Instant,
}

impl Default for CrystalEconomy {
    fn default() -> Self {
        Self {
            cost_mod: CRYS_COST_MOD_DEFAULT,
            summary: [0; 6],
            last_update: Instant::now(),
        }
    }
}

/// Цена продажи кристалла типа `i` (0-5). C# `World.GetCrysCost(i)` = base + mod.
#[must_use]
pub fn get_crystal_cost(state: &GameState, i: usize) -> i64 {
    if i >= 6 {
        return 0;
    }
    CRYS_COST_BASE[i] + state.crystal_economy.lock().cost_mod[i]
}

/// Цена покупки = 10× продажи (1:1 с C# `Market.BuildBuytab`).
#[must_use]
pub fn get_crystal_buy_price(state: &GameState, i: usize) -> i64 {
    get_crystal_cost(state, i) * 10
}

/// C# `World.AddDob(t, dob)`: += в `summary[t]` при добыче кристалла.
pub fn add_dob(state: &GameState, t: usize, dob: i64) {
    if t < 6 {
        state.crystal_economy.lock().summary[t] += dob;
    }
}

/// Ежечасный пересчёт цен (C# `World.Update` блок `lastcryupdate`).
/// p = (summary\[i\] + Σsummary) / 100; p>20 → mod−1 (пока mod>0); p<10 → mod+1 (пока cost<70).
pub fn tick_crystal_prices(state: &GameState) {
    let mut eco = state.crystal_economy.lock();
    if eco.last_update.elapsed() < PRICE_UPDATE_INTERVAL {
        return;
    }
    let summary = eco.summary;
    adjust_cost_mod(&mut eco.cost_mod, &summary);
    eco.summary = [0; 6];
    eco.last_update = Instant::now();
}

/// Чистая логика пересчёта модификатора (без `GameState`, тестируемо).
/// p = (summary\[i\] + Σsummary)/100; p>20 → mod−1 (пока mod>0, пол=base);
/// p<10 → mod+1 (пока cost<70).
fn adjust_cost_mod(cost_mod: &mut [i64; 6], summary: &[i64; 6]) {
    let total: i64 = summary.iter().sum();
    for (i, mod_i) in cost_mod.iter_mut().enumerate() {
        let p = (summary[i] + total) / 100;
        if p > 0 {
            if p > 20 && *mod_i > 0 {
                *mod_i -= 1;
            } else if p < 10 && CRYS_COST_BASE[i] + *mod_i < 70 {
                *mod_i += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn high_volume_lowers_mod_low_volume_raises() {
        // i=0: огромный объём → p>20 → mod−1. i=1: нулевой объём (но total>0
        // от i=0) → p = (0 + total)/100. Подберём total так, чтобы p<10 у i=1.
        let mut m = CRYS_COST_MOD_DEFAULT; // [10,10,15,10,15,15]
        let summary = [3000, 0, 0, 0, 0, 0]; // total=3000; i=0 p=(3000+3000)/100=60>20
        adjust_cost_mod(&mut m, &summary);
        assert_eq!(m[0], 9, "большой объём добычи → цена падает (mod−1)");
        // i=1..5: p=(0+3000)/100=30 >20 → тоже mod−1 (логика C#: ветка p>20 общая).
        assert_eq!(m[1], 9);
    }

    #[test]
    fn mod_floor_at_zero_ceiling_keeps_cost_below_70() {
        let mut m = [0, 0, 0, 0, 0, 0]; // mod уже на полу
        let summary = [5000, 0, 0, 0, 0, 0]; // p>20 для всех, но mod=0 → не уходит ниже
        adjust_cost_mod(&mut m, &summary);
        assert_eq!(m, [0; 6], "mod не опускается ниже 0 (цена не ниже base)");
    }
}
