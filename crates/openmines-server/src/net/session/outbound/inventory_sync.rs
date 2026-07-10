use crate::game::player::PlayerInventory;
use crate::net::session::prelude::*;

/// Как `Inventory.InvToSend` → `InventoryShowPacket` (`Inventory.cs`).
pub fn send_inventory(tx: &dyn PacketSink, inv: &mut PlayerInventory) {
    let pkt = crate::game::logic::inventory::inventory_packet(inv);
    send_u_packet(tx, pkt.0, &pkt.1);
}

#[cfg(test)]
mod tests {
    use super::send_inventory;
    use crate::game::player::PlayerInventory;
    use bytes::BytesMut;
    use std::collections::HashMap;

    fn sent_payload(mut inv: PlayerInventory) -> Vec<u8> {
        let (tx, mut rx) = crate::net::session::outbox::channel();
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
