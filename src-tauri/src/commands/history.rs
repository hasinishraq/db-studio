//! Run history.

use crate::error::AppResult;
use crate::state::AppState;
use studio_types::{AppError, HistoryEntry};
use tauri::State;

/// Lists recent runs.
#[tauri::command]
#[specta::specta]
pub async fn history_list(state: State<'_, AppState>, limit: u32) -> Result<Vec<HistoryEntry>, AppError> {
    state.store.list_history(limit.min(10_000)).await.app()
}

/// Clears the history.
#[tauri::command]
#[specta::specta]
pub async fn history_clear(state: State<'_, AppState>) -> Result<(), AppError> {
    state.store.clear_history().await.app()
}
