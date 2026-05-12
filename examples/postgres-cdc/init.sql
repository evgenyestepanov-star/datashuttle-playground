-- DataShuttle PostgreSQL CDC Demo: E-Commerce Schema
-- Initializes: customers, products, orders, order_items, payments
-- Designed for CDC demonstration: realistic data, foreign keys, varied types.

-- ── Schema ────────────────────────────────────────────

CREATE TABLE customers (
    id          SERIAL PRIMARY KEY,
    first_name  VARCHAR(50)  NOT NULL,
    last_name   VARCHAR(50)  NOT NULL,
    email       VARCHAR(120) NOT NULL UNIQUE,
    phone       VARCHAR(20),
    address     TEXT,
    city        VARCHAR(80),
    country     VARCHAR(3)   DEFAULT 'US',
    segment     VARCHAR(20)  DEFAULT 'standard',
    created_at  TIMESTAMPTZ  NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ  NOT NULL DEFAULT now()
);

CREATE TABLE products (
    id          SERIAL PRIMARY KEY,
    sku         VARCHAR(20)  NOT NULL UNIQUE,
    name        VARCHAR(200) NOT NULL,
    category    VARCHAR(50),
    price       NUMERIC(10,2) NOT NULL,
    weight_kg   NUMERIC(6,2),
    is_active   BOOLEAN DEFAULT true,
    created_at  TIMESTAMPTZ  NOT NULL DEFAULT now()
);

CREATE TABLE orders (
    id           SERIAL PRIMARY KEY,
    customer_id  INTEGER NOT NULL REFERENCES customers(id),
    -- product_id is a convenience denormalisation so the playground
    -- actions (insert-order, burst-100, live-tick) can insert a whole
    -- order in one statement. Real schemas still track per-line
    -- products via order_items; keeping it here means the CDC demo
    -- has a single-table happy path without disabling the relational
    -- integrity showcased by order_items.
    product_id   INTEGER REFERENCES products(id),
    status       VARCHAR(20) NOT NULL DEFAULT 'pending',
    total        NUMERIC(12,2) NOT NULL DEFAULT 0,
    -- total_cents mirrors `total` as an integer so actions can insert
    -- random ints without the NUMERIC cast boilerplate. Callers that
    -- only touch `total` keep working; callers that only touch
    -- `total_cents` also work.
    total_cents  INTEGER,
    currency     VARCHAR(3)   DEFAULT 'USD',
    notes        TEXT,
    -- Free-form tag stamped by playground actions so operators can
    -- filter their rows from the seed data (SELECT * WHERE internal_tag).
    internal_tag VARCHAR(50),
    created_at   TIMESTAMPTZ  NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ  NOT NULL DEFAULT now()
);

CREATE TABLE order_items (
    id          SERIAL PRIMARY KEY,
    order_id    INTEGER NOT NULL REFERENCES orders(id),
    product_id  INTEGER NOT NULL REFERENCES products(id),
    quantity    INTEGER NOT NULL DEFAULT 1,
    unit_price  NUMERIC(10,2) NOT NULL,
    discount    NUMERIC(5,2) DEFAULT 0,
    created_at  TIMESTAMPTZ  NOT NULL DEFAULT now()
);

CREATE TABLE payments (
    id          SERIAL PRIMARY KEY,
    order_id    INTEGER NOT NULL REFERENCES orders(id),
    method      VARCHAR(30) NOT NULL,
    amount      NUMERIC(12,2) NOT NULL,
    status      VARCHAR(20) DEFAULT 'pending',
    processed_at TIMESTAMPTZ,
    created_at  TIMESTAMPTZ  NOT NULL DEFAULT now()
);

-- ── Replication setup ─────────────────────────────────
--
-- Publications are CLUSTER-level objects (not schema-local). On the
-- shared cloud `postgres-playground` sidecar two concurrent sessions
-- would clobber each other on a fixed `datashuttle_pub` name, so we
-- splice the session's shuttle id into the publication name and
-- scope the publication to this session's schema only.
--
-- This file is the playground per-session template: the playground
-- session manager substitutes `{shuttle}` / `{namespace}` before
-- dispatching to the source via exec_source_sql. The docker-compose
-- entrypoint mounts a separate `docker-init.sql` (static names) so
-- psql's initdb step doesn't choke on unsubstituted placeholders.
--
-- The one-time legacy cleanup at the top guards against
-- `datashuttle_pub` rows left behind by earlier playground builds
-- before this rename landed — safe to remove once jarvis-cloud has
-- rolled past 2026-05.

DROP PUBLICATION IF EXISTS datashuttle_pub;
CREATE PUBLICATION "{shuttle}_pub" FOR TABLES IN SCHEMA "{namespace}";

-- ── Seed data: Products (100) ─────────────────────────

INSERT INTO products (sku, name, category, price, weight_kg) VALUES
('LAPTOP-001', 'ProBook 15', 'electronics', 1299.99, 2.1),
('LAPTOP-002', 'UltraSlim 14', 'electronics', 999.99, 1.4),
('LAPTOP-003', 'GameMaster X', 'electronics', 1899.99, 2.8),
('PHONE-001', 'SmartPhone Pro', 'electronics', 899.99, 0.19),
('PHONE-002', 'SmartPhone Lite', 'electronics', 499.99, 0.17),
('PHONE-003', 'SmartPhone Max', 'electronics', 1199.99, 0.21),
('TABLET-001', 'TabPro 12', 'electronics', 749.99, 0.58),
('TABLET-002', 'TabMini 8', 'electronics', 349.99, 0.32),
('HEADPH-001', 'NoiseFree Pro', 'audio', 299.99, 0.25),
('HEADPH-002', 'BassBoost X', 'audio', 149.99, 0.30),
('HEADPH-003', 'AirPods Ultra', 'audio', 249.99, 0.05),
('SPEAKER-001', 'SoundBar 5.1', 'audio', 599.99, 3.50),
('SPEAKER-002', 'PortaBoom Mini', 'audio', 79.99, 0.40),
('MONITOR-001', 'ClearView 27 4K', 'displays', 449.99, 5.20),
('MONITOR-002', 'UltraWide 34', 'displays', 699.99, 7.10),
('MONITOR-003', 'ProDisplay 32', 'displays', 1299.99, 8.30),
('KEYBOARD-001', 'MechKey Pro', 'peripherals', 129.99, 0.85),
('KEYBOARD-002', 'ErgoType Split', 'peripherals', 179.99, 0.95),
('MOUSE-001', 'PrecisionClick', 'peripherals', 69.99, 0.10),
('MOUSE-002', 'GameMouse RGB', 'peripherals', 89.99, 0.12),
('CABLE-001', 'USB-C Cable 2m', 'accessories', 14.99, 0.05),
('CABLE-002', 'HDMI Cable 3m', 'accessories', 19.99, 0.10),
('CASE-001', 'Laptop Sleeve 15', 'accessories', 39.99, 0.30),
('CASE-002', 'Phone Case Pro', 'accessories', 29.99, 0.05),
('CHARGER-001', 'Fast Charger 65W', 'accessories', 49.99, 0.15);

-- Fill to 100 products
INSERT INTO products (sku, name, category, price, weight_kg)
SELECT
    'GEN-' || LPAD(i::text, 3, '0'),
    (ARRAY['Widget', 'Gadget', 'Doohickey', 'Thingamajig', 'Gizmo'])[1 + (i % 5)]
        || ' ' || (ARRAY['Alpha', 'Beta', 'Gamma', 'Delta', 'Omega'])[1 + (i % 5)]
        || ' v' || (1 + i % 10),
    (ARRAY['electronics', 'audio', 'accessories', 'peripherals', 'displays'])[1 + (i % 5)],
    round((10 + random() * 490)::numeric, 2),
    round((0.05 + random() * 5)::numeric, 2)
FROM generate_series(26, 100) AS i;

-- ── Seed data: Customers (500) ────────────────────────

INSERT INTO customers (first_name, last_name, email, phone, address, city, country, segment)
SELECT
    (ARRAY['James','Mary','John','Patricia','Robert','Jennifer','Michael','Linda',
           'David','Elizabeth','William','Barbara','Richard','Susan','Joseph','Jessica',
           'Thomas','Sarah','Charles','Karen','Emma','Oliver','Ava','Liam'])[1 + (i % 24)],
    (ARRAY['Smith','Johnson','Williams','Brown','Jones','Garcia','Miller','Davis',
           'Rodriguez','Martinez','Hernandez','Lopez','Gonzalez','Wilson','Anderson',
           'Thomas','Taylor','Moore','Jackson','Martin','Lee','Perez','White','Harris'])[1 + (i % 24)],
    'user' || i || '@example.com',
    '+1-555-' || LPAD((1000 + i % 9000)::text, 4, '0'),
    (100 + i % 900) || ' ' || (ARRAY['Main St','Oak Ave','Elm Dr','Pine Rd','Cedar Ln','Maple Ct','Park Blvd','Lake Way'])[1 + (i % 8)],
    (ARRAY['New York','Los Angeles','Chicago','Houston','Phoenix','Philadelphia',
           'San Antonio','San Diego','Dallas','Austin','Miami','Seattle','Denver','Boston'])[1 + (i % 14)],
    (ARRAY['US','US','US','US','CA','CA','GB','DE','FR','AU'])[1 + (i % 10)],
    (ARRAY['standard','premium','enterprise','standard','standard','premium'])[1 + (i % 6)]
FROM generate_series(1, 500) AS i;

-- ── Seed data: Orders (2000) + Order Items (5000) + Payments ──

INSERT INTO orders (customer_id, status, total, currency, notes, created_at)
SELECT
    1 + (i % 500),
    (ARRAY['pending','confirmed','shipped','delivered','cancelled'])[1 + (i % 5)],
    round((10 + random() * 2000)::numeric, 2),
    (ARRAY['USD','USD','USD','EUR','GBP'])[1 + (i % 5)],
    CASE WHEN i % 7 = 0 THEN 'Rush delivery requested'
         WHEN i % 11 = 0 THEN 'Gift wrapping'
         ELSE NULL END,
    now() - (random() * interval '90 days')
FROM generate_series(1, 2000) AS i;

-- Update totals
UPDATE orders SET updated_at = created_at + interval '1 hour';

INSERT INTO order_items (order_id, product_id, quantity, unit_price, discount)
SELECT
    1 + (i % 2000),
    1 + (i % 100),
    1 + (i % 5),
    round((10 + random() * 500)::numeric, 2),
    CASE WHEN i % 10 = 0 THEN round((random() * 20)::numeric, 2) ELSE 0 END
FROM generate_series(1, 5000) AS i;

INSERT INTO payments (order_id, method, amount, status, processed_at)
SELECT
    id,
    (ARRAY['credit_card','debit_card','paypal','bank_transfer','apple_pay'])[1 + (id % 5)],
    total,
    CASE WHEN status IN ('delivered','shipped') THEN 'completed'
         WHEN status = 'cancelled' THEN 'refunded'
         ELSE 'pending' END,
    CASE WHEN status IN ('delivered','shipped') THEN created_at + interval '5 minutes' ELSE NULL END
FROM orders;

-- ── Summary ───────────────────────────────────────────

DO $$
DECLARE
    c_count BIGINT; p_count BIGINT; o_count BIGINT; oi_count BIGINT; pay_count BIGINT;
BEGIN
    SELECT count(*) INTO c_count FROM customers;
    SELECT count(*) INTO p_count FROM products;
    SELECT count(*) INTO o_count FROM orders;
    SELECT count(*) INTO oi_count FROM order_items;
    SELECT count(*) INTO pay_count FROM payments;
    RAISE NOTICE 'DataShuttle PostgreSQL demo data loaded:';
    RAISE NOTICE '  customers:   %', c_count;
    RAISE NOTICE '  products:    %', p_count;
    RAISE NOTICE '  orders:      %', o_count;
    RAISE NOTICE '  order_items: %', oi_count;
    RAISE NOTICE '  payments:    %', pay_count;
END $$;
