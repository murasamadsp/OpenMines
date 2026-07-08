CREATE INDEX IF NOT EXISTS idx_orders_ready
ON orders(bet_time, buyer_id)
WHERE buyer_id > 0 AND bet_time > 0;
