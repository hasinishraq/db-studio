//! Export and CSV import. The file dialogs open here, in Rust, so the webview never supplies a path.

use super::check_sql;
use super::rows::literal;
use super::schema::check_filters;
use crate::error::AppResult;
use crate::state::AppState;
use serde_json::{json, Map, Value as Json};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use studio_db::sql::{qualified, qualified_ref, quote_ident, FilterBuilder};
use studio_safety::sha256_hex;
use studio_session::{RunOptions, SessionManager};
use studio_types::{
    AppError, ApprovalToken, ColumnMeta, Engine, ErrorCode, ExportFormat, FilterRule, ImportMapping,
    ImportPlan, ImportPreview, RunEvent, RunId, SessionId, TableRef, Value,
};
use tauri::ipc::Channel;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;
use tokio::sync::mpsc;

const PREVIEW_ROWS: usize = 100;
const MAX_BATCH: u32 = 10_000;
/// The most rows a worksheet holds, not counting the header.
const XLSX_MAX_ROWS: u32 = 1_048_575;
/// The most rows the webview may hand over at once. Larger results export from the database.
const MAX_PICKED_ROWS: usize = 1_000_000;
const MAX_PICKED_COLUMNS: usize = 4096;

fn io_error(error: impl std::fmt::Display) -> AppError {
    AppError::new(ErrorCode::Internal, format!("file error: {error}"))
}

/// The text form of a cell.
fn cell_text(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::Bool(v) => v.to_string(),
        Value::Int(v) => v.to_string(),
        Value::Float(v) => v.to_string(),
        Value::BigInt(v) | Value::Decimal(v) | Value::Text(v) | Value::Json(v) | Value::Temporal(v) => {
            v.clone()
        }
        Value::Bytes { preview_hex, .. } => preview_hex.clone(),
        Value::Unsupported(name) => format!("<{name}>"),
    }
}

fn cell_json(value: &Value) -> Json {
    match value {
        Value::Null => Json::Null,
        Value::Bool(v) => json!(v),
        Value::Int(v) => json!(v),
        Value::Float(v) => json!(v),
        other => json!(cell_text(other)),
    }
}

fn cell_sql(engine: Engine, value: &Value) -> Result<String, AppError> {
    Ok(match value {
        Value::Null => "NULL".to_owned(),
        Value::Int(v) => v.to_string(),
        Value::Float(v) => v.to_string(),
        Value::Bool(v) => v.to_string(),
        Value::BigInt(v) | Value::Decimal(v) => v.clone(),
        other => literal(engine, &cell_text(other))?,
    })
}

enum Output {
    Csv(Box<csv::Writer<BufWriter<File>>>),
    JsonLines(BufWriter<File>),
    Sql {
        file: BufWriter<File>,
        engine: Engine,
        target: String,
    },
    Xlsx {
        book: Box<rust_xlsxwriter::Workbook>,
        path: PathBuf,
        row: u32,
    },
}

/// Writes rows straight to disk, keeping memory flat.
struct Exporter {
    output: Output,
    columns: Vec<ColumnMeta>,
}

impl Exporter {
    fn create(format: ExportFormat, path: &Path, engine: Engine, target: String) -> Result<Self, AppError> {
        let file = || File::create(path).map(BufWriter::new).map_err(io_error);
        let output = match format {
            ExportFormat::Csv => Output::Csv(Box::new(csv::Writer::from_writer(file()?))),
            ExportFormat::JsonLines => Output::JsonLines(file()?),
            ExportFormat::SqlInserts => Output::Sql {
                file: file()?,
                engine,
                target,
            },
            ExportFormat::Xlsx => {
                let mut book = rust_xlsxwriter::Workbook::new();
                book.add_worksheet_with_constant_memory().map_err(io_error)?;
                Output::Xlsx {
                    book: Box::new(book),
                    path: path.to_owned(),
                    row: 0,
                }
            }
        };
        Ok(Self {
            output,
            columns: Vec::new(),
        })
    }

    fn columns(&mut self, columns: Vec<ColumnMeta>) -> Result<(), AppError> {
        match &mut self.output {
            Output::Csv(writer) => {
                writer
                    .write_record(columns.iter().map(|c| c.name.as_str()))
                    .map_err(io_error)?;
            }
            Output::Xlsx { book, row, .. } => {
                let sheet = book.worksheet_from_index(0).map_err(io_error)?;
                for (index, column) in columns.iter().enumerate() {
                    let index = u16::try_from(index).map_err(io_error)?;
                    sheet.write_string(0, index, &column.name).map_err(io_error)?;
                }
                *row = 1;
            }
            _ => {}
        }
        self.columns = columns;
        Ok(())
    }

    fn row(&mut self, cells: &[Value]) -> Result<(), AppError> {
        match &mut self.output {
            Output::Csv(writer) => {
                writer
                    .write_record(cells.iter().map(cell_text))
                    .map_err(io_error)?;
            }
            Output::JsonLines(file) => {
                let mut object = Map::new();
                for (column, value) in self.columns.iter().zip(cells) {
                    object.insert(column.name.clone(), cell_json(value));
                }
                writeln!(file, "{}", Json::Object(object)).map_err(io_error)?;
            }
            Output::Sql { file, engine, target } => {
                let names: Vec<String> = self
                    .columns
                    .iter()
                    .map(|c| quote_ident(*engine, &c.name))
                    .collect();
                let values = cells
                    .iter()
                    .map(|v| cell_sql(*engine, v))
                    .collect::<Result<Vec<_>, _>>()?;
                writeln!(
                    file,
                    "INSERT INTO {target} ({}) VALUES ({});",
                    names.join(", "),
                    values.join(", ")
                )
                .map_err(io_error)?;
            }
            Output::Xlsx { book, row, .. } => {
                if *row > XLSX_MAX_ROWS {
                    return Err(AppError::invalid(
                        "an Excel sheet holds at most 1,048,575 rows; use CSV",
                    ));
                }
                let sheet = book.worksheet_from_index(0).map_err(io_error)?;
                for (index, value) in cells.iter().enumerate() {
                    let index = u16::try_from(index).map_err(io_error)?;
                    match value {
                        Value::Null => {}
                        Value::Int(v) => {
                            sheet.write_number(*row, index, f64::from(*v)).map_err(io_error)?;
                        }
                        Value::Float(v) => {
                            sheet.write_number(*row, index, *v).map_err(io_error)?;
                        }
                        other => {
                            sheet
                                .write_string(*row, index, cell_text(other))
                                .map_err(io_error)?;
                        }
                    }
                }
                *row += 1;
            }
        }
        Ok(())
    }

    fn finish(self) -> Result<(), AppError> {
        match self.output {
            Output::Csv(mut writer) => writer.flush().map_err(io_error),
            Output::JsonLines(mut file) | Output::Sql { mut file, .. } => file.flush().map_err(io_error),
            Output::Xlsx { mut book, path, .. } => book.save(path).map_err(io_error),
        }
    }
}

/// Streams a query to a file. Only reads pass: it goes through `run_user` with no token.
#[allow(clippy::too_many_arguments)]
pub async fn export(
    sessions: &SessionManager,
    session: SessionId,
    sql: &str,
    format: ExportFormat,
    path: &Path,
    target: String,
    run_id: RunId,
    progress: impl Fn(u64) + Send + 'static,
) -> Result<u64, AppError> {
    check_sql(sql)?;
    let open = sessions.get(session).app()?;
    let engine = open.engine();
    if studio_safety::classify_script(sql, engine).len() != 1 {
        return Err(AppError::invalid("export one statement at a time"));
    }
    let mut exporter = Exporter::create(format, path, engine, target)?;
    let (sink, mut inbox) = mpsc::channel::<RunEvent>(8);
    // File writes run off the async threads, and the bounded channel slows the read to match them.
    let writer = tokio::task::spawn_blocking(move || -> Result<u64, AppError> {
        let mut total = 0u64;
        while let Some(event) = inbox.blocking_recv() {
            match event {
                RunEvent::Columns { columns, .. } => exporter.columns(columns)?,
                RunEvent::Rows { batch, .. } => {
                    for row in &batch.rows {
                        exporter.row(row)?;
                    }
                    total += batch.rows.len() as u64;
                    progress(total);
                }
                _ => {}
            }
        }
        exporter.finish()?;
        Ok(total)
    });
    let outcomes = sessions
        .run_user(session, sql, None, run_id, RunOptions::default(), sink)
        .await
        .app();
    let written = writer.await.map_err(io_error)??;
    for outcome in outcomes? {
        if let studio_types::StatementOutcome::Error { error } = outcome {
            return Err(error);
        }
    }
    Ok(written)
}

/// The read a table export runs, with the grid's filters written in as literals.
pub fn table_select(
    engine: Engine,
    table: &TableRef,
    filters: &[FilterRule],
    casts: &HashMap<String, String>,
) -> Result<String, AppError> {
    if filters
        .iter()
        .any(|f| f.value.as_deref().is_some_and(|v| v.contains('\0')))
    {
        return Err(AppError::invalid("a filter holds a NUL character"));
    }
    let clause = FilterBuilder::new(engine)
        .with_casts(casts)
        .inline()
        .build(filters);
    Ok(format!(
        "SELECT * FROM {}{}",
        qualified_ref(engine, table),
        clause.sql
    ))
}

/// Refuses rows that do not fit their columns, or that could only be written incorrectly.
fn check_picked(columns: &[ColumnMeta], rows: &[Vec<Value>], format: ExportFormat) -> Result<(), AppError> {
    if columns.is_empty() || columns.len() > MAX_PICKED_COLUMNS {
        return Err(AppError::invalid("the export has no usable columns"));
    }
    if rows.is_empty() || rows.len() > MAX_PICKED_ROWS {
        return Err(AppError::invalid("export between 1 and 1,000,000 selected rows"));
    }
    if rows.iter().any(|row| row.len() != columns.len()) {
        return Err(AppError::invalid("a row does not match its columns"));
    }
    let cut_short = |value: &Value| matches!(value, Value::Bytes { len, .. } if *len > 64);
    if format == ExportFormat::SqlInserts && rows.iter().flatten().any(cut_short) {
        return Err(AppError::invalid(
            "the grid holds only the start of long binary values, so SQL inserts would lose data",
        ));
    }
    Ok(())
}

/// Writes rows the person picked in a grid, exactly as the grid holds them.
pub fn write_rows(
    format: ExportFormat,
    path: &Path,
    engine: Engine,
    target: String,
    columns: Vec<ColumnMeta>,
    rows: &[Vec<Value>],
) -> Result<u64, AppError> {
    check_picked(&columns, rows, format)?;
    let mut exporter = Exporter::create(format, path, engine, target)?;
    exporter.columns(columns)?;
    for row in rows {
        exporter.row(row)?;
    }
    exporter.finish()?;
    Ok(rows.len() as u64)
}

fn pick_save(app: &AppHandle, format: ExportFormat) -> Result<PathBuf, AppError> {
    let (label, extension) = match format {
        ExportFormat::Csv => ("CSV", "csv"),
        ExportFormat::JsonLines => ("JSON lines", "jsonl"),
        ExportFormat::Xlsx => ("Excel", "xlsx"),
        ExportFormat::SqlInserts => ("SQL", "sql"),
    };
    let chosen = app
        .dialog()
        .file()
        .add_filter(label, &[extension])
        .blocking_save_file();
    let chosen = chosen.ok_or_else(|| AppError::new(ErrorCode::Cancelled, "no file was chosen"))?;
    chosen.into_path().map_err(io_error)
}

/// Streams a query to a file the user chooses.
#[tauri::command]
#[specta::specta]
pub async fn export_query(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: SessionId,
    sql: String,
    format: ExportFormat,
    run_id: RunId,
    on_progress: Channel<u64>,
) -> Result<u64, AppError> {
    let path = pick_save(&app, format)?;
    let progress = move |rows: u64| {
        let _ = on_progress.send(rows);
    };
    export(
        &state.sessions,
        session_id,
        &sql,
        format,
        &path,
        "exported".to_owned(),
        run_id,
        progress,
    )
    .await
}

/// Writes the rows picked in a grid to a file the user chooses. No SQL runs, so masked values stay
/// masked. `table` names the target of SQL inserts.
#[tauri::command]
#[specta::specta]
pub async fn export_rows(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: SessionId,
    columns: Vec<ColumnMeta>,
    rows: Vec<Vec<Value>>,
    format: ExportFormat,
    table: Option<TableRef>,
) -> Result<u64, AppError> {
    let engine = state.sessions.get(session_id).app()?.engine();
    check_picked(&columns, &rows, format)?;
    let target = table.map_or_else(|| "exported".to_owned(), |t| qualified_ref(engine, &t));
    let path = pick_save(&app, format)?;
    tokio::task::spawn_blocking(move || write_rows(format, &path, engine, target, columns, &rows))
        .await
        .map_err(io_error)?
}

/// Streams a table to a file the user chooses, honouring the grid's filters.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
#[specta::specta]
pub async fn export_table(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: SessionId,
    table: TableRef,
    filters: Vec<FilterRule>,
    format: ExportFormat,
    run_id: RunId,
    on_progress: Channel<u64>,
) -> Result<u64, AppError> {
    let open = state.sessions.get(session_id).app()?;
    let target = qualified_ref(open.engine(), &table);
    check_filters(&filters)?;
    // Filters compare by column type on PostgreSQL.
    let mut casts = HashMap::new();
    if !filters.is_empty() {
        let structure = state.sessions.table_structure(session_id, &table).await.app()?;
        casts.extend(structure.columns.into_iter().map(|c| (c.name, c.data_type)));
    }
    let sql = table_select(open.engine(), &table, &filters, &casts)?;
    let path = pick_save(&app, format)?;
    let progress = move |rows: u64| {
        let _ = on_progress.send(rows);
    };
    export(
        &state.sessions,
        session_id,
        &sql,
        format,
        &path,
        target,
        run_id,
        progress,
    )
    .await
}

/// Reads the header and the first rows.
pub fn preview(path: &Path) -> Result<(Vec<String>, Vec<Vec<String>>), AppError> {
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .from_path(path)
        .map_err(io_error)?;
    let headers = reader
        .headers()
        .map_err(io_error)?
        .iter()
        .map(str::to_owned)
        .collect();
    let mut rows = Vec::with_capacity(PREVIEW_ROWS);
    for record in reader.records().take(PREVIEW_ROWS) {
        rows.push(record.map_err(io_error)?.iter().map(str::to_owned).collect());
    }
    Ok((headers, rows))
}

fn file_hash_and_rows(path: &Path) -> Result<(String, u64), AppError> {
    use std::io::Read;
    let mut file = File::open(path).map_err(io_error)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(io_error)?;
    let text = String::from_utf8_lossy(&bytes);
    let hash = sha256_hex(&text);
    let mut reader = csv::Reader::from_reader(bytes.as_slice());
    let rows = reader.records().count() as u64;
    Ok((hash, rows))
}

fn insert_head(engine: Engine, table: &TableRef, mappings: &[ImportMapping]) -> String {
    let columns: Vec<String> = mappings.iter().map(|m| quote_ident(engine, &m.column)).collect();
    format!(
        "INSERT INTO {} ({}) VALUES ",
        qualified(engine, &table.schema, &table.name),
        columns.join(", ")
    )
}

fn render_row(
    engine: Engine,
    record: &csv::StringRecord,
    mappings: &[ImportMapping],
) -> Result<String, AppError> {
    let cells = mappings
        .iter()
        .map(|m| match record.get(m.csv_column as usize) {
            Some("") | None => Ok("NULL".to_owned()),
            Some(value) => literal(engine, value),
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(format!("({})", cells.join(", ")))
}

/// Builds what the user reviews. It holds the file's hash, so a file that changes after the review
/// no longer matches the approval.
pub fn plan_import(
    engine: Engine,
    path: &Path,
    table: &TableRef,
    mappings: &[ImportMapping],
) -> Result<ImportPlan, AppError> {
    if mappings.is_empty() {
        return Err(AppError::invalid("map at least one column"));
    }
    let (hash, rows) = file_hash_and_rows(path)?;
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .from_path(path)
        .map_err(io_error)?;
    let first = reader.records().next().transpose().map_err(io_error)?;
    let first = first.ok_or_else(|| AppError::invalid("the file has no rows"))?;
    let review_sql = format!(
        "{}{} /* CSV import: {rows} rows, sha256 {hash} */",
        insert_head(engine, table, mappings),
        render_row(engine, &first, mappings)?
    );
    Ok(ImportPlan { review_sql, rows })
}

/// Runs an import in one transaction. A bad row undoes every row.
#[allow(clippy::too_many_arguments)]
pub async fn import(
    sessions: &SessionManager,
    session: SessionId,
    path: PathBuf,
    table: TableRef,
    mappings: Vec<ImportMapping>,
    batch_size: u32,
    token: ApprovalToken,
    run_id: RunId,
) -> Result<u64, AppError> {
    if batch_size == 0 || batch_size > MAX_BATCH {
        return Err(AppError::invalid("the batch size must be between 1 and 10000"));
    }
    let engine = sessions.get(session).app()?.engine();
    let plan = plan_import(engine, &path, &table, &mappings)?;
    let head = insert_head(engine, &table, &mappings);
    let (sender, batches) = mpsc::channel::<(String, u64)>(2);
    let reader = tokio::task::spawn_blocking(move || -> Result<(), AppError> {
        let mut csv = csv::ReaderBuilder::new()
            .flexible(true)
            .from_path(path)
            .map_err(io_error)?;
        let mut rows = Vec::with_capacity(batch_size as usize);
        for record in csv.records() {
            rows.push(render_row(engine, &record.map_err(io_error)?, &mappings)?);
            if rows.len() >= batch_size as usize {
                let count = rows.len() as u64;
                let sql = format!("{head}{}", rows.join(", "));
                rows.clear();
                if sender.blocking_send((sql, count)).is_err() {
                    return Ok(());
                }
            }
        }
        if !rows.is_empty() {
            let count = rows.len() as u64;
            let _ = sender.blocking_send((format!("{head}{}", rows.join(", ")), count));
        }
        Ok(())
    });
    let result = sessions
        .run_import(session, &plan.review_sql, &token, run_id, batches)
        .await
        .app();
    reader.await.map_err(io_error)??;
    result
}

/// Asks the user for a CSV and previews it.
#[tauri::command]
#[specta::specta]
pub async fn import_pick_file(app: AppHandle, state: State<'_, AppState>) -> Result<ImportPreview, AppError> {
    let chosen = app
        .dialog()
        .file()
        .add_filter("CSV", &["csv"])
        .blocking_pick_file();
    let chosen = chosen.ok_or_else(|| AppError::new(ErrorCode::Cancelled, "no file was chosen"))?;
    let path = chosen.into_path().map_err(io_error)?;
    let (headers, rows) = preview(&path)?;
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let file_id = uuid::Uuid::new_v4().to_string();
    state.import_files.insert(file_id.clone(), path);
    Ok(ImportPreview {
        file_id,
        file_name,
        headers,
        rows,
    })
}

fn picked(state: &AppState, file_id: &str) -> Result<PathBuf, AppError> {
    let path = state.import_files.get(file_id).map(|p| p.clone());
    path.ok_or_else(|| AppError::invalid("that file is no longer selected"))
}

/// Builds the text the user reviews, which `approve_query` then binds a token to.
#[tauri::command]
#[specta::specta]
pub async fn import_plan(
    state: State<'_, AppState>,
    session_id: SessionId,
    file_id: String,
    table: TableRef,
    mappings: Vec<ImportMapping>,
) -> Result<ImportPlan, AppError> {
    let engine = state.sessions.get(session_id).app()?.engine();
    plan_import(engine, &picked(&state, &file_id)?, &table, &mappings)
}

/// Imports the file. One approval covers the whole import.
#[tauri::command]
#[specta::specta]
#[allow(clippy::too_many_arguments)]
pub async fn import_csv(
    state: State<'_, AppState>,
    session_id: SessionId,
    file_id: String,
    table: TableRef,
    mappings: Vec<ImportMapping>,
    batch_size: u32,
    token: ApprovalToken,
    run_id: RunId,
) -> Result<u64, AppError> {
    let path = picked(&state, &file_id)?;
    import(
        &state.sessions,
        session_id,
        path,
        table,
        mappings,
        batch_size,
        token,
        run_id,
    )
    .await
}

#[cfg(test)]
#[path = "transfer_tests.rs"]
mod transfer_tests;
