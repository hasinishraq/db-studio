//! Doors, approvals and policy, against the fake driver.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::Duration;
use studio_db::ConnScope;
use studio_session::testing::{harness, profile, Harness};
use studio_session::{RunOptions, SessionError};
use studio_types::*;
use tokio::sync::mpsc;

const WRITE: &str = "DELETE FROM users WHERE id = 5";

async fn connected(h: &Harness, read_only: bool) -> SessionId {
    let p = ConnectionProfile {
        read_only,
        ..profile()
    };
    h.manager.connect(p, None, None).await.unwrap().session_id
}

async fn run(
    h: &Harness,
    session: SessionId,
    sql: &str,
    approval: Option<ApprovalToken>,
) -> Result<Vec<StatementOutcome>, SessionError> {
    let (sink, mut inbox) = mpsc::channel(64);
    let drain = tokio::spawn(async move { while inbox.recv().await.is_some() {} });
    let result = h
        .manager
        .run_user(session, sql, approval, RunId::new(), RunOptions::default(), sink)
        .await;
    drain.await.unwrap();
    result
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

#[tokio::test]
async fn a_read_runs_without_a_token() {
    let h = harness();
    let s = connected(&h, false).await;
    let outcomes = run(&h, s, "SELECT 1", None).await.unwrap();
    assert!(matches!(outcomes[0], StatementOutcome::Rows { .. }));
}

#[tokio::test]
async fn a_write_without_a_token_never_reaches_the_database() {
    let h = harness();
    let s = connected(&h, false).await;
    assert_eq!(run(&h, s, WRITE, None).await, Err(SessionError::ApprovalRequired));
    assert!(executed(&h).is_empty());
}

#[tokio::test]
async fn unparseable_sql_needs_a_token() {
    let h = harness();
    let s = connected(&h, false).await;
    assert_eq!(
        run(&h, s, "SELEC 1", None).await,
        Err(SessionError::ApprovalRequired)
    );
    assert!(executed(&h).is_empty());
}

#[tokio::test]
async fn a_read_hiding_a_write_needs_a_token() {
    let h = harness();
    let s = connected(&h, false).await;
    let script = "SELECT 1; DROP TABLE users";
    assert_eq!(
        run(&h, s, script, None).await,
        Err(SessionError::ApprovalRequired)
    );
    assert!(executed(&h).is_empty(), "not even the read may run");
}

#[tokio::test]
async fn a_token_for_the_exact_script_runs_it_once() {
    let h = harness();
    let s = connected(&h, false).await;
    let token = h.approvals.issue(s, WRITE, Origin::Editor);
    run(&h, s, WRITE, Some(token.clone())).await.unwrap();
    assert_eq!(executed(&h), vec![WRITE.to_owned()]);
    assert_eq!(
        run(&h, s, WRITE, Some(token)).await,
        Err(SessionError::ApprovalRequired)
    );
    assert_eq!(executed(&h).len(), 1, "a used token must not run again");
}

#[tokio::test]
async fn a_token_for_other_sql_fails() {
    let h = harness();
    let s = connected(&h, false).await;
    let token = h.approvals.issue(s, WRITE, Origin::Editor);
    let other = "DELETE FROM users WHERE id = 6";
    assert_eq!(
        run(&h, s, other, Some(token)).await,
        Err(SessionError::ApprovalMismatch)
    );
    assert!(executed(&h).is_empty());
}

#[tokio::test]
async fn a_token_for_another_session_fails() {
    let h = harness();
    let (a, b) = (connected(&h, false).await, connected(&h, false).await);
    let token = h.approvals.issue(a, WRITE, Origin::Editor);
    assert_eq!(
        run(&h, b, WRITE, Some(token)).await,
        Err(SessionError::ApprovalMismatch)
    );
    assert!(executed(&h).is_empty());
}

#[tokio::test]
async fn an_ai_token_does_not_open_the_user_door() {
    let h = harness();
    let s = connected(&h, false).await;
    let token = h.approvals.issue(s, WRITE, Origin::Ai);
    assert_eq!(
        run(&h, s, WRITE, Some(token)).await,
        Err(SessionError::ApprovalMismatch)
    );
    assert!(executed(&h).is_empty());
}

#[tokio::test]
async fn an_editor_token_does_not_open_the_ai_door() {
    let h = harness();
    let s = connected(&h, false).await;
    let token = h.approvals.issue(s, WRITE, Origin::Editor);
    let result = h.manager.run_ai_approved(s, WRITE, QueryId::new(), &token).await;
    assert_eq!(result, Err(SessionError::ApprovalMismatch));
    assert!(executed(&h).is_empty());
}

#[tokio::test]
async fn a_read_only_profile_refuses_a_write_with_a_valid_token() {
    let h = harness();
    let s = connected(&h, true).await;
    let token = h.approvals.issue(s, WRITE, Origin::Editor);
    assert_eq!(
        run(&h, s, WRITE, Some(token)).await,
        Err(SessionError::ReadOnlyProfile)
    );
    let token = h.approvals.issue(s, WRITE, Origin::Ai);
    let result = h.manager.run_ai_approved(s, WRITE, QueryId::new(), &token).await;
    assert_eq!(result, Err(SessionError::ReadOnlyProfile));
    assert!(executed(&h).is_empty());
    assert!(run(&h, s, "SELECT 1", None).await.is_ok(), "reads still work");
}

#[tokio::test]
async fn ai_read_refuses_writes_and_runs_reads_read_only() {
    let h = harness();
    let s = connected(&h, false).await;
    for sql in [
        "UPDATE users SET a = 1",
        WRITE,
        "SELECT 1; DROP TABLE users",
        "SELECT 1; SELECT 2",
        "WITH d AS (DELETE FROM t RETURNING 1) SELECT * FROM d",
        "EXPLAIN ANALYZE DELETE FROM t",
        "SELECT pg_sleep(5)",
        "SELECT * FROM t FOR UPDATE",
        "SELEC 1",
        "",
    ] {
        let result = h.manager.run_ai_read(s, sql, QueryId::new()).await;
        assert!(
            matches!(result, Err(SessionError::Refused(_))),
            "{sql}: {result:?}"
        );
    }
    assert!(executed(&h).is_empty());

    let done = h
        .manager
        .run_ai_read(s, "SELECT * FROM users", QueryId::new())
        .await
        .unwrap();
    assert_eq!(done.rows.len(), 1);
    let log = h.driver.log.lock().unwrap();
    let (_, opts, scope) = &log.executed[0];
    assert!(
        opts.read_only_tx,
        "AI reads always run in a READ ONLY transaction"
    );
    assert_eq!(opts.timeout, Some(Duration::from_secs(30)));
    assert_eq!(opts.max_rows, Some(5000));
    assert_eq!(
        *scope,
        ConnScope::Isolated,
        "AI reads stay outside a manual transaction"
    );
}

#[tokio::test]
async fn ai_approved_runs_once_with_its_limits_and_is_audited() {
    let h = harness();
    let s = connected(&h, false).await;
    let token = h.approvals.issue(s, WRITE, Origin::Ai);
    h.manager
        .run_ai_approved(s, WRITE, QueryId::new(), &token)
        .await
        .unwrap();
    {
        let log = h.driver.log.lock().unwrap();
        assert_eq!(log.executed.len(), 1);
        assert_eq!(log.executed[0].1.timeout, Some(Duration::from_secs(60)));
        assert_eq!(log.executed[0].1.max_rows, Some(5000));
    }
    let entries = h.audit.entries.lock().unwrap().clone();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].origin, Origin::Ai);
    let again = h.manager.run_ai_approved(s, WRITE, QueryId::new(), &token).await;
    assert_eq!(again, Err(SessionError::ApprovalRequired));
}

#[tokio::test]
async fn ai_approved_takes_one_statement_only() {
    let h = harness();
    let s = connected(&h, false).await;
    let script = "DELETE FROM a; DELETE FROM b";
    let token = h.approvals.issue(s, script, Origin::Ai);
    let result = h.manager.run_ai_approved(s, script, QueryId::new(), &token).await;
    assert!(matches!(result, Err(SessionError::Refused(_))));
    assert!(executed(&h).is_empty());
}

#[tokio::test]
async fn audit_records_writes_only() {
    let h = harness();
    let s = connected(&h, false).await;
    run(&h, s, "SELECT 1", None).await.unwrap();
    assert!(h.audit.entries.lock().unwrap().is_empty());
    let script = "SELECT 1; DELETE FROM t WHERE id = 1";
    let token = h.approvals.issue(s, script, Origin::Grid);
    run(&h, s, script, Some(token)).await.unwrap();
    let entries = h.audit.entries.lock().unwrap().clone();
    assert_eq!(entries.len(), 1, "one entry per non-read statement");
    assert_eq!(entries[0].origin, Origin::Grid);
    assert_eq!(entries[0].sql_text, "DELETE FROM t WHERE id = 1");
    assert_eq!(entries[0].kind, StatementKind::Delete);
    assert_eq!(
        entries[0].sql_hash,
        studio_safety::sha256_hex("DELETE FROM t WHERE id = 1")
    );
    assert_eq!(entries[0].outcome, "ok");
}

#[tokio::test]
async fn a_failed_write_is_audited_as_an_error() {
    let h = harness();
    let s = connected(&h, false).await;
    let sql = "DELETE FROM boom";
    let token = h.approvals.issue(s, sql, Origin::Editor);
    run(&h, s, sql, Some(token)).await.unwrap();
    assert_eq!(h.audit.entries.lock().unwrap()[0].outcome, "error");
}

#[tokio::test]
async fn a_script_stops_at_the_first_error_and_keeps_earlier_results() {
    let h = harness();
    let s = connected(&h, false).await;
    let outcomes = run(&h, s, "SELECT 1; SELECT boom; SELECT 3", None).await.unwrap();
    assert_eq!(outcomes.len(), 2);
    assert!(matches!(outcomes[0], StatementOutcome::Rows { .. }));
    let StatementOutcome::Error { error } = &outcomes[1] else {
        panic!("expected an error")
    };
    assert_eq!(error.detail.as_ref().and_then(|d| d.position), Some(3));
    assert_eq!(executed(&h).len(), 2);
}

#[tokio::test]
async fn cancel_stops_a_run_and_is_never_a_partial_success() {
    let h = harness();
    let s = connected(&h, false).await;
    let run_id = RunId::new();
    let (sink, mut inbox) = mpsc::channel(64);
    let drain = tokio::spawn(async move {
        let mut events = Vec::new();
        while let Some(event) = inbox.recv().await {
            events.push(event);
        }
        events
    });
    let manager = h.manager.clone();
    let task = tokio::spawn(async move {
        manager
            .run_user(
                s,
                "SELECT 1; SELECT slow(); SELECT 3",
                None,
                run_id,
                RunOptions::default(),
                sink,
            )
            .await
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    h.manager.cancel(run_id).await.unwrap();
    let result = task.await.unwrap();
    assert_eq!(result, Err(SessionError::Db(studio_db::DbError::Cancelled)));
    assert_eq!(h.driver.log.lock().unwrap().cancels, 1);
    assert_eq!(
        executed(&h).len(),
        2,
        "the statement after the cancel must not run"
    );
    let events = drain.await.unwrap();
    assert_eq!(events.last(), Some(&RunEvent::Finished));
}

#[tokio::test]
async fn cancelling_an_unknown_run_is_fine() {
    let h = harness();
    assert!(h.manager.cancel(RunId::new()).await.is_ok());
}

#[tokio::test]
async fn manual_mode_reaches_the_driver_and_tracks_dirt() {
    let h = harness();
    let s = connected(&h, false).await;
    let state = h.manager.set_tx_mode(s, TxMode::Manual).await.unwrap();
    assert_eq!(
        state,
        TxState {
            mode: TxMode::Manual,
            dirty: false
        }
    );
    assert_eq!(h.driver.log.lock().unwrap().begins, 1);

    run(&h, s, "SELECT 1", None).await.unwrap();
    assert!(!h.manager.tx_state(s).await.unwrap().dirty, "a read is not dirt");

    let token = h.approvals.issue(s, WRITE, Origin::Editor);
    run(&h, s, WRITE, Some(token)).await.unwrap();
    assert!(h.manager.tx_state(s).await.unwrap().dirty);
    assert_eq!(
        h.manager.set_tx_mode(s, TxMode::Auto).await,
        Err(SessionError::DirtyTransaction)
    );

    let state = h.manager.commit(s).await.unwrap();
    assert!(!state.dirty);
    assert_eq!(state.mode, TxMode::Manual);
    assert_eq!(h.driver.log.lock().unwrap().commits, 1);
    assert_eq!(
        h.driver.log.lock().unwrap().begins,
        2,
        "commit opens the next transaction"
    );

    h.manager.rollback(s).await.unwrap();
    assert_eq!(h.driver.log.lock().unwrap().rollbacks, 1);
    h.manager.set_tx_mode(s, TxMode::Auto).await.unwrap();
    assert_eq!(h.manager.tx_state(s).await.unwrap().mode, TxMode::Auto);
}

#[tokio::test]
async fn commit_outside_manual_mode_is_refused() {
    let h = harness();
    let s = connected(&h, false).await;
    assert!(matches!(h.manager.commit(s).await, Err(SessionError::Refused(_))));
}

#[tokio::test]
async fn typed_transaction_control_is_refused_in_manual_mode() {
    let h = harness();
    let s = connected(&h, false).await;
    h.manager.set_tx_mode(s, TxMode::Manual).await.unwrap();
    let token = h.approvals.issue(s, "COMMIT", Origin::Editor);
    assert!(matches!(
        run(&h, s, "COMMIT", Some(token)).await,
        Err(SessionError::Refused(_))
    ));
    assert!(executed(&h).is_empty());
}

#[tokio::test]
async fn disconnect_guards_uncommitted_work() {
    let h = harness();
    let s = connected(&h, false).await;
    h.manager.set_tx_mode(s, TxMode::Manual).await.unwrap();
    let token = h.approvals.issue(s, WRITE, Origin::Editor);
    run(&h, s, WRITE, Some(token)).await.unwrap();
    assert_eq!(
        h.manager.disconnect(s, false).await,
        Err(SessionError::DirtyTransaction)
    );
    h.manager.disconnect(s, true).await.unwrap();
    assert_eq!(h.driver.log.lock().unwrap().rollbacks, 1);
    assert_eq!(h.driver.log.lock().unwrap().closes, 1);
    assert_eq!(
        run(&h, s, "SELECT 1", None).await,
        Err(SessionError::NotConnected)
    );
}

#[tokio::test]
async fn an_unknown_session_is_not_connected() {
    let h = harness();
    let id = SessionId::new();
    assert_eq!(
        run(&h, id, "SELECT 1", None).await,
        Err(SessionError::NotConnected)
    );
    let ai = h.manager.run_ai_read(id, "SELECT 1", QueryId::new()).await;
    assert_eq!(ai, Err(SessionError::NotConnected));
}

#[tokio::test]
async fn prepare_returns_verdicts_with_spans() {
    let h = harness();
    let s = connected(&h, false).await;
    let verdicts = h.manager.prepare(s, "SELECT 1; DROP TABLE t").unwrap();
    assert_eq!(verdicts.len(), 2);
    assert!(verdicts[0].read_only);
    assert!(!verdicts[1].read_only);
    assert!(!verdicts[1].risk_reasons.is_empty());
    assert_eq!(verdicts[1].span, Span { start: 10, end: 22 });
}

#[tokio::test]
async fn explain_takes_one_statement() {
    let h = harness();
    let s = connected(&h, false).await;
    assert!(h.manager.explain(s, "SELECT 1").await.is_ok());
    assert!(matches!(
        h.manager.explain(s, "SELECT 1; SELECT 2").await,
        Err(SessionError::Refused(_))
    ));
}

#[tokio::test]
async fn a_read_only_profile_asks_the_driver_for_read_only() {
    let h = harness();
    connected(&h, true).await;
    assert!(h.driver.log.lock().unwrap().configs[0].read_only);
}

#[tokio::test]
async fn a_tunnel_profile_without_a_connector_fails() {
    let h = harness();
    let mut p = profile();
    p.ssh = Some(SshConfig {
        host: "b".into(),
        port: 22,
        username: "u".into(),
        auth: SshAuth::Agent,
        has_secret: false,
    });
    assert!(matches!(
        h.manager.connect(p, None, None).await,
        Err(SessionError::Tunnel(_))
    ));
}
