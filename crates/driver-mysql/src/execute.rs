//! Running one statement on one connection.

use crate::connect::{code, map_error, Shared};
use crate::decode::{decode, type_name};
use mysql_async::prelude::Queryable;
use mysql_async::{Column, Conn as RawConn};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use studio_db::{Canceller, Conn, DbError, ExecOptions, RowEvent, RowSink};
use studio_types::{ColumnMeta, ExecSummary, RowBatch, Value};
use tokio::sync::OwnedMutexGuard;

/// How long past the server-side limit the client waits before killing the statement itself.
/// `MAX_EXECUTION_TIME` covers only SELECT, so this is what stops everything else.
const WATCHDOG_GRACE: Duration = Duration::from_millis(250);

/// How close to its limit a MariaDB statement must end to count as stopped by it.
const LIMIT_TOLERANCE: Duration = Duration::from_millis(50);

enum Holder {
    Pooled(RawConn),
    Pinned(OwnedMutexGuard<Option<RawConn>>),
}

/// Cancels the statement on one connection, using a second one.
pub(crate) struct MyCanceller {
    shared: Arc<Shared>,
    connection_id: u32,
    requested: AtomicBool,
    timed_out: AtomicBool,
}

impl MyCanceller {
    /// Sends `KILL QUERY`. The server cannot read it on the connection that is busy.
    async fn kill(&self) -> Result<(), DbError> {
        let mut conn = self.shared.get().await?;
        match conn
            .query_drop(format!("KILL QUERY {}", self.connection_id))
            .await
        {
            Ok(()) => Ok(()),
            // Unknown thread id: the statement already finished.
            Err(mysql_async::Error::Server(server)) if server.code == 1094 => Ok(()),
            Err(error) => Err(map_error(error)),
        }
    }
}

#[async_trait::async_trait]
impl Canceller for MyCanceller {
    async fn cancel(&self) -> Result<(), DbError> {
        self.requested.store(true, Ordering::SeqCst);
        self.kill().await
    }
}

/// One connection.
pub(crate) struct MyConn {
    holder: Holder,
    canceller: Arc<MyCanceller>,
    shared: Arc<Shared>,
}

impl MyConn {
    fn boxed(holder: Holder, connection_id: u32, shared: Arc<Shared>) -> Box<dyn Conn> {
        let canceller = Arc::new(MyCanceller {
            shared: shared.clone(),
            connection_id,
            requested: AtomicBool::new(false),
            timed_out: AtomicBool::new(false),
        });
        Box::new(Self {
            holder,
            canceller,
            shared,
        })
    }

    pub(crate) fn pooled(conn: RawConn, shared: Arc<Shared>) -> Box<dyn Conn> {
        let id = conn.id();
        Self::boxed(Holder::Pooled(conn), id, shared)
    }

    pub(crate) fn pinned(
        guard: OwnedMutexGuard<Option<RawConn>>,
        shared: Arc<Shared>,
    ) -> Result<Box<dyn Conn>, DbError> {
        let id = guard.as_ref().map(RawConn::id);
        let id = id.ok_or_else(|| DbError::Internal("the pinned connection is gone".to_owned()))?;
        Ok(Self::boxed(Holder::Pinned(guard), id, shared))
    }

    fn raw(&mut self) -> Result<&mut RawConn, DbError> {
        match &mut self.holder {
            Holder::Pooled(conn) => Ok(conn),
            Holder::Pinned(guard) => guard
                .as_mut()
                .ok_or_else(|| DbError::Internal("the pinned connection is gone".to_owned())),
        }
    }
}

fn set_limit(mariadb: bool, timeout: Duration) -> String {
    if mariadb {
        format!("SET SESSION max_statement_time = {:.3}", timeout.as_secs_f64())
    } else {
        format!("SET SESSION MAX_EXECUTION_TIME = {}", timeout.as_millis().max(1))
    }
}

fn clear_limit(mariadb: bool) -> &'static str {
    if mariadb {
        "SET SESSION max_statement_time = 0"
    } else {
        "SET SESSION MAX_EXECUTION_TIME = 0"
    }
}

/// `mysql_async` allows several statements per query, so only one must reach it. The server
/// refuses to prepare more than one; a few single statements cannot be prepared at all, which is
/// reported by its own error code and is fine.
async fn refuse_multiple(conn: &mut RawConn, stmt: &str) -> Result<(), DbError> {
    match conn.prep(stmt).await {
        Ok(prepared) => conn.close(prepared).await.map_err(map_error),
        Err(mysql_async::Error::Server(server)) if server.code == code::NOT_PREPARABLE => Ok(()),
        Err(error) => Err(map_error(error)),
    }
}

async fn send(sink: &RowSink, rows: Vec<Vec<Value>>, seq: &mut u32) -> bool {
    let batch = RowBatch { seq: *seq, rows };
    *seq += 1;
    sink.send(RowEvent::Batch(batch)).await.is_ok()
}

fn meta(columns: &[Column]) -> Vec<ColumnMeta> {
    columns
        .iter()
        .map(|c| ColumnMeta {
            name: c.name_str().into_owned(),
            type_name: type_name(c),
            nullable: None,
        })
        .collect()
}

async fn stream_rows(
    conn: &mut RawConn,
    stmt: &str,
    opts: ExecOptions,
    sink: &RowSink,
    canceller: &MyCanceller,
) -> Result<ExecSummary, DbError> {
    let mut summary = ExecSummary::default();
    let mut result = conn.query_iter(stmt).await.map_err(map_error)?;
    let columns: Arc<[Column]> = result.columns().unwrap_or_else(|| Arc::from(Vec::new()));
    summary.columns = meta(&columns);
    summary.returns_rows = !columns.is_empty();
    if summary.returns_rows {
        let _ = sink.send(RowEvent::Columns(summary.columns.clone())).await;
    }

    let batch_size = opts.batch_size.max(1);
    let mut rows = Vec::with_capacity(batch_size);
    let mut seq = 0;
    let mut abandoned = false;
    while let Some(row) = result.next().await.map_err(map_error)? {
        if opts.max_rows.is_some_and(|cap| summary.total_rows >= cap) {
            summary.truncated = true;
            break;
        }
        let cells = (0..row.len())
            .map(|i| match (row.as_ref(i), columns.get(i)) {
                (Some(value), Some(column)) => decode(value, column),
                _ => Value::Null,
            })
            .collect();
        rows.push(cells);
        summary.total_rows += 1;
        if rows.len() >= batch_size {
            let full = std::mem::replace(&mut rows, Vec::with_capacity(batch_size));
            if !send(sink, full, &mut seq).await {
                abandoned = true;
                break;
            }
        }
    }
    summary.rows_affected = result.affected_rows();
    if summary.truncated || abandoned {
        // The connection is unusable until the result is read to its end, so stop the server first.
        let _ = canceller.kill().await;
    }
    // Interrupting the statement surfaces here as an error we caused on purpose.
    let drained = result.drop_result().await;
    if abandoned {
        return Err(DbError::Cancelled);
    }
    if !summary.truncated {
        drained.map_err(map_error)?;
    }
    if !rows.is_empty() && !send(sink, rows, &mut seq).await {
        return Err(DbError::Cancelled);
    }
    Ok(summary)
}

#[async_trait::async_trait]
impl Conn for MyConn {
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
        let mariadb = self.shared.mariadb;
        let read_only_profile = self.shared.read_only;
        let conn = self.raw()?;
        let started = Instant::now();
        refuse_multiple(conn, stmt).await?;

        if let Some(timeout) = opts.timeout {
            conn.query_drop(set_limit(mariadb, timeout))
                .await
                .map_err(map_error)?;
        }
        if opts.read_only_tx {
            // A read-only transaction alone does not stop DDL: its implicit commit ends the
            // transaction and the statement then runs outside it. Making the session read-only as
            // well means the statement is still refused after that implicit commit.
            conn.query_drop("SET SESSION TRANSACTION READ ONLY")
                .await
                .map_err(map_error)?;
            conn.query_drop("START TRANSACTION READ ONLY")
                .await
                .map_err(map_error)?;
        }
        let watchdog = opts.timeout.map(|timeout| {
            let canceller = canceller.clone();
            tokio::spawn(async move {
                tokio::time::sleep(timeout + WATCHDOG_GRACE).await;
                canceller.timed_out.store(true, Ordering::SeqCst);
                let _ = canceller.kill().await;
            })
        });

        let result = stream_rows(conn, stmt, opts, &sink, &canceller).await;

        if let Some(watchdog) = watchdog {
            watchdog.abort();
        }
        // Runs on every path, including errors and cancel.
        let rolled_back = if opts.read_only_tx {
            conn.query_drop("ROLLBACK").await
        } else {
            Ok(())
        };
        if opts.timeout.is_some() {
            let _ = conn.query_drop(clear_limit(mariadb)).await;
        }
        if opts.read_only_tx && !read_only_profile {
            let _ = conn.query_drop("SET SESSION TRANSACTION READ WRITE").await;
        }
        rolled_back.map_err(map_error)?;

        // `SLEEP` and friends return normally when killed, so the flags decide the outcome.
        if canceller.requested.load(Ordering::SeqCst) {
            return Err(DbError::Cancelled);
        }
        if canceller.timed_out.load(Ordering::SeqCst) {
            return Err(DbError::Timeout);
        }
        let mut summary = result?;
        // MariaDB's limit interrupts `SLEEP` without an error, so a statement stopped by the limit can
        // come back looking finished. Its timer can also fire slightly early by our clock.
        let stopped_by_limit = |limit: Duration| started.elapsed() + LIMIT_TOLERANCE >= limit;
        if mariadb && opts.timeout.is_some_and(stopped_by_limit) {
            return Err(DbError::Timeout);
        }
        summary.elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        Ok(summary)
    }

    fn canceller(&self) -> Arc<dyn Canceller> {
        self.canceller.clone()
    }
}
