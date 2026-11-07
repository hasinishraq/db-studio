//! The doors statements go through. Each has its own rules; there is no other way to run SQL.

use crate::manager::{SessionError, SessionManager};
use crate::open::OpenSession;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use studio_db::{Canceller, Conn, ConnScope, DbError, ExecOptions, RowEvent};
use studio_safety::{ai_auto_allowed, classify_script, ClassifiedStatement};
use studio_types::{
    AppError, ApprovalToken, ColumnMeta, ErrorCode, ExecSummary, Origin, QueryId, RunEvent, RunId, SessionId,
    StatementKind, StatementOutcome, TxMode, Value,
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

const AI_READ_TIMEOUT: Duration = Duration::from_secs(30);
const AI_APPROVED_TIMEOUT: Duration = Duration::from_secs(60);
const AI_MAX_ROWS: u64 = 5_000;

/// Options for a user run.
#[derive(Clone, Copy, Debug, Default)]
pub struct RunOptions {
    /// Stop reading after this many rows per statement.
    pub max_rows: Option<u64>,
    /// Timeout per statement.
    pub timeout: Option<Duration>,
}

/// One running script.
pub(crate) struct RunHandle {
    session: SessionId,
    cancel: CancellationToken,
    canceller: Mutex<Option<Arc<dyn Canceller>>>,
}

/// Removes the run from the table on every exit path.
struct RunGuard<'a> {
    manager: &'a SessionManager,
    id: RunId,
    handle: Arc<RunHandle>,
}

impl Drop for RunGuard<'_> {
    fn drop(&mut self) {
        self.manager.runs.remove(&self.id);
    }
}

/// What the AI door returns. Rows are for the UI; `studio-ai` decides what the model may see.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AiQueryResult {
    /// Columns.
    pub columns: Vec<ColumnMeta>,
    /// Rows.
    pub rows: Vec<Vec<Value>>,
    /// Rows affected, for commands.
    pub rows_affected: Option<u64>,
    /// Whether the cap stopped reading.
    pub truncated: bool,
    /// Elapsed milliseconds.
    pub elapsed_ms: u64,
}

fn to_app_error(error: &DbError) -> AppError {
    let code = match error {
        DbError::Timeout => ErrorCode::Timeout,
        DbError::Cancelled => ErrorCode::Cancelled,
        DbError::Tls(_) => ErrorCode::Tls,
        DbError::Internal(_) => ErrorCode::Internal,
        _ => ErrorCode::Database,
    };
    let mut app = AppError::new(code, error.to_string());
    if let DbError::Query {
        sqlstate, position, ..
    } = error
    {
        app.detail = Some(studio_types::DbErrorDetail {
            sqlstate: sqlstate.clone(),
            position: *position,
            hint: None,
        });
    }
    app
}

fn outcome_of(summary: &ExecSummary) -> StatementOutcome {
    if summary.returns_rows {
        StatementOutcome::Rows {
            columns: summary.columns.clone(),
            total_rows: summary.total_rows,
            truncated: summary.truncated,
            elapsed_ms: summary.elapsed_ms,
        }
    } else {
        StatementOutcome::Command {
            rows_affected: summary.rows_affected,
            elapsed_ms: summary.elapsed_ms,
        }
    }
}

fn outcome_label(result: &Result<ExecSummary, DbError>) -> &'static str {
    match result {
        Ok(_) => "ok",
        Err(DbError::Cancelled) => "cancelled",
        Err(_) => "error",
    }
}

fn single(statements: &[ClassifiedStatement]) -> Result<&ClassifiedStatement, SessionError> {
    match statements {
        [statement] => Ok(statement),
        _ => Err(SessionError::Refused(
            "Exactly one statement is allowed.".to_owned(),
        )),
    }
}

impl SessionManager {
    fn begin_run(&self, session: SessionId, id: RunId) -> RunGuard<'_> {
        let handle = Arc::new(RunHandle {
            session,
            cancel: CancellationToken::new(),
            canceller: Mutex::new(None),
        });
        self.runs.insert(id, handle.clone());
        RunGuard {
            manager: self,
            id,
            handle,
        }
    }

    /// Stops a run. Succeeds when the run already ended.
    pub async fn cancel(&self, id: RunId) -> Result<(), SessionError> {
        let Some(handle) = self.runs.get(&id).map(|h| h.clone()) else {
            return Ok(());
        };
        handle.cancel.cancel();
        let canceller = handle.canceller.lock().ok().and_then(|c| c.clone());
        if let Some(canceller) = canceller {
            canceller.cancel().await?;
        }
        Ok(())
    }

    /// Stops an AI query.
    pub async fn cancel_query(&self, id: QueryId) -> Result<(), SessionError> {
        self.cancel(RunId(id.0)).await
    }

    /// Stops every run on a session.
    pub async fn cancel_session_runs(&self, session: SessionId) {
        let ids: Vec<RunId> = self
            .runs
            .iter()
            .filter(|r| r.value().session == session)
            .map(|r| *r.key())
            .collect();
        for id in ids {
            let _ = self.cancel(id).await;
        }
    }

    /// Hands out a connection and connects it to the run's cancel path.
    async fn attach(
        &self,
        open: &OpenSession,
        scope: ConnScope,
        run: &RunGuard<'_>,
    ) -> Result<Box<dyn Conn>, SessionError> {
        let conn = open.session.acquire(scope).await?;
        if let Ok(mut slot) = run.handle.canceller.lock() {
            *slot = Some(conn.canceller());
        }
        if run.handle.cancel.is_cancelled() {
            return Err(DbError::Cancelled.into());
        }
        Ok(conn)
    }

    /// The door for people. Reads run freely; anything else needs a token for exactly this script.
    /// Returns one outcome per statement that ran, stopping at the first error.
    pub async fn run_user(
        &self,
        session: SessionId,
        sql: &str,
        approval: Option<ApprovalToken>,
        run_id: RunId,
        options: RunOptions,
        sink: mpsc::Sender<RunEvent>,
    ) -> Result<Vec<StatementOutcome>, SessionError> {
        let open = self.get(session)?;
        let statements = classify_script(sql, open.engine());
        open.policy.check(&statements)?;

        let needs_approval = statements.iter().any(|s| !s.verdict.read_only);
        let origin = if needs_approval {
            let token = approval.ok_or(SessionError::ApprovalRequired)?;
            let origin = self.approvals.consume(&token, session, sql)?;
            if origin == Origin::Ai {
                return Err(SessionError::ApprovalMismatch);
            }
            origin
        } else {
            Origin::Editor
        };

        let manual = open.tx.lock().await.mode == TxMode::Manual;
        if manual && statements.iter().any(|s| s.verdict.kind == StatementKind::Tcl) {
            return Err(SessionError::Refused(
                "This session is in manual mode. Use Commit or Rollback.".to_owned(),
            ));
        }

        let run = self.begin_run(session, run_id);
        let mut conn = self.attach(&open, ConnScope::Current, &run).await?;
        let opts = ExecOptions {
            read_only_tx: false,
            timeout: options.timeout,
            max_rows: options.max_rows,
            batch_size: 500,
        };
        let result = self
            .run_script(&open, conn.as_mut(), &statements, origin, opts, &run, &sink)
            .await;
        let _ = sink.send(RunEvent::Finished).await;
        result
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_script(
        &self,
        open: &OpenSession,
        conn: &mut dyn Conn,
        statements: &[ClassifiedStatement],
        origin: Origin,
        opts: ExecOptions,
        run: &RunGuard<'_>,
        sink: &mpsc::Sender<RunEvent>,
    ) -> Result<Vec<StatementOutcome>, SessionError> {
        let mut outcomes = Vec::with_capacity(statements.len());
        for (position, statement) in statements.iter().enumerate() {
            if run.handle.cancel.is_cancelled() {
                return Err(DbError::Cancelled.into());
            }
            let index = u32::try_from(position).unwrap_or(u32::MAX);
            let _ = sink
                .send(RunEvent::StatementStart {
                    index,
                    span: statement.span,
                })
                .await;
            let result = stream_statement(conn, &statement.text, opts, index, sink).await;
            if !statement.verdict.read_only {
                let rows = result
                    .as_ref()
                    .ok()
                    .and_then(|s| i64::try_from(s.rows_affected).ok());
                let label = outcome_label(&result);
                self.audit_statement(open, origin, statement.verdict.kind, &statement.text, rows, label)
                    .await;
                if result.is_ok() {
                    let mut tx = open.tx.lock().await;
                    tx.dirty |= tx.mode == TxMode::Manual;
                }
            }
            match result {
                Ok(summary) => {
                    let outcome = outcome_of(&summary);
                    let _ = sink
                        .send(RunEvent::StatementDone {
                            index,
                            outcome: outcome.clone(),
                        })
                        .await;
                    outcomes.push(outcome);
                }
                Err(error) => {
                    let outcome = StatementOutcome::Error {
                        error: to_app_error(&error),
                    };
                    let _ = sink
                        .send(RunEvent::StatementDone {
                            index,
                            outcome: outcome.clone(),
                        })
                        .await;
                    // A cancelled run is never reported as a partial success.
                    if error == DbError::Cancelled {
                        return Err(error.into());
                    }
                    outcomes.push(outcome);
                    break;
                }
            }
        }
        Ok(outcomes)
    }

    /// The door for CSV imports. One approval covers the whole import, bound to `review_sql`, which
    /// holds a hash of the file. Every batch runs in one transaction, so a bad row undoes everything.
    pub async fn run_import(
        &self,
        session: SessionId,
        review_sql: &str,
        token: &ApprovalToken,
        run_id: RunId,
        mut batches: mpsc::Receiver<(String, u64)>,
    ) -> Result<u64, SessionError> {
        let open = self.get(session)?;
        open.policy.check(&classify_script(review_sql, open.engine()))?;
        if self.approvals.consume(token, session, review_sql)? != Origin::Import {
            return Err(SessionError::ApprovalMismatch);
        }
        if open.tx.lock().await.mode == TxMode::Manual {
            return Err(SessionError::Refused(
                "Finish the manual transaction before importing.".to_owned(),
            ));
        }
        let run = self.begin_run(session, run_id);
        let mut conn = self.attach(&open, ConnScope::Current, &run).await?;
        let opts = ExecOptions::default();
        collect(conn.as_mut(), "BEGIN", opts).await?;
        let mut imported = 0u64;
        let mut failure = None;
        while let Some((sql, rows)) = batches.recv().await {
            if run.handle.cancel.is_cancelled() {
                failure = Some(SessionError::Db(DbError::Cancelled));
                break;
            }
            let statements = classify_script(&sql, open.engine());
            if !matches!(statements.as_slice(), [one] if one.verdict.kind == StatementKind::Insert) {
                failure = Some(SessionError::Refused(
                    "An import batch must be one INSERT.".to_owned(),
                ));
                break;
            }
            match collect(conn.as_mut(), &sql, opts).await {
                Ok(_) => imported += rows,
                Err(error) => {
                    failure = Some(error);
                    break;
                }
            }
        }
        // Closing the channel tells the file reader to stop.
        batches.close();
        let finish = if failure.is_some() { "ROLLBACK" } else { "COMMIT" };
        let finished = collect(conn.as_mut(), finish, opts).await;
        let error = failure.or(finished.err());
        let (rows, label) = match &error {
            None => (i64::try_from(imported).ok(), "ok"),
            Some(SessionError::Db(DbError::Cancelled)) => (Some(0), "cancelled"),
            Some(_) => (Some(0), "error"),
        };
        self.audit_statement(
            &open,
            Origin::Import,
            StatementKind::Insert,
            review_sql,
            rows,
            label,
        )
        .await;
        match error {
            Some(error) => Err(error),
            None => Ok(imported),
        }
    }

    /// The AI's door for unattended reads. There is deliberately no approval parameter: only what
    /// `studio-safety` allows may pass, and then only inside a READ ONLY transaction.
    pub async fn run_ai_read(
        &self,
        session: SessionId,
        sql: &str,
        query: QueryId,
    ) -> Result<AiQueryResult, SessionError> {
        let open = self.get(session)?;
        ai_auto_allowed(sql, open.engine()).map_err(SessionError::Refused)?;
        let opts = ExecOptions {
            read_only_tx: true,
            timeout: Some(AI_READ_TIMEOUT),
            max_rows: Some(AI_MAX_ROWS),
            batch_size: 500,
        };
        let run = self.begin_run(session, RunId(query.0));
        let mut conn = self.attach(&open, ConnScope::Isolated, &run).await?;
        collect(conn.as_mut(), sql, opts).await
    }

    /// The AI's door after a human reviewed the exact statement in `ai_review`.
    pub async fn run_ai_approved(
        &self,
        session: SessionId,
        sql: &str,
        query: QueryId,
        token: &ApprovalToken,
    ) -> Result<AiQueryResult, SessionError> {
        let open = self.get(session)?;
        let statements = classify_script(sql, open.engine());
        open.policy.check(&statements)?;
        let origin = self.approvals.consume(token, session, sql)?;
        if origin != Origin::Ai {
            return Err(SessionError::ApprovalMismatch);
        }
        let statement = single(&statements)?;
        let opts = ExecOptions {
            read_only_tx: false,
            timeout: Some(AI_APPROVED_TIMEOUT),
            max_rows: Some(AI_MAX_ROWS),
            batch_size: 500,
        };
        let run = self.begin_run(session, RunId(query.0));
        let mut conn = self.attach(&open, ConnScope::Current, &run).await?;
        let started = Instant::now();
        let result = collect(conn.as_mut(), &statement.text, opts).await;
        if !statement.verdict.read_only {
            let (rows, label) = match &result {
                Ok(done) => (done.rows_affected.and_then(|n| i64::try_from(n).ok()), "ok"),
                Err(SessionError::Db(DbError::Cancelled)) => (None, "cancelled"),
                Err(_) => (None, "error"),
            };
            self.audit_statement(
                &open,
                Origin::Ai,
                statement.verdict.kind,
                &statement.text,
                rows,
                label,
            )
            .await;
            if result.is_ok() {
                let mut tx = open.tx.lock().await;
                tx.dirty |= tx.mode == TxMode::Manual;
            }
        }
        tracing::debug!(
            elapsed_ms = started.elapsed().as_millis() as u64,
            "AI statement finished"
        );
        result
    }
}

/// Runs one statement, relaying its stream as run events.
async fn stream_statement(
    conn: &mut dyn Conn,
    sql: &str,
    opts: ExecOptions,
    index: u32,
    sink: &mpsc::Sender<RunEvent>,
) -> Result<ExecSummary, DbError> {
    let (rows, mut inbox) = mpsc::channel(8);
    let relay = async {
        while let Some(event) = inbox.recv().await {
            let message = match event {
                RowEvent::Columns(columns) => RunEvent::Columns { index, columns },
                RowEvent::Batch(batch) => RunEvent::Rows { index, batch },
            };
            if sink.send(message).await.is_err() {
                // Closing the inbox makes the driver stop reading.
                inbox.close();
            }
        }
    };
    let (result, ()) = tokio::join!(conn.execute(sql, opts, rows), relay);
    result
}

/// Runs one statement, gathering what it returns.
async fn collect(conn: &mut dyn Conn, sql: &str, opts: ExecOptions) -> Result<AiQueryResult, SessionError> {
    let (rows, mut inbox) = mpsc::channel(8);
    let gather = async {
        let mut out = Vec::new();
        while let Some(event) = inbox.recv().await {
            if let RowEvent::Batch(batch) = event {
                out.extend(batch.rows);
            }
        }
        out
    };
    let (result, gathered) = tokio::join!(conn.execute(sql, opts, rows), gather);
    let summary = result?;
    Ok(AiQueryResult {
        rows_affected: (!summary.returns_rows).then_some(summary.rows_affected),
        columns: summary.columns,
        rows: gathered,
        truncated: summary.truncated,
        elapsed_ms: summary.elapsed_ms,
    })
}
