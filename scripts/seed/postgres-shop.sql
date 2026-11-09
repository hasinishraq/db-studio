-- Demo data for screenshots: the "shop" database. Generated, deterministic, no real people.
-- Run against an empty database named shop: psql -d shop -f postgres-shop.sql
SET client_min_messages = warning;
DROP SCHEMA IF EXISTS sales CASCADE;
DROP SCHEMA IF EXISTS inventory CASCADE;
DROP SCHEMA IF EXISTS hr CASCADE;
CREATE SCHEMA inventory;
CREATE SCHEMA sales;
CREATE SCHEMA hr;

CREATE TEMP TABLE first_names(i int, v text);
INSERT INTO first_names SELECT row_number() OVER (), v FROM unnest(ARRAY[
  'Ava','Liam','Noah','Emma','Mia','Ethan','Zara','Omar','Lena','Ravi','Sofia','Kai','Amir','Nora','Leo',
  'Ines','Yuki','Tariq','Elena','Jonas','Priya','Mateo','Hana','Felix','Aisha','Lucas','Maya','Sami','Ivy','Theo']) AS v;
CREATE TEMP TABLE last_names(i int, v text);
INSERT INTO last_names SELECT row_number() OVER (), v FROM unnest(ARRAY[
  'Rahman','Silva','Novak','Okafor','Larsen','Haddad','Moreau','Tanaka','Kowalski','Mensah','Ibrahim','Costa',
  'Fischer','Khan','Rossi','Nguyen','Petrov','Ahmed','Lopez','Berg']) AS v;

-- Inventory -------------------------------------------------------------------------------
CREATE TABLE inventory.suppliers (
  id serial PRIMARY KEY,
  name text NOT NULL,
  country text NOT NULL,
  rating numeric(2,1) NOT NULL
);
INSERT INTO inventory.suppliers (name, country, rating)
SELECT 'Supplier ' || chr(64 + (g % 26) + 1) || g, (ARRAY['Bangladesh','Germany','Japan','Brazil','Kenya','Canada','India','Poland'])[g % 8 + 1],
       round((3 + (g % 20) / 10.0)::numeric, 1)
FROM generate_series(1, 40) g;

CREATE TABLE inventory.warehouses (
  id serial PRIMARY KEY,
  city text NOT NULL,
  capacity int NOT NULL
);
INSERT INTO inventory.warehouses (city, capacity) VALUES
  ('Dhaka', 12000), ('Chittagong', 8000), ('Hamburg', 15000), ('Osaka', 9000), ('Toronto', 7000);

CREATE TABLE inventory.products (
  id serial PRIMARY KEY,
  sku text NOT NULL UNIQUE,
  name text NOT NULL,
  category text NOT NULL,
  price numeric(10,2) NOT NULL CHECK (price > 0),
  stock int NOT NULL DEFAULT 0,
  supplier_id int NOT NULL REFERENCES inventory.suppliers(id),
  warehouse_id int NOT NULL REFERENCES inventory.warehouses(id)
);
INSERT INTO inventory.products (sku, name, category, price, stock, supplier_id, warehouse_id)
SELECT 'SKU-' || lpad(g::text, 5, '0'),
       (ARRAY['Classic','Smart','Eco','Pro','Mini','Ultra','Daily','Studio'])[g % 8 + 1] || ' ' ||
       (ARRAY['Headphones','Kettle','Backpack','Desk Lamp','Notebook','Water Bottle','Keyboard','Jacket','Sneakers','Blender'])[g % 10 + 1],
       (ARRAY['Electronics','Home','Travel','Office','Sports','Fashion'])[g % 6 + 1],
       round((5 + (g * 37 % 495) + (g % 100) / 100.0)::numeric, 2),
       (g * 13) % 400,
       (g % 40) + 1,
       (g % 5) + 1
FROM generate_series(1, 500) g;

-- Sales ------------------------------------------------------------------------------------
CREATE TABLE sales.customers (
  id serial PRIMARY KEY,
  name text NOT NULL,
  email text NOT NULL UNIQUE,
  region text NOT NULL,
  signed_up date NOT NULL,
  vip boolean NOT NULL DEFAULT false
);
INSERT INTO sales.customers (name, email, region, signed_up, vip)
SELECT f.v || ' ' || l.v,
       lower(f.v || '.' || l.v || g) || '@example.com',
       -- Weighted, so regions differ the way real customers do.
       (ARRAY['Asia','Asia','Asia','Asia','Asia','Europe','Europe','Europe','Europe','North America','North America',
              'North America','South America','South America','Africa','Oceania'])[g % 16 + 1],
       date '2023-01-01' + (g * 7 % 1000),
       g % 17 = 0
FROM generate_series(1, 2000) g
JOIN first_names f ON f.i = g % 30 + 1
JOIN last_names l ON l.i = (g / 30) % 20 + 1;

CREATE TABLE sales.orders (
  id serial PRIMARY KEY,
  customer_id int NOT NULL REFERENCES sales.customers(id),
  ordered_at timestamptz NOT NULL,
  status text NOT NULL CHECK (status IN ('pending','paid','shipped','delivered','cancelled')),
  total numeric(12,2) NOT NULL DEFAULT 0
);
INSERT INTO sales.orders (customer_id, ordered_at, status)
SELECT (g * 7919 % 2000) + 1,
       -- Orders grow through the year: later months are busier.
       timestamptz '2025-01-01 08:00+00' + make_interval(mins => floor(power(g / 10000.0, 0.7) * 470000)::int + (g % 13) * 11),
       (ARRAY['pending','paid','shipped','delivered','delivered','delivered','cancelled'])[g % 7 + 1]
FROM generate_series(1, 10000) g;
CREATE INDEX orders_customer ON sales.orders(customer_id);
CREATE INDEX orders_ordered_at ON sales.orders(ordered_at);

CREATE TABLE sales.order_items (
  id serial PRIMARY KEY,
  order_id int NOT NULL REFERENCES sales.orders(id) ON DELETE CASCADE,
  product_id int NOT NULL REFERENCES inventory.products(id),
  quantity int NOT NULL CHECK (quantity > 0),
  unit_price numeric(10,2) NOT NULL
);
INSERT INTO sales.order_items (order_id, product_id, quantity, unit_price)
-- A few products sell far more than the rest.
SELECT g, p.id, (g % 4) + 1, p.price
FROM generate_series(1, 10000) g
JOIN inventory.products p ON p.id = 1 + floor(power(((g * 7919) % 1000) / 1000.0, 3) * 499)::int;
CREATE INDEX order_items_order ON sales.order_items(order_id);

UPDATE sales.orders o SET total = s.amount
FROM (SELECT order_id, sum(quantity * unit_price) AS amount FROM sales.order_items GROUP BY order_id) s
WHERE s.order_id = o.id;

CREATE TABLE sales.payments (
  id serial PRIMARY KEY,
  order_id int NOT NULL REFERENCES sales.orders(id),
  method text NOT NULL,
  amount numeric(12,2) NOT NULL,
  paid_at timestamptz NOT NULL
);
INSERT INTO sales.payments (order_id, method, amount, paid_at)
SELECT id, (ARRAY['card','bKash','bank transfer','cash on delivery','PayPal'])[id % 5 + 1], total, ordered_at + interval '2 hours'
FROM sales.orders WHERE status <> 'pending' AND status <> 'cancelled';

CREATE VIEW sales.monthly_revenue AS
SELECT date_trunc('month', ordered_at)::date AS month, count(*) AS orders, sum(total) AS revenue
FROM sales.orders WHERE status <> 'cancelled' GROUP BY 1 ORDER BY 1;

-- HR ---------------------------------------------------------------------------------------
CREATE TABLE hr.departments (
  id serial PRIMARY KEY,
  name text NOT NULL UNIQUE
);
INSERT INTO hr.departments (name) VALUES ('Engineering'), ('Sales'), ('Support'), ('Finance'), ('Warehouse'), ('Marketing');

CREATE TABLE hr.employees (
  id serial PRIMARY KEY,
  name text NOT NULL,
  email text NOT NULL UNIQUE,
  department_id int NOT NULL REFERENCES hr.departments(id),
  manager_id int REFERENCES hr.employees(id),
  salary numeric(10,2) NOT NULL,
  hired_on date NOT NULL,
  password_hash text NOT NULL
);
INSERT INTO hr.employees (name, email, department_id, salary, hired_on, password_hash)
SELECT f.v || ' ' || l.v, lower(f.v || '.' || l.v || '.' || g) || '@shop.example',
       (g % 6) + 1, 32000 + (g * 977 % 60000), date '2018-03-01' + (g * 23 % 2500),
       'demo$' || md5(g::text)
FROM generate_series(1, 300) g
JOIN first_names f ON f.i = (g * 7) % 30 + 1
JOIN last_names l ON l.i = (g * 3) % 20 + 1;
UPDATE hr.employees SET manager_id = ((id - 1) / 10) * 10 + 1 WHERE id % 10 <> 1;

ANALYZE;
