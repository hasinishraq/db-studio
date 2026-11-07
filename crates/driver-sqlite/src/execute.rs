//! Running one statement on a blocking thread.

use crate::session::{blocking, map_error, open, Shared};
use rusqlite::types::ValueRef;
use rusqlite::{Connection, InterruptHandle};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use studio_db::{Canceller, Conn, DbError, ExecOptions, RowEvent, RowSink, Value};
use studio_types::{ColumnMeta, ExecSummary, RowBatch};

/// Cancels through SQLite's interrupt, which is safe from another thread.
pub(crate) struct SqliteCanceller {
    handle: Mutex<Option<InterruptHandle>>,
    requested: AtomicBool,
    timed_out: AtomicBool,
}

impl SqliteCanceller {
    fn interrupt(&self) {
        if let Ok(slot) = self.handle.lock() {
            if let Some(handle) = slot.as_ref() {
                handle.interrupt();
            }
        }
    }
}

#[async_trait::async_trait]
impl Canceller for SqliteCanceller {
    async fn cancel(&self) -> Result<(), DbError> {
        self.requested.store(true, Ordering::SeqCst);
        self.interrupt();
        Ok(())
    }
}

/// One connection.
pub(crate) struct SqliteConn {
    conn: Shared,
    path: PathBuf,
    canceller: Arc<SqliteCanceller>,
}

impl SqliteConn {
    pub(crate) fn boxed(conn: Shared, path: PathBuf) -> Box<dyn Conn> {
        let canceller = Arc::new(SqliteCanceller {
            handle: Mutex::new(None),
            requested: AtomicBool::new(false),
            timed_out: AtomicBool::new(false),
        });
        Box::new(Self {
            conn,
            path,
            canceller,
        })
    }
}

/// Decodes one cell, using the declared type to recognise dates and JSON stored as text.
pub(crate) fn decode(value: ValueRef<'_>, declared: Option<&str>) -> Value {
    let declared = declared.unwrap_or_default().to_ascii_uppercase();
    match value {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(v) if declared.contains("BOOL") => Value::Bool(v != 0),
        ValueRef::Integer(v) => i32::try_from(v).map_or_else(|_| Value::BigInt(v.to_string()), Value::Int),
        ValueRef::Real(v) if v.is_finite() => Value::Float(v),
        ValueRef::Real(v) => Value::Text(v.to_string()),
        ValueRef::Blob(bytes) => Value::bytes(bytes),
        ValueRef::Text(bytes) => {
            let text = String::from_utf8_lossy(bytes).into_owned();
            if declared.contains("JSON") {
                Value::Json(text)
            } else if declared.contains("DATE") || declared.contains("TIME") {
                Value::Temporal(text)
            } else {
                Value::Text(text)
            }
        }
    }
}

fn run(conn: &Connection, sql: &str, opts: ExecOptions, sink: &RowSink) -> Result<ExecSummary, DbError> {
    let started = Instant::now();
    // `prepare` refuses a second statement, so nothing can ride along with the classified one.
    let mut statement = conn.prepare(sql).map_err(map_error)?;
    let mut summary = ExecSummary::default();
    if statement.column_count() == 0 {
        statement.raw_execute().map_err(map_error)?;
        summary.rows_affected = conn.changes();
        summary.elapsed_ms = millis(started);
        return Ok(summary);
    }
    let declared: Vec<Option<String>> = statement
        .columns()
        .iter()
        .map(|c| c.decl_type().map(str::to_owned))
        .collect();
    summary.columns = statement
        .column_names()
        .iter()
        .zip(&declared)
        .map(|(name, ty)| ColumnMeta {
            name: (*name).to_owned(),
            type_name: ty.clone().unwrap_or_default(),
            nullable: None,
        })
        .collect();
    summary.returns_rows = true;
    let _ = sink.blocking_send(RowEvent::Columns(summary.columns.clone()));

    let batch_size = opts.batch_size.max(1);
    let (mut batch, mut seq) = (Vec::with_capacity(batch_size), 0u32);
    let mut rows = statement.query([]).map_err(map_error)?;
    while let Some(row) = rows.next().map_err(map_error)? {
        if opts.max_rows.is_some_and(|cap| summary.total_rows >= cap) {
            summary.truncated = true;
            break;
        }
        let mut cells = Vec::with_capacity(declared.len());
        for (index, ty) in declared.iter().enumerate() {
            cells.push(decode(row.get_ref(index).map_err(map_error)?, ty.as_deref()));
        }
        batch.push(cells);
        summary.total_rows += 1;
        if batch.len() >= batch_size {
            let rows = std::mem::replace(&mut batch, Vec::with_capacity(batch_size));
            if sink
                .blocking_send(RowEvent::Batch(RowBatch { seq, rows }))
                .is_err()
            {
                return Err(DbError::Cancelled);
            }
            seq += 1;
        }
    }
    if !batch.is_empty()
        && sink
            .blocking_send(RowEvent::Batch(RowBatch { seq, rows: batch }))
            .is_err()
    {
        return Err(DbError::Cancelled);
    }
    summary.elapsed_ms = millis(started);
    Ok(summary)
}

fn millis(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

#[async_trait::async_trait]
impl Conn for SqliteConn {
    async fn execute(
        &mut self,
        stmt: &str,
        opts: ExecOptions,
        sink: RowSink,
    ) -> Result<ExecSummary, DbError> {
        let canceller = self.canceller.clone();
        if canceller.requested.load(Ordering::SeqCst) {
            return Err(DbError::Cancelled);
        }
        let (shared, path, sql) = (self.conn.clone(), self.path.clone(), stmt.to_owned());
        let worker = canceller.clone();
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
        let task = blocking(move || {
            let lock_error = || DbError::Internal("connection poisoned".to_owned());
            // Read-only work gets a connection opened read-only, which the engine itself enforces.
            let reader = if opts.read_only_tx {
                Some(open(&path, true)?)
            } else {
                None
            };
            let guard;
            let conn = match &reader {
                Some(reader) => reader,
                None => {
                    guard = shared.lock().map_err(|_| lock_error())?;
                    &*guard
                }
            };
            if let Ok(mut slot) = worker.handle.lock() {
                *slot = Some(conn.get_interrupt_handle());
            }
            let _ = ready_tx.send(());
            // A cancel that arrived before the handle was published had nothing to interrupt.
            if worker.requested.load(Ordering::SeqCst) {
                return Err(DbError::Cancelled);
            }
            let result = run(conn, &sql, opts, &sink);
            if let Ok(mut slot) = worker.handle.lock() {
                *slot = None;
            }
            result
        });
        let watchdog = opts.timeout.map(|timeout| {
            let canceller = canceller.clone();
            tokio::spawn(async move {
                let _ = ready_rx.await;
                tokio::time::sleep(timeout).await;
                canceller.timed_out.store(true, Ordering::SeqCst);
                canceller.interrupt();
            })
        });
        let result = task.await;
        if let Some(watchdog) = watchdog {
            watchdog.abort();
        }
        if canceller.timed_out.load(Ordering::SeqCst) {
            return Err(DbError::Timeout);
        }
        if canceller.requested.load(Ordering::SeqCst) {
            return Err(DbError::Cancelled);
        }
        result
    }

    fn canceller(&self) -> Arc<dyn Canceller> {
        self.canceller.clone()
    }
}
