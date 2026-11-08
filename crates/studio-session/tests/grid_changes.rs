//! The grid's door: all or nothing, exactly one row per change, grid tokens only.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use studio_session::testing::{harness, profile, Harness};
use studio_session::{RunOptions, SessionError, MAX_GRID_CHANGES};
use studio_types::*;
use tokio::sync::mpsc;

const SAVE: &str = "UPDATE t SET a = '1' WHERE id = '1';\nINSERT INTO t (a) VALUES ('2')";

async fn connected(h: &Harness, read_only: bool) -> SessionId {
    let p = ConnectionProfile {
        read_only,
        ..profile()
    };
    h.manager.connect(p, None, None).await.unwrap().session_id
}

async fn save(h: &Harness, s: SessionId, sql: &str, origin: Origin) -> Result<u64, SessionError> {
    let token = h.approvals.issue(s, sql, origin);
    h.manager.run_changes(s, sql, &token, RunId::new()).await
}

fn executed(h: &Harness) -> Vec<String> {
    h.driver
        .log
        .lock()
        .unwrap()
        .executed
        .iter()
        .map(|e| e.0.clone())
        .collect()
}

fn outcomes(h: &Harness) -> Vec<String> {
    h.audit
        .entries
        .lock()
        .unwrap()
        .iter()
        .map(|e| e.outcome.clone())
        .collect()
}

#[tokio::test]
async fn a_save_runs_inside_one_transaction() {
    let h = harness();
    let s = connected(&h, false).await;
    assert_eq!(save(&h, s, SAVE, Origin::Grid).await, Ok(2));
    assert_eq!(
        executed(&h),
        vec![
            "BEGIN".to_owned(),
            "UPDATE t SET a = '1' WHERE id = '1'".to_owned(),
            "INSERT INTO t (a) VALUES ('2')".to_owned(),
            "COMMIT".to_owned(),
        ]
    );
    assert_eq!(outcomes(&h), vec!["ok", "ok"]);
    let entries = h.audit.entries.lock().unwrap().clone();
    assert!(entries.iter().all(|e| e.origin == Origin::Grid));
}

#[tokio::test]
async fn only_a_grid_token_opens_the_door() {
    let h = harness();
    let s = connected(&h, false).await;
    for origin in [Origin::Editor, Origin::Ai, Origin::Import] {
        assert_eq!(
            save(&h, s, SAVE, origin).await,
            Err(SessionError::ApprovalMismatch)
        );
    }
    assert!(executed(&h).is_empty());
}

#[tokio::test]
async fn a_grid_token_does_not_open_the_user_door() {
    let h = harness();
    let s = connected(&h, false).await;
    let token = h.approvals.issue(s, SAVE, Origin::Grid);
    let (sink, mut inbox) = mpsc::channel(64);
    let drain = tokio::spawn(async move { while inbox.recv().await.is_some() {} });
    let result = h
        .manager
        .run_user(s, SAVE, Some(token), RunId::new(), RunOptions::default(), sink)
        .await;
    drain.await.unwrap();
    assert_eq!(result, Err(SessionError::ApprovalMismatch));
    assert!(executed(&h).is_empty());
}

#[tokio::test]
async fn the_token_is_used_up_even_when_the_save_is_refused() {
    let h = harness();
    let s = connected(&h, false).await;
    let sql = "DROP TABLE t";
    let token = h.approvals.issue(s, sql, Origin::Grid);
    assert!(matches!(
        h.manager.run_changes(s, sql, &token, RunId::new()).await,
        Err(SessionError::Refused(_))
    ));
    assert_eq!(
        h.manager.run_changes(s, sql, &token, RunId::new()).await,
        Err(SessionError::ApprovalRequired)
    );
}

#[tokio::test]
async fn anything_but_row_changes_is_refused_before_it_runs() {
    let h = harness();
    let s = connected(&h, false).await;
    for sql in [
        "DROP TABLE t",
        "SELECT 1",
        "UPDATE t SET a = '1' WHERE id = '1'; COMMIT",
        "UPDATE t SET a = '1' WHERE id = '1'; TRUNCATE t",
        "UPDAT t SET a = 1",
    ] {
        assert!(
            matches!(
                save(&h, s, sql, Origin::Grid).await,
                Err(SessionError::Refused(_))
            ),
            "{sql}"
        );
    }
    assert!(executed(&h).is_empty());
}

#[tokio::test]
async fn too_many_changes_are_refused() {
    let h = harness();
    let s = connected(&h, false).await;
    let sql = vec!["DELETE FROM t WHERE id = '1'"; MAX_GRID_CHANGES + 1].join(";\n");
    assert!(matches!(
        save(&h, s, &sql, Origin::Grid).await,
        Err(SessionError::Refused(_))
    ));
    assert!(executed(&h).is_empty());
}

#[tokio::test]
async fn a_read_only_profile_refuses_the_save() {
    let h = harness();
    let s = connected(&h, true).await;
    assert_eq!(
        save(&h, s, SAVE, Origin::Grid).await,
        Err(SessionError::ReadOnlyProfile)
    );
    assert!(executed(&h).is_empty());
}

#[tokio::test]
async fn a_change_that_hits_no_row_undoes_the_save() {
    let h = harness();
    let s = connected(&h, false).await;
    let sql = "UPDATE t SET a = '1' WHERE id = '1';\nDELETE FROM t WHERE id = 'nomatch';\nINSERT INTO t (a) VALUES ('2')";
    let error = save(&h, s, sql, Origin::Grid).await.unwrap_err();
    assert!(error.to_string().contains("Change 2 of 3"), "{error}");
    let log = executed(&h);
    assert_eq!(log.last().map(String::as_str), Some("ROLLBACK"));
    assert!(
        !log.iter().any(|s| s.starts_with("INSERT")),
        "later changes must not run"
    );
    assert!(!log.iter().any(|s| s == "COMMIT"));
    assert_eq!(outcomes(&h), vec!["rolled back", "rolled back"]);
}

#[tokio::test]
async fn a_change_that_hits_two_rows_undoes_the_save() {
    let h = harness();
    let s = connected(&h, false).await;
    let sql = "UPDATE t SET a = 'twice' WHERE id = '1'";
    assert!(matches!(
        save(&h, s, sql, Origin::Grid).await,
        Err(SessionError::Refused(_))
    ));
    assert_eq!(executed(&h).last().map(String::as_str), Some("ROLLBACK"));
}

#[tokio::test]
async fn a_failing_change_undoes_the_save() {
    let h = harness();
    let s = connected(&h, false).await;
    let sql = "UPDATE t SET a = '1' WHERE id = '1';\nUPDATE boom SET a = '2' WHERE id = '2'";
    assert!(matches!(
        save(&h, s, sql, Origin::Grid).await,
        Err(SessionError::Db(_))
    ));
    assert_eq!(executed(&h).last().map(String::as_str), Some("ROLLBACK"));
    assert_eq!(outcomes(&h), vec!["rolled back", "rolled back"]);
}

#[tokio::test]
async fn manual_mode_uses_a_savepoint_and_leaves_the_transaction_open() {
    let h = harness();
    let s = connected(&h, false).await;
    h.manager.set_tx_mode(s, TxMode::Manual).await.unwrap();
    assert_eq!(save(&h, s, SAVE, Origin::Grid).await, Ok(2));
    let log = executed(&h);
    assert_eq!(log.first().map(String::as_str), Some("SAVEPOINT studio_grid"));
    assert_eq!(
        log.last().map(String::as_str),
        Some("RELEASE SAVEPOINT studio_grid")
    );
    assert!(!log.iter().any(|s| s == "COMMIT" || s == "BEGIN"));
    assert!(h.manager.tx_state(s).await.unwrap().dirty);
    assert_eq!(h.driver.log.lock().unwrap().commits, 0);
}

#[tokio::test]
async fn manual_mode_failure_rolls_back_to_the_savepoint_only() {
    let h = harness();
    let s = connected(&h, false).await;
    h.manager.set_tx_mode(s, TxMode::Manual).await.unwrap();
    let sql = "DELETE FROM t WHERE id = 'nomatch'";
    assert!(save(&h, s, sql, Origin::Grid).await.is_err());
    let log = executed(&h);
    assert!(log.contains(&"ROLLBACK TO SAVEPOINT studio_grid".to_owned()));
    assert!(!log.iter().any(|s| s == "ROLLBACK"));
    assert_eq!(h.driver.log.lock().unwrap().rollbacks, 0);
    assert!(!h.manager.tx_state(s).await.unwrap().dirty);
}
