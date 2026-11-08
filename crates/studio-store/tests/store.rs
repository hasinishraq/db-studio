//! Store tests. Every test uses a temp dir and the memory secret store, never the OS keychain.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use secrecy::{ExposeSecret, SecretString};
use std::sync::Arc;
use studio_store::{KnownHost, MemorySecretStore, SecretKey, SecretStore, Store};
use studio_types::*;
use tempfile::TempDir;

async fn store() -> (Store, TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path(), Arc::new(MemorySecretStore::new()))
        .await
        .unwrap();
    (store, dir)
}

fn profile(name: &str) -> ConnectionProfile {
    ConnectionProfile {
        id: ProfileId::new(),
        name: name.into(),
        engine: Engine::MySql,
        host: "h".into(),
        port: 3306,
        database: "d".into(),
        username: "u".into(),
        tls_mode: TlsMode::VerifyCa,
        ca_cert_path: Some("ca.pem".into()),
        client_cert_path: None,
        client_key_path: None,
        ssh: Some(SshConfig {
            host: "b".into(),
            port: 22,
            username: "t".into(),
            auth: SshAuth::Agent,
            has_secret: false,
        }),
        environment: Environment::Staging,
        read_only: true,
        ai_policy: AiPolicy::NoRowSharing,
        color: Some("#fff".into()),
        default_schema: None,
        has_password: false,
    }
}

fn audit(sql: &str) -> AuditEntry {
    AuditEntry {
        at: 1,
        profile_id: None,
        environment: Environment::Development,
        origin: Origin::Editor,
        kind: StatementKind::Update,
        sql_hash: "h".into(),
        sql_text: sql.into(),
        rows_affected: Some(1),
        outcome: "ok".into(),
    }
}

#[tokio::test]
async fn migrates_from_empty_and_reopens() {
    let dir = tempfile::tempdir().unwrap();
    let secrets = Arc::new(MemorySecretStore::new());
    let first = Store::open(dir.path(), secrets.clone()).await.unwrap();
    first.upsert_profile(&profile("a")).await.unwrap();
    first.close().await;
    let again = Store::open(dir.path(), secrets).await.unwrap();
    assert_eq!(again.list_profiles().await.unwrap().len(), 1);
}

#[tokio::test]
async fn profile_crud_and_secret_flags() {
    let (store, _dir) = store().await;
    let mut p = profile("b");
    store.upsert_profile(&p).await.unwrap();
    assert_eq!(store.get_profile(p.id).await.unwrap(), p);

    store
        .secrets()
        .set(&SecretKey::ProfilePassword(p.id), SecretString::from("pw"))
        .await
        .unwrap();
    store
        .secrets()
        .set(&SecretKey::SshSecret(p.id), SecretString::from("pp"))
        .await
        .unwrap();
    let loaded = store.get_profile(p.id).await.unwrap();
    assert!(loaded.has_password);
    assert!(loaded.ssh.as_ref().unwrap().has_secret);
    let json = serde_json::to_string(&loaded).unwrap();
    assert!(!json.contains("\"pw\"") && !json.contains("\"pp\""));

    p.name = "renamed".into();
    store.upsert_profile(&p).await.unwrap();
    assert_eq!(store.list_profiles().await.unwrap()[0].name, "renamed");

    store.delete_profile(p.id).await.unwrap();
    assert!(store.list_profiles().await.unwrap().is_empty());
    assert!(store
        .secrets()
        .get(&SecretKey::ProfilePassword(p.id))
        .await
        .unwrap()
        .is_none());
    assert!(store
        .secrets()
        .get(&SecretKey::SshSecret(p.id))
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn saving_a_profile_never_stores_flags_as_truth() {
    let (store, _dir) = store().await;
    let mut p = profile("c");
    p.has_password = true;
    store.upsert_profile(&p).await.unwrap();
    assert!(!store.get_profile(p.id).await.unwrap().has_password);
}

#[tokio::test]
async fn history_trims_to_the_limit() {
    let (store, _dir) = store().await;
    for i in 0..5 {
        let e = HistoryEntry {
            id: RunId::new(),
            profile_id: None,
            sql: format!("select {i}"),
            started_at: i,
            duration_ms: 1,
            row_count: 0,
            status: "ok".into(),
        };
        store.add_history(&e, 3).await.unwrap();
    }
    let list = store.list_history(10).await.unwrap();
    assert_eq!(
        list.iter().map(|e| e.sql.as_str()).collect::<Vec<_>>(),
        ["select 4", "select 3", "select 2"]
    );
    store.clear_history().await.unwrap();
    assert!(store.list_history(10).await.unwrap().is_empty());
}

#[tokio::test]
async fn drafts_round_trip() {
    let (store, _dir) = store().await;
    let d = Draft {
        id: DraftId::new(),
        profile_id: None,
        title: "t".into(),
        sql: "a".into(),
        saved_sql: "b".into(),
        updated_at: 9,
    };
    store.save_draft(&d).await.unwrap();
    assert_eq!(store.list_drafts().await.unwrap(), vec![d.clone()]);
    store.delete_draft(d.id).await.unwrap();
    assert!(store.list_drafts().await.unwrap().is_empty());
}

#[tokio::test]
async fn settings_default_then_persist() {
    let (store, _dir) = store().await;
    assert_eq!(store.get_settings().await.unwrap(), Settings::default());
    let s = Settings {
        history_limit: 7,
        ..Settings::default()
    };
    store.set_settings(&s).await.unwrap();
    assert_eq!(store.get_settings().await.unwrap(), s);
}

#[tokio::test]
async fn forget_from_removes_that_turn_and_later_ones() {
    let (store, _dir) = store().await;
    let c = ConversationId::new();
    store.ensure_conversation(c, None, None, "t").await.unwrap();
    let ids: Vec<TurnId> = (0..4).map(|_| TurnId::new()).collect();
    for id in &ids {
        store.append_turn(c, *id, "q", "a", &[]).await.unwrap();
    }
    store.forget_from(c, ids[2]).await.unwrap();
    let left: Vec<TurnId> = store.list_turns(c).await.unwrap().iter().map(|t| t.id).collect();
    assert_eq!(left, ids[..2]);
}

#[tokio::test]
async fn compact_keeps_the_last_turns_and_the_summary() {
    let (store, _dir) = store().await;
    let c = ConversationId::new();
    store.ensure_conversation(c, None, None, "t").await.unwrap();
    for i in 0..8 {
        store
            .append_turn(c, TurnId::new(), &format!("q{i}"), "a", &[])
            .await
            .unwrap();
    }
    store.compact(c, "sum", 6).await.unwrap();
    let turns = store.list_turns(c).await.unwrap();
    assert_eq!(turns.len(), 6);
    assert_eq!(turns[0].user_text, "q2");
    assert_eq!(store.get_summary(c).await.unwrap().as_deref(), Some("sum"));
    store.clear_conversation(c).await.unwrap();
    assert!(
        store.list_turns(c).await.unwrap().is_empty(),
        "turns must cascade"
    );
}

#[tokio::test]
async fn audit_chain_verifies() {
    let (store, _dir) = store().await;
    for i in 0..5 {
        store.audit_append(&audit(&format!("update {i}"))).await.unwrap();
    }
    assert_eq!(store.verify_chain().await.unwrap(), None);
    let list = store
        .audit_list(&AuditQuery {
            limit: 2,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].prev_hash, list[1].entry_hash);
}

#[tokio::test]
async fn audit_detects_a_tampered_row() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path(), Arc::new(MemorySecretStore::new()))
        .await
        .unwrap();
    for i in 0..4 {
        store.audit_append(&audit(&format!("update {i}"))).await.unwrap();
    }
    store.close().await;

    // Tamper through a separate connection, as an attacker with file access would.
    let url = format!("sqlite://{}", dir.path().join("studio.db").display());
    let raw = sqlx::SqlitePool::connect(&url).await.unwrap();
    sqlx::query("UPDATE audit_log SET sql_text = 'nothing to see' WHERE id = 3")
        .execute(&raw)
        .await
        .unwrap();
    raw.close().await;

    let store = Store::open(dir.path(), Arc::new(MemorySecretStore::new()))
        .await
        .unwrap();
    assert_eq!(store.verify_chain().await.unwrap(), Some(3));
}

#[tokio::test]
async fn audit_detects_a_deleted_row() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path(), Arc::new(MemorySecretStore::new()))
        .await
        .unwrap();
    for i in 0..4 {
        store.audit_append(&audit(&format!("update {i}"))).await.unwrap();
    }
    store.close().await;
    let url = format!("sqlite://{}", dir.path().join("studio.db").display());
    let raw = sqlx::SqlitePool::connect(&url).await.unwrap();
    sqlx::query("DELETE FROM audit_log WHERE id = 2")
        .execute(&raw)
        .await
        .unwrap();
    raw.close().await;
    let store = Store::open(dir.path(), Arc::new(MemorySecretStore::new()))
        .await
        .unwrap();
    assert_eq!(store.verify_chain().await.unwrap(), Some(3));
}

#[tokio::test]
async fn concurrent_audit_appends_do_not_fork_the_chain() {
    let (store, _dir) = store().await;
    let mut tasks = Vec::new();
    for i in 0..20 {
        let store = store.clone();
        tasks.push(tokio::spawn(async move {
            store.audit_append(&audit(&format!("u{i}"))).await
        }));
    }
    for task in tasks {
        task.await.unwrap().unwrap();
    }
    assert_eq!(store.verify_chain().await.unwrap(), None);
}

#[tokio::test]
async fn known_hosts_trust_does_not_overwrite_a_pin() {
    let (store, _dir) = store().await;
    let first = KnownHost {
        key_type: "ed25519".into(),
        fingerprint: "AAA".into(),
    };
    let other = KnownHost {
        key_type: "ed25519".into(),
        fingerprint: "BBB".into(),
    };
    store.trust_host_key("h", 22, &first).await.unwrap();
    store.trust_host_key("h", 22, &other).await.unwrap();
    assert_eq!(store.known_host("h", 22).await.unwrap(), vec![first]);
    store.replace_host_key("h", 22, &other).await.unwrap();
    assert_eq!(store.known_host("h", 22).await.unwrap(), vec![other]);
}

#[tokio::test]
async fn masking_rules_replace() {
    let (store, _dir) = store().await;
    let p = ProfileId::new();
    let rule = MaskingRule {
        schema_name: "s".into(),
        table_name: "t".into(),
        column_name: "email".into(),
    };
    store
        .set_masking_rules(p, std::slice::from_ref(&rule))
        .await
        .unwrap();
    assert_eq!(store.list_masking_rules(p).await.unwrap(), vec![rule]);
    store.set_masking_rules(p, &[]).await.unwrap();
    assert!(store.list_masking_rules(p).await.unwrap().is_empty());
}

#[tokio::test]
async fn memory_secrets_round_trip() {
    let s = MemorySecretStore::new();
    let key = SecretKey::AiKey("openai".into());
    assert!(s.get(&key).await.unwrap().is_none());
    s.set(&key, SecretString::from("k")).await.unwrap();
    assert_eq!(s.get(&key).await.unwrap().unwrap().expose_secret(), "k");
    s.delete(&key).await.unwrap();
    s.delete(&key).await.unwrap();
    assert!(s.get(&key).await.unwrap().is_none());
}

#[test]
fn entry_names_match_the_design() {
    let id = ProfileId::new();
    assert_eq!(
        SecretKey::ProfilePassword(id).entry_name(),
        format!("profile:{id}:password")
    );
    assert_eq!(SecretKey::SshSecret(id).entry_name(), format!("profile:{id}:ssh"));
    assert_eq!(SecretKey::AiKey("x".into()).entry_name(), "ai:x");
}

#[tokio::test]
async fn a_conversation_names_its_connection_and_database() {
    let (store, _dir) = store().await;
    let p = profile("Local MySQL");
    store.upsert_profile(&p).await.unwrap();
    let c = ConversationId::new();
    store
        .ensure_conversation(c, Some(p.id), Some("shop"), "first title")
        .await
        .unwrap();
    store
        .ensure_conversation(c, Some(p.id), Some("archive"), "second title")
        .await
        .unwrap();
    let listed = store.list_conversations().await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].title, "first title", "the title is kept");
    assert_eq!(
        listed[0].database.as_deref(),
        Some("archive"),
        "the database follows a switch"
    );
    assert_eq!(listed[0].connection.as_deref(), Some(p.name.as_str()));
}
