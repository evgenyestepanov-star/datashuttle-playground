-- Playground action: attach 3 line items to the most-recent order.
-- Demonstrates fan-out where one INSERT into the source produces 3
-- rows in warehouse.{namespace}.order_items downstream (branch 4 of 5).
WITH latest_order AS (
    SELECT id, product_id FROM orders ORDER BY id DESC LIMIT 1
)
INSERT INTO order_items (order_id, product_id, quantity, unit_price)
SELECT
    (SELECT id FROM latest_order),
    p.id,
    (random() * 5 + 1)::int,
    round((random() * 100 + 5)::numeric, 2)
FROM products p
ORDER BY random()
LIMIT 3
RETURNING order_id, product_id, quantity, unit_price;
