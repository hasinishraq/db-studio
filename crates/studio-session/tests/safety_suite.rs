//! The release-blocking safety suite, against every real engine.
//!
//! Each tricky write must be refused twice: by the classifier at the AI door, and then, when forced
//! past the classifier, by the database inside the read-only transaction. A sentinel table proves
//! nothing changed. Any failure blocks a release; fix the code, never this file.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use secrecy::SecretString;
use std::collections::HashMap;
use std::sync::Arc;
use studio_db::{ConnScope, DbError, Driver, ExecOptions};
use studio_safety::ApprovalBook;
use studio_session::testing::{profile, MemoryAudit};
use studio_session::{RunOptions, SessionError, SessionManager};
use studio_types::*;
use tokio::sync::mpsc;

struct Engine_ {
    label: &'static str,
    engine: Engine,
    driver: Arc<dyn Driver>,
    config: ConnectionProfile,
    password: Option<SecretString>,
    /// Keeps a temp SQLite file alive.
    _dir: Option<tempfile::TempDir>,
}

fn parse(url: &str, engine: Engine) -> (ConnectionProfile, SecretString) {
    let rest = url.split_once("://").unwrap().1;
    let (credentials, location) = rest.split_once('@').unwrap();
    let (user, password) = credentials.split_once(':').unwrap();
    let (address, database) = location.split_once('/').unwrap();
    let (host, port) = address.split_once(':').unwrap();
    let config = ConnectionProfile {
        engine,
        host: host.into(),
        port: port.parse().unwrap(),
        database: database.into(),
        username: user.into(),
        tls_mode: TlsMode::Disable,
        ..profile()
    };
    (config, SecretString::from(password.to_owned()))
}

fn engines() -> Vec<Engine_> {
    let mut found = Vec::new();
    let servers: [(&str, &str, Engine, Arc<dyn Driver>); 3] = [
        (
            "TEST_PG_URL",
            "PostgreSQL",
            Engine::Postgres,
            Arc::new(driver_postgres::PostgresDriver),
        ),
        (
            "TEST_MYSQL_URL",
            "MySQL",
            Engine::MySql,
            Arc::new(driver_mysql::MySqlDriver),
        ),
        (
            "TEST_MARIADB_URL",
            "MariaDB",
            Engine::MySql,
            Arc::new(driver_mysql::MySqlDriver),
        ),
    ];
    for (variable, label, engine, driver) in servers {
        match std::env::var(variable) {
            Ok(url) => {
                let (config, password) = parse(&url, engine);
                found.push(Engine_ {
                    label,
                    engine,
                    driver,
                    config,
                    password: Some(password),
                    _dir: None,
                });
            }
            Err(_) => println!("skipped: {variable} not set"),
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let database = dir.path().join("suite.db").to_string_lossy().into_owned();
    let config = ConnectionProfile {
        engine: Engine::Sqlite,
        host: String::new(),
        port: 0,
        database,
        ..profile()
    };
    found.push(Engine_ {
        label: "SQLite",
        engine: Engine::Sqlite,
        driver: Arc::new(driver_sqlite::SqliteDriver),
        config,
        password: None,
        _dir: Some(dir),
    });
    found
}

struct Fixture {
    manager: SessionManager,
    approvals: Arc<ApprovalBook>,
    session: SessionId,
    engine: Engine,
    table: String,
    label: &'static str,
    audit: MemoryAudit,
}

impl Fixture {
    async fn new(e: &Engine_, read_only: bool) -> Self {
        let approvals = Arc::new(ApprovalBook::new());
        let audit = MemoryAudit::default();
        let drivers: HashMap<Engine, Arc<dyn Driver>> = HashMap::from([(e.engine, e.driver.clone())]);
        let manager = SessionManager::new(drivers, approvals.clone(), Arc::new(audit.clone()));
        let config = ConnectionProfile {
            read_only,
            ..e.config.clone()
        };
        let session = manager
            .connect(config, e.password.clone(), None)
            .await
            .unwrap()
            .session_id;
        let suffix = uuid_suffix();
        let table = format!("sentinel_{suffix}");
        Self {
            manager,
            approvals,
            session,
            engine: e.engine,
            table,
            label: e.label,
            audit,
        }
    }

    /// Runs setup SQL through the approval path, as a human would.
    async fn admin(&self, sql: &str) {
        let token = self.approvals.issue(self.session, sql, Origin::Editor);
        let (sink, mut inbox) = mpsc::channel(64);
        let drain = tokio::spawn(async move { while inbox.recv().await.is_some() {} });
        let outcomes = self
            .manager
            .run_user(
                self.session,
                sql,
                Some(token),
                RunId::new(),
                RunOptions::default(),
                sink,
            )
            .await
            .unwrap_or_else(|e| panic!("{}: {sql}: {e}", self.label));
        drain.await.unwrap();
        for outcome in outcomes {
            assert!(
                !matches!(outcome, StatementOutcome::Error { .. }),
                "{}: {sql}: {outcome:?}",
                self.label
            );
        }
    }

    async fn create_sentinel(&self) {
        self.admin(&format!(
            "CREATE TABLE {} (id INTEGER PRIMARY KEY, note VARCHAR(50))",
            self.table
        ))
        .await;
        self.admin(&format!(
            "INSERT INTO {} (id, note) VALUES (1, 'a'), (2, 'b'), (3, 'c')",
            self.table
        ))
        .await;
    }

    async fn drop_sentinel(&self) {
        self.admin(&format!("DROP TABLE {}", self.table)).await;
    }

    /// The sentinel's row count and checksum, read through the safe door.
    async fn fingerprint(&self) -> Vec<Vec<Value>> {
        let sql = format!("SELECT id, note FROM {} ORDER BY id", self.table);
        self.manager
            .run_ai_read(self.session, &sql, QueryId::new())
            .await
            .unwrap_or_else(|e| panic!("{}: the sentinel is gone: {e}", self.label))
            .rows
    }

    /// Skips the classifier and runs the statement in the read-only transaction directly.
    async fn force_read_only(&self, sql: &str) -> Result<(), DbError> {
        let open = self.manager.get(self.session).unwrap();
        let mut conn = open.session.acquire(ConnScope::Isolated).await.unwrap();
        let (sink, mut inbox) = mpsc::channel(8);
        let drain = tokio::spawn(async move { while inbox.recv().await.is_some() {} });
        let opts = ExecOptions {
            read_only_tx: true,
            ..Default::default()
        };
        let result = conn.execute(sql, opts, sink).await;
        drain.await.unwrap();
        result.map(|_| ())
    }
}

fn uuid_suffix() -> String {
    format!("{:x}", SessionId::new().0.as_u128() & 0xffff_ffff_ffff)
}

/// Statements `studio-safety` lets run unattended. None may change anything.
fn auto_reads(engine: Engine, t: &str) -> Vec<String> {
    let mut list: Vec<String> = [
        format!("SELECT * FROM {t}"),
        format!("SELECT COUNT(*) FROM {t}"),
        format!("SELECT id FROM {t} WHERE note = 'a'"),
        format!("SELECT MAX(id), MIN(id) FROM {t}"),
        format!("SELECT note, COUNT(*) FROM {t} GROUP BY note"),
        format!("SELECT * FROM {t} ORDER BY id DESC LIMIT 1"),
        format!("SELECT a.id FROM {t} a JOIN {t} b ON a.id = b.id"),
        format!("SELECT * FROM (SELECT id FROM {t}) AS s"),
        format!("SELECT id FROM {t} UNION SELECT id FROM {t}"),
        format!("WITH c AS (SELECT id FROM {t}) SELECT * FROM c"),
        format!("SELECT 'DROP TABLE {t}'"),
        format!("SELECT 1 -- DELETE FROM {t}"),
        format!("SELECT 1 /* ; DROP TABLE {t} */"),
        "SELECT 1".to_owned(),
    ]
    .to_vec();
    match engine {
        Engine::Postgres => list.extend([
            format!("EXPLAIN SELECT * FROM {t}"),
            format!("EXPLAIN DELETE FROM {t}"),
            format!("EXPLAIN ANALYZE SELECT * FROM {t}"),
            "SHOW server_version".to_owned(),
            format!("SELECT $$DROP TABLE {t}$$"),
        ]),
        Engine::MySql => list.extend([
            format!("EXPLAIN SELECT * FROM {t}"),
            format!("EXPLAIN DELETE FROM {t}"),
            "SHOW TABLES".to_owned(),
            format!("SHOW COLUMNS FROM {t}"),
            format!("DESCRIBE {t}"),
        ]),
        Engine::Sqlite => list.extend([
            format!("EXPLAIN QUERY PLAN SELECT * FROM {t}"),
            format!("PRAGMA table_info({t})"),
            "PRAGMA database_list".to_owned(),
        ]),
    }
    list
}

/// Known tricky writes. The classifier must refuse each, and the database must refuse each again.
fn tricky_writes(engine: Engine, t: &str) -> Vec<String> {
    let mut list = vec![
        format!("DELETE FROM {t}"),
        format!("UPDATE {t} SET note = 'x'"),
        format!("INSERT INTO {t} (id, note) VALUES (99, 'x')"),
        format!("DROP TABLE {t}"),
    ];
    match engine {
        Engine::Postgres => list.extend([
            format!("WITH d AS (DELETE FROM {t} RETURNING *) SELECT * FROM d"),
            format!("WITH u AS (UPDATE {t} SET note = 'x' RETURNING *) SELECT * FROM u"),
            format!("EXPLAIN ANALYZE DELETE FROM {t}"),
            format!("EXPLAIN (ANALYZE) DELETE FROM {t}"),
            format!("SELECT * INTO {t}_copy FROM {t}"),
            format!("TRUNCATE {t}"),
            format!("ALTER TABLE {t} ADD COLUMN evil INTEGER"),
            format!("CREATE TABLE {t}_new (a INTEGER)"),
        ]),
        Engine::MySql => list.extend([
            format!("TRUNCATE TABLE {t}"),
            format!("ALTER TABLE {t} ADD COLUMN evil INTEGER"),
            format!("CREATE TABLE {t}_new (a INTEGER)"),
            format!("REPLACE INTO {t} (id, note) VALUES (1, 'x')"),
        ]),
        Engine::Sqlite => list.extend([
            format!("ALTER TABLE {t} ADD COLUMN evil INTEGER"),
            format!("CREATE TABLE {t}_new (a INTEGER)"),
            format!("REPLACE INTO {t} (id, note) VALUES (1, 'x')"),
        ]),
    }
    list
}

#[tokio::test(flavor = "multi_thread")]
async fn every_ai_auto_statement_changes_nothing() {
    for e in engines() {
        let f = Fixture::new(&e, false).await;
        f.create_sentinel().await;
        let before = f.fingerprint().await;
        for sql in auto_reads(f.engine, &f.table) {
            let result = f.manager.run_ai_read(f.session, &sql, QueryId::new()).await;
            // MySQL refuses even `EXPLAIN DELETE` in a read-only transaction. Being stricter than the
            // classifier is fine; the rule is that nothing changes, and that is checked just below.
            let stricter = matches!(result, Err(SessionError::Db(DbError::ReadOnlyViolation)));
            assert!(result.is_ok() || stricter, "{}: {sql}: {result:?}", f.label);
            assert_eq!(f.fingerprint().await, before, "{}: {sql} changed data", f.label);
        }
        f.drop_sentinel().await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn tricky_writes_are_refused_by_the_door_and_by_the_database() {
    for e in engines() {
        let f = Fixture::new(&e, false).await;
        f.create_sentinel().await;
        let before = f.fingerprint().await;
        for sql in tricky_writes(f.engine, &f.table) {
            let door = f.manager.run_ai_read(f.session, &sql, QueryId::new()).await;
            assert!(
                matches!(door, Err(SessionError::Refused(_))),
                "{}: {sql} passed the door",
                f.label
            );

            let forced = f.force_read_only(&sql).await;
            assert!(forced.is_err(), "{}: the database accepted {sql}", f.label);
            assert_eq!(f.fingerprint().await, before, "{}: {sql} changed data", f.label);
        }
        f.drop_sentinel().await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn multi_statement_scripts_are_refused_at_every_layer() {
    for e in engines() {
        let f = Fixture::new(&e, false).await;
        f.create_sentinel().await;
        let before = f.fingerprint().await;
        let script = format!("SELECT 1; DELETE FROM {}", f.table);
        let door = f.manager.run_ai_read(f.session, &script, QueryId::new()).await;
        assert!(matches!(door, Err(SessionError::Refused(_))), "{}", f.label);
        assert!(
            f.force_read_only(&script).await.is_err(),
            "{}: a second statement ran",
            f.label
        );
        assert_eq!(f.fingerprint().await, before, "{}", f.label);
        f.drop_sentinel().await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn executable_comments_are_refused_on_mysql() {
    for e in engines().into_iter().filter(|e| e.engine == Engine::MySql) {
        let f = Fixture::new(&e, false).await;
        f.create_sentinel().await;
        let before = f.fingerprint().await;
        let sql = format!("SELECT 1 /*!50000 , (SELECT COUNT(*) FROM {}) */", f.table);
        let door = f.manager.run_ai_read(f.session, &sql, QueryId::new()).await;
        assert!(matches!(door, Err(SessionError::Refused(_))), "{}", f.label);
        assert_eq!(f.fingerprint().await, before);
        f.drop_sentinel().await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn approval_tokens_fail_on_reuse_other_sql_session_and_origin() {
    for e in engines() {
        let f = Fixture::new(&e, false).await;
        f.create_sentinel().await;
        let before = f.fingerprint().await;
        let write = format!("DELETE FROM {} WHERE id = 1", f.table);
        let run = |sql: String, token: ApprovalToken| {
            let manager = &f.manager;
            let session = f.session;
            async move {
                let (sink, mut inbox) = mpsc::channel(64);
                let drain = tokio::spawn(async move { while inbox.recv().await.is_some() {} });
                let result = manager
                    .run_user(
                        session,
                        &sql,
                        Some(token),
                        RunId::new(),
                        RunOptions::default(),
                        sink,
                    )
                    .await;
                drain.await.unwrap();
                result
            }
        };

        let other_sql = f
            .approvals
            .issue(f.session, "DELETE FROM nothing", Origin::Editor);
        assert_eq!(
            run(write.clone(), other_sql).await,
            Err(SessionError::ApprovalMismatch)
        );

        let other_session = f.approvals.issue(SessionId::new(), &write, Origin::Editor);
        assert_eq!(
            run(write.clone(), other_session).await,
            Err(SessionError::ApprovalMismatch)
        );

        let other_origin = f.approvals.issue(f.session, &write, Origin::Ai);
        assert_eq!(
            run(write.clone(), other_origin).await,
            Err(SessionError::ApprovalMismatch)
        );
        assert_eq!(
            f.fingerprint().await,
            before,
            "{}: a refused token changed data",
            f.label
        );

        let good = f.approvals.issue(f.session, &write, Origin::Editor);
        run(write.clone(), good.clone()).await.unwrap();
        assert_eq!(
            f.fingerprint().await.len(),
            before.len() - 1,
            "{}: a good token must work",
            f.label
        );
        assert_eq!(
            run(write, good).await,
            Err(SessionError::ApprovalRequired),
            "{}: reuse",
            f.label
        );
        f.drop_sentinel().await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn read_only_profiles_refuse_writes_with_a_valid_token() {
    for e in engines() {
        let setup = Fixture::new(&e, false).await;
        setup.create_sentinel().await;

        let f = Fixture::new(&e, true).await;
        let write = format!("DELETE FROM {}", setup.table);
        let token = f.approvals.issue(f.session, &write, Origin::Editor);
        let (sink, mut inbox) = mpsc::channel(64);
        let drain = tokio::spawn(async move { while inbox.recv().await.is_some() {} });
        let result = f
            .manager
            .run_user(
                f.session,
                &write,
                Some(token),
                RunId::new(),
                RunOptions::default(),
                sink,
            )
            .await;
        drain.await.unwrap();
        assert_eq!(result, Err(SessionError::ReadOnlyProfile), "{}", f.label);

        let token = f.approvals.issue(f.session, &write, Origin::Ai);
        let result = f
            .manager
            .run_ai_approved(f.session, &write, QueryId::new(), &token)
            .await;
        assert_eq!(result, Err(SessionError::ReadOnlyProfile), "{}", f.label);

        assert_eq!(setup.fingerprint().await.len(), 3, "{}", f.label);
        setup.drop_sentinel().await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn an_approved_ai_write_is_audited_on_a_real_database() {
    for e in engines() {
        let f = Fixture::new(&e, false).await;
        f.create_sentinel().await;
        let write = format!("UPDATE {} SET note = 'z' WHERE id = 1", f.table);
        let token = f.approvals.issue(f.session, &write, Origin::Ai);
        let done = f
            .manager
            .run_ai_approved(f.session, &write, QueryId::new(), &token)
            .await
            .unwrap();
        assert_eq!(done.rows_affected, Some(1), "{}", f.label);
        let entries = f.audit.entries.lock().unwrap().clone();
        let entry = entries.iter().find(|a| a.sql_text == write).expect("audited");
        assert_eq!(entry.origin, Origin::Ai);
        assert_eq!(entry.rows_affected, Some(1));
        assert_eq!(entry.outcome, "ok");
        f.drop_sentinel().await;
    }
}
