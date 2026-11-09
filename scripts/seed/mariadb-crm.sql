-- Demo data for screenshots: MariaDB database "crm". Generated, deterministic.
SET SESSION max_recursive_iterations = 20000;
DROP DATABASE IF EXISTS crm;
CREATE DATABASE crm CHARACTER SET utf8mb4;
USE crm;

CREATE TABLE companies (
  id INT AUTO_INCREMENT PRIMARY KEY,
  name VARCHAR(100) NOT NULL,
  industry VARCHAR(40) NOT NULL,
  employees INT NOT NULL
);
INSERT INTO companies (name, industry, employees)
WITH RECURSIVE n(g) AS (SELECT 1 UNION ALL SELECT g + 1 FROM n WHERE g < 400)
SELECT CONCAT(ELT(g % 6 + 1, 'Blue', 'North', 'Green', 'Bright', 'Delta', 'Summit'), ' ',
              ELT(g % 5 + 1, 'Logistics', 'Foods', 'Labs', 'Textiles', 'Energy'), ' ', g),
       ELT(g % 5 + 1, 'Logistics', 'Food', 'Software', 'Textile', 'Energy'), 10 + g * 37 % 5000
FROM n;

CREATE TABLE contacts (
  id INT AUTO_INCREMENT PRIMARY KEY,
  company_id INT NOT NULL,
  full_name VARCHAR(100) NOT NULL,
  email VARCHAR(160) NOT NULL,
  phone VARCHAR(30) NULL,
  created_at DATETIME NOT NULL,
  CONSTRAINT fk_contacts_company FOREIGN KEY (company_id) REFERENCES companies(id)
);
INSERT INTO contacts (company_id, full_name, email, phone, created_at)
WITH RECURSIVE n(g) AS (SELECT 1 UNION ALL SELECT g + 1 FROM n WHERE g < 5000)
SELECT g % 400 + 1,
       CONCAT(ELT(g % 10 + 1, 'Nadia', 'Arif', 'Lena', 'Carlos', 'Mei', 'Tomas', 'Rina', 'Kofi', 'Sara', 'Jamal'), ' ',
              ELT(g % 9 + 1, 'Chowdhury', 'Weber', 'Santos', 'Ito', 'Hassan', 'Novak', 'Owusu', 'Park', 'Dubois')),
       CONCAT('contact', g, '@crm.example'), IF(g % 4 = 0, NULL, CONCAT('+880 17', LPAD(g * 7919 % 100000000, 8, '0'))),
       TIMESTAMP('2024-06-01 09:00:00') + INTERVAL (g * 131) MINUTE
FROM n;

CREATE TABLE deals (
  id INT AUTO_INCREMENT PRIMARY KEY,
  company_id INT NOT NULL,
  title VARCHAR(140) NOT NULL,
  stage ENUM('lead', 'qualified', 'proposal', 'won', 'lost') NOT NULL,
  value DECIMAL(12,2) NOT NULL,
  closes_on DATE NULL,
  CONSTRAINT fk_deals_company FOREIGN KEY (company_id) REFERENCES companies(id)
);
INSERT INTO deals (company_id, title, stage, value, closes_on)
WITH RECURSIVE n(g) AS (SELECT 1 UNION ALL SELECT g + 1 FROM n WHERE g < 2000)
SELECT g * 7 % 400 + 1, CONCAT(ELT(g % 4 + 1, 'Annual plan', 'Pilot', 'Renewal', 'Expansion'), ' #', g),
       g % 5 + 1, ROUND(500 + g * 211 % 50000, 2), IF(g % 6 = 0, NULL, DATE('2026-01-01') + INTERVAL (g % 365) DAY)
FROM n;

CREATE TABLE notes (
  id INT AUTO_INCREMENT PRIMARY KEY,
  contact_id INT NOT NULL,
  body TEXT NOT NULL,
  created_at DATETIME NOT NULL,
  CONSTRAINT fk_notes_contact FOREIGN KEY (contact_id) REFERENCES contacts(id)
);
INSERT INTO notes (contact_id, body, created_at)
WITH RECURSIVE n(g) AS (SELECT 1 UNION ALL SELECT g + 1 FROM n WHERE g < 2000)
SELECT g * 3 % 5000 + 1,
       CONCAT(ELT(g % 4 + 1, 'Called about the renewal; asked for a revised quote with volume pricing.',
                             'Sent the onboarding guide. Follow up next week about the training session.',
                             'Met at the trade fair. Interested in the pilot for their Chittagong warehouse.',
                             'Requested invoice copies for the last two quarters.'), ' Ref ', g),
       TIMESTAMP('2025-01-01 10:00:00') + INTERVAL (g * 241) MINUTE
FROM n;
