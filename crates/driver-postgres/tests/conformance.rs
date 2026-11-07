//! Runs against real servers named by `TEST_PG_URL` and `TEST_PG_TLS_URL`.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use driver_postgres::PostgresDriver;
use secrecy::SecretString;
use std::path::PathBuf;
use studio_db::{ConnectConfig, DbError, Driver, ExecOptions, RowEvent, Value};
use studio_types::TlsMode;
use tokio::sync::mpsc;

/// Parses `postgres://user:pass@host:port/db`.
fn config(url: &str, mode: TlsMode) -> ConnectConfig {
    let rest = url.split_once("://").unwrap().1;
    let (credentials, location) = rest.split_once('@').unwrap();
    let (user, password) = credentials.split_once(':').unwrap();
    let (address, database) = location.split_once('/').unwrap();
    let (host, port) = address.split_once(':').unwrap();
    let mut cfg = ConnectConfig::new(host, port.parse().unwrap(), database);
    cfg.username = user.to_owned();
    cfg.password = Some(SecretString::from(password.to_owned()));
    cfg.tls.mode = mode;
    cfg
}

fn env(name: &str) -> Option<String> {
    let value = std::env::var(name).ok();
    if value.is_none() {
        println!("skipped: {name} not set");
    }
    value
}

fn ca_file() -> String {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docker/tls/ca.crt");
    root.to_string_lossy().into_owned()
}

async fn scalar(session: &dyn studio_db::Session, sql: &str) -> Value {
    let (sink, mut inbox) = mpsc::channel(8);
    session.execute(sql, ExecOptions::default(), sink).await.unwrap();
    while let Some(event) = inbox.recv().await {
        if let RowEvent::Batch(mut batch) = event {
            return batch.rows.remove(0).remove(0);
        }
    }
    panic!("no rows");
}

#[tokio::test]
async fn conformance() {
    let Some(url) = env("TEST_PG_URL") else { return };
    studio_db::conformance::run_all(&PostgresDriver, config(&url, TlsMode::Disable)).await;
}

#[tokio::test]
async fn verify_full_rejects_an_unknown_ca() {
    let Some(url) = env("TEST_PG_TLS_URL") else { return };
    let result = PostgresDriver.connect(&config(&url, TlsMode::VerifyFull)).await;
    let error = result.err().expect("a self-signed server must be rejected");
    assert!(matches!(error, DbError::Tls(_)), "got {error:?}");
}

#[tokio::test]
async fn verify_full_passes_with_the_ca_file() {
    let Some(url) = env("TEST_PG_TLS_URL") else { return };
    let mut cfg = config(&url, TlsMode::VerifyFull);
    cfg.tls.ca_path = Some(ca_file());
    let session = PostgresDriver.connect(&cfg).await.expect("connect");
    let info = session.server_info().await.unwrap();
    assert!(info.tls && info.tls_verified);
    assert_eq!(info.certificates.len(), 1, "the server certificate is reported");
    assert!(info.certificates[0].subject.contains("localhost"));
    let ssl = scalar(
        session.as_ref(),
        "SELECT ssl FROM pg_stat_ssl WHERE pid = pg_backend_pid()",
    )
    .await;
    assert_eq!(ssl, Value::Bool(true));
}

#[tokio::test]
async fn verify_full_rejects_the_wrong_host_name() {
    let Some(url) = env("TEST_PG_TLS_URL") else { return };
    let mut cfg = config(&url, TlsMode::VerifyFull);
    cfg.tls.ca_path = Some(ca_file());
    cfg.tls.server_name = Some("db.example.com".to_owned());
    cfg.host = "127.0.0.1".to_owned();
    let error = PostgresDriver
        .connect(&cfg)
        .await
        .err()
        .expect("wrong name must fail");
    assert!(matches!(error, DbError::Tls(_)), "got {error:?}");
}

#[tokio::test]
async fn verify_ca_ignores_only_the_host_name() {
    let Some(url) = env("TEST_PG_TLS_URL") else { return };
    let mut cfg = config(&url, TlsMode::VerifyCa);
    cfg.tls.ca_path = Some(ca_file());
    cfg.tls.server_name = Some("db.example.com".to_owned());
    cfg.host = "127.0.0.1".to_owned();
    PostgresDriver
        .connect(&cfg)
        .await
        .expect("the chain is valid, so the name is forgiven");

    let error = PostgresDriver
        .connect(&config(&url, TlsMode::VerifyCa))
        .await
        .err();
    assert!(
        matches!(error, Some(DbError::Tls(_))),
        "an unknown CA must still fail: {error:?}"
    );
}

#[tokio::test]
async fn require_encrypts_without_verifying() {
    let Some(url) = env("TEST_PG_TLS_URL") else { return };
    let session = PostgresDriver
        .connect(&config(&url, TlsMode::Require))
        .await
        .expect("connect");
    let info = session.server_info().await.unwrap();
    assert!(info.tls && !info.tls_verified);
}

#[tokio::test]
async fn dates_round_trip_in_a_non_utc_session() {
    let Some(url) = env("TEST_PG_URL") else { return };
    let session = PostgresDriver
        .connect(&config(&url, TlsMode::Disable))
        .await
        .unwrap();
    let mut conn = session.acquire(studio_db::ConnScope::Isolated).await.unwrap();
    let (sink, mut inbox) = mpsc::channel(8);
    let drain = tokio::spawn(async move {
        let mut rows = Vec::new();
        while let Some(event) = inbox.recv().await {
            if let RowEvent::Batch(batch) = event {
                rows.extend(batch.rows);
            }
        }
        rows
    });
    let none = ExecOptions::default();
    conn.execute("SET TIME ZONE 'Pacific/Auckland'", none, sink.clone())
        .await
        .unwrap();
    conn.execute("SELECT DATE '2026-01-01'", none, sink)
        .await
        .unwrap();
    let rows = drain.await.unwrap();
    assert_eq!(rows[0][0], Value::Temporal("2026-01-01".into()));
}

#[tokio::test]
async fn a_second_command_cannot_ride_along() {
    let Some(url) = env("TEST_PG_URL") else { return };
    let session = PostgresDriver
        .connect(&config(&url, TlsMode::Disable))
        .await
        .unwrap();
    let (sink, _inbox) = mpsc::channel(8);
    let result = session
        .execute("SELECT 1; SELECT 2", ExecOptions::default(), sink)
        .await;
    assert!(matches!(result, Err(DbError::Query { .. })), "got {result:?}");
}

#[tokio::test]
async fn read_only_profile_defaults_to_read_only() {
    let Some(url) = env("TEST_PG_URL") else { return };
    let mut cfg = config(&url, TlsMode::Disable);
    cfg.read_only = true;
    let session = PostgresDriver.connect(&cfg).await.unwrap();
    let value = scalar(session.as_ref(), "SHOW default_transaction_read_only").await;
    assert_eq!(value, Value::Text("on".into()));
}

#[tokio::test]
async fn manual_transaction_commit_and_rollback() {
    let Some(url) = env("TEST_PG_URL") else { return };
    let session = PostgresDriver
        .connect(&config(&url, TlsMode::Disable))
        .await
        .unwrap();
    let name = format!("tx_{}", std::process::id());
    let run = |sql: String| {
        let session = &session;
        async move {
            let (sink, mut inbox) = mpsc::channel(8);
            let drain = tokio::spawn(async move { while inbox.recv().await.is_some() {} });
            session.execute(&sql, ExecOptions::default(), sink).await.unwrap();
            drain.await.unwrap();
        }
    };
    run(format!("CREATE TABLE {name} (a int)")).await;
    session.begin(false).await.unwrap();
    run(format!("INSERT INTO {name} VALUES (1)")).await;
    session.rollback().await.unwrap();
    let count = format!("SELECT COUNT(*)::int4 FROM {name}");
    assert_eq!(scalar(session.as_ref(), &count).await, Value::Int(0));
    session.begin(false).await.unwrap();
    run(format!("INSERT INTO {name} VALUES (1)")).await;
    session.commit().await.unwrap();
    assert_eq!(scalar(session.as_ref(), &count).await, Value::Int(1));
    run(format!("DROP TABLE {name}")).await;
}

/// Regression: a peer that accepts and then says nothing used to block `connect` forever.
#[tokio::test]
async fn a_silent_server_times_out() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let hold = tokio::spawn(async move {
        let mut open = Vec::new();
        while let Ok((socket, _)) = listener.accept().await {
            open.push(socket);
        }
    });
    let mut cfg = ConnectConfig::new("127.0.0.1", port, "db");
    cfg.username = "me".to_owned();
    cfg.tls.mode = TlsMode::Disable;
    cfg.connect_timeout = std::time::Duration::from_millis(500);
    let result = tokio::time::timeout(std::time::Duration::from_secs(5), PostgresDriver.connect(&cfg)).await;
    hold.abort();
    let outcome = result.expect("connect must give up by itself");
    assert!(matches!(outcome, Err(DbError::Connect(_))), "{:?}", outcome.err());
}
