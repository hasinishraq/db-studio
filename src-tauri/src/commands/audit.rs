//! The audit log.

use crate::error::AppResult;
use crate::state::AppState;
use std::path::Path;
use studio_store::Store;
use studio_types::{AppError, AuditQuery, AuditRecord, ErrorCode, ExportFormat};
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

fn io_error(error: impl std::fmt::Display) -> AppError {
    AppError::new(ErrorCode::Internal, format!("file error: {error}"))
}

/// Writes records to a CSV or JSON lines file.
pub fn write_records(records: &[AuditRecord], format: ExportFormat, path: &Path) -> Result<(), AppError> {
    match format {
        ExportFormat::JsonLines => {
            let lines: Vec<String> = records
                .iter()
                .map(serde_json::to_string)
                .collect::<Result<_, _>>()
                .map_err(io_error)?;
            std::fs::write(path, lines.join("\n")).map_err(io_error)
        }
        ExportFormat::Csv => {
            let mut writer = csv::Writer::from_path(path).map_err(io_error)?;
            writer
                .write_record([
                    "id",
                    "at",
                    "origin",
                    "kind",
                    "outcome",
                    "rows",
                    "sql",
                    "entry_hash",
                ])
                .map_err(io_error)?;
            for r in records {
                writer
                    .write_record([
                        r.id.to_string(),
                        r.entry.at.to_string(),
                        format!("{:?}", r.entry.origin),
                        format!("{:?}", r.entry.kind),
                        r.entry.outcome.clone(),
                        r.entry.rows_affected.map(|n| n.to_string()).unwrap_or_default(),
                        r.entry.sql_text.clone(),
                        r.entry_hash.clone(),
                    ])
                    .map_err(io_error)?;
            }
            writer.flush().map_err(io_error)
        }
        _ => Err(AppError::invalid("the audit log exports as CSV or JSON lines")),
    }
}

async fn list(store: &Store, query: AuditQuery) -> Result<Vec<AuditRecord>, AppError> {
    let limit = query.limit.clamp(1, 100_000);
    store.audit_list(&AuditQuery { limit, ..query }).await.app()
}

/// Lists audit records.
#[tauri::command]
#[specta::specta]
pub async fn audit_list(state: State<'_, AppState>, query: AuditQuery) -> Result<Vec<AuditRecord>, AppError> {
    list(&state.store, query).await
}

/// Recomputes the hash chain. Returns the first broken record, if any.
#[tauri::command]
#[specta::specta]
pub async fn audit_verify(state: State<'_, AppState>) -> Result<Option<i64>, AppError> {
    state.store.verify_chain().await.app()
}

/// Exports matching records to a file the user chooses.
#[tauri::command]
#[specta::specta]
pub async fn audit_export(
    app: AppHandle,
    state: State<'_, AppState>,
    query: AuditQuery,
    format: ExportFormat,
) -> Result<u32, AppError> {
    let records = list(&state.store, query).await?;
    let chosen = app.dialog().file().blocking_save_file();
    let chosen = chosen.ok_or_else(|| AppError::new(ErrorCode::Cancelled, "no file was chosen"))?;
    write_records(&records, format, &chosen.into_path().map_err(io_error)?)?;
    Ok(u32::try_from(records.len()).unwrap_or(u32::MAX))
}
