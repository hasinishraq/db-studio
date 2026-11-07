//! SQLite needs no server, so this always runs.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use driver_sqlite::SqliteDriver;
use studio_db::{ConnectConfig, DbError, Driver, ExecOptions, Session};
use tempfile::TempDir;
use tokio::sync::mpsc;

fn config(dir: &TempDir) -> ConnectConfig {
    ConnectConfig::new("", 0, dir.path().join("test.db").to_string_lossy().into_owned())
}

async fn run(session: &dyn Session, sql: &str) -> Result<(), DbError> {
    let (sink, mut inbox) = mpsc::channel(8);
    let drain = tokio::spawn(async move { while inbox.recv().await.is_some() {} });
    let result = session.execute(sql, ExecOptions::default(), sink).await;
    drain.await.unwrap();
    result.map(|_| ())
}

#[tokio::test(flavor = "multi_thread")]
async fn conformance() {
    let dir = tempfile::tempdir().unwrap();
    studio_db::conformance::run_all(&SqliteDriver, config(&dir)).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_second_statement_cannot_ride_along() {
    let dir = tempfile::tempdir().unwrap();
    let session = SqliteDriver.connect(&config(&dir)).await.unwrap();
    let result = run(session.as_ref(), "SELECT 1; SELECT 2").await;
    assert!(matches!(result, Err(DbError::Query { .. })), "got {result:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_read_only_profile_cannot_write() {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = config(&dir);
    let writer = SqliteDriver.connect(&cfg).await.unwrap();
    run(writer.as_ref(), "CREATE TABLE t (a INTEGER)").await.unwrap();
    cfg.read_only = true;
    let reader = SqliteDriver.connect(&cfg).await.unwrap();
    let result = run(reader.as_ref(), "INSERT INTO t VALUES (1)").await;
    assert_eq!(result, Err(DbError::ReadOnlyViolation));
}

#[tokio::test(flavor = "multi_thread")]
async fn manual_transaction_commit_and_rollback() {
    let dir = tempfile::tempdir().unwrap();
    let session = SqliteDriver.connect(&config(&dir)).await.unwrap();
    let session = session.as_ref();
    run(session, "CREATE TABLE t (a INTEGER)").await.unwrap();
    let table = studio_types::TableRef {
        schema: "main".into(),
        name: "t".into(),
    };
    let count = || async {
        session
            .table_count(&table, &[], std::time::Duration::from_secs(5))
            .await
            .unwrap()
    };
    session.begin(false).await.unwrap();
    run(session, "INSERT INTO t VALUES (1)").await.unwrap();
    session.rollback().await.unwrap();
    assert_eq!(count().await, Some(0));
    session.begin(false).await.unwrap();
    run(session, "INSERT INTO t VALUES (1)").await.unwrap();
    session.commit().await.unwrap();
    assert_eq!(count().await, Some(1));
}

#[tokio::test(flavor = "multi_thread")]
async fn plans_are_json() {
    let dir = tempfile::tempdir().unwrap();
    let session = SqliteDriver.connect(&config(&dir)).await.unwrap();
    run(session.as_ref(), "CREATE TABLE t (a INTEGER)").await.unwrap();
    let plan = session.explain("SELECT * FROM t").await.unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&plan).unwrap();
    assert!(parsed.as_array().is_some_and(|nodes| !nodes.is_empty()));
}
