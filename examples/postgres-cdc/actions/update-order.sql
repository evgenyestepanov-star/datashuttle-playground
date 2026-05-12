-- Playground action: advance the status of the most recent order.

UPDATE orders
SET status = CASE status
                 WHEN 'pending'   THEN 'shipped'
                 WHEN 'shipped'   THEN 'delivered'
                 WHEN 'delivered' THEN 'returned'
                 ELSE 'pending'
             END,
    notes = coalesce(notes, '') || E'\nstatus advanced @ ' || now()::text
WHERE id = (SELECT id FROM orders ORDER BY id DESC LIMIT 1)
RETURNING id, status;
