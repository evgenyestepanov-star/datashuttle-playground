-- Playground action: delete the oldest order to exercise CDC delete events.

DELETE FROM orders
WHERE id = (SELECT id FROM orders ORDER BY id ASC LIMIT 1)
RETURNING id;
