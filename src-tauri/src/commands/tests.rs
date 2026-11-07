//! The plain functions behind the commands, with a temp store, memory secrets and a fake driver.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::{profiles, query, schema, secrets, sessions, settings, MAX_SQL_BYTES};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use studio_session::testing::{harness, profile, Harness};
use studio_store::{MemorySecretStore, SecretKey, Store};
use studio_types::*;
use tempfile::TempDir;

struct Env {
    h: Harness,
    store: Store,
    _dir: TempDir,
}

async fn env() -> Env {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path(), Arc::new(MemorySecretStore::new()))
        .await
        .unwrap();
    Env {
        h: harness(),
        store,
        _dir: dir,
    }
}

impl Env {
    async fn session(&self) -> SessionId {
        let saved = profiles::save(&self.store, profile(), None).await.unwrap();
        sessions::open(&self.store, &self.h.manager, saved.id)
            .await
            .unwrap()
            .session_id
    }

    async fn run(
        &self,
        session: SessionId,
        sql: &str,
        token: Option<ApprovalToken>,
    ) -> Result<Vec<StatementOutcome>, AppError> {
        query::run(
            &self.store,
            &self.h.manager,
            session,
            sql.to_owned(),
            token,
            RunId::new(),
            |_| {},
        )
        .await
    }
}

#[tokio::test]
async fn saving_a_password_stores_it_and_never_returns_it() {
    let e = env().await;
    let saved = profiles::save(&e.store, profile(), Some("hunter2".into()))
        .await
        .unwrap();
    assert!(saved.has_password);
    let listed = profiles::list(&e.store).await.unwrap();
    assert!(listed[0].has_password);
    let json = serde_json::to_string(&listed).unwrap();
    assert!(!json.contains("hunter2"), "{json}");
    assert!(e
        .store
        .secrets()
        .get(&SecretKey::ProfilePassword(saved.id))
        .await
        .unwrap()
        .is_some());
}

#[tokio::test]
async fn a_saved_password_reaches_the_driver() {
    use secrecy::ExposeSecret;
    let e = env().await;
    let saved = profiles::save(&e.store, profile(), Some("hunter2".into()))
        .await
        .unwrap();
    sessions::open(&e.store, &e.h.manager, saved.id).await.unwrap();
    let log = e.h.driver.log.lock().unwrap();
    assert_eq!(
        log.configs[0].password.as_ref().unwrap().expose_secret(),
        "hunter2"
    );
}

#[tokio::test]
async fn testing_prefers_a_new_password() {
    use secrecy::ExposeSecret;
    let e = env().await;
    let saved = profiles::save(&e.store, profile(), Some("old".into()))
        .await
        .unwrap();
    sessions::test(&e.store, &e.h.manager, saved.clone(), Some("new".into()))
        .await
        .unwrap();
    sessions::test(&e.store, &e.h.manager, saved, None).await.unwrap();
    let log = e.h.driver.log.lock().unwrap();
    assert_eq!(log.configs[0].password.as_ref().unwrap().expose_secret(), "new");
    assert_eq!(log.configs[1].password.as_ref().unwrap().expose_secret(), "old");
}

#[tokio::test]
async fn invalid_profiles_are_refused() {
    let e = env().await;
    for bad in [
        ConnectionProfile {
            name: "  ".into(),
            ..profile()
        },
        ConnectionProfile {
            name: "x".repeat(201),
            ..profile()
        },
        ConnectionProfile {
            host: String::new(),
            ..profile()
        },
        ConnectionProfile { port: 0, ..profile() },
        ConnectionProfile {
            database: String::new(),
            ..profile()
        },
    ] {
        let error = profiles::save(&e.store, bad, None)
            .await
            .expect_err("must be refused");
        assert_eq!(error.code, ErrorCode::InvalidInput);
    }
}

#[tokio::test]
async fn secrets_can_be_set_and_cleared_but_not_read() {
    let e = env().await;
    let saved = profiles::save(&e.store, profile(), None).await.unwrap();
    let target = SecretTarget::ProfilePassword { profile_id: saved.id };
    secrets::set(&e.store, target.clone(), "pw".into()).await.unwrap();
    assert!(e.store.get_profile(saved.id).await.unwrap().has_password);
    secrets::clear(&e.store, target.clone()).await.unwrap();
    assert!(!e.store.get_profile(saved.id).await.unwrap().has_password);
    let error = secrets::set(&e.store, target, String::new()).await.err().unwrap();
    assert_eq!(error.code, ErrorCode::InvalidInput);
}

#[tokio::test]
async fn approve_then_run_succeeds_and_reuse_fails() {
    let e = env().await;
    let s = e.session().await;
    let sql = "DELETE FROM users WHERE id = 1";
    let grant = query::approve(&e.h.approvals, &e.h.manager, s, sql, Origin::Editor).unwrap();
    assert_eq!(grant.verdicts.len(), 1);
    assert!(!grant.verdicts[0].read_only);
    e.run(s, sql, Some(grant.token.clone())).await.unwrap();
    let error = e.run(s, sql, Some(grant.token)).await.err().unwrap();
    assert_eq!(error.code, ErrorCode::ApprovalRequired);
}

#[tokio::test]
async fn a_write_without_approval_reports_the_stable_code() {
    let e = env().await;
    let s = e.session().await;
    let error = e.run(s, "DROP TABLE users", None).await.err().unwrap();
    assert_eq!(error.code, ErrorCode::ApprovalRequired);
    assert!(
        e.store.list_history(10).await.unwrap().is_empty(),
        "a refusal is not history"
    );
}

#[tokio::test]
async fn approve_query_refuses_the_ai_origin() {
    let e = env().await;
    let s = e.session().await;
    let error = query::approve(&e.h.approvals, &e.h.manager, s, "DELETE FROM t", Origin::Ai)
        .err()
        .unwrap();
    assert_eq!(error.code, ErrorCode::InvalidInput);
    assert_eq!(e.h.approvals.outstanding(), 0, "no token may have been minted");
}

#[tokio::test]
async fn approve_query_needs_a_real_session() {
    let e = env().await;
    let id = SessionId::new();
    let error = query::approve(&e.h.approvals, &e.h.manager, id, "DELETE FROM t", Origin::Editor);
    assert_eq!(error.err().unwrap().code, ErrorCode::NotConnected);
    assert_eq!(e.h.approvals.outstanding(), 0);
}

#[tokio::test]
async fn sql_input_is_validated() {
    let e = env().await;
    let s = e.session().await;
    let long = "x".repeat(MAX_SQL_BYTES + 1);
    for bad in ["", "   ", long.as_str()] {
        assert_eq!(
            e.run(s, bad, None).await.err().unwrap().code,
            ErrorCode::InvalidInput
        );
    }
}

#[tokio::test]
async fn runs_are_recorded_in_history() {
    let e = env().await;
    let s = e.session().await;
    e.run(s, "SELECT 1", None).await.unwrap();
    let history = e.store.list_history(10).await.unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].status, "ok");
    assert_eq!(history[0].row_count, 1);
}

#[tokio::test]
async fn events_reach_the_emitter_in_order() {
    let e = env().await;
    let s = e.session().await;
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = seen.clone();
    let emit = move |event: RunEvent| sink.lock().unwrap().push(event);
    query::run(
        &e.store,
        &e.h.manager,
        s,
        "SELECT 1".into(),
        None,
        RunId::new(),
        emit,
    )
    .await
    .unwrap();
    let events = seen.lock().unwrap();
    assert!(matches!(events[0], RunEvent::StatementStart { .. }));
    assert!(matches!(events[1], RunEvent::Columns { .. }));
    assert!(matches!(events[2], RunEvent::Rows { .. }));
    assert!(matches!(events[3], RunEvent::StatementDone { .. }));
    assert_eq!(events.last(), Some(&RunEvent::Finished));
}

#[tokio::test]
async fn masked_columns_never_reach_the_grid_when_masking_is_on() {
    let e = env().await;
    let saved = profiles::save(&e.store, profile(), None).await.unwrap();
    let s = sessions::open(&e.store, &e.h.manager, saved.id)
        .await
        .unwrap()
        .session_id;
    let rule = MaskingRule {
        schema_name: "public".into(),
        table_name: "users".into(),
        column_name: "email".into(),
    };
    e.store.set_masking_rules(saved.id, &[rule]).await.unwrap();
    let meta = |name: &str| ColumnMeta {
        name: name.into(),
        type_name: "text".into(),
        nullable: None,
    };
    *e.h.driver.rows.lock().unwrap() = Some((
        vec![meta("name"), meta("email")],
        vec![vec![
            Value::Text("Ann".into()),
            Value::Text("ann@example.com".into()),
        ]],
    ));

    let run = |sessions_seen: Arc<std::sync::Mutex<Vec<RunEvent>>>| {
        move |event: RunEvent| sessions_seen.lock().unwrap().push(event)
    };
    let off = Arc::new(std::sync::Mutex::new(Vec::new()));
    query::run(
        &e.store,
        &e.h.manager,
        s,
        "SELECT 1".into(),
        None,
        RunId::new(),
        run(off.clone()),
    )
    .await
    .unwrap();
    assert!(
        format!("{:?}", off.lock().unwrap()).contains("ann@example.com"),
        "masking is off"
    );

    let settings = Settings {
        mask_in_grid: true,
        ..Settings::default()
    };
    e.store.set_settings(&settings).await.unwrap();
    let on = Arc::new(std::sync::Mutex::new(Vec::new()));
    query::run(
        &e.store,
        &e.h.manager,
        s,
        "SELECT 2".into(),
        None,
        RunId::new(),
        run(on.clone()),
    )
    .await
    .unwrap();
    let seen = format!("{:?}", on.lock().unwrap());
    assert!(!seen.contains("ann@example.com"), "{seen}");
    assert!(seen.contains("***"));
    assert!(seen.contains("Ann"), "other columns stay visible");
}

#[test]
fn settings_are_validated() {
    assert!(settings::check(&Settings::default()).is_ok());
    let bad = Settings {
        history_limit: 0,
        ..Settings::default()
    };
    assert!(settings::check(&bad).is_err());
    let bad = Settings {
        ai_endpoint: "file:///etc/passwd".into(),
        ..Settings::default()
    };
    assert!(settings::check(&bad).is_err());
}

#[test]
fn page_requests_are_validated() {
    let page = PageRequest {
        limit: 100,
        offset: 0,
        sort: vec![],
        filters: vec![],
    };
    assert!(schema::check_page(&page).is_ok());
    assert!(schema::check_page(&PageRequest {
        limit: 0,
        ..page.clone()
    })
    .is_err());
    assert!(schema::check_page(&PageRequest {
        limit: 10_001,
        ..page
    })
    .is_err());
}

fn rust_sources(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if path.is_dir() {
            if !matches!(name.as_str(), "target" | "tests" | "gen") {
                rust_sources(&path, found);
            }
        } else if path.extension().is_some_and(|e| e == "rs")
            && !matches!(name.as_str(), "tests.rs" | "testing.rs")
            && !name.ends_with("_tests.rs")
        {
            found.push(path);
        }
    }
}

/// Rule 6: tokens are minted only by commands a human click triggers. Test modules live in their
/// own `tests.rs` or `*_tests.rs` files, so every line of production code is scanned.
#[test]
fn only_human_click_commands_mint_tokens() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut files = Vec::new();
    rust_sources(&root.join("crates"), &mut files);
    rust_sources(&root.join("src-tauri/src"), &mut files);
    assert!(files.len() > 30, "the scan must really cover the workspace");

    let mut minters: Vec<(String, usize)> = Vec::new();
    for file in files {
        if file.ends_with(Path::new("studio-safety").join("src").join("approval.rs")) {
            continue;
        }
        let text = std::fs::read_to_string(&file).unwrap();
        let code = text.split("#[cfg(test)]").next().unwrap_or_default();
        let calls = code.matches(".issue(").count();
        let in_tests = text.matches(".issue(").count() - calls;
        // ai.rs keeps its review tests inline; they call `review`, never `issue`.
        assert_eq!(
            in_tests,
            0,
            "{} mints in a test module; move it to a *_tests.rs file",
            file.display()
        );
        if calls > 0 {
            minters.push((file.file_name().unwrap().to_string_lossy().into_owned(), calls));
        }
    }
    minters.sort();
    let expected = vec![("ai.rs".to_owned(), 1), ("query.rs".to_owned(), 1)];
    assert_eq!(
        minters, expected,
        "ApprovalBook::issue is called from an unexpected place"
    );
}
