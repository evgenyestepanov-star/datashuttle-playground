-- Playground action: seed a new customer row. The CDC shuttle picks
-- this up and writes a corresponding row to warehouse.{namespace}.customers
-- (fan-out branch 1 of 5).
INSERT INTO customers (first_name, last_name, email, phone, city, country, segment)
VALUES (
    'PG-' || substr(md5(random()::text), 1, 6),
    'Demo',
    'pg-' || substr(md5(random()::text), 1, 8) || '@playground.local',
    '+1-555-' || lpad((random() * 9999)::int::text, 4, '0'),
    (ARRAY['NYC', 'SF', 'AUS', 'LON', 'TYO'])[(random() * 4 + 1)::int],
    'US',
    'playground'
)
RETURNING id, first_name, last_name, email, segment;
