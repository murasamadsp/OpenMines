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
/// Примечание: Для обхода бага парсера клиента на событие "show" (пытается парсить
/// строку грида как число price), сервер всегда шлёт пакеты в формате "full".
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
    // Сортируем по id ВСЕГДА (и mini, и full). Клиент рендерит сетку строго в
    // порядке payload (InventoryPanel.makeGrid), поэтому несовпадение порядка
    // mini (порядок miniq) и full (по ключу) приводит к «скачкам» предметов при
    // сворачивании/разворачивании. Стабильный порядок по id убирает скачки.
    grid.sort_by_key(|(k, _)| *k);

    // Если у игрока в сумме 5 или более предметов, мы дополняем сетку
    // пустыми слотами (-1, 0) до 5 предметов, чтобы на клиенте кнопка
    // сворачивания/разворачивания была активна (требует не менее 10 элементов в сплите #).
    if all >= 5 {
        while grid.len() < 5 {
            grid.push((-1, 0));
        }
    }

    send_u_packet(tx, "IN", &inventory_full(&grid, inv.selected).1);
}
