//! Saved queries.

use super::MAX_SQL_BYTES;
use crate::error::AppResult;
use crate::state::AppState;
use studio_types::{AppError, Draft, DraftId};
use tauri::State;

/// Lists drafts.
#[tauri::command]
#[specta::specta]
pub async fn drafts_list(state: State<'_, AppState>) -> Result<Vec<Draft>, AppError> {
    state.store.list_drafts().await.app()
}

/// Saves a draft.
#[tauri::command]
#[specta::specta]
pub async fn drafts_save(state: State<'_, AppState>, draft: Draft) -> Result<(), AppError> {
    if draft.sql.len() > MAX_SQL_BYTES || draft.saved_sql.len() > MAX_SQL_BYTES {
        return Err(AppError::invalid("the draft is longer than 1 MB"));
    }
    if draft.title.len() > 500 {
        return Err(AppError::invalid("the title is too long"));
    }
    state.store.save_draft(&draft).await.app()
}

/// Deletes a draft.
#[tauri::command]
#[specta::specta]
pub async fn drafts_delete(state: State<'_, AppState>, id: DraftId) -> Result<(), AppError> {
    state.store.delete_draft(id).await.app()
}
