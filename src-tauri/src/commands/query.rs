//! Running SQL.

use super::check_sql;
use crate::error::AppResult;
use crate::state::AppState;
use std::collections::HashMap;
use std::time::Instant;
use studio_ai::mask::{Masks, MASK};
use studio_safety::ApprovalBook;
use studio_session::{RunOptions, SessionManager};
use studio_store::Store;
use studio_types::{
    AppError, ApprovalGrant, ApprovalToken, ColumnMeta, HistoryEntry, Origin, ProfileId, RunEvent, RunId,
    SessionId, StatementOutcome, StatementVerdict, TxMode, TxState, Value,
};
use tauri::ipc::Channel;
use tauri::State;
use tokio::sync::mpsc;

/// Issues the token for a confirmed script. The only editor-side caller of `ApprovalBook::issue`;
/// the UI calls it only from the confirm dialog's button.
pub fn approve(
    approvals: &ApprovalBook,
    sessions: &SessionManager,
    session: SessionId,
    sql: &str,
    origin: Origin,
) -> Result<ApprovalGrant, AppError> {
    check_sql(sql)?;
    if origin == Origin::Ai {
        return Err(AppError::invalid("AI statements are approved through ai_review"));
    }
    let verdicts = sessions.prepare(session, sql).app()?;
    let token = approvals.issue(session, sql, origin);
    Ok(ApprovalGrant { token, verdicts })
}

fn summarise(outcomes: &[StatementOutcome]) -> (i64, &'static str) {
    let mut rows = 0i64;
    let mut status = "ok";
    for outcome in outcomes {
        match outcome {
            StatementOutcome::Rows { total_rows, .. } => {
                rows += i64::try_from(*total_rows).unwrap_or(i64::MAX);
            }
            StatementOutcome::Command { rows_affected, .. } => {
                rows += i64::try_from(*rows_affected).unwrap_or(i64::MAX);
            }
            StatementOutcome::Error { .. } => status = "error",
        }
    }
    (rows, status)
}

/// The masks to apply to rows shown in the grid, when the user turned that on.
pub async fn grid_masks(store: &Store, profile: ProfileId) -> Result<Option<Masks>, AppError> {
    if !store.get_settings().await.app()?.mask_in_grid {
        return Ok(None);
    }
    let rules = store.list_masking_rules(profile).await.app()?;
    Ok(Some(Masks::new(&rules)))
}

/// Replaces hidden cells. A column's position is what links a cell to its name.
pub fn mask_rows(masks: &Masks, columns: &[ColumnMeta], rows: &mut [Vec<Value>]) {
    let hidden: Vec<usize> = columns
        .iter()
        .enumerate()
        .filter(|(_, column)| masks.hides(&column.name))
        .map(|(index, _)| index)
        .collect();
    for row in rows {
        for index in &hidden {
            if let Some(cell) = row.get_mut(*index) {
                *cell = Value::Text(MASK.to_owned());
            }
        }
    }
}

/// Runs a script, relaying events to `emit`, and records it in the history.
pub async fn run(
    store: &Store,
    sessions: &SessionManager,
    session: SessionId,
    sql: String,
    approval: Option<ApprovalToken>,
    run_id: RunId,
    emit: impl Fn(RunEvent) + Send + 'static,
) -> Result<Vec<StatementOutcome>, AppError> {
    check_sql(&sql)?;
    let open = sessions.get(session).app()?;
    let masks = grid_masks(store, open.profile.id).await?;
    let (sink, mut inbox) = mpsc::channel::<RunEvent>(8);
    let relay = tokio::spawn(async move {
        let mut columns: HashMap<u32, Vec<ColumnMeta>> = HashMap::new();
        while let Some(mut event) = inbox.recv().await {
            match &mut event {
                RunEvent::Columns {
                    index,
                    columns: known,
                } => {
                    columns.insert(*index, known.clone());
                }
                RunEvent::Rows { index, batch } => {
                    if let (Some(masks), Some(known)) = (&masks, columns.get(index)) {
                        mask_rows(masks, known, &mut batch.rows);
                    }
                }
                _ => {}
            }
            emit(event);
        }
    });
    let started_at = chrono::Utc::now().timestamp_millis();
    let started = Instant::now();
    let result = sessions
        .run_user(session, &sql, approval, run_id, RunOptions::default(), sink)
        .await;
    let _ = relay.await;

    let (row_count, status) = match &result {
        Ok(outcomes) => summarise(outcomes),
        Err(studio_session::SessionError::Db(studio_db::DbError::Cancelled)) => (0, "cancelled"),
        Err(_) => (0, "error"),
    };
    // Refusals never reached the database, so they are not history.
    let reached_database = matches!(&result, Ok(_) | Err(studio_session::SessionError::Db(_)));
    if reached_database {
        let entry = HistoryEntry {
            id: run_id,
            profile_id: Some(open.profile.id),
            sql,
            started_at,
            duration_ms: i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX),
            row_count,
            status: status.to_owned(),
        };
        let limit = store.get_settings().await.app()?.history_limit;
        store.add_history(&entry, limit).await.app()?;
    }
    result.app()
}

/// Classifies a script and returns per-statement verdicts.
#[tauri::command]
#[specta::specta]
pub async fn prepare_query(
    state: State<'_, AppState>,
    session_id: SessionId,
    sql: String,
) -> Result<Vec<StatementVerdict>, AppError> {
    check_sql(&sql)?;
    state.sessions.prepare(session_id, &sql).app()
}

/// Called by the confirm dialog. Returns a single-use token for that exact SQL.
#[tauri::command]
#[specta::specta]
pub async fn approve_query(
    state: State<'_, AppState>,
    session_id: SessionId,
    sql: String,
    origin: Origin,
) -> Result<ApprovalGrant, AppError> {
    approve(&state.approvals, &state.sessions, session_id, &sql, origin)
}

/// Runs a script, streaming results.
#[tauri::command]
#[specta::specta]
pub async fn run_query(
    state: State<'_, AppState>,
    session_id: SessionId,
    sql: String,
    approval: Option<ApprovalToken>,
    run_id: RunId,
    on_event: Channel<RunEvent>,
) -> Result<Vec<StatementOutcome>, AppError> {
    let emit = move |event: RunEvent| {
        let _ = on_event.send(event);
    };
    run(
        &state.store,
        &state.sessions,
        session_id,
        sql,
        approval,
        run_id,
        emit,
    )
    .await
}

/// Cancels one run.
#[tauri::command]
#[specta::specta]
pub async fn cancel_run(state: State<'_, AppState>, run_id: RunId) -> Result<(), AppError> {
    state.sessions.cancel(run_id).await.app()
}

/// Switches commit mode.
#[tauri::command]
#[specta::specta]
pub async fn set_tx_mode(
    state: State<'_, AppState>,
    session_id: SessionId,
    mode: TxMode,
) -> Result<TxState, AppError> {
    state.sessions.set_tx_mode(session_id, mode).await.app()
}

/// Commits the manual transaction.
#[tauri::command]
#[specta::specta]
pub async fn commit_tx(state: State<'_, AppState>, session_id: SessionId) -> Result<TxState, AppError> {
    state.sessions.commit(session_id).await.app()
}

/// Rolls back the manual transaction.
#[tauri::command]
#[specta::specta]
pub async fn rollback_tx(state: State<'_, AppState>, session_id: SessionId) -> Result<TxState, AppError> {
    state.sessions.rollback(session_id).await.app()
}

/// Reports the transaction state.
#[tauri::command]
#[specta::specta]
pub async fn tx_state(state: State<'_, AppState>, session_id: SessionId) -> Result<TxState, AppError> {
    state.sessions.tx_state(session_id).await.app()
}

/// Returns the JSON plan of one statement.
#[tauri::command]
#[specta::specta]
pub async fn explain_query(
    state: State<'_, AppState>,
    session_id: SessionId,
    sql: String,
) -> Result<String, AppError> {
    check_sql(&sql)?;
    state.sessions.explain(session_id, &sql).await.app()
}
