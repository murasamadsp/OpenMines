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
    // Клиент рендерит сетку строго в порядке payload.
    grid.sort_by_key(|(k, _)| *k);

    if all >= 5 {
        grid.resize(grid.len().max(5), (-1, 0));
    }

    // НАМЕРЕННАЯ ДЕВИАЦИЯ от C# по ПРЯМОМУ ТРЕБОВАНИЮ ПОЛЬЗОВАТЕЛЯ:
    // Unity отвергает `show:` с непустой `k#v`-сеткой, поэтому оба режима используют `full:`.
    crate::protocol::packets::inventory_full(&grid, inv.selected)
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

    fn assert_unity_full_payload(payload: &[u8]) {
        let payload = std::str::from_utf8(payload).expect("inventory payload must be UTF-8");
        let mut fields = payload.split(':');
        assert_eq!(fields.next(), Some("full"));
        fields
            .next()
            .expect("selected field")
            .parse::<i32>()
            .expect("Unity parses selected as i32");
        let grid = fields.next().expect("grid field");
        assert!(
            fields.next().is_none(),
            "full payload has exactly three fields"
        );
        let grid_fields: Vec<&str> = grid.split('#').collect();
        assert_eq!(grid_fields.len() % 2, 0, "grid is key/value pairs");
        assert!(grid_fields.iter().all(|field| field.parse::<i32>().is_ok()));
    }

    #[test]
    fn mini_inventory_uses_client_compatible_full_with_toggle_padding() {
        let mut inv = PlayerInventory {
            items: HashMap::from([(1, 5), (2, 3), (3, 1), (4, 2), (5, 9)]),
            selected: 2,
            minv: true,
            miniq: vec![2, 5],
        };

        let (event, payload) = super::inventory_packet(&mut inv);

        assert_eq!(event, "IN");
        assert_eq!(payload, b"full:2:1#5#2#3#3#1#5#9#-1#0");
        assert_unity_full_payload(&payload);
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
        assert_unity_full_payload(&payload);
    }

    #[tokio::test]
    async fn inventory_toggle_emits_client_parseable_full_in_both_modes() {
        let test = crate::test_support::ServerTestHarness::new(
            "inventory_toggle_wire",
            "inventory-toggle-user",
        )
        .await;
        let mut receiver = test.connect(1);
        crate::test_support::ServerTestHarness::drain_events(&mut receiver);
        let player_id = crate::game::PlayerId(test.player.id);
        test.state
            .modify_player(player_id, |ecs, entity| {
                let mut inv = ecs.get_mut::<PlayerInventory>(entity)?;
                inv.items = HashMap::from([(1, 5), (2, 3), (3, 1), (4, 2), (5, 9)]);
                inv.selected = 2;
                inv.minv = true;
                inv.miniq = vec![2, 5];
                Some(())
            })
            .flatten()
            .expect("inventory state");

        for expected_minimized in [false, true] {
            let super::InventoryMutation::Packets(packets) =
                super::toggle_inventory(&test.state, player_id)
            else {
                panic!("inventory toggle must emit packets");
            };
            assert_eq!(packets.len(), 1);
            assert_eq!(packets[0].0, "IN");
            assert_unity_full_payload(&packets[0].1);
            let minimized = test
                .state
                .query_player_opt(player_id, |ecs, entity| {
                    ecs.get::<PlayerInventory>(entity).map(|inv| inv.minv)
                })
                .expect("inventory state");
            assert_eq!(minimized, expected_minimized);
        }
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
