//! Metadata and table data.

use super::check_text;
use super::query::{grid_masks, mask_rows};
use crate::error::AppResult;
use crate::state::AppState;
use studio_types::{
    AppError, ColumnInfo, DbObject, FilterRule, PageRequest, SchemaRef, SessionId, TablePage, TableRef,
    TableStructure,
};
use tauri::State;

const MAX_PAGE: u32 = 10_000;
const MAX_NAME: usize = 1024;
const MAX_RULES: usize = 64;

fn check_table(table: &TableRef) -> Result<(), AppError> {
    check_text("the schema", &table.schema, MAX_NAME)?;
    check_text("the table", &table.name, MAX_NAME)
}

fn check_filters(filters: &[FilterRule]) -> Result<(), AppError> {
    if filters.len() > MAX_RULES {
        return Err(AppError::invalid("too many filters"));
    }
    filters
        .iter()
        .try_for_each(|f| check_text("the filter column", &f.column, MAX_NAME))
}

/// Refuses a page request that is out of bounds.
pub fn check_page(page: &PageRequest) -> Result<(), AppError> {
    if page.limit == 0 || page.limit > MAX_PAGE {
        return Err(AppError::invalid("the page size must be between 1 and 10000"));
    }
    if page.sort.len() > MAX_RULES {
        return Err(AppError::invalid("too many sort keys"));
    }
    page.sort
        .iter()
        .try_for_each(|s| check_text("the sort column", &s.column, MAX_NAME))?;
    check_filters(&page.filters)
}

/// Lists schemas.
#[tauri::command]
#[specta::specta]
pub async fn list_schemas(
    state: State<'_, AppState>,
    session_id: SessionId,
) -> Result<Vec<SchemaRef>, AppError> {
    state.sessions.list_schemas(session_id).await.app()
}

/// Lists tables and views.
#[tauri::command]
#[specta::specta]
pub async fn list_objects(
    state: State<'_, AppState>,
    session_id: SessionId,
    schema: SchemaRef,
) -> Result<Vec<DbObject>, AppError> {
    check_text("the schema", &schema.name, MAX_NAME)?;
    state.sessions.list_objects(session_id, &schema).await.app()
}

/// Lists every column of a schema.
#[tauri::command]
#[specta::specta]
pub async fn columns(
    state: State<'_, AppState>,
    session_id: SessionId,
    schema: SchemaRef,
) -> Result<Vec<ColumnInfo>, AppError> {
    check_text("the schema", &schema.name, MAX_NAME)?;
    state.sessions.columns(session_id, &schema).await.app()
}

/// Describes a table.
#[tauri::command]
#[specta::specta]
pub async fn table_structure(
    state: State<'_, AppState>,
    session_id: SessionId,
    table: TableRef,
) -> Result<TableStructure, AppError> {
    check_table(&table)?;
    state.sessions.table_structure(session_id, &table).await.app()
}

/// Reads a page of a table.
#[tauri::command]
#[specta::specta]
pub async fn table_page(
    state: State<'_, AppState>,
    session_id: SessionId,
    table: TableRef,
    page: PageRequest,
) -> Result<TablePage, AppError> {
    check_table(&table)?;
    check_page(&page)?;
    let open = state.sessions.get(session_id).app()?;
    let mut result = state.sessions.table_page(session_id, &table, &page).await.app()?;
    if let Some(masks) = grid_masks(&state.store, open.profile.id).await? {
        mask_rows(&masks, &result.columns, &mut result.rows);
    }
    Ok(result)
}

/// Counts rows; `None` means it took too long.
#[tauri::command]
#[specta::specta]
pub async fn table_count(
    state: State<'_, AppState>,
    session_id: SessionId,
    table: TableRef,
    filters: Vec<FilterRule>,
) -> Result<Option<u64>, AppError> {
    check_table(&table)?;
    check_filters(&filters)?;
    state
        .sessions
        .table_count(session_id, &table, &filters)
        .await
        .app()
}
