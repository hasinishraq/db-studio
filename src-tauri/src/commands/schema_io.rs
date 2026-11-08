//! Schema files: writing a database's structure out, and reading a file in for preview.
//! Applying a file is not here. It goes through the normal review and run path like any other SQL.

use super::{check_sql, MAX_SQL_BYTES};
use crate::error::AppResult;
use crate::state::AppState;
use std::collections::HashSet;
use std::path::Path;
use studio_db::sql::{qualified_ref, quote_ident};
use studio_db::Session;
use studio_types::{
    AppError, Engine, ErrorCode, ObjectKind, SchemaFile, SchemaOutline, SchemaRef, SessionId, TableRef,
    TableStructure,
};
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

pub(crate) fn file_error(error: impl std::fmt::Display) -> AppError {
    AppError::new(ErrorCode::Internal, format!("file error: {error}"))
}

/// Reads every table and view of a schema.
pub async fn read_structures(session: &dyn Session, schema: &str) -> Result<Vec<TableStructure>, AppError> {
    let reference = SchemaRef {
        name: schema.to_owned(),
    };
    let objects = session.list_objects(&reference).await.app()?;
    let mut structures = Vec::with_capacity(objects.len());
    for object in objects {
        let table = TableRef {
            schema: object.schema,
            name: object.name,
        };
        structures.push(session.table_structure(&table).await.app()?);
    }
    Ok(structures)
}

/// Tables in an order where each comes after the tables it references. Tables in a cycle cannot be
/// ordered, so they keep their place at the end.
fn creation_order(tables: &[&TableStructure]) -> Vec<usize> {
    let names: HashSet<&str> = tables.iter().map(|t| t.table.name.as_str()).collect();
    let mut done: HashSet<&str> = HashSet::new();
    let mut order = Vec::with_capacity(tables.len());
    let mut waiting: Vec<usize> = (0..tables.len()).collect();
    while !waiting.is_empty() {
        let (ready, blocked): (Vec<usize>, Vec<usize>) = waiting.iter().partition(|&&index| {
            tables[index].foreign_keys.iter().all(|key| {
                let target = key.referenced_table.as_str();
                target == tables[index].table.name || !names.contains(target) || done.contains(target)
            })
        });
        if ready.is_empty() {
            order.extend(blocked);
            break;
        }
        done.extend(ready.iter().map(|&index| tables[index].table.name.as_str()));
        order.extend(ready);
        waiting = blocked;
    }
    order
}

/// The sequences a PostgreSQL table's defaults draw from. They must exist before the table does.
fn sequences(ddl: &str) -> Vec<String> {
    ddl.split("nextval('")
        .skip(1)
        .filter_map(|rest| rest.split_once('\'').map(|(name, _)| name.to_owned()))
        .collect()
}

fn statement(out: &mut String, text: &str) {
    out.push_str(text.trim().trim_end_matches(';').trim_end());
    out.push_str(";\n\n");
}

/// Indexes a table's own DDL does not already create.
fn extra_indexes(structure: &TableStructure) -> impl Iterator<Item = &String> {
    let constraints: HashSet<&str> = structure.constraints.iter().map(|c| c.name.as_str()).collect();
    structure
        .indexes
        .iter()
        .filter(move |index| !index.primary && !constraints.contains(index.name.as_str()))
        .filter_map(|index| index.definition.as_ref())
}

/// Writes a schema as a script that recreates it: tables in dependency order, then views.
pub fn schema_script(engine: Engine, schema: &str, structures: &[TableStructure]) -> String {
    let (tables, views): (Vec<&TableStructure>, Vec<&TableStructure>) =
        structures.iter().partition(|s| s.kind == ObjectKind::Table);
    let mut out = format!(
        "-- Structure of {schema}, written by AI DB Studio. It holds no rows.\n\
         -- Types, triggers and functions are not included.\n\n"
    );
    if engine == Engine::Postgres && schema != "public" {
        let name = quote_ident(engine, schema);
        statement(&mut out, &format!("CREATE SCHEMA IF NOT EXISTS {name}"));
    }
    if engine == Engine::Postgres {
        let mut seen = HashSet::new();
        for ddl in tables.iter().filter_map(|t| t.ddl.as_deref()) {
            for sequence in sequences(ddl) {
                if seen.insert(sequence.clone()) {
                    statement(&mut out, &format!("CREATE SEQUENCE IF NOT EXISTS {sequence}"));
                }
            }
        }
    }
    for index in creation_order(&tables) {
        let table = tables[index];
        if let Some(ddl) = &table.ddl {
            statement(&mut out, ddl);
        }
        for definition in extra_indexes(table) {
            statement(&mut out, definition);
        }
    }
    // After every table, so the order of the tables cannot break them, even in a cycle.
    for table in &tables {
        for key in &table.foreign_keys {
            if let Some(definition) = &key.definition {
                let name = quote_ident(engine, &key.name);
                let target = qualified_ref(engine, &table.table);
                statement(
                    &mut out,
                    &format!("ALTER TABLE {target} ADD CONSTRAINT {name} {definition}"),
                );
            }
        }
    }
    for view in views.iter().filter_map(|v| v.ddl.as_deref()) {
        statement(&mut out, view);
    }
    out
}

/// Reads a script for preview. A byte-order mark is dropped; anything but UTF-8 is refused.
pub fn read_script(path: &Path) -> Result<SchemaFile, AppError> {
    let size = std::fs::metadata(path).map_err(file_error)?.len();
    if size > MAX_SQL_BYTES as u64 {
        return Err(AppError::invalid(
            "the file is larger than 1 MB; schema files hold no rows",
        ));
    }
    let bytes = std::fs::read(path).map_err(file_error)?;
    let text = String::from_utf8(bytes)
        .map_err(|_| AppError::invalid("the file is not UTF-8 text; save it as UTF-8 and try again"))?;
    let sql = text.strip_prefix('\u{feff}').unwrap_or(&text).to_owned();
    check_sql(&sql)?;
    let name = path
        .file_name()
        .map_or_else(|| "schema.sql".to_owned(), |n| n.to_string_lossy().into_owned());
    Ok(SchemaFile { name, sql })
}

/// Writes a schema's structure to a file the user chooses, and says how many objects it held.
#[tauri::command]
#[specta::specta]
pub async fn export_schema(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: SessionId,
    schema: String,
) -> Result<u32, AppError> {
    super::check_text("the schema", &schema, 1024)?;
    let open = state.sessions.get(session_id).app()?;
    let structures = read_structures(open.session.as_ref(), &schema).await?;
    let script = schema_script(open.engine(), &schema, &structures);
    let chosen = app
        .dialog()
        .file()
        .add_filter("SQL", &["sql"])
        .set_file_name(format!("{schema}.sql"))
        .blocking_save_file()
        .ok_or_else(|| AppError::new(ErrorCode::Cancelled, "no file was chosen"))?;
    let path = chosen.into_path().map_err(file_error)?;
    std::fs::write(path, script).map_err(file_error)?;
    Ok(u32::try_from(structures.len()).unwrap_or(u32::MAX))
}

/// Asks for a script and returns its text. The path stays here, in Rust.
#[tauri::command]
#[specta::specta]
pub async fn open_schema_file(app: AppHandle) -> Result<SchemaFile, AppError> {
    let chosen = app
        .dialog()
        .file()
        .add_filter("SQL", &["sql"])
        .blocking_pick_file()
        .ok_or_else(|| AppError::new(ErrorCode::Cancelled, "no file was chosen"))?;
    read_script(&chosen.into_path().map_err(file_error)?)
}

/// The tables a script would create, for drawing. Nothing runs.
#[tauri::command]
#[specta::specta]
pub async fn outline_schema(sql: String, engine: Engine) -> Result<SchemaOutline, AppError> {
    check_sql(&sql)?;
    Ok(studio_safety::schema_outline(&sql, engine))
}

#[cfg(test)]
#[path = "schema_io_tests.rs"]
mod tests;
