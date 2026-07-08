use crate::game::player::PlayerInventory;
use crate::net::session::prelude::*;
use crate::protocol::packets::inventory_show;
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
    let is_mini = inv.minv;
    let mut grid: Vec<(i32, i32)> = if is_mini {
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
    // Сортируем по id ВСЕГДА (и mini, и full). Клиент рендерит сетку строго в
    // порядке payload (InventoryPanel.makeGrid), поэтому несовпадение порядка
    // mini (порядок miniq) и full (по ключу) приводит к «скачкам» предметов при
    // сворачивании/разворачивании. Стабильный порядок по id убирает скачки.
    grid.sort_by_key(|(k, _)| *k);

    let pkt = if is_mini {
        inventory_show(&grid, inv.selected, all)
    } else {
        inventory_full(&grid, inv.selected)
    };
    send_u_packet(tx, pkt.0, &pkt.1);
}

#[cfg(test)]
mod tests {
    use super::send_inventory;
    use crate::game::player::PlayerInventory;
    use bytes::BytesMut;
    use std::collections::HashMap;

    fn sent_payload(mut inv: PlayerInventory) -> Vec<u8> {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        send_inventory(&tx, &mut inv);
        let frame = rx.try_recv().expect("inventory frame");
        let mut buf = BytesMut::from(&frame[..]);
        let packet = crate::protocol::Packet::try_decode(&mut buf)
            .expect("valid frame")
            .expect("decoded frame");
        assert_eq!(packet.event_str(), "IN");
        packet.payload.to_vec()
    }

    #[test]
    fn mini_inventory_uses_show_with_total_count() {
        let inv = PlayerInventory {
            items: HashMap::from([(1, 5), (2, 3), (3, 1), (4, 2), (5, 9)]),
            selected: 2,
            minv: true,
            miniq: vec![2, 5],
        };

        assert_eq!(sent_payload(inv), b"show:5:2:1#5#2#3#3#1#5#9");
    }

    #[test]
    fn full_inventory_uses_full_without_padding() {
        let inv = PlayerInventory {
            items: HashMap::from([(1, 5), (2, 3), (3, 1), (4, 2)]),
            selected: -1,
            minv: false,
            miniq: vec![1, 2],
        };

        assert_eq!(sent_payload(inv), b"full:-1:1#5#2#3#3#1#4#2");
    }
}
