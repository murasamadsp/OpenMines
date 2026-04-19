use crate::game::player::PlayerInventory;
use crate::net::session::prelude::*;
use std::collections::HashMap;

#[inline]
fn inventory_nonzero_count(items: &HashMap<i32, i32>) -> i32 {
    items.values().filter(|v| **v > 0).count() as i32
}

/// Как `Inventory.AddChoose` в `Inventory.cs`.
pub fn add_choose_miniq(miniq: &mut Vec<i32>, id: i32) {
    if id == -1 || miniq.contains(&id) {
        return;
    }
    if miniq.len() >= 4 {
        miniq.remove(0);
    }
    miniq.push(id);
}

fn prefill_miniq_if_needed(inv: &mut PlayerInventory) {
    let len = inventory_nonzero_count(&inv.items);
    if inv.minv && inv.miniq.len() < 4 && len > 0 {
        let mut keys: Vec<i32> = inv
            .items
            .iter()
            .filter(|(_, v)| **v > 0)
            .map(|(k, _)| *k)
            .collect();
        keys.sort_unstable();
        for k in keys {
            add_choose_miniq(&mut inv.miniq, k);
            if inv.miniq.len() >= 4 {
                break;
            }
        }
    }
}

/// Как `Inventory.InvToSend` → `InventoryShowPacket` (`Inventory.cs`).
pub fn send_inventory(tx: &mpsc::UnboundedSender<Vec<u8>>, inv: &mut PlayerInventory) {
    prefill_miniq_if_needed(inv);
    let all = inventory_nonzero_count(&inv.items);
    let mut grid: Vec<(i32, i32)> = if inv.minv {
        inv.miniq
            .iter()
            .filter_map(|&id| inv.items.get(&id).map(|&c| (id, c)))
            .filter(|(_, c)| *c > 0)
            .collect()
    } else {
        inv.items
            .iter()
            .filter(|(_, v)| **v > 0)
            .map(|(k, v)| (*k, *v))
            .collect()
    };
    if !inv.minv {
        grid.sort_by_key(|(k, _)| *k);
    }
    send_u_packet(tx, "IN", &inventory_show(&grid, inv.selected, all).1);
}
