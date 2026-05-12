-- Playground action: one live insert + one live update per invocation,
-- used by the backfill-plus-live scenario (repeat=50) so the backfill
-- sees meaningful churn while the snapshot scan is in progress.

INSERT INTO orders (customer_id, product_id, status, total_cents, notes, internal_tag)
SELECT
    (SELECT id FROM customers ORDER BY random() LIMIT 1),
    (SELECT id FROM products  ORDER BY random() LIMIT 1),
    'pending',
    (random() * 30000 + 200)::integer,
    'live-tick @ ' || now()::text,
    'live';

UPDATE orders
SET status = 'shipped', notes = coalesce(notes, '') || ' [live]'
WHERE id = (SELECT id FROM orders WHERE status = 'pending' ORDER BY id DESC LIMIT 1);
