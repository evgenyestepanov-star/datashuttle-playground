-- Playground action: insert a payment row for the most-recent order.
-- Fan-out branch 5 of 5 — completes the multi-table CDC story.
INSERT INTO payments (order_id, method, amount, status, processed_at)
SELECT
    (SELECT id FROM orders ORDER BY id DESC LIMIT 1),
    (ARRAY['card', 'wire', 'ach', 'paypal'])[(random() * 3 + 1)::int],
    round((random() * 500 + 5)::numeric, 2),
    'completed',
    now()
RETURNING order_id, method, amount, status;
