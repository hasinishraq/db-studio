-- Demo data for screenshots: MySQL databases "store" and "archive". Generated, deterministic.
SET SESSION cte_max_recursion_depth = 20000;
DROP DATABASE IF EXISTS store;
DROP DATABASE IF EXISTS archive;
CREATE DATABASE store CHARACTER SET utf8mb4;
CREATE DATABASE archive CHARACTER SET utf8mb4;
USE store;

CREATE TABLE suppliers (
  id INT AUTO_INCREMENT PRIMARY KEY,
  name VARCHAR(80) NOT NULL,
  country VARCHAR(40) NOT NULL
);
INSERT INTO suppliers (name, country)
WITH RECURSIVE n(g) AS (SELECT 1 UNION ALL SELECT g + 1 FROM n WHERE g < 30)
SELECT CONCAT('Maker ', g), ELT(g % 6 + 1, 'Bangladesh', 'Vietnam', 'Germany', 'Mexico', 'Turkey', 'USA') FROM n;

CREATE TABLE products (
  id INT AUTO_INCREMENT PRIMARY KEY,
  sku VARCHAR(20) NOT NULL UNIQUE,
  name VARCHAR(120) NOT NULL,
  category ENUM('Grocery', 'Clothing', 'Toys', 'Garden', 'Kitchen') NOT NULL,
  price DECIMAL(10,2) NOT NULL,
  supplier_id INT NOT NULL,
  active TINYINT(1) NOT NULL DEFAULT 1,
  CONSTRAINT fk_products_supplier FOREIGN KEY (supplier_id) REFERENCES suppliers(id)
);
INSERT INTO products (sku, name, category, price, supplier_id, active)
WITH RECURSIVE n(g) AS (SELECT 1 UNION ALL SELECT g + 1 FROM n WHERE g < 500)
SELECT CONCAT('ST-', LPAD(g, 5, '0')),
       CONCAT(ELT(g % 5 + 1, 'Organic', 'Cotton', 'Wooden', 'Solar', 'Steel'), ' ',
              ELT(g % 8 + 1, 'Rice 5kg', 'T-Shirt', 'Puzzle', 'Lantern', 'Pan', 'Tea Box', 'Scarf', 'Planter')),
       g % 5 + 1, ROUND(2 + (g * 29 % 300) + (g % 100) / 100, 2), g % 30 + 1, g % 23 <> 0
FROM n;

CREATE TABLE stock_moves (
  id INT AUTO_INCREMENT PRIMARY KEY,
  product_id INT NOT NULL,
  moved_at DATETIME NOT NULL,
  kind ENUM('in', 'out', 'return', 'adjust') NOT NULL,
  quantity INT NOT NULL,
  note VARCHAR(200) NULL,
  KEY moved_at (moved_at),
  CONSTRAINT fk_moves_product FOREIGN KEY (product_id) REFERENCES products(id)
);
INSERT INTO stock_moves (product_id, moved_at, kind, quantity, note)
WITH RECURSIVE n(g) AS (SELECT 1 UNION ALL SELECT g + 1 FROM n WHERE g < 10000)
SELECT g * 17 % 500 + 1, TIMESTAMP('2026-01-01 07:00:00') + INTERVAL (g * 37) MINUTE,
       ELT(g % 7 + 1, 'in', 'out', 'out', 'out', 'return', 'in', 'adjust'),
       (g % 20) + 1, IF(g % 9 = 0, CONCAT('Batch ', g DIV 9), NULL)
FROM n;

CREATE VIEW stock_levels AS
SELECT p.id, p.sku, p.name,
       SUM(CASE m.kind WHEN 'in' THEN m.quantity WHEN 'return' THEN m.quantity WHEN 'out' THEN -m.quantity ELSE 0 END) AS on_hand
FROM products p LEFT JOIN stock_moves m ON m.product_id = p.id GROUP BY p.id, p.sku, p.name;

USE archive;
CREATE TABLE orders_2024 (
  id INT PRIMARY KEY,
  customer VARCHAR(80) NOT NULL,
  placed_on DATE NOT NULL,
  total DECIMAL(12,2) NOT NULL
);
INSERT INTO orders_2024
WITH RECURSIVE n(g) AS (SELECT 1 UNION ALL SELECT g + 1 FROM n WHERE g < 2000)
SELECT g, CONCAT('Customer ', g * 13 % 700), DATE('2024-01-01') + INTERVAL (g * 3 % 366) DAY, ROUND(10 + (g * 53 % 900), 2) FROM n;
