//! Inline grid editing. The frontend sends cell values only; Rust decides which columns may be
//! edited, writes the SQL, and runs it through the grid's own door, which applies a save all or
//! nothing and checks that every statement changes exactly one row.

use super::check_sql;
use super::query::grid_masks;
use super::rows::{check_columns, check_unmasked, rendered, where_row};
use crate::error::AppResult;
use crate::state::AppState;
use std::collections::HashSet;
use studio_ai::mask::Masks;
use studio_db::sql::{qualified, quote_ident};
use studio_session::MAX_GRID_CHANGES;
use studio_types::{
    AppError, ApprovalToken, CellValue, ChangeSet, ColumnInfo, Engine, GridEditInfo, LockReason,
    LockedColumn, PlannedSql, RunId, SessionId, TableRef,
};
use tauri::State;

/// A type name without its length, precision or `unsigned`, in lower case.
fn base_type(data_type: &str) -> String {
    let lower = data_type.trim().to_lowercase();
    let base = lower.split('(').next().unwrap_or_default();
    base.trim_end_matches(" unsigned").trim().to_owned()
}

/// Whether a column holds binary data, of which the grid has only a preview.
pub fn is_binary(data_type: &str) -> bool {
    let base = base_type(data_type);
    matches!(base.as_str(), "bytea" | "binary" | "varbinary") || base.contains("blob")
}

/// Whether the database's text for a value of this type compares equal to the stored value, so
/// an UPDATE can also match the value as loaded. Floats, JSON and unknown types are left out:
/// a false mismatch would refuse a good save.
pub fn compares_exactly(data_type: &str) -> bool {
    let base = base_type(data_type);
    const EXACT: &[&str] = &[
        "smallint",
        "integer",
        "int",
        "bigint",
        "tinyint",
        "mediumint",
        "int2",
        "int4",
        "int8",
        "serial",
        "bigserial",
        "smallserial",
        "numeric",
        "decimal",
        "text",
        "tinytext",
        "mediumtext",
        "longtext",
        "varchar",
        "char",
        "nchar",
        "nvarchar",
        "bpchar",
        "citext",
        "uuid",
        "enum",
        "boolean",
        "bool",
        "date",
        "datetime",
        "year",
    ];
    EXACT.contains(&base.as_str()) || base.starts_with("character") || base.starts_with("time")
}

/// Which columns the grid may edit.
pub fn edit_info(columns: &[ColumnInfo], masks: Option<&Masks>) -> GridEditInfo {
    let locked = columns
        .iter()
        .filter_map(|column| {
            let reason = if masks.is_some_and(|m| m.hides(&column.name)) {
                LockReason::Masked
            } else if is_binary(&column.data_type) {
                LockReason::Binary
            } else {
                return None;
            };
            Some(LockedColumn {
                name: column.name.clone(),
                reason,
            })
        })
        .collect();
    GridEditInfo {
        primary_key: columns
            .iter()
            .filter(|c| c.primary_key)
            .map(|c| c.name.clone())
            .collect(),
        locked,
        max_changes: u32::try_from(MAX_GRID_CHANGES).unwrap_or(u32::MAX),
    }
}

fn check_writable(
    cells: &[CellValue],
    columns: &[ColumnInfo],
    masks: Option<&Masks>,
) -> Result<(), AppError> {
    check_columns(cells, columns)?;
    check_unmasked(cells, masks)?;
    let mut seen = HashSet::new();
    for cell in cells {
        if !seen.insert(cell.column.as_str()) {
            return Err(AppError::invalid(format!("{} appears twice", cell.column)));
        }
        if columns
            .iter()
            .any(|c| c.name == cell.column && is_binary(&c.data_type))
        {
            return Err(AppError::invalid(format!(
                "{} is binary and cannot be edited in the grid",
                cell.column
            )));
        }
    }
    Ok(())
}

/// The primary key values from `key`, in key order. Every key column must be present.
fn primary_key(
    key: &[CellValue],
    primary: &[&ColumnInfo],
    masks: Option<&Masks>,
) -> Result<Vec<CellValue>, AppError> {
    let cells = primary
        .iter()
        .map(|column| {
            key.iter()
                .find(|cell| cell.column == column.name)
                .cloned()
                .ok_or_else(|| AppError::invalid(format!("the key is missing column {}", column.name)))
        })
        .collect::<Result<Vec<_>, _>>()?;
    check_unmasked(&cells, masks)?;
    Ok(cells)
}

/// Plans a whole grid save: deletes, then updates, then inserts, one row per statement.
pub fn plan_changes(
    engine: Engine,
    table: &TableRef,
    columns: &[ColumnInfo],
    masks: Option<&Masks>,
    changes: &ChangeSet,
) -> Result<PlannedSql, AppError> {
    let total = changes.updates.len() + changes.inserts.len() + changes.deletes.len();
    if total == 0 {
        return Err(AppError::invalid("there are no changes to save"));
    }
    if total > MAX_GRID_CHANGES {
        return Err(AppError::invalid(format!(
            "one save holds at most {MAX_GRID_CHANGES} changes"
        )));
    }
    let primary: Vec<&ColumnInfo> = columns.iter().filter(|c| c.primary_key).collect();
    if primary.is_empty() {
        return Err(AppError::invalid(
            "inline editing needs a primary key, and this table has none",
        ));
    }
    let target = qualified(engine, &table.schema, &table.name);
    let mut statements = Vec::with_capacity(total);

    for key in &changes.deletes {
        check_columns(key, columns)?;
        let key = primary_key(key, &primary, masks)?;
        statements.push(format!("DELETE FROM {target} WHERE {}", where_row(engine, &key)?));
    }

    for update in &changes.updates {
        if update.after.is_empty() {
            return Err(AppError::invalid("an edited row has no changed values"));
        }
        check_columns(&update.key, columns)?;
        check_columns(&update.before, columns)?;
        check_writable(&update.after, columns, masks)?;
        let mut matched = primary_key(&update.key, &primary, masks)?;
        let mut sets = Vec::with_capacity(update.after.len());
        for cell in &update.after {
            let before = update
                .before
                .iter()
                .find(|b| b.column == cell.column)
                .ok_or_else(|| {
                    AppError::invalid(format!("the loaded value of {} is missing", cell.column))
                })?;
            let column = columns.iter().find(|c| c.name == cell.column);
            let check = column.is_some_and(|c| !c.primary_key && compares_exactly(&c.data_type));
            if check {
                matched.push(before.clone());
            }
            sets.push(format!(
                "{} = {}",
                quote_ident(engine, &cell.column),
                rendered(engine, cell.value.as_deref())?
            ));
        }
        statements.push(format!(
            "UPDATE {target} SET {} WHERE {}",
            sets.join(", "),
            where_row(engine, &matched)?
        ));
    }

    for values in &changes.inserts {
        check_writable(values, columns, masks)?;
        if values.is_empty() {
            statements.push(match engine {
                Engine::MySql => format!("INSERT INTO {target} () VALUES ()"),
                _ => format!("INSERT INTO {target} DEFAULT VALUES"),
            });
            continue;
        }
        let names: Vec<String> = values.iter().map(|c| quote_ident(engine, &c.column)).collect();
        let cells = values
            .iter()
            .map(|c| rendered(engine, c.value.as_deref()))
            .collect::<Result<Vec<_>, _>>()?;
        statements.push(format!(
            "INSERT INTO {target} ({}) VALUES ({})",
            names.join(", "),
            cells.join(", ")
        ));
    }

    let sql = statements.join(";\n");
    check_sql(&sql)?;
    Ok(PlannedSql {
        sql,
        warnings: vec![
            "These changes are saved all or nothing. If any row was changed or removed since it \
             was loaded, nothing is saved."
                .to_owned(),
        ],
    })
}

/// What the grid may edit on a table.
#[tauri::command]
#[specta::specta]
pub async fn grid_edit_info(
    state: State<'_, AppState>,
    session_id: SessionId,
    table: TableRef,
) -> Result<GridEditInfo, AppError> {
    let open = state.sessions.get(session_id).app()?;
    let structure = state.sessions.table_structure(session_id, &table).await.app()?;
    let masks = grid_masks(&state.store, open.profile.id).await?;
    Ok(edit_info(&structure.columns, masks.as_ref()))
}

/// Turns the pending grid edits into SQL for review.
#[tauri::command]
#[specta::specta]
pub async fn plan_row_changes(
    state: State<'_, AppState>,
    session_id: SessionId,
    table: TableRef,
    changes: ChangeSet,
) -> Result<PlannedSql, AppError> {
    let open = state.sessions.get(session_id).app()?;
    let structure = state.sessions.table_structure(session_id, &table).await.app()?;
    let masks = grid_masks(&state.store, open.profile.id).await?;
    plan_changes(
        open.engine(),
        &table,
        &structure.columns,
        masks.as_ref(),
        &changes,
    )
}

/// Runs reviewed grid SQL all or nothing. Needs a token minted by `approve_query` for the grid.
/// Returns how many changes were saved.
#[tauri::command]
#[specta::specta]
pub async fn run_row_changes(
    state: State<'_, AppState>,
    session_id: SessionId,
    sql: String,
    approval: ApprovalToken,
    run_id: RunId,
) -> Result<u32, AppError> {
    check_sql(&sql)?;
    let saved = state
        .sessions
        .run_changes(session_id, &sql, &approval, run_id)
        .await
        .app()?;
    Ok(u32::try_from(saved).unwrap_or(u32::MAX))
}

#[cfg(test)]
#[path = "grid_tests.rs"]
mod grid_tests;
