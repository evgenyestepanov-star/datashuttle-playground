-- Playground action: insert a single order with realistic column values.
-- Idempotent across re-runs because the order id is generated server-side.

INSERT INTO orders (customer_id, product_id, status, total_cents, notes, internal_tag)
SELECT
    (SELECT id FROM customers ORDER BY random() LIMIT 1),
    (SELECT id FROM products ORDER BY random() LIMIT 1),
    'pending',
    (random() * 50000 + 500)::integer,
    'playground insert @ ' || now()::text,
    'playground'
RETURNING id, customer_id, product_id, status, total_cents;
