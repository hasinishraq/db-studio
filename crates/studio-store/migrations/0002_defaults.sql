-- A remembered default schema (PostgreSQL), applied on connect to the profile's database.
ALTER TABLE profiles ADD COLUMN default_schema TEXT;
-- The database a conversation works in, so the list can say "server · database".
ALTER TABLE conversations ADD COLUMN database TEXT;
