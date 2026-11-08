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

#[tokio::test]
async fn postgres_reports_and_changes_its_schema() {
    let h = harness();
    let info = h.manager.connect(profile(), None, None).await.unwrap();
    assert_eq!(info.schema.as_deref(), Some("public"));
    let chosen = h.manager.use_schema(info.session_id, "public").await.unwrap();
    assert_eq!(chosen.session_id, info.session_id);
    assert_eq!(chosen.schema.as_deref(), Some("public"));
    assert_eq!(h.driver.log.lock().unwrap().used_schemas, ["public"]);
}

#[tokio::test]
async fn a_schema_choice_is_checked_like_a_database_choice() {
    let h = harness();
    let id = h.manager.connect(profile(), None, None).await.unwrap().session_id;
    let unknown = h.manager.use_schema(id, "x\"; SET search_path TO evil; --").await;
    assert!(matches!(unknown, Err(SessionError::Refused(_))));
    h.manager.set_tx_mode(id, TxMode::Manual).await.unwrap();
    assert!(matches!(
        h.manager.use_schema(id, "public").await,
        Err(SessionError::DirtyTransaction)
    ));
    assert!(h.driver.log.lock().unwrap().used_schemas.is_empty());

    let my = h.manager.connect(mysql(), None, None).await.unwrap().session_id;
    assert!(matches!(
        h.manager.use_schema(my, "public").await,
        Err(SessionError::Refused(_))
    ));
}

#[tokio::test]
async fn a_remembered_schema_is_used_on_connect() {
    let h = harness();
    let mut p = profile();
    p.default_schema = Some("public".into());
    let info = h.manager.connect(p, None, None).await.unwrap();
    assert_eq!(info.schema.as_deref(), Some("public"));
    assert_eq!(
        h.driver.log.lock().unwrap().used_schemas,
        vec!["public".to_owned()]
    );
}

#[tokio::test]
async fn a_remembered_schema_that_is_gone_is_ignored() {
    let h = harness();
    let mut p = profile();
    p.default_schema = Some("dropped".into());
    let info = h.manager.connect(p, None, None).await.unwrap();
    assert!(h.driver.log.lock().unwrap().used_schemas.is_empty());
    assert_eq!(
        info.schema.as_deref(),
        Some("public"),
        "the server's own default stays"
    );
}

#[tokio::test]
async fn a_remembered_schema_is_not_carried_to_another_database() {
    let h = harness();
    let mut p = profile();
    p.default_schema = Some("public".into());
    let id = h.manager.connect(p, None, None).await.unwrap().session_id;
    h.driver.log.lock().unwrap().used_schemas.clear();
    h.manager.use_database(id, "other", None, None).await.unwrap();
    assert!(h.driver.log.lock().unwrap().used_schemas.is_empty());
}
