-- Playground action: delete the oldest order to exercise CDC delete events.
-- order_items references orders via FK without ON DELETE CASCADE, so the
-- child rows must go first. Wrapping in a CTE keeps the parent's id
-- column identifying the deleted row for the RETURNING clause.

WITH victim AS (
  SELECT id FROM orders ORDER BY id ASC LIMIT 1
), purged_items AS (
  DELETE FROM order_items WHERE order_id IN (SELECT id FROM victim)
), purged_payments AS (
  DELETE FROM payments WHERE order_id IN (SELECT id FROM victim)
)
DELETE FROM orders WHERE id IN (SELECT id FROM victim)
RETURNING id;
