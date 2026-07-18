-- Аукцион (1:1 C# `Sys_Market/Order.cs`). Лот: инициатор выставляет itemid×num
-- за стартовую cost; покупатели перебивают ставку (buyer_id/cost), через 5 мин
-- после последней ставки лот финализируется (см. Order.CheckReady).
-- bet_time — unix-секунды последней ставки (0 = ставок ещё не было).
CREATE TABLE IF NOT EXISTS orders (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    initiator_id INTEGER NOT NULL,
    item_id INTEGER NOT NULL,
    num INTEGER NOT NULL,
    cost INTEGER NOT NULL,
    buyer_id INTEGER NOT NULL DEFAULT 0,
    bet_time INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_orders_item ON orders(item_id);
