//! The agent's safety and privacy rules, with a scripted model and a fake database.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use studio_ai::hub::{AgentHub, Decision};
use studio_ai::providers::fake::{FakeProvider, Reply};
use studio_ai::{AiError, ChatChunk, Engine, TurnOptions};
use studio_session::testing::{harness, profile, Harness};
use studio_store::{MemorySecretStore, Store};
use studio_types::*;
use tempfile::TempDir;
use tokio::sync::mpsc;

const SENSITIVE: &str = "alice-private-value@example.com";

struct Env {
    h: Harness,
    store: Store,
    engine: Arc<Engine>,
    hub: Arc<AgentHub>,
    session: SessionId,
    conversation: ConversationId,
    _dir: TempDir,
}

async fn env_with(p: ConnectionProfile) -> Env {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path(), Arc::new(MemorySecretStore::new()))
        .await
        .unwrap();
    let h = harness();
    let session = h.manager.connect(p, None, None).await.unwrap().session_id;
    let hub = Arc::new(AgentHub::default());
    let engine = Arc::new(Engine::new(h.manager.clone(), store.clone(), hub.clone()));
    Env {
        h,
        store,
        engine,
        hub,
        session,
        conversation: ConversationId::new(),
        _dir: dir,
    }
}

async fn env() -> Env {
    env_with(profile()).await
}

fn call(sql: &str) -> ChatChunk {
    ChatChunk::ToolCall {
        id: "c1".into(),
        name: "run_sql".into(),
        args_json: json!({ "sql": sql }).to_string(),
    }
}

fn tool(chunks: Vec<ChatChunk>) -> Reply {
    Reply::Chunks(chunks)
}

fn say(text: &str) -> Reply {
    Reply::Chunks(vec![ChatChunk::Text(text.into()), ChatChunk::Done])
}

impl Env {
    fn request(&self, share_rows: bool, auto_run: bool) -> AiSendRequest {
        AiSendRequest {
            conversation_id: self.conversation,
            turn_id: TurnId::new(),
            session_id: Some(self.session),
            text: "question".into(),
            auto_run,
            share_rows,
        }
    }

    fn start(
        &self,
        req: AiSendRequest,
        provider: Arc<FakeProvider>,
    ) -> (tokio::task::JoinHandle<()>, mpsc::Receiver<AiEvent>) {
        let (events, inbox) = mpsc::channel(64);
        let engine = self.engine.clone();
        let options = TurnOptions {
            provider,
            model_key: "fake".into(),
            endpoint: "http://localhost:11434".into(),
        };
        (
            tokio::spawn(async move { engine.run_turn(req, options, events).await }),
            inbox,
        )
    }

    async fn run(&self, provider: Arc<FakeProvider>, share: bool, auto: bool) -> Vec<AiEvent> {
        let (task, mut inbox) = self.start(self.request(share, auto), provider);
        let mut events = Vec::new();
        while let Some(event) = inbox.recv().await {
            events.push(event);
        }
        task.await.unwrap();
        events
    }

    fn executed(&self) -> Vec<String> {
        self.h
            .driver
            .log
            .lock()
            .unwrap()
            .executed
            .iter()
            .map(|e| e.0.clone())
            .collect()
    }

    fn with_sensitive_rows(&self) {
        let columns = vec![
            ColumnMeta {
                name: "name".into(),
                type_name: "text".into(),
                nullable: None,
            },
            ColumnMeta {
                name: "email".into(),
                type_name: "text".into(),
                nullable: None,
            },
        ];
        let rows = vec![vec![
            Value::Text(SENSITIVE.into()),
            Value::Text("masked-email@example.com".into()),
        ]];
        *self.h.driver.rows.lock().unwrap() = Some((columns, rows));
    }

    /// Reviews the first query that asks for it, like a click on the card.
    async fn review_next(
        &self,
        inbox: &mut mpsc::Receiver<AiEvent>,
        turn: TurnId,
        approve: bool,
    ) -> Vec<AiEvent> {
        let mut seen = Vec::new();
        while let Some(event) = inbox.recv().await {
            let id = match &event {
                AiEvent::QueryStart {
                    id,
                    needs_review: true,
                    ..
                } => Some(*id),
                _ => None,
            };
            seen.push(event);
            if let Some(id) = id {
                decide(self, turn, id, approve).await;
                break;
            }
        }
        while let Some(event) = inbox.recv().await {
            seen.push(event);
        }
        seen
    }
}

/// What the `ai_review` command does.
async fn decide(e: &Env, turn: TurnId, query: QueryId, approve: bool) {
    for _ in 0..200 {
        if let Some(pending) = e.hub.take_review(turn, query) {
            let decision = if !approve {
                Decision::Declined
            } else if pending.auto_read {
                Decision::Approved(None)
            } else {
                Decision::Approved(Some(e.h.approvals.issue(
                    pending.session,
                    &pending.sql,
                    Origin::Ai,
                )))
            };
            pending.reply.send(decision).unwrap();
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("the query never reached the hub");
}

#[tokio::test]
async fn a_safe_read_runs_with_no_review() {
    let e = env().await;
    let provider = Arc::new(FakeProvider::new(vec![
        tool(vec![call("SELECT * FROM users")]),
        say("done"),
    ]));
    let events = e.run(provider, false, true).await;
    assert!(events.iter().any(|ev| matches!(ev, AiEvent::QueryResult { .. })));
    assert!(matches!(events.last(), Some(AiEvent::Done { error: None })));
    let log = e.h.driver.log.lock().unwrap();
    assert!(
        log.executed[0].1.read_only_tx,
        "must run in a read-only transaction"
    );
}

#[tokio::test]
async fn a_declined_write_does_not_run() {
    let e = env().await;
    let provider = Arc::new(FakeProvider::new(vec![
        tool(vec![call("DELETE FROM users")]),
        say("ok"),
    ]));
    let req = e.request(false, true);
    let turn = req.turn_id;
    let (task, mut inbox) = e.start(req, provider.clone());
    let events = e.review_next(&mut inbox, turn, false).await;
    task.await.unwrap();
    assert!(e.executed().is_empty(), "a declined statement must not run");
    assert!(events
        .iter()
        .any(|ev| matches!(ev, AiEvent::QueryDeclined { .. })));
    assert!(
        provider.everything_sent().contains("declined"),
        "the model is told"
    );
}

#[tokio::test]
async fn a_write_runs_only_after_approval_and_is_audited() {
    let e = env().await;
    let provider = Arc::new(FakeProvider::new(vec![
        tool(vec![call("DELETE FROM users")]),
        say("ok"),
    ]));
    let req = e.request(false, true);
    let turn = req.turn_id;
    let (task, mut inbox) = e.start(req, provider);

    let start = inbox.recv().await.unwrap();
    let AiEvent::QueryStart {
        id,
        needs_review: true,
        read_only: false,
        ..
    } = start
    else {
        panic!("expected a review, got {start:?}")
    };
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(e.executed().is_empty(), "nothing may run while waiting");

    decide(&e, turn, id, true).await;
    while inbox.recv().await.is_some() {}
    task.await.unwrap();
    assert_eq!(e.executed(), vec!["DELETE FROM users".to_owned()]);
    let entries = e.h.audit.entries.lock().unwrap();
    assert_eq!(entries[0].origin, Origin::Ai);
}

#[tokio::test]
async fn nothing_the_model_sends_can_skip_review() {
    let cases: Vec<(&str, ChatChunk)> = vec![
        (
            "extra approval fields",
            ChatChunk::ToolCall {
                id: "1".into(),
                name: "run_sql".into(),
                args_json: json!({
                    "sql": "DROP TABLE users", "approved": true, "needs_review": false, "token": "x"
                })
                .to_string(),
            },
        ),
        ("a multi-statement script", call("SELECT 1; DROP TABLE users")),
        (
            "a data-modifying CTE",
            call("WITH d AS (DELETE FROM t RETURNING 1) SELECT * FROM d"),
        ),
        ("EXPLAIN ANALYZE", call("EXPLAIN ANALYZE DELETE FROM t")),
        ("an executable comment", call("SELECT 1 /*! ; DROP TABLE t */")),
        ("unparseable text", call("DROPP TABLE users")),
    ];
    for (label, chunk) in cases {
        let e = env().await;
        let provider = Arc::new(FakeProvider::new(vec![tool(vec![chunk]), say("ok")]));
        let req = e.request(true, true);
        let turn = req.turn_id;
        let (task, mut inbox) = e.start(req, provider);
        let events = e.review_next(&mut inbox, turn, false).await;
        task.await.unwrap();
        assert!(e.executed().is_empty(), "{label} reached the database");
        assert!(
            events.iter().any(|ev| matches!(
                ev,
                AiEvent::QueryStart {
                    needs_review: true,
                    ..
                }
            )),
            "{label} was not sent for review"
        );
    }
}

#[tokio::test]
async fn fenced_sql_in_no_tools_mode_cannot_skip_review() {
    let e = env().await;
    let reply = "```sql\nDELETE FROM users\n```";
    let provider = Arc::new(FakeProvider::new(vec![say(reply), say("ok")]).without_tools());
    let req = e.request(false, true);
    let turn = req.turn_id;
    let (task, mut inbox) = e.start(req, provider);
    e.review_next(&mut inbox, turn, false).await;
    task.await.unwrap();
    assert!(e.executed().is_empty());
}

#[tokio::test]
async fn a_rejected_tools_call_falls_back_to_fenced_mode_and_is_remembered() {
    let e = env().await;
    let provider = Arc::new(FakeProvider::new(vec![
        Reply::Fail(AiError::ToolsUnsupported),
        say("```sql\nSELECT * FROM users\n```"),
        say("done"),
        say("second turn"),
    ]));
    e.run(provider.clone(), false, true).await;
    assert_eq!(e.executed().len(), 1, "the fenced read ran");
    let requests = provider.requests();
    assert!(!requests[0].tools.is_empty());
    assert!(
        requests[1].tools.is_empty(),
        "tools must be dropped after the rejection"
    );

    e.run(provider.clone(), false, true).await;
    assert!(
        provider.requests().last().unwrap().tools.is_empty(),
        "the model is remembered as no-tools"
    );
}

#[tokio::test]
async fn with_auto_run_off_a_read_waits_and_still_runs_read_only() {
    let e = env().await;
    let provider = Arc::new(FakeProvider::new(vec![
        tool(vec![call("SELECT * FROM users")]),
        say("ok"),
    ]));
    let req = e.request(false, false);
    let turn = req.turn_id;
    let (task, mut inbox) = e.start(req, provider);
    e.review_next(&mut inbox, turn, true).await;
    task.await.unwrap();
    let log = e.h.driver.log.lock().unwrap();
    assert_eq!(log.executed.len(), 1);
    assert!(log.executed[0].1.read_only_tx);
    assert_eq!(e.h.approvals.outstanding(), 0, "a read needs no token");
}

#[tokio::test]
async fn rows_reach_the_model_only_when_sharing_is_on() {
    let e = env().await;
    e.with_sensitive_rows();
    let provider = Arc::new(FakeProvider::new(vec![
        tool(vec![call("SELECT * FROM users")]),
        say("ok"),
    ]));
    let events = e.run(provider.clone(), false, true).await;
    assert!(!provider.everything_sent().contains(SENSITIVE), "sharing is off");
    let rows = events.iter().find_map(|ev| match ev {
        AiEvent::QueryResult { rows, .. } => Some(rows.clone()),
        _ => None,
    });
    assert_eq!(rows.unwrap().len(), 1, "the UI still gets the rows");

    let e = env().await;
    e.with_sensitive_rows();
    let provider = Arc::new(FakeProvider::new(vec![
        tool(vec![call("SELECT * FROM users")]),
        say("ok"),
    ]));
    e.run(provider.clone(), true, true).await;
    let sent = provider.everything_sent();
    assert!(sent.contains(SENSITIVE), "sharing is on");
    assert!(sent.contains("untrusted"), "shared rows are labelled");
}

#[tokio::test]
async fn masked_columns_are_hidden_from_the_model() {
    let p = profile();
    let id = p.id;
    let e = env_with(p).await;
    e.store
        .upsert_profile(&e.h.manager.get(e.session).unwrap().profile)
        .await
        .unwrap();
    let rule = MaskingRule {
        schema_name: "public".into(),
        table_name: "users".into(),
        column_name: "email".into(),
    };
    e.store.set_masking_rules(id, &[rule]).await.unwrap();
    e.with_sensitive_rows();
    let provider = Arc::new(FakeProvider::new(vec![
        tool(vec![call("SELECT * FROM users")]),
        say("ok"),
    ]));
    e.run(provider.clone(), true, true).await;
    let sent = provider.everything_sent();
    assert!(!sent.contains("masked-email@example.com"));
    assert!(sent.contains("***"));
}

#[tokio::test]
async fn rows_never_enter_memory_even_with_sharing_on() {
    let e = env().await;
    e.with_sensitive_rows();
    let quoted = format!("The first name is {SENSITIVE}.");
    let provider = Arc::new(FakeProvider::new(vec![
        tool(vec![call("SELECT * FROM users")]),
        say(&quoted),
    ]));
    e.run(provider, true, true).await;
    let turns = e.store.list_turns(e.conversation).await.unwrap();
    let stored = format!("{turns:?}");
    assert!(
        !stored.contains(SENSITIVE),
        "memory holds a shared value: {stored}"
    );
    assert_eq!(turns[0].queries[0].outcome, "ok");
    assert_eq!(turns[0].queries[0].sql, "SELECT * FROM users");
}

#[tokio::test]
async fn a_duplicate_statement_is_not_rerun() {
    let e = env().await;
    let provider = Arc::new(FakeProvider::new(vec![
        tool(vec![call("SELECT * FROM users")]),
        tool(vec![call(" select * from users ; ")]),
        say("ok"),
    ]));
    e.run(provider.clone(), false, true).await;
    assert_eq!(e.executed().len(), 1);
    assert!(provider.everything_sent().contains("already ran"));
}

#[tokio::test]
async fn the_seventh_query_is_refused() {
    let e = env().await;
    let calls: Vec<ChatChunk> = (0..7).map(|i| call(&format!("SELECT {i}"))).collect();
    let provider = Arc::new(FakeProvider::new(vec![tool(calls), say("ok")]));
    e.run(provider.clone(), false, true).await;
    assert_eq!(e.executed().len(), 6);
    assert!(provider.everything_sent().contains("limit of 6"));
}

#[tokio::test]
async fn a_failed_querys_error_goes_back_to_the_model() {
    let e = env().await;
    let provider = Arc::new(FakeProvider::new(vec![
        tool(vec![call("SELECT boom FROM users")]),
        say("ok"),
    ]));
    let events = e.run(provider.clone(), false, true).await;
    assert!(events.iter().any(|ev| matches!(ev, AiEvent::QueryError { .. })));
    assert!(provider.everything_sent().contains("boom failed"));
}

#[tokio::test]
async fn without_a_connection_nothing_can_run() {
    let e = env().await;
    let provider = Arc::new(FakeProvider::new(vec![tool(vec![call("SELECT 1")]), say("ok")]));
    let mut req = e.request(false, true);
    req.session_id = None;
    let (task, mut inbox) = e.start(req, provider.clone());
    while inbox.recv().await.is_some() {}
    task.await.unwrap();
    assert!(e.executed().is_empty());
    assert!(provider.everything_sent().contains("No database is connected"));
}

#[tokio::test]
async fn a_disabled_policy_refuses_the_turn() {
    let e = env_with(ConnectionProfile {
        ai_policy: AiPolicy::Disabled,
        ..profile()
    })
    .await;
    let provider = Arc::new(FakeProvider::new(vec![say("never")]));
    let events = e.run(provider.clone(), false, true).await;
    assert!(
        provider.requests().is_empty(),
        "the model must not even be called"
    );
    assert!(matches!(events.last(), Some(AiEvent::Done { error: Some(_) })));
}

#[tokio::test]
async fn local_only_refuses_a_cloud_endpoint() {
    let e = env_with(ConnectionProfile {
        ai_policy: AiPolicy::LocalOnly,
        ..profile()
    })
    .await;
    let provider = Arc::new(FakeProvider::new(vec![say("never")]));
    let (events, mut inbox) = mpsc::channel(8);
    let options = TurnOptions {
        provider: provider.clone(),
        model_key: "m".into(),
        endpoint: "https://api.openai.com/v1".into(),
    };
    e.engine.run_turn(e.request(false, true), options, events).await;
    let done = inbox.recv().await.unwrap();
    assert!(matches!(done, AiEvent::Done { error: Some(_) }));
    assert!(provider.requests().is_empty());
}

#[tokio::test]
async fn no_row_sharing_forces_sharing_off() {
    let e = env_with(ConnectionProfile {
        ai_policy: AiPolicy::NoRowSharing,
        ..profile()
    })
    .await;
    e.with_sensitive_rows();
    let provider = Arc::new(FakeProvider::new(vec![
        tool(vec![call("SELECT * FROM users")]),
        say("ok"),
    ]));
    e.run(provider.clone(), true, true).await;
    assert!(!provider.everything_sent().contains(SENSITIVE));
}

#[tokio::test]
async fn production_defaults_to_no_row_sharing() {
    let e = env_with(ConnectionProfile {
        environment: Environment::Production,
        ..profile()
    })
    .await;
    e.with_sensitive_rows();
    let provider = Arc::new(FakeProvider::new(vec![
        tool(vec![call("SELECT * FROM users")]),
        say("ok"),
    ]));
    e.run(provider.clone(), true, true).await;
    assert!(!provider.everything_sent().contains(SENSITIVE));
}

#[tokio::test]
async fn cancelling_a_turn_declines_pending_reviews() {
    let e = env().await;
    let provider = Arc::new(FakeProvider::new(vec![
        tool(vec![call("DELETE FROM users")]),
        say("ok"),
    ]));
    let req = e.request(false, true);
    let turn = req.turn_id;
    let (task, mut inbox) = e.start(req, provider);
    let start = inbox.recv().await.unwrap();
    assert!(matches!(
        start,
        AiEvent::QueryStart {
            needs_review: true,
            ..
        }
    ));
    tokio::time::sleep(Duration::from_millis(20)).await;
    e.hub.cancel(turn);
    while inbox.recv().await.is_some() {}
    task.await.unwrap();
    assert!(e.executed().is_empty());
    assert!(!e.hub.is_active(turn));
}

#[tokio::test]
async fn an_unreachable_model_falls_back_to_the_offline_engine() {
    let e = env().await;
    let provider = Arc::new(FakeProvider::new(vec![Reply::Fail(AiError::Unreachable(
        "refused".into(),
    ))]));
    let (events, mut inbox) = mpsc::channel(32);
    let mut req = e.request(false, true);
    req.text = "how many users are there".into();
    let options = TurnOptions {
        provider,
        model_key: "m".into(),
        endpoint: "http://localhost:11434".into(),
    };
    e.engine.run_turn(req, options, events).await;
    let mut seen = Vec::new();
    while let Ok(event) = inbox.try_recv() {
        seen.push(event);
    }
    assert!(
        seen.iter().any(|ev| matches!(ev, AiEvent::QueryResult { .. })),
        "{seen:?}"
    );
    let log = e.h.driver.log.lock().unwrap();
    assert!(log.executed[0].0.to_uppercase().contains("COUNT"));
    assert!(
        log.executed[0].1.read_only_tx,
        "offline answers pass the same gate"
    );
}

#[tokio::test]
async fn the_loop_stops_after_eight_steps() {
    let e = env().await;
    let script: Vec<Reply> = (0..20)
        .map(|i| {
            tool(vec![ChatChunk::ToolCall {
                id: format!("{i}"),
                name: "list_tables".into(),
                args_json: "{}".into(),
            }])
        })
        .collect();
    let provider = Arc::new(FakeProvider::new(script));
    e.run(provider.clone(), false, true).await;
    assert_eq!(provider.requests().len(), 8);
}

#[tokio::test]
async fn compaction_keeps_the_last_six_turns() {
    let e = env().await;
    e.store
        .ensure_conversation(e.conversation, None, "t")
        .await
        .unwrap();
    for i in 0..9 {
        let long = format!("{i}{}", "x".repeat(4_000));
        e.store
            .append_turn(e.conversation, TurnId::new(), &long, "a", &[])
            .await
            .unwrap();
    }
    let provider = Arc::new(FakeProvider::new(vec![
        say("hello"),
        say("summary of older turns"),
    ]));
    e.run(provider, false, true).await;
    let turns = e.store.list_turns(e.conversation).await.unwrap();
    assert_eq!(turns.len(), 6);
    assert_eq!(turns.last().unwrap().assistant_text, "hello");
    assert_eq!(
        e.store.get_summary(e.conversation).await.unwrap().as_deref(),
        Some("summary of older turns")
    );
}

#[tokio::test]
async fn compaction_loses_nothing_when_summarising_fails() {
    for failing in [Reply::Fail(AiError::Provider("down".into())), say("   ")] {
        let e = env().await;
        e.store
            .ensure_conversation(e.conversation, None, "t")
            .await
            .unwrap();
        for i in 0..9 {
            let long = format!("{i}{}", "x".repeat(4_000));
            e.store
                .append_turn(e.conversation, TurnId::new(), &long, "a", &[])
                .await
                .unwrap();
        }
        let provider = Arc::new(FakeProvider::new(vec![say("hello"), failing]));
        e.run(provider, false, true).await;
        assert_eq!(e.store.list_turns(e.conversation).await.unwrap().len(), 10);
        assert_eq!(e.store.get_summary(e.conversation).await.unwrap(), None);
    }
}
