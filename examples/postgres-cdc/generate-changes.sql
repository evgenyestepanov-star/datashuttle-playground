-- Generate realistic CDC changes for the e-commerce demo.
-- Run while the shuttle is active to see CDC in action:
--   psql -h localhost -U postgres -d ecommerce -f examples/postgres-cdc/generate-changes.sql

-- ── New customers ──────────────────────────────────────

INSERT INTO customers (first_name, last_name, email, phone, city, country, segment) VALUES
('Alice', 'Nakamura', 'alice.n@example.com', '+1-555-8001', 'Seattle', 'US', 'premium'),
('Boris', 'Petrov', 'boris.p@example.com', '+7-495-1234', 'Moscow', 'RU', 'enterprise'),
('Clara', 'Schmidt', 'clara.s@example.com', '+49-30-5678', 'Berlin', 'DE', 'standard');

-- ── New orders with items ──────────────────────────────

DO $$
DECLARE
    new_order_id INTEGER;
BEGIN
    -- Order 1: Alice buys a laptop and headphones
    INSERT INTO orders (customer_id, status, total, currency, notes)
    VALUES (
        (SELECT id FROM customers WHERE email = 'alice.n@example.com'),
        'confirmed', 1549.98, 'USD', 'Express shipping please'
    ) RETURNING id INTO new_order_id;

    INSERT INTO order_items (order_id, product_id, quantity, unit_price) VALUES
    (new_order_id, (SELECT id FROM products WHERE sku = 'LAPTOP-001'), 1, 1299.99),
    (new_order_id, (SELECT id FROM products WHERE sku = 'HEADPH-001'), 1, 249.99);

    INSERT INTO payments (order_id, method, amount, status)
    VALUES (new_order_id, 'credit_card', 1549.98, 'completed');

    -- Order 2: Boris buys monitors
    INSERT INTO orders (customer_id, status, total, currency)
    VALUES (
        (SELECT id FROM customers WHERE email = 'boris.p@example.com'),
        'pending', 1399.98, 'EUR'
    ) RETURNING id INTO new_order_id;

    INSERT INTO order_items (order_id, product_id, quantity, unit_price) VALUES
    (new_order_id, (SELECT id FROM products WHERE sku = 'MONITOR-002'), 2, 699.99);
END $$;

-- ── Update existing orders (status transitions) ───────

UPDATE orders SET status = 'shipped', updated_at = now()
WHERE status = 'confirmed' AND id IN (SELECT id FROM orders WHERE status = 'confirmed' LIMIT 50);

UPDATE orders SET status = 'delivered', updated_at = now()
WHERE status = 'shipped' AND id IN (SELECT id FROM orders WHERE status = 'shipped' LIMIT 30);

-- ── Update customer segments (promotions) ─────────────

UPDATE customers SET segment = 'premium', updated_at = now()
WHERE segment = 'standard' AND id IN (
    SELECT customer_id FROM orders GROUP BY customer_id HAVING sum(total) > 3000
);

-- ── Cancel some orders (DELETE from payments, UPDATE order status) ──

UPDATE orders SET status = 'cancelled', updated_at = now()
WHERE status = 'pending' AND created_at < now() - interval '60 days'
AND id IN (SELECT id FROM orders WHERE status = 'pending' AND created_at < now() - interval '60 days' LIMIT 20);

UPDATE payments SET status = 'refunded'
WHERE order_id IN (SELECT id FROM orders WHERE status = 'cancelled')
AND status = 'pending';

-- ── Price adjustments (product updates) ───────────────

UPDATE products SET price = price * 0.9
WHERE category = 'accessories' AND price > 30;

-- ── Deactivate discontinued products ──────────────────

UPDATE products SET is_active = false
WHERE sku IN ('GEN-090', 'GEN-091', 'GEN-092');

-- ── Report ────────────────────────────────────────────

SELECT 'Changes generated:' AS info;
SELECT 'New customers:  3' AS detail
UNION ALL SELECT 'New orders:     2'
UNION ALL SELECT 'Status updates: ~100'
UNION ALL SELECT 'Segment upgrades: varies'
UNION ALL SELECT 'Cancelled orders: ~20'
UNION ALL SELECT 'Price adjustments: accessories'
UNION ALL SELECT 'Deactivated products: 3';
