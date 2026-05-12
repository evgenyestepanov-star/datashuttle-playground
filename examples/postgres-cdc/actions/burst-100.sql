-- Playground action: burst 100 inserts in a single statement so the Arrow
-- Flight hot buffer shows an obvious spike while the Iceberg commit loop
-- batches them into one snapshot.

INSERT INTO orders (customer_id, product_id, status, total_cents, notes, internal_tag)
SELECT
    (SELECT id FROM customers ORDER BY random() LIMIT 1),
    (SELECT id FROM products  ORDER BY random() LIMIT 1),
    (ARRAY['pending','shipped','delivered','returned'])[1 + floor(random()*4)::int],
    (random() * 80000 + 500)::integer,
    'burst-100 #' || g || ' @ ' || now()::text,
    'playground-burst'
FROM generate_series(1, 100) AS g;
