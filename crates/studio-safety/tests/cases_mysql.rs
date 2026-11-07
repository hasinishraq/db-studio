//! MySQL and MariaDB classifier table.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;
use common::*;
use studio_types::Dialect;

const CASES: &[Case] = &[
    ("SELECT FROM WHERE", Write),
    // Reads.
    ("SHOW TABLES", Auto),
    ("SHOW DATABASES", Auto),
    ("SHOW COLUMNS FROM t", Auto),
    ("SHOW CREATE TABLE t", Auto),
    ("SHOW VARIABLES", Auto),
    ("SHOW STATUS", Auto),
    ("SHOW PROCESSLIST", Auto),
    ("DESCRIBE t", Auto),
    ("DESC t", Auto),
    ("EXPLAIN SELECT 1", Auto),
    ("EXPLAIN FORMAT=JSON SELECT 1", Auto),
    ("EXPLAIN DELETE FROM t", Auto),
    ("SELECT `a` FROM `t`", Auto),
    ("SELECT `DROP` FROM `DELETE`", Auto),
    ("SELECT `a;b` FROM t", Auto),
    ("SELECT \"a; DROP TABLE x\"", Auto),
    ("SELECT NOW()", Auto),
    ("SELECT VERSION()", Auto),
    ("SELECT * FROM information_schema.tables", Auto),
    ("SELECT 1 # DROP TABLE x", Auto),
    ("SELECT 1 # ; DROP TABLE x\n", Auto),
    // Backslash keeps the string open here, unlike PostgreSQL.
    (r"SELECT 'a\'; DROP TABLE x; -- '", Auto),
    (r#"SELECT "a\"; DROP TABLE x; -- ""#, Auto),
    // `--` without a following space is arithmetic, so the next statement is real.
    ("SELECT 1--1; DROP TABLE x", Write),
    ("SELECT 1--1", Auto),
    ("SELECT 1-- 1\n", Auto),
    // Locks.
    ("SELECT * FROM t FOR UPDATE", ReadNotAuto),
    ("SELECT * FROM t FOR SHARE", ReadNotAuto),
    // Side-effect functions.
    ("SELECT SLEEP(5)", ReadNotAuto),
    ("SELECT sleep(5)", ReadNotAuto),
    ("SELECT `sleep`(5)", ReadNotAuto),
    ("SELECT BENCHMARK(1000000, MD5('a'))", ReadNotAuto),
    ("SELECT GET_LOCK('a', 10)", ReadNotAuto),
    ("SELECT RELEASE_LOCK('a')", ReadNotAuto),
    ("SELECT LOAD_FILE('/etc/passwd')", ReadNotAuto),
    ("SELECT 1 WHERE SLEEP(5) = 0", ReadNotAuto),
    ("SELECT * FROM t ORDER BY SLEEP(1)", ReadNotAuto),
    ("SELECT IF(1 = 1, SLEEP(5), 0)", ReadNotAuto),
    ("SELECT * FROM t WHERE id = (SELECT SLEEP(1))", ReadNotAuto),
    ("SELECT MASTER_POS_WAIT('a', 1)", ReadNotAuto),
    // SELECT INTO writes a file or a variable.
    ("SELECT * INTO OUTFILE '/tmp/x' FROM t", Write),
    ("SELECT * FROM t INTO OUTFILE '/tmp/x'", Write),
    ("SELECT 'x' INTO DUMPFILE '/tmp/x'", Write),
    // EXPLAIN ANALYZE really runs its statement.
    ("EXPLAIN ANALYZE DELETE FROM t", Write),
    ("EXPLAIN ANALYZE UPDATE t SET a = 1", Write),
    // Executable comments are the usual way to smuggle a write past a lexical check.
    ("SELECT 1 /*!50000 INTO OUTFILE '/tmp/x' */", Write),
    ("/*!50000 DROP TABLE x */", Write),
    ("/*!DELETE FROM t*/", Write),
    ("SELECT /*M!100100 SLEEP(5), */ 1", Write),
    // Writes.
    ("REPLACE INTO t VALUES (1)", Write),
    ("INSERT IGNORE INTO t VALUES (1)", Write),
    ("INSERT INTO t VALUES (1) ON DUPLICATE KEY UPDATE a = 1", Write),
    ("UPDATE a JOIN b ON a.id = b.id SET a.x = 1", Write),
    ("DELETE a FROM a JOIN b ON a.id = b.id", Write),
    ("DELETE FROM t ORDER BY id LIMIT 1", Write),
    ("TRUNCATE TABLE t", Write),
    ("RENAME TABLE a TO b", Write),
    ("CREATE DATABASE d", Write),
    ("DROP DATABASE d", Write),
    ("GRANT ALL ON *.* TO 'a'@'%'", Write),
    ("REVOKE ALL ON *.* FROM 'a'@'%'", Write),
    ("SET ROLE admin", Write),
    ("SET GLOBAL max_connections = 1", Write),
    ("SET @a = 1", Write),
    ("SET sql_mode = ''", Write),
    ("USE other", Write),
    ("LOAD DATA INFILE '/tmp/x' INTO TABLE t", Write),
    ("LOCK TABLES t WRITE", Write),
    ("UNLOCK TABLES", Write),
    ("FLUSH TABLES", Write),
    ("KILL 1", Write),
    ("CALL p()", Write),
    ("HANDLER t OPEN", Write),
    ("OPTIMIZE TABLE t", Write),
    ("START TRANSACTION", Write),
    ("PREPARE s FROM 'DROP TABLE x'", Write),
    ("EXECUTE s", Write),
];

#[test]
fn table() {
    run(Dialect::MySql, CASES);
}

#[test]
fn edge_cases() {
    run_edge_cases(Dialect::MySql);
}
