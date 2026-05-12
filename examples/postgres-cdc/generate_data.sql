-- ============================================================================
-- DataShuttle Demo Data Generator for PostgreSQL (DBeaver-compatible)
-- ============================================================================
-- Usage: Set @count below to the desired number of records, then execute.
--        The script inserts into all 5 e-commerce demo tables proportionally:
--          customers:   @count
--          products:    @count * 0.2  (1 product per 5 customers)
--          orders:      @count * 4    (4 orders per customer)
--          order_items: @count * 10   (~2.5 items per order)
--          payments:    @count * 4    (1 payment per order)
-- ============================================================================

DO $$
DECLARE
    -- ========================================================================
    -- CONFIGURE: Set the number of customers to generate.
    -- All other tables scale proportionally.
    -- ========================================================================
    v_count         INT := 1000;

    -- Derived counts
    v_products      INT;
    v_orders        INT;
    v_items         INT;
    v_payments      INT;

    -- Tracking
    v_cust_start    INT;
    v_prod_start    INT;
    v_ord_start     INT;

    -- Helpers
    v_first_names   TEXT[] := ARRAY[
        'James','Mary','Robert','Patricia','John','Jennifer','Michael','Linda',
        'David','Elizabeth','William','Barbara','Richard','Susan','Joseph','Jessica',
        'Thomas','Sarah','Charles','Karen','Christopher','Lisa','Daniel','Nancy',
        'Matthew','Betty','Anthony','Margaret','Mark','Sandra','Donald','Ashley',
        'Steven','Dorothy','Paul','Kimberly','Andrew','Emily','Joshua','Donna',
        'Kenneth','Michelle','Kevin','Carol','Brian','Amanda','George','Melissa',
        'Timothy','Deborah','Ronald','Stephanie','Edward','Rebecca','Jason','Sharon',
        'Jeffrey','Laura','Ryan','Cynthia','Jacob','Kathleen','Gary','Amy',
        'Nicholas','Angela','Eric','Shirley','Jonathan','Anna','Stephen','Brenda',
        'Larry','Pamela','Justin','Emma','Scott','Nicole','Brandon','Helen',
        'Benjamin','Samantha','Samuel','Katherine','Raymond','Christine','Gregory','Debra',
        'Frank','Rachel','Alexander','Carolyn','Patrick','Janet','Jack','Catherine'
    ];
    v_last_names    TEXT[] := ARRAY[
        'Smith','Johnson','Williams','Brown','Jones','Garcia','Miller','Davis',
        'Rodriguez','Martinez','Hernandez','Lopez','Gonzalez','Wilson','Anderson',
        'Thomas','Taylor','Moore','Jackson','Martin','Lee','Perez','Thompson',
        'White','Harris','Sanchez','Clark','Ramirez','Lewis','Robinson','Walker',
        'Young','Allen','King','Wright','Scott','Torres','Nguyen','Hill',
        'Flores','Green','Adams','Nelson','Baker','Hall','Rivera','Campbell',
        'Mitchell','Carter','Roberts','Gomez','Phillips','Evans','Turner','Diaz',
        'Parker','Cruz','Edwards','Collins','Reyes','Stewart','Morris','Morales',
        'Murphy','Cook','Rogers','Gutierrez','Ortiz','Morgan','Cooper','Peterson',
        'Bailey','Reed','Kelly','Howard','Ramos','Kim','Cox','Ward',
        'Richardson','Watson','Brooks','Chavez','Wood','James','Bennett','Gray',
        'Mendoza','Ruiz','Hughes','Price','Alvarez','Castillo','Sanders','Patel'
    ];
    v_countries     TEXT[] := ARRAY['US','DE','JP','GB','FR','CA','AU','BR','IN','MX','IT','ES','NL','SE','KR'];
    v_segments      TEXT[] := ARRAY['standard','premium','enterprise','starter','professional'];
    v_categories    TEXT[] := ARRAY['Electronics','Clothing','Books','Home','Sports','Food','Beauty','Toys','Garden','Automotive'];
    v_statuses      TEXT[] := ARRAY['pending','confirmed','shipped','delivered','cancelled','returned'];
    v_currencies    TEXT[] := ARRAY['USD','EUR','GBP','JPY','CAD','AUD','BRL','INR'];
    v_pay_methods   TEXT[] := ARRAY['credit_card','debit_card','paypal','bank_transfer','apple_pay','google_pay','crypto'];
    v_pay_statuses  TEXT[] := ARRAY['pending','completed','completed','completed','completed','failed','refunded'];

BEGIN
    -- Calculate proportional counts
    v_products  := GREATEST(v_count / 5, 10);
    v_orders    := v_count * 4;
    v_items     := v_count * 10;
    v_payments  := v_orders;

    RAISE NOTICE '=== DataShuttle Demo Data Generator ===';
    RAISE NOTICE 'Generating: % customers, % products, % orders, % items, % payments',
        v_count, v_products, v_orders, v_items, v_payments;

    -- ========================================================================
    -- 1. CUSTOMERS
    -- ========================================================================
    v_cust_start := (SELECT COALESCE(MAX(id), 0) FROM customers);

    INSERT INTO customers (first_name, last_name, email, phone, address, city, country, segment, created_at, updated_at)
    SELECT
        v_first_names[1 + (g % array_length(v_first_names, 1))],
        v_last_names[1 + ((g * 7) % array_length(v_last_names, 1))],
        'user_' || (v_cust_start + g) || '_' || substr(md5(g::text), 1, 6) || '@example.com',
        '+1-' || lpad((random() * 999)::int::text, 3, '0') || '-' || lpad((random() * 9999)::int::text, 4, '0'),
        (100 + (random() * 9900)::int)::text || ' ' || v_last_names[1 + ((g * 3) % array_length(v_last_names, 1))] || ' St',
        (ARRAY['New York','Los Angeles','Chicago','Houston','Phoenix','Philadelphia','San Antonio',
               'San Diego','Dallas','San Jose','Austin','Berlin','Munich','Tokyo','Osaka',
               'London','Paris','Toronto','Sydney','Mumbai','Sao Paulo','Mexico City'])[1 + (g % 22)],
        v_countries[1 + (g % array_length(v_countries, 1))],
        v_segments[1 + (g % array_length(v_segments, 1))],
        now() - (random() * interval '730 days'),
        now() - (random() * interval '30 days')
    FROM generate_series(1, v_count) g;

    RAISE NOTICE 'Inserted % customers', v_count;

    -- ========================================================================
    -- 2. PRODUCTS
    -- ========================================================================
    v_prod_start := (SELECT COALESCE(MAX(id), 0) FROM products);

    INSERT INTO products (sku, name, category, price, weight_kg, is_active, created_at)
    SELECT
        'SKU-' || lpad((v_prod_start + g)::text, 6, '0'),
        v_categories[1 + (g % array_length(v_categories, 1))]
            || ' Item '
            || (v_prod_start + g)::text,
        v_categories[1 + (g % array_length(v_categories, 1))],
        round((5 + random() * 995)::numeric, 2),
        round((0.1 + random() * 29.9)::numeric, 2),
        random() > 0.1,
        now() - (random() * interval '365 days')
    FROM generate_series(1, v_products) g;

    RAISE NOTICE 'Inserted % products', v_products;

    -- ========================================================================
    -- 3. ORDERS
    -- ========================================================================
    v_ord_start := (SELECT COALESCE(MAX(id), 0) FROM orders);

    INSERT INTO orders (customer_id, status, total, currency, notes, created_at, updated_at)
    SELECT
        v_cust_start + 1 + (g % v_count),
        v_statuses[1 + (g % array_length(v_statuses, 1))],
        0,  -- will be updated after order_items
        v_currencies[1 + (g % array_length(v_currencies, 1))],
        CASE WHEN random() < 0.3 THEN 'Note for order ' || g ELSE NULL END,
        now() - (random() * interval '365 days'),
        now() - (random() * interval '30 days')
    FROM generate_series(1, v_orders) g;

    RAISE NOTICE 'Inserted % orders', v_orders;

    -- ========================================================================
    -- 4. ORDER_ITEMS
    -- ========================================================================
    INSERT INTO order_items (order_id, product_id, quantity, unit_price, discount, created_at)
    SELECT
        v_ord_start + 1 + (g % v_orders),
        v_prod_start + 1 + (g % v_products),
        1 + (random() * 4)::int,
        round((5 + random() * 495)::numeric, 2),
        CASE WHEN random() < 0.2 THEN round((random() * 20)::numeric, 2) ELSE 0 END,
        now() - (random() * interval '365 days')
    FROM generate_series(1, v_items) g;

    RAISE NOTICE 'Inserted % order_items', v_items;

    -- ========================================================================
    -- 5. UPDATE ORDER TOTALS
    -- ========================================================================
    UPDATE orders o
    SET total = sub.total
    FROM (
        SELECT order_id, COALESCE(SUM(quantity * unit_price * (1 - discount / 100)), 0) AS total
        FROM order_items
        WHERE order_id > v_ord_start
        GROUP BY order_id
    ) sub
    WHERE o.id = sub.order_id;

    RAISE NOTICE 'Updated order totals';

    -- ========================================================================
    -- 6. PAYMENTS
    -- ========================================================================
    INSERT INTO payments (order_id, method, amount, status, processed_at, created_at)
    SELECT
        o.id,
        v_pay_methods[1 + ((o.id * 3) % array_length(v_pay_methods, 1))],
        o.total,
        v_pay_statuses[1 + ((o.id * 5) % array_length(v_pay_statuses, 1))],
        CASE WHEN random() < 0.85 THEN o.created_at + (random() * interval '2 hours') ELSE NULL END,
        o.created_at
    FROM orders o
    WHERE o.id > v_ord_start;

    RAISE NOTICE 'Inserted % payments', v_payments;

    -- ========================================================================
    -- SUMMARY
    -- ========================================================================
    RAISE NOTICE '=== Generation complete ===';
    RAISE NOTICE 'Total rows: %', v_count + v_products + v_orders + v_items + v_payments;
END $$;

-- Show final counts
SELECT 'customers'   AS "table", count(*) AS "rows" FROM customers   UNION ALL
SELECT 'products',               count(*)           FROM products    UNION ALL
SELECT 'orders',                 count(*)           FROM orders      UNION ALL
SELECT 'order_items',            count(*)           FROM order_items UNION ALL
SELECT 'payments',               count(*)           FROM payments
ORDER BY "table";
