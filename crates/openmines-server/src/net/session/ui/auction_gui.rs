//! Аукцион — GUI-флоу (1:1 C# `MarketSystem`). Stateless-редизайн stateful C#
//! `Window`/`History`: action-строки самодостаточны (id встроены), т.к. клиент
//! эхо-ит action назад дословно — содержимое action серверное (НЕ wire-frozen);
//! frozen — только horb-поля (`inv`/`card`/`list`/`buttons`/`input`). Клик
//! item-грида клиент хардкодит как `choose:{id}` (`PopupManager` `InvButton`).
//!
//! Окно держим `market:{x}:{y}:auc` на всех страницах — координаты сохраняются
//! для табов и навигации (кнопка «НАЗАД» = action `auc`/`choose:{item}`).
use crate::game::player::{PlayerFlags, PlayerInventory, PlayerStats, PlayerUI};
use crate::net::session::outbound::inventory_sync::send_inventory;
use crate::net::session::prelude::*;
use crate::net::session::ui::gui_buttons::{market_tabs, resolve_market_window};
use crate::net::session::ui::horb::{Button, Horb, ListRow};
use crate::tasks::auction::{credit_money, now_unix};

use crate::game::logic::items::item_name as pack_name;

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

fn auc_window_tag(bx: i32, by: i32) -> String {
    format!("market:{bx}:{by}:auc")
}

fn auc_page(title: impl Into<String>) -> Horb {
    market_tabs("auc")
        .into_iter()
        .fold(Horb::new(title), Horb::tab)
}

fn send_auc(page: &Horb, state: &Arc<GameState>, tx: &Outbox, pid: PlayerId, bx: i32, by: i32) {
    page.send(state, tx, pid, auc_window_tag(bx, by));
}

fn send_auc_error(tx: &Outbox, message: &str) {
    send_u_packet(tx, "OK", &ok_message("МАРКЕТ", message).1);
}

fn send_auc_state_error(tx: &Outbox) {
    send_auc_error(tx, "Данные игрока недоступны.");
}

/// `MarketSystem.GlobalFirstPage`/`Items` — item-грид (51 тип, кроме 49=Money)
/// с числом ордеров и мин. ценой. Клик → `choose:{i}`.
pub async fn open_auc_grid(state: &Arc<GameState>, tx: &Outbox, pid: PlayerId, bx: i32, by: i32) {
    let counts = match state.db.order_counts_by_item().await {
        Ok(counts) => counts,
        Err(e) => {
            tracing::error!(player_id = %pid, error = ?e, "Failed to load auction item grid");
            send_auc_error(tx, "Не удалось загрузить список ордеров.");
            return;
        }
    };
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

    let page = auc_page("МАРКЕТ").inventory(inv).close_button();
    send_auc(&page, state, tx, pid, bx, by);
}

/// `MarketSystem.OpenItemAuc` — список ордеров по типу + «Создать Ордер».
pub async fn open_item_auc(state: &Arc<GameState>, tx: &Outbox, pid: PlayerId, item: i32) {
    let Some((bx, by, _)) = resolve_market_window(state, pid) else {
        return;
    };
    let orders = match state.db.list_orders_by_item(item).await {
        Ok(orders) => orders,
        Err(e) => {
            tracing::error!(player_id = %pid, item_id = item, error = ?e, "Failed to load auction item orders");
            send_auc_error(tx, "Не удалось загрузить ордера.");
            return;
        }
    };
    // list = тройки [label, btnLabel, action] (1:1 C# GetItems, сорт по cost).
    let mut page =
        auc_page(format!("Auc {}", pack_name(item))).card(format!("i{item}:{}", pack_name(item)));
    for o in &orders {
        let display = min_bid(o.cost, o.buyer_id > 0);
        page = page.list_row(ListRow::new(
            format!("{} x{}", pack_name(o.item_id), o.num),
            format!("<color=#aaeeaa>{display}$</color>"),
            format!("openorder:{}", o.id),
        ));
    }
    let page = page
        .button(Button::new("Создать Ордер", format!("auccreate:{item}")))
        .button(Button::new("НАЗАД", "auc"))
        .close_button();
    send_auc(&page, state, tx, pid, bx, by);
}

/// `MarketSystem.OpenOrder` — деталь ордера: карточка + ставка.
pub async fn open_order(state: &Arc<GameState>, tx: &Outbox, pid: PlayerId, order_id: i32) {
    let Some((bx, by, _)) = resolve_market_window(state, pid) else {
        return;
    };
    let o = match state.db.get_order(order_id).await {
        Ok(Some(order)) => order,
        Ok(None) => {
            send_auc_error(tx, "Ордер не найден.");
            return;
        }
        Err(e) => {
            tracing::error!(player_id = %pid, order_id, error = ?e, "Failed to load auction order");
            send_auc_error(tx, "Не удалось загрузить ордер.");
            return;
        }
    };
    let has_buyer = o.buyer_id > 0;
    let min = min_bid(o.cost, has_buyer);
    let timer = if has_buyer {
        let left = (300 - (now_unix() - o.bet_time)).max(0);
        format!("(time till ends {:02}:{:02})", left / 60, left % 60)
    } else {
        String::new()
    };
    let buyer_name = if !has_buyer {
        None
    } else {
        match state.db.get_player_by_id(o.buyer_id).await {
            Ok(Some(player)) => Some(player.name),
            Ok(None) => {
                tracing::error!(
                    player_id = %pid,
                    order_id,
                    buyer_id = o.buyer_id,
                    "Auction order references missing buyer"
                );
                send_auc_error(tx, "Данные ордера повреждены.");
                return;
            }
            Err(e) => {
                tracing::error!(player_id = %pid, order_id, buyer_id = o.buyer_id, error = ?e, "Failed to load auction buyer");
                send_auc_error(tx, "Не удалось загрузить ордер.");
                return;
            }
        }
    };

    let mut page = auc_page(format!("Order {timer}"))
        .card(format!(
            "i{}:{} x{} costs <color=#aaeeaa>{}$</color>",
            o.item_id,
            pack_name(o.item_id),
            o.num,
            o.cost
        ))
        .input(
            format!("minimal bet is <color=#aaeeaa>{min}$</color>"),
            false,
        )
        .button(Button::new("minimalbet", format!("aucminbet:{order_id}")))
        .button(Button::new("bet", format!("aucbet:{order_id}:%I%")))
        .button(Button::new("НАЗАД", format!("choose:{}", o.item_id)))
        .close_button();
    if let Some(name) = buyer_name {
        page = page
            .text("Last bet")
            .list_row(ListRow::new(format!("by: {name}"), "", ""));
    }
    send_auc(&page, state, tx, pid, bx, by);
}

/// `MarketSystem.OpenOrderCreation` — ввод стартовой цены.
pub fn open_order_creation(state: &Arc<GameState>, tx: &Outbox, pid: PlayerId, item: i32) {
    let Some((bx, by, _)) = resolve_market_window(state, pid) else {
        return;
    };
    let page = auc_page(format!("Order creation {}", pack_name(item)))
        .card(format!("i{item}:{}", pack_name(item)))
        .text("Enter cost")
        .input("cost", false)
        .button(Button::new("createorder", format!("aucsetcost:{item}:%I%")))
        .button(Button::new("НАЗАД", format!("choose:{item}")))
        .close_button();
    send_auc(&page, state, tx, pid, bx, by);
}

/// `MarketSystem.OrderCreationNum` — ввод количества (цена уже выбрана).
pub fn open_order_creation_num(
    state: &Arc<GameState>,
    tx: &Outbox,
    pid: PlayerId,
    item: i32,
    cost: i64,
) {
    let Some((bx, by, _)) = resolve_market_window(state, pid) else {
        return;
    };
    let page = auc_page(format!("Order creation {}", pack_name(item)))
        .card(format!("i{item}:{}", pack_name(item)))
        .text("Enter count")
        .input("num", false)
        .button(Button::new(
            "createorder",
            format!("aucsetnum:{item}:{cost}:%I%"),
        ))
        .button(Button::new("НАЗАД", format!("auccreate:{item}")))
        .close_button();
    send_auc(&page, state, tx, pid, bx, by);
}

/// `MarketSystem.CreateOrder` — списать предметы, создать ордер, подтвердить.
pub async fn create_order(
    state: &Arc<GameState>,
    tx: &Outbox,
    pid: PlayerId,
    item: i32,
    num: i32,
    cost: i64,
) {
    // C#: если предметов < num или num<=0 → закрыть окно, выйти.
    let ok = state
        .modify_player(pid, |ecs, e| {
            if ecs.get::<PlayerFlags>(e).is_none() {
                tracing::error!(player_id = %pid, component = "PlayerFlags", "Player component missing for auction order creation");
                send_auc_state_error(tx);
                return None;
            }
            let Some(mut inv) = ecs.get_mut::<PlayerInventory>(e) else {
                tracing::error!(player_id = %pid, component = "PlayerInventory", "Player component missing for auction order creation");
                send_auc_state_error(tx);
                return None;
            };
            let have = inv.items.get(&item).copied().unwrap_or(0);
            if num <= 0 || have < num {
                return Some(false);
            }
            *inv.items.entry(item).or_insert(0) -= num;
            let Some(mut f) = ecs.get_mut::<PlayerFlags>(e) else {
                tracing::error!(player_id = %pid, component = "PlayerFlags", "Player component missing while applying auction order creation");
                send_auc_state_error(tx);
                return None;
            };
            f.dirty = true;
            Some(true)
        })
        .flatten();

    let Some(ok) = ok else {
        return;
    };
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
        let Some(mut inv) = ecs.get_mut::<PlayerInventory>(e) else {
            tracing::error!(player_id = %pid, component = "PlayerInventory", "Player component missing after auction order item deduction");
            send_auc_state_error(tx);
            return None;
        };
        send_inventory(tx, &mut inv);
        Some(())
    });
    if let Err(e) = state.db.create_order(pid.into(), item, num, cost).await {
        tracing::error!(error = ?e, "Failed to create auction order");
        // Refund: undo the item deduction so the player doesn't lose their items.
        let refunded = state.modify_player(pid, |ecs, e| {
            let Some(mut inv) = ecs.get_mut::<PlayerInventory>(e) else {
                tracing::error!(player_id = %pid, component = "PlayerInventory", "Player component missing while refunding failed auction order creation");
                return None;
            };
            *inv.items.entry(item).or_insert(0) += num;
            send_inventory(tx, &mut inv);
            let Some(mut f) = ecs.get_mut::<PlayerFlags>(e) else {
                tracing::error!(player_id = %pid, component = "PlayerFlags", "Player component missing while refunding failed auction order creation");
                return None;
            };
            f.dirty = true;
            Some(())
        });
        if refunded.is_none() {
            send_auc_state_error(tx);
            return;
        }
        send_auc_error(tx, "Не удалось создать ордер.");
        return;
    }

    let Some((bx, by, _)) = resolve_market_window(state, pid) else {
        return;
    };
    let page = auc_page("ok")
        .text("u just created order u can cancel it within five mins after first bet")
        .button(Button::new("НАЗАД", "auc"))
        .close_button();
    send_auc(&page, state, tx, pid, bx, by);
}

/// Кнопка «minimalbet» — ставка ровно минимальной суммой (1:1 C#).
pub async fn place_minimal_bet(state: &Arc<GameState>, tx: &Outbox, pid: PlayerId, order_id: i32) {
    let o = match state.db.get_order(order_id).await {
        Ok(Some(order)) => order,
        Ok(None) => {
            send_auc_error(tx, "Ордер не найден.");
            return;
        }
        Err(e) => {
            tracing::error!(player_id = %pid, order_id, error = ?e, "Failed to load auction order for minimal bet");
            send_auc_error(tx, "Не удалось загрузить ордер.");
            return;
        }
    };
    let amount = min_bid(o.cost, o.buyer_id > 0);
    place_bet(state, tx, pid, order_id, amount).await;
}

fn apply_online_money_delta(
    state: &Arc<GameState>,
    pid: PlayerId,
    delta: i64,
) -> Option<(i64, i64)> {
    state
        .modify_player(pid, |ecs, e| {
            if ecs.get::<PlayerStats>(e).is_none() {
                tracing::error!(player_id = %pid, component = "PlayerStats", "Player component missing for auction money delta");
                return None;
            }
            if ecs.get::<PlayerFlags>(e).is_none() {
                tracing::error!(player_id = %pid, component = "PlayerFlags", "Player component missing for auction money delta");
                return None;
            }
            let mut s = ecs.get_mut::<PlayerStats>(e)?;
            s.money += delta;
            let pair = (s.money, s.creds);
            let mut f = ecs.get_mut::<PlayerFlags>(e)?;
            f.dirty = true;
            Some(pair)
        })
        .flatten()
}

async fn rollback_auction_bet(
    state: &Arc<GameState>,
    pid: PlayerId,
    order_id: i32,
    amount: i64,
    old_order: &crate::db::orders::OrderRow,
    reason: &str,
) {
    match state
        .db
        .try_update_order_bet_cas(
            order_id,
            old_order.cost,
            old_order.buyer_id,
            old_order.bet_time,
            pid.into(),
            amount,
        )
        .await
    {
        Ok(1) => {}
        Ok(rows) => {
            tracing::error!(
                player_id = %pid,
                order_id,
                affected_rows = rows,
                reason,
                "Auction bet rollback did not restore previous buyer"
            );
        }
        Err(e) => {
            tracing::error!(
                player_id = %pid,
                order_id,
                reason,
                error = ?e,
                "Auction bet rollback failed"
            );
        }
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
    tx: &Outbox,
    pid: PlayerId,
    order_id: i32,
    amount: i64,
) {
    let o = match state.db.get_order(order_id).await {
        Ok(Some(order)) => order,
        Ok(None) => {
            send_auc_error(tx, "Ордер не найден.");
            return;
        }
        Err(e) => {
            tracing::error!(player_id = %pid, order_id, error = ?e, "Failed to load auction order for bet");
            send_auc_error(tx, "Не удалось загрузить ордер.");
            return;
        }
    };
    let required = min_bid(o.cost, o.buyer_id > 0);
    let Some(bidder_money) =
        state.query_player_opt(pid, |ecs, e| ecs.get::<PlayerStats>(e).map(|s| s.money))
    else {
        tracing::error!(player_id = %pid, order_id, "Player stats missing for auction bet");
        send_auc_error(tx, "Данные игрока недоступны.");
        return;
    };
    // C# guard: required > amount ИЛИ bidder.money < amount → no-op.
    if required <= amount && bidder_money >= amount {
        let Some(charged_pair) = apply_online_money_delta(state, pid, -amount) else {
            tracing::error!(
                player_id = %pid,
                order_id,
                amount,
                "Auction bet charge failed before CAS update"
            );
            send_auc_error(tx, "Не удалось списать деньги за ставку.");
            return;
        };

        // CAS: обновляем ордер ТОЛЬКО если buyer_id и cost не изменились.
        // Если rows_affected==0 — другой игрок поставил раньше → просто
        // показать обновлённый ордер (без рефанда и списания).
        let won = match state
            .db
            .try_update_order_bet_cas(order_id, amount, pid.into(), now_unix(), o.buyer_id, o.cost)
            .await
        {
            Ok(rows) => rows,
            Err(e) => {
                tracing::error!(player_id = %pid, order_id, amount, error = ?e, "Failed to update auction bet");
                if apply_online_money_delta(state, pid, amount).is_none() {
                    tracing::error!(
                        player_id = %pid,
                        order_id,
                        amount,
                        "Auction bet charge rollback failed after CAS error"
                    );
                }
                send_auc_error(tx, "Не удалось сделать ставку.");
                return;
            }
        };
        if won > 0 {
            // Рефанд старому покупателю — только победитель CAS делает это.
            if o.buyer_id != 0 {
                if let Err(e) = credit_money(state, o.buyer_id.into(), o.cost).await {
                    tracing::error!(
                        player_id = %pid,
                        order_id,
                        old_buyer_id = o.buyer_id,
                        refund = o.cost,
                        error = ?e,
                        "Auction bet refund failed after CAS update"
                    );
                    rollback_auction_bet(
                        state,
                        pid,
                        order_id,
                        amount,
                        &o,
                        "old buyer refund failed",
                    )
                    .await;
                    if apply_online_money_delta(state, pid, amount).is_none() {
                        tracing::error!(
                            player_id = %pid,
                            order_id,
                            amount,
                            "Auction bet bidder refund failed after old buyer refund failure"
                        );
                    }
                    send_auc_error(tx, "Не удалось вернуть деньги предыдущему покупателю.");
                    return;
                }
            }
            send_u_packet(tx, "P$", &money(charged_pair.0, charged_pair.1).1);
        } else if apply_online_money_delta(state, pid, amount).is_none() {
            tracing::error!(
                player_id = %pid,
                order_id,
                amount,
                "Auction bet charge rollback failed after CAS race"
            );
        }
    }
    // C#: OpenOrder(p, orderid) после Bet в любом случае.
    open_order(state, tx, pid, order_id).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{ServerTestHarness, drain_events};
    use std::sync::Arc;

    async fn make_auction_test_state(label: &str) -> ServerTestHarness {
        ServerTestHarness::new(&format!("auction_{label}"), "auction-user").await
    }

    fn set_item_count(state: &Arc<GameState>, pid: PlayerId, item: i32, count: i32) {
        state.modify_player(pid, |ecs, entity| {
            let mut inv = ecs.get_mut::<PlayerInventory>(entity)?;
            inv.items.insert(item, count);
            Some(())
        });
    }

    fn item_count(state: &Arc<GameState>, pid: PlayerId, item: i32) -> i32 {
        state
            .query_player_opt(pid, |ecs, entity| {
                let inv = ecs.get::<PlayerInventory>(entity)?;
                Some(inv.items.get(&item).copied().unwrap_or(0))
            })
            .unwrap()
    }

    fn set_money(state: &Arc<GameState>, pid: PlayerId, amount: i64) {
        state.modify_player(pid, |ecs, entity| {
            let mut player_stats = ecs.get_mut::<PlayerStats>(entity)?;
            player_stats.money = amount;
            Some(())
        });
    }

    fn player_money(state: &Arc<GameState>, pid: PlayerId) -> i64 {
        state
            .query_player_opt(pid, |ecs, entity| {
                let player_stats = ecs.get::<PlayerStats>(entity)?;
                Some(player_stats.money)
            })
            .unwrap()
    }

    #[tokio::test]
    async fn create_order_missing_inventory_is_explicit_error_not_window_close_noop() {
        let test = make_auction_test_state("missing_inventory").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            ecs.entity_mut(entity).remove::<PlayerInventory>();
        }

        create_order(&test.state, &tx, pid, 5, 1, 100).await;

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        let message = std::str::from_utf8(&events[0].1).unwrap();
        assert!(message.contains("Данные игрока недоступны."));
    }

    #[tokio::test]
    async fn create_order_missing_flags_is_explicit_error_without_item_deduction() {
        let test = make_auction_test_state("missing_flags").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        set_item_count(&test.state, pid, 5, 2);
        let before_count = item_count(&test.state, pid, 5);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            ecs.entity_mut(entity).remove::<PlayerFlags>();
        }

        create_order(&test.state, &tx, pid, 5, 1, 100).await;

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        let message = std::str::from_utf8(&events[0].1).unwrap();
        assert!(message.contains("Данные игрока недоступны."));
        assert_eq!(item_count(&test.state, pid, 5), before_count);
    }

    #[tokio::test]
    async fn place_bet_missing_flags_is_explicit_error_without_money_mutation() {
        let test = make_auction_test_state("bet_missing_flags").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        set_money(&test.state, pid, 1_000);
        let before_money = player_money(&test.state, pid);
        let order_id = test.state.db.create_order(0, 5, 1, 100).await.unwrap();
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            ecs.entity_mut(entity).remove::<PlayerFlags>();
        }

        place_bet(&test.state, &tx, pid, order_id, 100).await;

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        let message = std::str::from_utf8(&events[0].1).unwrap();
        assert!(message.contains("Не удалось списать деньги за ставку."));
        assert_eq!(player_money(&test.state, pid), before_money);
    }
}
