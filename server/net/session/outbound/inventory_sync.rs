use crate::net::session::prelude::*;
use std::collections::HashMap;

pub fn send_inventory(tx: &mpsc::UnboundedSender<Vec<u8>>, inv: &HashMap<i32, i32>, selected: i32) {
    let items: Vec<(i32, i32)> = inv
        .iter()
        .filter(|(_, v)| **v > 0)
        .map(|(k, v)| (*k, *v))
        .collect();
    let total = i32::try_from(items.len()).unwrap_or(i32::MAX);
    send_u_packet(tx, "IN", &inventory_show(&items, selected, total).1);
}
