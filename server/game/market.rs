//! Crystal pricing for the Market building.
//!
//! C# reference: `World.GetCrysCost(i)` = `cryscostbase[i] + cryscostmod[i]`
//! Sell price = cost, Buy price = cost * 10.

/// Base crystal costs per type (Green, Blue, Red, Violet, White, Cyan).
/// C# `World.cryscostbase = { 8, 16, 24, 26, 24, 40 }`.
const CRYS_COST_BASE: [i64; 6] = [8, 16, 24, 26, 24, 40];

/// Crystal cost modifier per type.
/// C# `World.cryscostmod = { 10, 10, 15, 10, 15, 15 }`.
const CRYS_COST_MOD: [i64; 6] = [10, 10, 15, 10, 15, 15];

/// Get sell price for crystal type `i` (0-5).
/// C# `World.GetCrysCost(i)` = base + mod.
#[inline]
pub fn get_crystal_cost(i: usize) -> i64 {
    if i >= 6 {
        return 0;
    }
    CRYS_COST_BASE[i] + CRYS_COST_MOD[i]
}

/// Buy price is 10x sell price (1:1 with C# Market.BuildBuytab).
#[inline]
pub fn get_crystal_buy_price(i: usize) -> i64 {
    get_crystal_cost(i) * 10
}
