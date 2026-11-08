//! The grid's door against real databases: all or nothing, one row per change.
//! SQLite always runs. PostgreSQL, MySQL and MariaDB run when TEST_PG_URL, TEST_MYSQL_URL or
//! TEST_MARIADB_URL is set.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use secrecy::SecretString;
use std::collections::HashMap;
use std::sync::Arc;
use studio_db::Driver;
use studio_safety::ApprovalBook;
use studio_session::testing::{profile, MemoryAudit};
use studio_session::{RunOptions, SessionError, SessionManager};
use studio_types::*;
use tokio::sync::mpsc;

struct Server {
    label: &'static str,
    engine: Engine,
    driver: Arc<dyn Driver>,
    config: ConnectionProfile,
    password: Option<SecretString>,
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

fn servers() -> Vec<Server> {
    let mut found = Vec::new();
    let remote: [(&str, &'static str, Engine, Arc<dyn Driver>); 3] = [
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
    for (variable, label, engine, driver) in remote {
        match std::env::var(variable) {
            Ok(url) => {
                let (config, password) = parse(&url, engine);
                found.push(Server {
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
    let config = ConnectionProfile {
        engine: Engine::Sqlite,
        host: String::new(),
        port: 0,
        database: dir.path().join("grid.db").to_string_lossy().into_owned(),
        ..profile()
    };
    found.push(Server {
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
    table: String,
    label: &'static str,
}

impl Fixture {
    async fn new(server: &Server) -> Self {
        let approvals = Arc::new(ApprovalBook::new());
        let drivers: HashMap<Engine, Arc<dyn Driver>> =
            HashMap::from([(server.engine, server.driver.clone())]);
        let manager = SessionManager::new(drivers, approvals.clone(), Arc::new(MemoryAudit::default()));
        let session = manager
            .connect(server.config.clone(), server.password.clone(), None)
            .await
            .unwrap()
            .session_id;
        let table = format!("grid_{:x}", SessionId::new().0.as_u128() & 0xffff_ffff_ffff);
        let fixture = Self {
            manager,
            approvals,
            session,
            table,
            label: server.label,
        };
        let t = &fixture.table;
        fixture
            .admin(&format!(
                "CREATE TABLE {t} (id INTEGER PRIMARY KEY, note VARCHAR(50), n INTEGER)"
            ))
            .await;
        fixture
            .admin(&format!(
                "INSERT INTO {t} (id, note, n) VALUES (1, 'a', 10), (2, 'b', 20), (3, 'c', 30)"
            ))
            .await;
        fixture
    }

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

    async fn save(&self, sql: &str) -> Result<u64, SessionError> {
        let token = self.approvals.issue(self.session, sql, Origin::Grid);
        self.manager
            .run_changes(self.session, sql, &token, RunId::new())
            .await
    }

    async fn contents(&self) -> Vec<String> {
        let sql = format!("SELECT id, note, n FROM {} ORDER BY id", self.table);
        let rows = self
            .manager
            .run_ai_read(self.session, &sql, QueryId::new())
            .await
            .unwrap()
            .rows;
        rows.iter()
            .map(|row| {
                row.iter()
                    .map(|v| match v {
                        Value::Null => "NULL".to_owned(),
                        Value::Int(n) => n.to_string(),
                        Value::BigInt(s) | Value::Text(s) | Value::Decimal(s) => s.clone(),
                        other => format!("{other:?}"),
                    })
                    .collect::<Vec<_>>()
                    .join("|")
            })
            .collect()
    }

    async fn finish(self) {
        let _ = self.manager.set_tx_mode(self.session, TxMode::Auto).await;
        self.admin(&format!("DROP TABLE {}", self.table)).await;
    }
}

const START: [&str; 3] = ["1|a|10", "2|b|20", "3|c|30"];

#[tokio::test]
async fn a_mixed_save_commits_every_change() {
    for server in servers() {
        let f = Fixture::new(&server).await;
        let t = &f.table;
        let sql = format!(
            "DELETE FROM {t} WHERE id = '3';\n\
             UPDATE {t} SET note = 'A', n = NULL WHERE id = '1' AND note = 'a' AND n = '10';\n\
             INSERT INTO {t} (id, note) VALUES ('4', 'it''s')"
        );
        assert_eq!(f.save(&sql).await, Ok(3), "{}", f.label);
        assert_eq!(
            f.contents().await,
            vec!["1|A|NULL", "2|b|20", "4|it's|NULL"],
            "{}",
            f.label
        );
        f.finish().await;
    }
}

#[tokio::test]
async fn a_failing_change_leaves_the_table_untouched() {
    for server in servers() {
        let f = Fixture::new(&server).await;
        let t = &f.table;
        // The second insert repeats a key, so the server refuses it after the first two ran.
        let sql = format!(
            "UPDATE {t} SET note = 'z' WHERE id = '1';\n\
             DELETE FROM {t} WHERE id = '2';\n\
             INSERT INTO {t} (id, note) VALUES ('3', 'dup')"
        );
        assert!(
            matches!(f.save(&sql).await, Err(SessionError::Db(_))),
            "{}",
            f.label
        );
        assert_eq!(f.contents().await, START, "{}", f.label);
        f.finish().await;
    }
}

#[tokio::test]
async fn a_row_changed_since_loading_refuses_the_save() {
    for server in servers() {
        let f = Fixture::new(&server).await;
        let t = &f.table;
        // Someone else changes row 2 after the grid loaded it with note = 'b'.
        f.admin(&format!("UPDATE {t} SET note = 'other' WHERE id = 2"))
            .await;
        let sql = format!(
            "DELETE FROM {t} WHERE id = '1';\n\
             UPDATE {t} SET note = 'mine' WHERE id = '2' AND note = 'b'"
        );
        let error = f.save(&sql).await.unwrap_err();
        assert!(
            error.to_string().contains("Change 2 of 2"),
            "{}: {error}",
            f.label
        );
        assert_eq!(
            f.contents().await,
            vec!["1|a|10", "2|other|20", "3|c|30"],
            "{}",
            f.label
        );
        f.finish().await;
    }
}

#[tokio::test]
async fn saving_the_same_value_counts_as_one_row() {
    // MySQL reports rows changed unless the client asks for rows found; the driver asks.
    for server in servers() {
        let f = Fixture::new(&server).await;
        let t = &f.table;
        let sql = format!("UPDATE {t} SET note = 'a' WHERE id = '1' AND note = 'a'");
        assert_eq!(f.save(&sql).await, Ok(1), "{}", f.label);
        f.finish().await;
    }
}

#[tokio::test]
async fn manual_mode_failure_keeps_earlier_work_in_the_transaction() {
    for server in servers() {
        let f = Fixture::new(&server).await;
        let t = &f.table;
        f.manager.set_tx_mode(f.session, TxMode::Manual).await.unwrap();
        f.admin(&format!("UPDATE {t} SET n = 99 WHERE id = 3")).await;

        let bad = format!(
            "UPDATE {t} SET note = 'z' WHERE id = '1';\n\
             INSERT INTO {t} (id, note) VALUES ('2', 'dup')"
        );
        assert!(f.save(&bad).await.is_err(), "{}", f.label);
        let good = format!("UPDATE {t} SET note = 'B' WHERE id = '2' AND note = 'b'");
        assert_eq!(
            f.save(&good).await,
            Ok(1),
            "{}: the transaction must still work",
            f.label
        );
        f.manager.commit(f.session).await.unwrap();
        assert_eq!(
            f.contents().await,
            vec!["1|a|10", "2|B|20", "3|c|99"],
            "{}",
            f.label
        );
        f.finish().await;
    }
}
