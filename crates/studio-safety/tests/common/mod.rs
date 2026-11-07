//! The case-table runner and the cases that hold on every dialect.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use studio_safety::{ai_auto_allowed, classify_script};
use studio_types::{Dialect, StatementKind};

/// Expected treatment of a whole script.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Want {
    /// Every statement is a read the AI may run alone.
    Auto,
    /// A read for a human, but never unattended.
    ReadNotAuto,
    /// Needs approval from everyone.
    Write,
}
pub use Want::*;

/// `(sql, expected)`.
pub type Case = (&'static str, Want);

/// Cases valid on PostgreSQL, MySQL and SQLite alike.
pub const SHARED: &[Case] = &[
    // Plain reads.
    ("SELECT 1", Auto),
    ("select 1", Auto),
    ("  SELECT 1  ", Auto),
    ("SELECT 1;", Auto),
    ("SELECT 1 ; ", Auto),
    ("SELECT * FROM users", Auto),
    ("SELECT id, name FROM users WHERE id = 1", Auto),
    ("SELECT DISTINCT name FROM users", Auto),
    ("SELECT COUNT(*) FROM users", Auto),
    ("SELECT MAX(age), MIN(age), AVG(age), SUM(age) FROM users", Auto),
    ("SELECT name, COUNT(*) FROM users GROUP BY name", Auto),
    (
        "SELECT name, COUNT(*) FROM users GROUP BY name HAVING COUNT(*) > 1",
        Auto,
    ),
    ("SELECT * FROM users ORDER BY id DESC", Auto),
    ("SELECT * FROM users LIMIT 10", Auto),
    ("SELECT * FROM users LIMIT 10 OFFSET 5", Auto),
    ("SELECT * FROM a JOIN b ON a.id = b.a_id", Auto),
    ("SELECT * FROM a LEFT JOIN b ON a.id = b.a_id", Auto),
    (
        "SELECT * FROM a INNER JOIN b ON a.id = b.a_id WHERE b.x > 1",
        Auto,
    ),
    ("SELECT * FROM a CROSS JOIN b", Auto),
    ("SELECT * FROM (SELECT id FROM users) AS t", Auto),
    ("SELECT id FROM a UNION SELECT id FROM b", Auto),
    ("SELECT id FROM a UNION ALL SELECT id FROM b", Auto),
    ("SELECT id FROM a WHERE id IN (SELECT id FROM b)", Auto),
    (
        "SELECT id FROM a WHERE EXISTS (SELECT 1 FROM b WHERE b.id = a.id)",
        Auto,
    ),
    ("SELECT CASE WHEN a > 1 THEN 'x' ELSE 'y' END FROM t", Auto),
    ("SELECT COALESCE(a, b) FROM t", Auto),
    ("SELECT CAST(a AS CHAR(10)) FROM t", Auto),
    ("SELECT LOWER(name), UPPER(name), LENGTH(name) FROM t", Auto),
    ("SELECT ROW_NUMBER() OVER (ORDER BY id) FROM t", Auto),
    ("WITH c AS (SELECT 1 AS x) SELECT x FROM c", Auto),
    ("WITH a AS (SELECT 1), b AS (SELECT 2) SELECT * FROM a, b", Auto),
    (
        "WITH RECURSIVE c(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM c WHERE n < 5) SELECT * FROM c",
        Auto,
    ),
    // Keywords inside strings and comments are not statements.
    ("SELECT 'DROP TABLE users'", Auto),
    ("SELECT 'DELETE FROM users' AS label", Auto),
    ("SELECT 'a; DROP TABLE users'", Auto),
    ("SELECT '/* not a comment */'", Auto),
    ("SELECT '-- not a comment'", Auto),
    ("SELECT 'it''s; DROP TABLE x'", Auto),
    ("SELECT 1 -- DROP TABLE users", Auto),
    ("SELECT 1 /* DROP TABLE users */", Auto),
    ("/* DELETE FROM users */ SELECT 1", Auto),
    ("-- INSERT INTO x\nSELECT 1", Auto),
    ("SELECT 1 -- ; DROP TABLE x\n", Auto),
    ("SELECT 1 /* ; DROP TABLE x; */", Auto),
    ("SELECT * FROM users WHERE name = 'update'", Auto),
    ("SELECT * FROM users WHERE note LIKE '%insert%'", Auto),
    // Columns and aliases named like keywords.
    ("SELECT t.delete_flag FROM t", Auto),
    ("SELECT updated_at, deleted_at, created_at FROM t", Auto),
    ("SELECT 1 AS dropped", Auto),
    ("SELECT id AS inserted FROM t", Auto),
    ("SELECT * FROM update_log", Auto),
    ("SELECT * FROM drop_list", Auto),
    // Inserts.
    ("INSERT INTO t VALUES (1)", Write),
    ("insert into t (a) values (1)", Write),
    ("INSERT INTO t (a, b) VALUES (1, 2), (3, 4)", Write),
    ("INSERT INTO t SELECT * FROM u", Write),
    // Updates.
    ("UPDATE t SET a = 1", Write),
    ("update t set a = 1 where id = 2", Write),
    ("UPDATE t SET a = 1, b = 2 WHERE id IN (1, 2)", Write),
    ("UPDATE t SET a = (SELECT MAX(x) FROM u)", Write),
    // Deletes.
    ("DELETE FROM t", Write),
    ("delete from t where id = 1", Write),
    ("DELETE FROM t WHERE id IN (SELECT id FROM u)", Write),
    // DDL.
    ("CREATE TABLE t (id INTEGER)", Write),
    ("CREATE TABLE IF NOT EXISTS t (id INTEGER)", Write),
    ("CREATE TABLE t2 AS SELECT * FROM t", Write),
    ("CREATE VIEW v AS SELECT 1", Write),
    ("CREATE INDEX i ON t (a)", Write),
    ("CREATE UNIQUE INDEX i ON t (a)", Write),
    ("DROP TABLE t", Write),
    ("DROP TABLE IF EXISTS t", Write),
    ("DROP VIEW v", Write),
    ("DROP INDEX i", Write),
    ("ALTER TABLE t ADD COLUMN c INTEGER", Write),
    ("ALTER TABLE t DROP COLUMN c", Write),
    ("ALTER TABLE t RENAME TO u", Write),
    // Transactions are never unattended.
    ("BEGIN", Write),
    ("COMMIT", Write),
    ("ROLLBACK", Write),
    ("SAVEPOINT s", Write),
    ("RELEASE SAVEPOINT s", Write),
    ("ROLLBACK TO SAVEPOINT s", Write),
    // Scripts are judged per statement; the worst one decides.
    ("SELECT 1; DROP TABLE x", Write),
    ("SELECT 1;DROP TABLE x;", Write),
    ("SELECT 1; SELECT 2; DELETE FROM t", Write),
    ("DROP TABLE x; SELECT 1", Write),
    ("SELECT 1;\nUPDATE t SET a = 1", Write),
    ("BEGIN; DELETE FROM t; COMMIT", Write),
    ("SELECT 1; /* c */ INSERT INTO t VALUES (1)", Write),
    // Parse failures fail closed.
    ("SELEC 1", Write),
    ("SELECT (", Write),
    ("SELECT 'unterminated", Write),
    ("GARBAGE", Write),
    ("DROP", Write),
    ("!!!", Write),
    ("(", Write),
    ("SELECT * FROM", Write),
    ("UPDATE", Write),
    // Executable comments are code we cannot read, on every dialect.
    ("SELECT /*! SLEEP(5) */ 1", Write),
    ("SELECT 1 /*!50000 , 2 */", Write),
    ("SELECT 1 /*M! , 2 */", Write),
    ("/*! DROP TABLE x */", Write),
    ("/*!40101 SET NAMES utf8 */", Write),
    ("SELECT 1 /*! ; DROP TABLE x */", Write),
];

fn worst(sql: &str, dialect: Dialect) -> Want {
    let classified = classify_script(sql, dialect);
    if classified.iter().any(|c| !c.verdict.read_only) {
        Write
    } else if classified.iter().all(|c| c.verdict.ai_auto) {
        Auto
    } else {
        ReadNotAuto
    }
}

/// Checks every row, reporting all failures at once.
pub fn run(dialect: Dialect, specific: &[Case]) {
    let total = SHARED.len() + specific.len();
    assert!(total >= 150, "{dialect:?} has only {total} rows");
    let mut failures = Vec::new();
    for (sql, want) in SHARED.iter().chain(specific) {
        let got = worst(sql, dialect);
        if got != *want {
            failures.push(format!("{sql:?}: want {want:?}, got {got:?}"));
        }
        // The strict gate must agree: only a single Auto statement passes it.
        let single = classify_script(sql, dialect).len() == 1;
        let allowed = ai_auto_allowed(sql, dialect).is_ok();
        if allowed != (*want == Auto && single) {
            failures.push(format!(
                "{sql:?}: ai_auto_allowed = {allowed}, want {want:?}, single {single}"
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {total} rows failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// Rules that are the same on every dialect but need a scripted check.
pub fn run_edge_cases(dialect: Dialect) {
    // Empty input has nothing to run and is never approved for the AI.
    assert!(classify_script("", dialect).is_empty());
    assert!(classify_script("   ;; -- nothing\n", dialect).is_empty());
    assert!(ai_auto_allowed("", dialect).is_err());
    assert!(ai_auto_allowed("-- nothing", dialect).is_err());

    // Two reads are fine for a human, but the AI submits one statement at a time.
    let two = "SELECT 1; SELECT 2";
    assert_eq!(worst(two, dialect), Auto);
    assert!(ai_auto_allowed(two, dialect).is_err());

    // Very long input must neither crash nor pass the gate when it hides a write.
    let long = format!("SELECT {} 1", "1 + ".repeat(20_000));
    let _ = classify_script(&long, dialect);
    let hidden = format!("SELECT '{}'; DROP TABLE x", "a".repeat(200_000));
    assert_eq!(worst(&hidden, dialect), Write);
    assert!(ai_auto_allowed(&hidden, dialect).is_err());

    // Regression: a long operator chain overflowed the stack and aborted the process.
    for chain in [
        format!("SELECT {}1", "1 + ".repeat(200_000)),
        format!("SELECT 1 WHERE {}1 = 1", "1 = 1 AND ".repeat(100_000)),
        format!("SELECT {}1", "NOT ".repeat(100_000)),
    ] {
        assert_eq!(worst(&chain, dialect), Write);
        assert!(ai_auto_allowed(&chain, dialect).is_err());
    }
    // Wide but shallow input is ordinary and must still classify.
    let list = format!("SELECT * FROM t WHERE id IN ({}1)", "1, ".repeat(50_000));
    assert_eq!(worst(&list, dialect), Auto);
    let rows = format!("INSERT INTO t VALUES {}(1, 2)", "(1, 2), ".repeat(20_000));
    let classified = classify_script(&rows, dialect);
    assert_eq!(
        classified[0].verdict.kind,
        StatementKind::Insert,
        "a big insert must still parse"
    );

    // Deep nesting hits the parser's limit and fails closed instead of overflowing the stack.
    let deep = format!("SELECT {}1{}", "(".repeat(5_000), ")".repeat(5_000));
    assert_eq!(worst(&deep, dialect), Write);
}
