//! Аукцион — GUI-флоу (1:1 C# `MarketSystem`). Stateless-редизайн stateful C#
//! `Window`/`History`: action-строки самодостаточны (id встроены), т.к. клиент
//! эхо-ит action назад дословно — содержимое action серверное (НЕ wire-frozen);
//! frozen — только horb-поля (`inv`/`card`/`list`/`buttons`/`input`). Клик
//! item-грида клиент хардкодит как `choose:{id}` (`PopupManager` `InvButton`).
//!
//! Окно держим `market:{x}:{y}:auc` на всех страницах — координаты сохраняются
//! для табов и навигации (кнопка «НАЗАД» = action `auc`/`choose:{item}`).
use crate::game::player::{PlayerFlags, PlayerInventory, PlayerStats, PlayerUI};
use crate::net::auction::{credit_money, now_unix};
use crate::net::session::outbound::inventory_sync::send_inventory;
use crate::net::session::prelude::*;
use crate::net::session::ui::gui_buttons::{build_market_tabs, resolve_market_window};

/// C# `MarketSystem.PackName` — имена 51 типа предмета (индекс = `item_id`).
const PACK_NAMES: [&str; 51] = [
    "TP",
    "Resp",
    "UP",
    "Market",
    "Clans",
    "boom",
    "prot",
    "raz",
    "Cred",
    "Rembot",
    "geopack",
    "CyanAlive",
    "RedAlive",
    "VioletAlive",
    "BlackAlive",
    "WhiteAlive",
    "BlueAlive",
    "VulcRadar",
    "AliveRadar",
    "BotRadar",
    "TPR",
    "Konstr Bot",
    "Boy gay",
    "Zalupa Zalupa",
    "Crafter",
    "BoomShop",
    "Gun",
    "Gate",
    "Dizz",
    "Storage",
    "PackRadar",
    "x3 up",
    "freeup",
    "mine x4",
    "Gypno",
    "poli",
    "nano bot",
    "accum",
    "transgender",
    "Comp",
    "c190",
    "Fed",
    "BlackRock",
    "RedRock",
    "AntiMage",
    "EMO",
    "RainbowAlive",
    "spot",
    "NC",
    "Money",
    "Оперативные Порно Покемоны.",
];

fn pack_name(i: i32) -> &'static str {
    usize::try_from(i)
        .ok()
        .and_then(|u| PACK_NAMES.get(u))
        .copied()
        .unwrap_or("")
}

/// Минимальная ставка: `buyer>0 ? ceil(cost*1.01) : cost` (1:1 C#).
fn min_bid(cost: i64, has_buyer: bool) -> i64 {
    if has_buyer {
        #[allow(
            clippy::cast_precision_loss,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss
        )]
        {
            (cost as f64 * 1.01).ceil() as i64
        }
    } else {
        cost
    }
}

fn send_horb(tx: &mpsc::UnboundedSender<Vec<u8>>, gui: &serde_json::Value) {
    send_u_packet(tx, "GU", format!("horb:{gui}").as_bytes());
}

fn set_auc_window(state: &Arc<GameState>, pid: PlayerId, bx: i32, by: i32) {
    state.modify_player(pid, |ecs, e| {
        if let Some(mut ui) = ecs.get_mut::<PlayerUI>(e) {
            ui.current_window = Some(format!("market:{bx}:{by}:auc"));
        }
        Some(())
    });
}

/// `MarketSystem.GlobalFirstPage`/`Items` — item-грид (51 тип, кроме 49=Money)
/// с числом ордеров и мин. ценой. Клик → `choose:{i}`.
pub async fn open_auc_grid(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    bx: i32,
    by: i32,
) {
    let counts = state.db.order_counts_by_item().await.unwrap_or_default();
    // item_id → (count, min_cost)
    let mut by_item: std::collections::HashMap<i32, (i64, i64)> = std::collections::HashMap::new();
    for (item, cnt, min_cost) in counts {
        by_item.insert(item, (cnt, min_cost));
    }
    // inv = "id: {up};!{down}" на каждый предмет, склеено ':' (1:1 C# InventoryItem).
    let inv = (0..51)
        .filter(|&i| i != 49)
        .map(|i| {
            let (cnt, cost) = by_item.get(&i).copied().unwrap_or((0, 0));
            let up = if cnt > 0 {
                cnt.to_string()
            } else {
                String::new()
            };
            let down = if cnt > 0 {
                format!("{cost}$")
            } else {
                String::new()
            };
            format!("{i}: {up};!{down}")
        })
        .collect::<Vec<_>>()
        .join(":");

    let gui = serde_json::json!({
        "title": "МАРКЕТ",
        "tabs": build_market_tabs("auc"),
        "inv": inv,
        "buttons": ["ВЫЙТИ", "exit"],
        "back": false
    });
    send_horb(tx, &gui);
    set_auc_window(state, pid, bx, by);
}

/// `MarketSystem.OpenItemAuc` — список ордеров по типу + «Создать Ордер».
pub async fn open_item_auc(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    item: i32,
) {
    let Some((bx, by, _)) = resolve_market_window(state, pid) else {
        return;
    };
    let orders = state.db.list_orders_by_item(item).await.unwrap_or_default();
    // list = тройки [label, btnLabel, action] (1:1 C# GetItems, сорт по cost).
    let mut list: Vec<serde_json::Value> = Vec::new();
    for o in &orders {
        let display = min_bid(o.cost, o.buyer_id > 0);
        list.push(serde_json::json!(format!(
            "{} x{}",
            pack_name(o.item_id),
            o.num
        )));
        list.push(serde_json::json!(format!(
            "<color=#aaeeaa>{display}$</color>"
        )));
        list.push(serde_json::json!(format!("openorder:{}", o.id)));
    }

    let gui = serde_json::json!({
        "title": format!("Auc {}", pack_name(item)),
        "tabs": build_market_tabs("auc"),
        "card": format!("i{item}:{}", pack_name(item)),
        "list": list,
        "buttons": ["Создать Ордер", format!("auccreate:{item}"), "НАЗАД", "auc", "ВЫЙТИ", "exit"],
        "back": false
    });
    send_horb(tx, &gui);
    set_auc_window(state, pid, bx, by);
}

/// `MarketSystem.OpenOrder` — деталь ордера: карточка + ставка.
pub async fn open_order(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    order_id: i32,
) {
    let Some((bx, by, _)) = resolve_market_window(state, pid) else {
        return;
    };
    let Ok(Some(o)) = state.db.get_order(order_id).await else {
        return;
    };
    let has_buyer = o.buyer_id > 0;
    let min = min_bid(o.cost, has_buyer);
    let timer = if has_buyer {
        let left = (300 - (now_unix() - o.bet_time)).max(0);
        format!("(time till ends {:02}:{:02})", left / 60, left % 60)
    } else {
        String::new()
    };
    let buyer_name = if has_buyer {
        state
            .db
            .get_player_by_id(o.buyer_id)
            .await
            .ok()
            .flatten()
            .map(|p| p.name)
    } else {
        None
    };

    let mut gui = serde_json::json!({
        "title": format!("Order {timer}"),
        "tabs": build_market_tabs("auc"),
        "card": format!("i{}:{} x{} costs <color=#aaeeaa>{}$</color>", o.item_id, pack_name(o.item_id), o.num, o.cost),
        "input_place": format!("minimal bet is <color=#aaeeaa>{min}$</color>"),
        "buttons": [
            "minimalbet", format!("aucminbet:{order_id}"),
            "bet", format!("aucbet:{order_id}:%I%"),
            "НАЗАД", format!("choose:{}", o.item_id),
            "ВЫЙТИ", "exit"
        ],
        "back": false
    });
    if let Some(name) = buyer_name {
        gui["text"] = serde_json::json!(format!("last bet by: {name}"));
    }
    send_horb(tx, &gui);
    set_auc_window(state, pid, bx, by);
}

/// `MarketSystem.OpenOrderCreation` — ввод стартовой цены.
pub fn open_order_creation(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    item: i32,
) {
    let Some((bx, by, _)) = resolve_market_window(state, pid) else {
        return;
    };
    let gui = serde_json::json!({
        "title": format!("Order creation {}", pack_name(item)),
        "tabs": build_market_tabs("auc"),
        "card": format!("i{item}:{}", pack_name(item)),
        "text": "Enter cost",
        "input_place": "cost",
        "buttons": ["createorder", format!("aucsetcost:{item}:%I%"), "НАЗАД", format!("choose:{item}"), "ВЫЙТИ", "exit"],
        "back": false
    });
    send_horb(tx, &gui);
    set_auc_window(state, pid, bx, by);
}

/// `MarketSystem.OrderCreationNum` — ввод количества (цена уже выбрана).
pub fn open_order_creation_num(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    item: i32,
    cost: i64,
) {
    let Some((bx, by, _)) = resolve_market_window(state, pid) else {
        return;
    };
    let gui = serde_json::json!({
        "title": format!("Order creation {}", pack_name(item)),
        "tabs": build_market_tabs("auc"),
        "card": format!("i{item}:{}", pack_name(item)),
        "text": format!("{} to sell count", pack_name(item)),
        "input_place": "num",
        "buttons": ["createorder", format!("aucsetnum:{item}:{cost}:%I%"), "НАЗАД", format!("auccreate:{item}"), "ВЫЙТИ", "exit"],
        "back": false
    });
    send_horb(tx, &gui);
    set_auc_window(state, pid, bx, by);
}

/// `MarketSystem.CreateOrder` — списать предметы, создать ордер, подтвердить.
pub async fn create_order(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    item: i32,
    num: i32,
    cost: i64,
) {
    // C#: если предметов < num или num<=0 → закрыть окно, выйти.
    let ok = state
        .modify_player(pid, |ecs, e| {
            let mut inv = ecs.get_mut::<PlayerInventory>(e)?;
            let have = inv.items.get(&item).copied().unwrap_or(0);
            if num <= 0 || have < num {
                return Some(false);
            }
            *inv.items.entry(item).or_insert(0) -= num;
            if let Some(mut f) = ecs.get_mut::<PlayerFlags>(e) {
                f.dirty = true;
            }
            Some(true)
        })
        .flatten()
        .unwrap_or(false);

    if !ok {
        state.modify_player(pid, |ecs, e| {
            if let Some(mut ui) = ecs.get_mut::<PlayerUI>(e) {
                ui.current_window = None;
            }
            Some(())
        });
        let g = gu_close();
        send_u_packet(tx, g.0, &g.1);
        return;
    }

    // обновить инвентарь у клиента
    state.modify_player(pid, |ecs, e| {
        let mut inv = ecs.get_mut::<PlayerInventory>(e)?;
        send_inventory(tx, &mut inv);
        Some(())
    });
    if let Err(e) = state.db.create_order(pid, item, num, cost).await {
        tracing::error!("auction: create_order failed: {e}");
        // Refund: undo the item deduction so the player doesn't lose their items.
        state.modify_player(pid, |ecs, e| {
            let mut inv = ecs.get_mut::<PlayerInventory>(e)?;
            *inv.items.entry(item).or_insert(0) += num;
            send_inventory(tx, &mut inv);
            if let Some(mut f) = ecs.get_mut::<PlayerFlags>(e) {
                f.dirty = true;
            }
            Some(())
        });
        return;
    }

    let Some((bx, by, _)) = resolve_market_window(state, pid) else {
        return;
    };
    let gui = serde_json::json!({
        "title": "ok",
        "tabs": build_market_tabs("auc"),
        "text": "u just created order u can cancel it within five mins after first bet",
        "buttons": ["НАЗАД", "auc", "ВЫЙТИ", "exit"],
        "back": false
    });
    send_horb(tx, &gui);
    set_auc_window(state, pid, bx, by);
}

/// Кнопка «minimalbet» — ставка ровно минимальной суммой (1:1 C#).
pub async fn place_minimal_bet(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    order_id: i32,
) {
    if let Ok(Some(o)) = state.db.get_order(order_id).await {
        let amount = min_bid(o.cost, o.buyer_id > 0);
        place_bet(state, tx, pid, order_id, amount).await;
    }
}

/// `Order.Bet` — ставка (1:1 C#): мин-проверка, рефанд старому покупателю,
/// списание у нового. Затем переоткрыть деталь ордера.
///
/// CAS-паттерн предотвращает двойной рефанд при одновременных ставках:
/// обновление БД атомарно проверяет что `buyer_id`/`cost` не изменились с момента
/// чтения — только один из конкурирующих запросов пройдёт CAS.
pub async fn place_bet(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    order_id: i32,
    amount: i64,
) {
    if let Ok(Some(o)) = state.db.get_order(order_id).await {
        let required = min_bid(o.cost, o.buyer_id > 0);
        let bidder_money = state
            .query_player_opt(pid, |ecs, e| ecs.get::<PlayerStats>(e).map(|s| s.money))
            .unwrap_or(0);
        // C# guard: required > amount ИЛИ bidder.money < amount → no-op.
        if required <= amount && bidder_money >= amount {
            // CAS: обновляем ордер ТОЛЬКО если buyer_id и cost не изменились.
            // Если rows_affected==0 — другой игрок поставил раньше → просто
            // показать обновлённый ордер (без рефанда и списания).
            let won = state
                .db
                .try_update_order_bet_cas(order_id, amount, pid, now_unix(), o.buyer_id, o.cost)
                .await
                .unwrap_or(0);
            if won > 0 {
                // Рефанд старому покупателю — только победитель CAS делает это.
                if o.buyer_id != 0 {
                    credit_money(state, o.buyer_id, o.cost).await;
                }
                // Списать у нового покупателя (он online — кликнул) + P$.
                state.modify_player(pid, |ecs, e| {
                    let pair = {
                        let mut s = ecs.get_mut::<PlayerStats>(e)?;
                        s.money -= amount;
                        (s.money, s.creds)
                    };
                    if let Some(mut f) = ecs.get_mut::<PlayerFlags>(e) {
                        f.dirty = true;
                    }
                    send_u_packet(tx, "P$", &money(pair.0, pair.1).1);
                    Some(())
                });
            }
        }
    }
    // C#: OpenOrder(p, orderid) после Bet в любом случае.
    open_order(state, tx, pid, order_id).await;
}
