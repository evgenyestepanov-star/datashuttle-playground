-- Playground action: seed a new product row. Fan-out branch 2 of 5.
INSERT INTO products (sku, name, category, price, weight_kg, is_active)
VALUES (
    'SKU-' || substr(md5(random()::text), 1, 8),
    'Demo product ' || substr(md5(random()::text), 1, 5),
    (ARRAY['electronics', 'books', 'home', 'apparel', 'sports'])[(random() * 4 + 1)::int],
    round((random() * 500 + 5)::numeric, 2),
    round((random() * 10 + 0.1)::numeric, 2),
    true
)
RETURNING id, sku, name, category, price;
