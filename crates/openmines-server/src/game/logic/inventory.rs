use crate::game::{GameState, PlayerId};
use std::sync::Arc;

pub type PacketIntent = (&'static str, Vec<u8>);

#[derive(Debug, Eq, PartialEq)]
pub enum InventoryMutation {
    Packets(Vec<PacketIntent>),
    MissingState(&'static str),
    MissingEntity,
    RejectedPayload,
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

pub fn toggle_inventory(state: &Arc<GameState>, pid: PlayerId) -> InventoryMutation {
    state
        .modify_player(pid, |ecs, entity| {
            let Some(mut inv) = ecs.get_mut::<crate::game::player::PlayerInventory>(entity) else {
                return Some(InventoryMutation::MissingState("PlayerInventory"));
            };
            inv.minv = !inv.minv;
            Some(InventoryMutation::Packets(vec![inventory_packet(&mut inv)]))
        })
        .flatten()
        .unwrap_or(InventoryMutation::MissingEntity)
}

pub fn choose_inventory(
    state: &Arc<GameState>,
    pid: PlayerId,
    payload: &[u8],
) -> InventoryMutation {
    let Ok(s) = std::str::from_utf8(payload) else {
        return InventoryMutation::RejectedPayload;
    };
    let Ok(id) = s.parse::<i32>() else {
        return InventoryMutation::RejectedPayload;
    };
    state
        .modify_player(pid, |ecs, entity| {
            let Some(mut inv) = ecs.get_mut::<crate::game::player::PlayerInventory>(entity) else {
                return Some(InventoryMutation::MissingState("PlayerInventory"));
            };
            add_choose_miniq(&mut inv.miniq, id);
            inv.selected = id;
            let mut packets = vec![inventory_packet(&mut inv)];
            packets.push(if id == -1 {
                crate::protocol::packets::inventory_close()
            } else {
                crate::protocol::packets::inventory_choose()
            });
            Some(InventoryMutation::Packets(packets))
        })
        .flatten()
        .unwrap_or(InventoryMutation::MissingEntity)
}

pub fn inventory_packet(inv: &mut crate::game::player::PlayerInventory) -> PacketIntent {
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
    // Клиент рендерит сетку строго в порядке payload.
    grid.sort_by_key(|(k, _)| *k);

    if is_mini {
        crate::protocol::packets::inventory_show(&grid, inv.selected, all)
    } else {
        crate::protocol::packets::inventory_full(&grid, inv.selected)
    }
}

fn inventory_nonzero_count(items: &std::collections::HashMap<i32, i32>) -> i32 {
    i32::try_from(items.values().filter(|v| **v > 0).count()).unwrap_or(i32::MAX)
}

fn prefill_miniq_if_needed(inv: &mut crate::game::player::PlayerInventory) {
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

#[cfg(test)]
mod tests {
    use crate::game::player::PlayerInventory;
    use std::collections::HashMap;

    #[test]
    fn mini_inventory_uses_show_with_total_count() {
        let mut inv = PlayerInventory {
            items: HashMap::from([(1, 5), (2, 3), (3, 1), (4, 2), (5, 9)]),
            selected: 2,
            minv: true,
            miniq: vec![2, 5],
        };

        let (event, payload) = super::inventory_packet(&mut inv);

        assert_eq!(event, "IN");
        assert_eq!(payload, b"show:5:2:1#5#2#3#3#1#5#9");
    }

    #[test]
    fn full_inventory_uses_full_without_padding() {
        let mut inv = PlayerInventory {
            items: HashMap::from([(1, 5), (2, 3), (3, 1), (4, 2)]),
            selected: -1,
            minv: false,
            miniq: vec![1, 2],
        };

        let (event, payload) = super::inventory_packet(&mut inv);

        assert_eq!(event, "IN");
        assert_eq!(payload, b"full:-1:1#5#2#3#3#1#4#2");
    }

    #[test]
    fn add_choose_miniq_is_csharp_style_recent_four_unique_non_close_ids() {
        let mut miniq = vec![1, 2, 3, 4];

        super::add_choose_miniq(&mut miniq, 3);
        super::add_choose_miniq(&mut miniq, -1);
        super::add_choose_miniq(&mut miniq, 5);

        assert_eq!(miniq, vec![2, 3, 4, 5]);
    }
}
