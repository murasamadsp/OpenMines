use crate::game::player::PlayerInventory;
use crate::net::session::prelude::*;

pub fn send_inventory(tx: &mpsc::UnboundedSender<Vec<u8>>, inv: &PlayerInventory) {
    let items: Vec<(i32, i32)> = inv
        .items
        .iter()
        .filter(|(_, v)| **v > 0)
        .map(|(k, v)| (*k, *v))
        .collect();
    let total = i32::try_from(items.len()).unwrap_or(i32::MAX);
    send_u_packet(tx, "IN", &inventory_show(&items, inv.selected, total).1);
}
