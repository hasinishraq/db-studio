-- Demo data for screenshots: the "analytics" database. Generated, deterministic.
SET client_min_messages = warning;
DROP TABLE IF EXISTS events, sessions, daily_stats CASCADE;

CREATE TABLE sessions (
  id serial PRIMARY KEY,
  user_id int NOT NULL,
  started_at timestamptz NOT NULL,
  duration_s int NOT NULL,
  device text NOT NULL
);
INSERT INTO sessions (user_id, started_at, duration_s, device)
SELECT (g * 37 % 1500) + 1, timestamptz '2026-08-10 00:00+00' + make_interval(mins => g * 43),
       60 + (g * 97 % 1800), (ARRAY['desktop','mobile','tablet'])[g % 3 + 1]
FROM generate_series(1, 3000) g;

CREATE TABLE events (
  id bigserial PRIMARY KEY,
  session_id int NOT NULL REFERENCES sessions(id),
  kind text NOT NULL,
  page text NOT NULL,
  at timestamptz NOT NULL
);
INSERT INTO events (session_id, kind, page, at)
SELECT (g % 3000) + 1, (ARRAY['view','view','view','click','search','add_to_cart','checkout'])[g % 7 + 1],
       (ARRAY['/','/products','/cart','/checkout','/account','/search','/blog'])[g * 3 % 7 + 1],
       timestamptz '2026-08-10 00:00+00' + make_interval(mins => (g % 3000) * 43, secs => g % 600)
FROM generate_series(1, 10000) g;
CREATE INDEX events_at ON events(at);

CREATE TABLE daily_stats (
  day date PRIMARY KEY,
  visitors int NOT NULL,
  signups int NOT NULL,
  revenue numeric(12,2) NOT NULL
);
INSERT INTO daily_stats
SELECT d::date,
       900 + (extract(doy FROM d)::int * 37 % 400) + (CASE WHEN extract(isodow FROM d) >= 6 THEN 350 ELSE 0 END),
       20 + (extract(doy FROM d)::int * 11 % 45),
       round((4000 + 2500 * sin(extract(doy FROM d) / 58.0) + (extract(doy FROM d)::int * 73 % 900))::numeric, 2)
FROM generate_series(date '2025-11-01', date '2026-10-31', interval '1 day') d;

ANALYZE;
