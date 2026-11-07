CREATE TABLE profiles (
  id TEXT PRIMARY KEY, name TEXT NOT NULL, engine TEXT NOT NULL,
  host TEXT NOT NULL DEFAULT '', port INTEGER NOT NULL DEFAULT 0,
  database TEXT NOT NULL DEFAULT '', username TEXT NOT NULL DEFAULT '',
  tls_mode TEXT NOT NULL DEFAULT 'VerifyFull', ca_cert_path TEXT, client_cert_path TEXT, client_key_path TEXT,
  ssh_json TEXT, -- bastion host/port/user/auth kind, no secrets
  environment TEXT NOT NULL DEFAULT 'Development', read_only INTEGER NOT NULL DEFAULT 0,
  ai_policy TEXT NOT NULL DEFAULT 'Default', color TEXT,
  created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL
);
CREATE TABLE drafts (
  id TEXT PRIMARY KEY, profile_id TEXT, title TEXT NOT NULL, sql TEXT NOT NULL,
  saved_sql TEXT NOT NULL, updated_at INTEGER NOT NULL
);
CREATE TABLE history (
  id TEXT PRIMARY KEY, profile_id TEXT, sql TEXT NOT NULL, started_at INTEGER NOT NULL,
  duration_ms INTEGER NOT NULL, row_count INTEGER NOT NULL, status TEXT NOT NULL
);
CREATE INDEX history_started ON history(started_at DESC);
CREATE TABLE settings (key TEXT PRIMARY KEY, value_json TEXT NOT NULL);
CREATE TABLE conversations (
  id TEXT PRIMARY KEY, profile_id TEXT, title TEXT NOT NULL, summary TEXT, updated_at INTEGER NOT NULL
);
CREATE TABLE turns (
  id TEXT PRIMARY KEY, conversation_id TEXT NOT NULL REFERENCES conversations ON DELETE CASCADE,
  seq INTEGER NOT NULL, user_text TEXT NOT NULL, assistant_text TEXT NOT NULL, queries_json TEXT NOT NULL
);
CREATE INDEX turns_conversation ON turns(conversation_id, seq);
CREATE TABLE audit_log (
  id INTEGER PRIMARY KEY AUTOINCREMENT, at INTEGER NOT NULL, profile_id TEXT, environment TEXT NOT NULL,
  origin TEXT NOT NULL, kind TEXT NOT NULL, sql_hash TEXT NOT NULL, sql_text TEXT NOT NULL,
  rows_affected INTEGER, outcome TEXT NOT NULL, prev_hash TEXT NOT NULL, entry_hash TEXT NOT NULL
);
CREATE TABLE known_hosts (
  host TEXT NOT NULL, port INTEGER NOT NULL, key_type TEXT NOT NULL, fingerprint TEXT NOT NULL,
  PRIMARY KEY (host, port, key_type)
);
CREATE TABLE masking_rules (
  profile_id TEXT NOT NULL, schema_name TEXT NOT NULL, table_name TEXT NOT NULL, column_name TEXT NOT NULL,
  PRIMARY KEY (profile_id, schema_name, table_name, column_name)
);
