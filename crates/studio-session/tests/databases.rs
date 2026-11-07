//! Choosing a default database.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use studio_session::testing::{harness, profile};
use studio_session::SessionError;
use studio_types::{ConnectionProfile, Engine, TxMode};

fn mysql() -> ConnectionProfile {
    let mut p = profile();
    p.engine = Engine::MySql;
    p.database = String::new();
    p
}

#[tokio::test]
async fn a_blank_database_connects_and_reports_where_it_landed() {
    let h = harness();
    let info = h.manager.connect(mysql(), None, None).await.unwrap();
    assert_eq!(info.database, None);

    let mut blank = profile();
    blank.database = String::new();
    let info = h.manager.connect(blank, None, None).await.unwrap();
    assert_eq!(info.database.as_deref(), Some("postgres"));
}

#[tokio::test]
async fn mysql_changes_database_on_the_same_session() {
    let h = harness();
    let id = h.manager.connect(mysql(), None, None).await.unwrap().session_id;
    let info = h.manager.use_database(id, "other", None, None).await.unwrap();
    assert_eq!(info.session_id, id);
    assert_eq!(info.database.as_deref(), Some("other"));
    assert_eq!(h.manager.current_database(id).await.as_deref(), Some("other"));
    assert_eq!(h.driver.log.lock().unwrap().used_databases, ["other"]);
}

#[tokio::test]
async fn postgres_replaces_the_session_and_keeps_the_policy() {
    let h = harness();
    let mut read_only = profile();
    read_only.read_only = true;
    let old = h.manager.connect(read_only, None, None).await.unwrap().session_id;
    let info = h.manager.use_database(old, "other", None, None).await.unwrap();
    assert_ne!(info.session_id, old);
    assert_eq!(info.database.as_deref(), Some("other"));
    assert!(matches!(h.manager.get(old), Err(SessionError::NotConnected)));
    let new = h.manager.get(info.session_id).unwrap();
    assert!(new.profile.read_only);
    assert_eq!(new.profile.database, "other");
}

#[tokio::test]
async fn an_unknown_database_is_refused() {
    let h = harness();
    let id = h.manager.connect(mysql(), None, None).await.unwrap().session_id;
    let result = h
        .manager
        .use_database(id, "`; DROP DATABASE app; --", None, None)
        .await;
    assert!(matches!(result, Err(SessionError::Refused(_))));
    assert!(h.driver.log.lock().unwrap().used_databases.is_empty());
}

#[tokio::test]
async fn an_open_transaction_blocks_the_change() {
    let h = harness();
    let id = h.manager.connect(profile(), None, None).await.unwrap().session_id;
    h.manager.set_tx_mode(id, TxMode::Manual).await.unwrap();
    let result = h.manager.use_database(id, "other", None, None).await;
    assert!(matches!(result, Err(SessionError::DirtyTransaction)));
    assert!(h.manager.get(id).is_ok());
}

#[tokio::test]
async fn sqlite_has_nothing_to_change() {
    let h = harness();
    let mut p = profile();
    p.engine = Engine::Sqlite;
    let id = h
        .manager
        .connect(p, None, None)
        .await
        .unwrap_or_else(|_| panic!("no fake driver"));
    let result = h.manager.use_database(id.session_id, "other", None, None).await;
    assert!(matches!(result, Err(SessionError::Refused(_))));
}

#[tokio::test]
async fn a_dropped_mysql_database_is_forgotten() {
    let h = harness();
    let mut p = mysql();
    p.database = "gone".into();
    let id = h.manager.connect(p, None, None).await.unwrap().session_id;
    assert_eq!(h.manager.current_database(id).await.as_deref(), Some("gone"));
    // The fake server lists only `public`.
    h.manager.list_schemas(id).await.unwrap();
    assert_eq!(h.manager.current_database(id).await, None);
}
