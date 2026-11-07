//! SQLite classifier table.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;
use common::*;
use studio_types::Dialect;

const CASES: &[Case] = &[
    ("SELECT FROM WHERE", Write),
    // Reads.
    ("EXPLAIN SELECT 1", Auto),
    ("EXPLAIN QUERY PLAN SELECT * FROM t", Auto),
    ("SELECT [a] FROM [t]", Auto),
    ("SELECT [a;b] FROM t", Auto),
    ("SELECT `a` FROM `t`", Auto),
    ("SELECT \"DROP\" FROM \"DELETE\"", Auto),
    ("SELECT * FROM sqlite_schema", Auto),
    ("SELECT * FROM sqlite_master", Auto),
    ("SELECT rowid, * FROM t", Auto),
    ("SELECT date('now')", Auto),
    ("SELECT json_extract(a, '$.b') FROM t", Auto),
    ("SELECT * FROM pragma_table_info('t')", Auto),
    ("VALUES (1), (2)", Auto),
    (r"SELECT 'a\'; DROP TABLE x; --'", Write),
    // Read-only pragmas.
    ("PRAGMA table_info(t)", Auto),
    ("PRAGMA table_info('t')", Auto),
    ("PRAGMA index_list(t)", Auto),
    ("PRAGMA foreign_key_list(t)", Auto),
    ("PRAGMA database_list", Auto),
    ("PRAGMA compile_options", Auto),
    ("PRAGMA  table_info( t", Write),
    ("PRAGMA table_info(t); DROP TABLE x", Write),
    ("PRAGMA table_info(t) DROP TABLE x", Write),
    ("PRAGMA table_info(t, u)", Write),
    ("PRAGMA table_info(a b)", Write),
    ("PRAGMA journal_mode(wal)", Write),
    ("PRAGMA writable_schema(1)", Write),
    ("PRAGMAtable_info(t)", Write),
    // Anything else may write.
    ("PRAGMA journal_mode = WAL", Write),
    ("PRAGMA user_version = 5", Write),
    ("PRAGMA writable_schema = ON", Write),
    ("PRAGMA foreign_keys = OFF", Write),
    ("PRAGMA synchronous = OFF", Write),
    ("PRAGMA unknown_pragma", Write),
    ("PRAGMA wal_checkpoint", Write),
    ("PRAGMA optimize", Write),
    ("PRAGMA incremental_vacuum", Write),
    // Side-effect functions.
    ("SELECT load_extension('evil')", ReadNotAuto),
    ("SELECT LOAD_EXTENSION('evil')", ReadNotAuto),
    ("SELECT writefile('/tmp/x', 'y')", ReadNotAuto),
    ("SELECT readfile('/etc/passwd')", ReadNotAuto),
    ("SELECT edit('x')", ReadNotAuto),
    ("SELECT fts3_tokenizer('x')", ReadNotAuto),
    ("SELECT 1 WHERE load_extension('evil') IS NULL", ReadNotAuto),
    // Writes.
    ("EXPLAIN ANALYZE DELETE FROM t", Write),
    ("INSERT OR REPLACE INTO t VALUES (1)", Write),
    ("INSERT OR IGNORE INTO t VALUES (1)", Write),
    ("REPLACE INTO t VALUES (1)", Write),
    ("UPDATE OR ABORT t SET a = 1", Write),
    ("INSERT INTO t VALUES (1) ON CONFLICT DO NOTHING", Write),
    ("INSERT INTO t VALUES (1) RETURNING *", Write),
    ("DELETE FROM t RETURNING *", Write),
    ("WITH c AS (SELECT 1) INSERT INTO t SELECT * FROM c", Write),
    ("WITH c AS (SELECT 1) DELETE FROM t", Write),
    ("WITH c AS (SELECT 1) UPDATE t SET a = 1", Write),
    ("ATTACH DATABASE '/tmp/x' AS y", Write),
    ("DETACH DATABASE y", Write),
    ("VACUUM", Write),
    ("VACUUM INTO '/tmp/x'", Write),
    ("REINDEX", Write),
    ("ANALYZE", Write),
    ("CREATE TEMP TABLE t (a)", Write),
    ("CREATE VIRTUAL TABLE v USING fts5(a)", Write),
    (
        "CREATE TRIGGER g AFTER INSERT ON t BEGIN DELETE FROM u; END",
        Write,
    ),
    ("DROP TRIGGER g", Write),
];

#[test]
fn table() {
    run(Dialect::Sqlite, CASES);
}

#[test]
fn edge_cases() {
    run_edge_cases(Dialect::Sqlite);
}
