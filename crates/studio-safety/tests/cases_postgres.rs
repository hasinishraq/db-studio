//! PostgreSQL classifier table.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;
use common::*;
use studio_safety::classify_script;
use studio_types::{Dialect, StatementKind};

const CASES: &[Case] = &[
    // Reads.
    ("SHOW server_version", Auto),
    ("SHOW ALL", Auto),
    ("EXPLAIN SELECT 1", Auto),
    ("EXPLAIN SELECT * FROM t", Auto),
    ("EXPLAIN (FORMAT JSON) SELECT 1", Auto),
    ("EXPLAIN (VERBOSE, COSTS) SELECT 1", Auto),
    ("EXPLAIN DELETE FROM t", Auto),
    ("EXPLAIN UPDATE t SET a = 1", Auto),
    ("SELECT * FROM pg_catalog.pg_tables", Auto),
    ("SELECT * FROM information_schema.tables", Auto),
    ("SELECT now()", Auto),
    ("SELECT current_user", Auto),
    ("SELECT a::text FROM t", Auto),
    ("SELECT a->>'k' FROM t", Auto),
    ("SELECT * FROM generate_series(1, 10)", Auto),
    ("SELECT ARRAY[1, 2, 3]", Auto),
    ("SELECT DISTINCT ON (a) a, b FROM t", Auto),
    ("VALUES (1), (2)", Auto),
    ("SELECT \"DROP\" FROM \"DELETE\"", Auto),
    ("SELECT \"update\", \"insert\" FROM \"table\"", Auto),
    ("SELECT \"a;b\" FROM t", Auto),
    // PostgreSQL allows an empty select list, so this parses as a read of a table named WHERE. It
    // cannot write under any reading, and the server rejects it. Still a parse failure elsewhere.
    ("SELECT FROM WHERE", Auto),
    // Dollar quotes hide their body from the splitter.
    ("SELECT $$DROP TABLE x$$", Auto),
    ("SELECT $$a; DROP TABLE x$$", Auto),
    ("SELECT $tag$ ; DELETE FROM t; $tag$", Auto),
    ("SELECT $a$ $b$ ; $b$ $a$", Auto),
    // E-strings.
    (r"SELECT E'a\'; DROP TABLE x; --'", Auto),
    // Comments nest.
    ("SELECT 1 /* a /* ; DROP TABLE x */ b */", Auto),
    // A backslash does not escape in a plain string, so the quote closes here.
    (r"SELECT 'a\'; DROP TABLE x; --'", Write),
    // Locks.
    ("SELECT * FROM t FOR UPDATE", ReadNotAuto),
    ("SELECT * FROM t FOR SHARE", ReadNotAuto),
    // The parser does not know this valid lock clause, so it fails closed: stricter, never looser.
    ("SELECT * FROM t FOR NO KEY UPDATE", Write),
    ("SELECT * FROM t FOR UPDATE SKIP LOCKED", ReadNotAuto),
    ("SELECT * FROM (SELECT * FROM t FOR UPDATE) AS s", ReadNotAuto),
    (
        "WITH c AS (SELECT * FROM t FOR UPDATE) SELECT * FROM c",
        ReadNotAuto,
    ),
    // SELECT INTO creates a table.
    ("SELECT * INTO new_t FROM t", Write),
    ("SELECT 1 INTO TEMP x", Write),
    ("WITH c AS (SELECT 1) SELECT * INTO new_t FROM c", Write),
    // Data-modifying CTEs.
    ("WITH d AS (DELETE FROM t RETURNING *) SELECT * FROM d", Write),
    (
        "WITH u AS (UPDATE t SET a = 1 RETURNING *) SELECT * FROM u",
        Write,
    ),
    (
        "WITH i AS (INSERT INTO t VALUES (1) RETURNING *) SELECT * FROM i",
        Write,
    ),
    (
        "WITH a AS (SELECT 1), d AS (DELETE FROM t RETURNING 1) SELECT * FROM a, d",
        Write,
    ),
    (
        "WITH a AS (WITH d AS (DELETE FROM t RETURNING 1) SELECT * FROM d) SELECT * FROM a",
        Write,
    ),
    ("WITH c AS (SELECT 1) INSERT INTO t SELECT * FROM c", Write),
    ("WITH c AS (SELECT 1) UPDATE t SET a = 1", Write),
    ("WITH c AS (SELECT 1) DELETE FROM t", Write),
    // EXPLAIN ANALYZE really runs its statement.
    ("EXPLAIN ANALYZE SELECT 1", Auto),
    ("EXPLAIN ANALYZE DELETE FROM t", Write),
    ("EXPLAIN ANALYZE UPDATE t SET a = 1", Write),
    ("EXPLAIN ANALYZE INSERT INTO t VALUES (1)", Write),
    ("EXPLAIN (ANALYZE) DELETE FROM t", Write),
    ("EXPLAIN (ANALYZE TRUE) DELETE FROM t", Write),
    ("EXPLAIN (ANALYZE, BUFFERS) UPDATE t SET a = 1", Write),
    ("EXPLAIN (FORMAT JSON, ANALYZE) DELETE FROM t", Write),
    ("EXPLAIN (analyze) DELETE FROM t", Write),
    ("EXPLAIN (ANALYSE) DELETE FROM t", Write),
    (
        "EXPLAIN ANALYZE WITH d AS (DELETE FROM t RETURNING 1) SELECT * FROM d",
        Write,
    ),
    // Side-effect functions.
    ("SELECT pg_sleep(5)", ReadNotAuto),
    ("SELECT PG_SLEEP(5)", ReadNotAuto),
    ("SELECT pg_catalog.pg_sleep(5)", ReadNotAuto),
    ("SELECT \"pg_sleep\"(5)", ReadNotAuto),
    ("SELECT pg_catalog.\"pg_sleep\"(5)", ReadNotAuto),
    ("SELECT pg_sleep_for('5 minutes')", ReadNotAuto),
    ("SELECT nextval('s')", ReadNotAuto),
    ("SELECT setval('s', 1)", ReadNotAuto),
    ("SELECT 1 WHERE pg_sleep(5) IS NULL", ReadNotAuto),
    ("SELECT CASE WHEN true THEN pg_sleep(1) END", ReadNotAuto),
    ("SELECT * FROM t ORDER BY pg_sleep(1)", ReadNotAuto),
    ("SELECT * FROM t WHERE id = (SELECT nextval('s'))", ReadNotAuto),
    ("WITH c AS (SELECT pg_sleep(5)) SELECT * FROM c", ReadNotAuto),
    ("SELECT * FROM pg_read_file('/etc/passwd')", ReadNotAuto),
    ("SELECT pg_read_file('/etc/passwd')", ReadNotAuto),
    ("SELECT lo_import('/etc/passwd')", ReadNotAuto),
    ("SELECT lo_export(1, '/tmp/x')", ReadNotAuto),
    ("SELECT dblink_exec('c', 'DROP TABLE x')", ReadNotAuto),
    (
        "SELECT * FROM dblink('c', 'DELETE FROM t') AS r(a int)",
        ReadNotAuto,
    ),
    ("SELECT pg_advisory_lock(1)", ReadNotAuto),
    ("SELECT pg_terminate_backend(1)", ReadNotAuto),
    ("SELECT set_config('a', 'b', false)", ReadNotAuto),
    (
        "SELECT query_to_xml('DELETE FROM t', true, true, '')",
        ReadNotAuto,
    ),
    ("SELECT pg_notify('c', 'x')", ReadNotAuto),
    // Privileges.
    ("GRANT SELECT ON t TO bob", Write),
    ("REVOKE SELECT ON t FROM bob", Write),
    ("SET ROLE admin", Write),
    ("SET SESSION AUTHORIZATION bob", Write),
    // Other writes.
    ("TRUNCATE t", Write),
    ("TRUNCATE TABLE t CASCADE", Write),
    (
        "MERGE INTO t USING u ON t.id = u.id WHEN MATCHED THEN DELETE",
        Write,
    ),
    ("COPY t TO '/tmp/x'", Write),
    ("COPY t FROM '/tmp/x'", Write),
    ("COPY (SELECT 1) TO PROGRAM 'rm -rf /'", Write),
    ("CALL do_something()", Write),
    ("DO $$ BEGIN DELETE FROM t; END $$", Write),
    ("VACUUM t", Write),
    ("ANALYZE t", Write),
    ("REINDEX TABLE t", Write),
    ("CLUSTER t", Write),
    ("LOCK TABLE t", Write),
    ("REFRESH MATERIALIZED VIEW v", Write),
    ("CREATE SCHEMA s", Write),
    (
        "CREATE FUNCTION f() RETURNS int AS $$ SELECT 1 $$ LANGUAGE sql",
        Write,
    ),
    ("SET search_path = evil", Write),
    ("RESET ALL", Write),
    ("LISTEN c", Write),
    ("NOTIFY c", Write),
    ("PREPARE p AS DELETE FROM t", Write),
    ("EXECUTE p", Write),
    ("DEALLOCATE p", Write),
    ("DECLARE c CURSOR FOR SELECT 1", Write),
    ("COMMENT ON TABLE t IS 'x'", Write),
];

#[test]
fn table() {
    run(Dialect::Postgres, CASES);
}

#[test]
fn edge_cases() {
    run_edge_cases(Dialect::Postgres);
}

#[test]
fn kinds_and_risk() {
    let one = |sql: &str| classify_script(sql, Dialect::Postgres).remove(0).verdict;
    assert_eq!(
        one("WITH d AS (DELETE FROM t RETURNING 1) SELECT * FROM d").kind,
        StatementKind::Delete
    );
    assert_eq!(one("EXPLAIN SELECT 1").kind, StatementKind::Explain);
    assert_eq!(one("EXPLAIN ANALYZE DELETE FROM t").kind, StatementKind::Delete);
    assert_eq!(one("GRANT SELECT ON t TO b").kind, StatementKind::Dcl);
    assert_eq!(one("SET ROLE a").kind, StatementKind::Dcl);
    assert_eq!(one("BEGIN").kind, StatementKind::Tcl);
    assert_eq!(one("DROP TABLE t").kind, StatementKind::Ddl);
    let high = |sql: &str| matches!(one(sql).risk, studio_safety::Risk::High { .. });
    for sql in [
        "DROP TABLE t",
        "DROP SCHEMA s CASCADE",
        "TRUNCATE t",
        "DELETE FROM t",
        "UPDATE t SET a = 1",
        "ALTER TABLE t DROP COLUMN c",
        "GRANT SELECT ON t TO b",
        "REVOKE SELECT ON t FROM b",
        "EXPLAIN ANALYZE DELETE FROM t",
        "SELEC 1",
    ] {
        assert!(high(sql), "{sql} must be high risk");
    }
    for sql in [
        "DELETE FROM t WHERE id = 1",
        "UPDATE t SET a = 1 WHERE id = 1",
        "INSERT INTO t VALUES (1)",
    ] {
        assert!(!high(sql), "{sql} must not be high risk");
    }
}
