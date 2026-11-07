//! Runs against real servers named by `TEST_MYSQL_URL` and `TEST_MARIADB_URL`.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use driver_mysql::MySqlDriver;
use secrecy::SecretString;
use std::time::Duration;
use studio_db::{ConnectConfig, DbError, Driver, ExecOptions, Session};
use studio_types::{PageRequest, SortSpec, TableRef, TlsMode, Value};
use tokio::sync::mpsc;

/// Parses `mysql://user:pass@host:port/db`.
fn config(url: &str) -> ConnectConfig {
    let rest = url.split_once("://").unwrap().1;
    let (credentials, location) = rest.split_once('@').unwrap();
    let (user, password) = credentials.split_once(':').unwrap();
    let (address, database) = location.split_once('/').unwrap();
    let (host, port) = address.split_once(':').unwrap();
    let mut cfg = ConnectConfig::new(host, port.parse().unwrap(), database);
    cfg.username = user.to_owned();
    cfg.password = Some(SecretString::from(password.to_owned()));
    cfg.tls.mode = TlsMode::Disable;
    cfg
}

fn urls() -> Vec<String> {
    let mut found = Vec::new();
    for name in ["TEST_MYSQL_URL", "TEST_MARIADB_URL"] {
        match std::env::var(name) {
            Ok(url) => found.push(url),
            Err(_) => println!("skipped: {name} not set"),
        }
    }
    found
}

async fn run(session: &dyn Session, sql: &str) {
    let (sink, mut inbox) = mpsc::channel(8);
    let drain = tokio::spawn(async move { while inbox.recv().await.is_some() {} });
    session
        .execute(sql, ExecOptions::default(), sink)
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"));
    drain.await.unwrap();
}

#[tokio::test]
async fn conformance() {
    for url in urls() {
        studio_db::conformance::run_all(&MySqlDriver, config(&url)).await;
    }
}

#[tokio::test]
async fn backticks_in_a_table_name_work() {
    for url in urls() {
        let cfg = config(&url);
        let schema = cfg.database.clone();
        let session = MySqlDriver.connect(&cfg).await.unwrap();
        let session = session.as_ref();
        let name = format!("odd`name_{}", std::process::id());
        let quoted = studio_db::sql::qualified(studio_types::Engine::MySql, &schema, &name);
        run(
            session,
            &format!("CREATE TABLE {quoted} (id INT PRIMARY KEY, v INT)"),
        )
        .await;
        run(session, &format!("INSERT INTO {quoted} VALUES (1, 2)")).await;
        let table = TableRef { schema, name };
        let structure = session.table_structure(&table).await.expect("structure");
        assert_eq!(structure.columns.len(), 2);
        assert_eq!(structure.indexes.len(), 1);
        assert!(structure.indexes[0].primary);
        let page = PageRequest {
            limit: 10,
            offset: 0,
            sort: vec![SortSpec {
                column: "id".into(),
                descending: false,
            }],
            filters: vec![],
        };
        let rows = session.table_page(&table, &page).await.expect("page").rows;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0][1], Value::Int(2));
        let count = session
            .table_count(&table, &[], Duration::from_secs(5))
            .await
            .expect("count");
        assert_eq!(count, Some(1));
        run(session, &format!("DROP TABLE {quoted}")).await;
    }
}

#[tokio::test]
async fn a_second_statement_cannot_ride_along() {
    for url in urls() {
        let session = MySqlDriver.connect(&config(&url)).await.unwrap();
        let (sink, _inbox) = mpsc::channel(8);
        let result = session
            .execute("SELECT 1; SELECT 2", ExecOptions::default(), sink)
            .await;
        assert!(matches!(result, Err(DbError::Query { .. })), "got {result:?}");
    }
}

#[tokio::test]
async fn a_killed_sleep_is_reported_as_cancelled() {
    for url in urls() {
        let session = MySqlDriver.connect(&config(&url)).await.unwrap();
        let mut conn = session.acquire(studio_db::ConnScope::Isolated).await.unwrap();
        let canceller = conn.canceller();
        let (sink, mut inbox) = mpsc::channel(8);
        let drain = tokio::spawn(async move { while inbox.recv().await.is_some() {} });
        let stop = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            canceller.cancel().await.unwrap();
        });
        let result = conn
            .execute("SELECT SLEEP(30)", ExecOptions::default(), sink)
            .await;
        stop.await.unwrap();
        drain.await.unwrap();
        assert!(
            matches!(result, Err(DbError::Cancelled)),
            "SLEEP returns normally; got {result:?}"
        );
    }
}

#[tokio::test]
async fn the_watchdog_stops_what_the_server_limit_does_not() {
    for url in urls() {
        let session = MySqlDriver.connect(&config(&url)).await.unwrap();
        let (sink, mut inbox) = mpsc::channel(8);
        let drain = tokio::spawn(async move { while inbox.recv().await.is_some() {} });
        let opts = ExecOptions {
            timeout: Some(Duration::from_millis(200)),
            ..Default::default()
        };
        let result = session.execute("DO SLEEP(30)", opts, sink).await;
        drain.await.unwrap();
        assert!(matches!(result, Err(DbError::Timeout)), "got {result:?}");
    }
}

#[tokio::test]
async fn read_only_profile_refuses_writes_at_the_server() {
    for url in urls() {
        let mut cfg = config(&url);
        let name = format!("ro_{}", std::process::id());
        let plain = MySqlDriver.connect(&cfg).await.unwrap();
        run(plain.as_ref(), &format!("DROP TABLE IF EXISTS {name}")).await;
        run(plain.as_ref(), &format!("CREATE TABLE {name} (a INT)")).await;

        cfg.read_only = true;
        let session = MySqlDriver.connect(&cfg).await.unwrap();
        let (sink, _inbox) = mpsc::channel(8);
        let sql = format!("INSERT INTO {name} VALUES (1)");
        let result = session.execute(&sql, ExecOptions::default(), sink).await;
        assert!(
            matches!(result, Err(DbError::ReadOnlyViolation)),
            "got {result:?}"
        );
        run(plain.as_ref(), &format!("DROP TABLE {name}")).await;
    }
}

#[tokio::test]
async fn manual_transaction_commit_and_rollback() {
    for url in urls() {
        let session = MySqlDriver.connect(&config(&url)).await.unwrap();
        let session = session.as_ref();
        let name = format!("tx_{}", std::process::id());
        run(session, &format!("DROP TABLE IF EXISTS {name}")).await;
        run(session, &format!("CREATE TABLE {name} (a INT) ENGINE=InnoDB")).await;
        let cfg = TableRef {
            schema: config(&url).database,
            name: name.clone(),
        };
        let count = || async {
            session
                .table_count(&cfg, &[], Duration::from_secs(5))
                .await
                .unwrap()
        };

        session.begin(false).await.unwrap();
        run(session, &format!("INSERT INTO {name} VALUES (1)")).await;
        session.rollback().await.unwrap();
        assert_eq!(count().await, Some(0));

        session.begin(false).await.unwrap();
        run(session, &format!("INSERT INTO {name} VALUES (1)")).await;
        session.commit().await.unwrap();
        assert_eq!(count().await, Some(1));
        run(session, &format!("DROP TABLE {name}")).await;
    }
}

/// Regression: DDL commits implicitly, which ended a plain READ ONLY transaction and then ran.
#[tokio::test]
async fn ddl_cannot_escape_a_read_only_transaction() {
    for url in urls() {
        let session = MySqlDriver.connect(&config(&url)).await.unwrap();
        let session = session.as_ref();
        let name = format!("ddl_{}", std::process::id());
        run(session, &format!("DROP TABLE IF EXISTS {name}")).await;
        run(session, &format!("CREATE TABLE {name} (a INT)")).await;
        let read_only = ExecOptions {
            read_only_tx: true,
            ..Default::default()
        };
        for sql in [
            format!("DROP TABLE {name}"),
            format!("TRUNCATE TABLE {name}"),
            format!("ALTER TABLE {name} ADD COLUMN b INT"),
            format!("CREATE TABLE {name}_x (a INT)"),
        ] {
            let (sink, mut inbox) = mpsc::channel(8);
            let drain = tokio::spawn(async move { while inbox.recv().await.is_some() {} });
            let result = session.execute(&sql, read_only, sink).await;
            drain.await.unwrap();
            assert!(
                matches!(result, Err(DbError::ReadOnlyViolation)),
                "{sql}: {result:?}"
            );
        }
        // The session must be writable again, and the table must have survived.
        run(session, &format!("INSERT INTO {name} VALUES (1)")).await;
        run(session, &format!("DROP TABLE {name}")).await;
    }
}

fn tls_config(mode: TlsMode) -> Option<ConnectConfig> {
    let Ok(url) = std::env::var("TEST_MYSQL_TLS_URL") else {
        println!("skipped: TEST_MYSQL_TLS_URL not set");
        return None;
    };
    let _ = rustls::crypto::ring::default_provider().install_default();
    let mut cfg = config(&url);
    cfg.tls.mode = mode;
    Some(cfg)
}

fn ca_file() -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docker/tls/ca.crt");
    path.to_string_lossy().into_owned()
}

/// Proves the bytes really are encrypted, not just that a mode was asked for.
async fn cipher(session: &dyn Session) -> String {
    let (sink, mut inbox) = mpsc::channel(8);
    let collect = tokio::spawn(async move {
        let mut found = String::new();
        while let Some(event) = inbox.recv().await {
            if let studio_db::RowEvent::Batch(batch) = event {
                if let Some(Value::Text(text)) = batch.rows.first().and_then(|row| row.get(1)) {
                    found = text.clone();
                }
            }
        }
        found
    });
    let sql = "SHOW SESSION STATUS LIKE 'Ssl_cipher'";
    session.execute(sql, ExecOptions::default(), sink).await.unwrap();
    collect.await.unwrap()
}

#[tokio::test]
async fn verify_full_rejects_an_unknown_ca() {
    let Some(cfg) = tls_config(TlsMode::VerifyFull) else {
        return;
    };
    let error = MySqlDriver
        .connect(&cfg)
        .await
        .err()
        .expect("a private CA must be rejected");
    assert!(matches!(error, DbError::Tls(_)), "got {error:?}");
}

#[tokio::test]
async fn verify_full_passes_with_the_ca_file_and_encrypts() {
    let Some(mut cfg) = tls_config(TlsMode::VerifyFull) else {
        return;
    };
    cfg.tls.ca_path = Some(ca_file());
    let session = MySqlDriver.connect(&cfg).await.expect("connect");
    assert!(
        !cipher(session.as_ref()).await.is_empty(),
        "the connection must really be encrypted"
    );
    let info = session.server_info().await.unwrap();
    assert!(info.tls && info.tls_verified);
}

#[tokio::test]
async fn verify_full_rejects_the_wrong_host_name() {
    let Some(mut cfg) = tls_config(TlsMode::VerifyFull) else {
        return;
    };
    cfg.tls.ca_path = Some(ca_file());
    cfg.tls.server_name = Some("db.example.com".to_owned());
    let error = MySqlDriver
        .connect(&cfg)
        .await
        .err()
        .expect("the wrong name must fail");
    assert!(matches!(error, DbError::Tls(_)), "got {error:?}");
}

/// `mysql_async` forgives a host name mismatch by searching the error text for `NotValidForName`, and
/// current rustls words that error differently. So verify-ca is as strict as verify-full here. That fails
/// safe; when upstream fixes it, the mismatch case below will start connecting and this test will say so.
#[tokio::test]
async fn verify_ca_checks_the_chain_and_is_stricter_than_named() {
    let Some(mut cfg) = tls_config(TlsMode::VerifyCa) else {
        return;
    };
    let unknown = MySqlDriver.connect(&cfg).await.err();
    assert!(
        matches!(unknown, Some(DbError::Tls(_))),
        "an unknown CA must fail: {unknown:?}"
    );
    cfg.tls.ca_path = Some(ca_file());
    let session = MySqlDriver.connect(&cfg).await.expect("a valid chain and name");
    assert!(!cipher(session.as_ref()).await.is_empty());
    cfg.tls.server_name = Some("db.example.com".to_owned());
    let mismatch = MySqlDriver.connect(&cfg).await.err();
    assert!(
        matches!(mismatch, Some(DbError::Tls(_))),
        "see above: {mismatch:?}"
    );
}

#[tokio::test]
async fn require_encrypts_without_verifying() {
    let Some(cfg) = tls_config(TlsMode::Require) else {
        return;
    };
    let session = MySqlDriver.connect(&cfg).await.expect("connect");
    assert!(!cipher(session.as_ref()).await.is_empty());
    let info = session.server_info().await.unwrap();
    assert!(info.tls && !info.tls_verified);
}
